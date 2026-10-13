//! Somewhere to keep the rooms.
//!
//! One JSON file, rewritten whenever a room changes. That is not a database
//! and is not trying to be: a few hundred rooms of a few kilobytes each,
//! written a handful of times a minute by one process. What it buys is that
//! restarting the server mid-evening does not lose the evening.

use std::{collections::HashMap, fs, io::Write, path::PathBuf, sync::Arc};

use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{Mutex, MutexGuard};

use crate::{key, now_ms};

/// What a tool keeps in a room. The tool decides what that is; this decides
/// when to forget it.
pub trait Room: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Somebody looked at the room. The default does nothing, which means
    /// the room expires on its own schedule rather than on attention.
    fn seen(&mut self, _now: u64) {}

    /// Move the room on to `now` — close a round that has run out of time,
    /// say. Return whether anything changed, so watchers are only woken for
    /// something. The default is a room that only changes when someone acts.
    fn tick(&mut self, _now: u64) -> bool {
        false
    }

    /// Whether this room can be dropped. Called every few minutes.
    fn expired(&self, now: u64) -> bool;
}

pub struct Rooms<R: Room> {
    rooms: Mutex<HashMap<String, R>>,
    path: PathBuf,
    /// A cap so that a script cannot fill the disk by asking for rooms.
    max: usize,
}

type Held<'a, R> = MutexGuard<'a, HashMap<String, R>>;

impl<R: Room> Rooms<R> {
    /// Rooms from `path`, or no rooms if it is missing or unreadable. An
    /// unreadable file is not fatal on purpose: the alternative is a tool
    /// that will not start because of an evening that is over anyway.
    pub fn load(path: impl Into<PathBuf>, max: usize) -> Arc<Self> {
        let path = path.into();
        let rooms = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Arc::new(Rooms {
            rooms: Mutex::new(rooms),
            path,
            max,
        })
    }

    /// Opens a room and returns its key. `make` is handed the key, because
    /// a room usually wants to keep links to itself. `None` when the store
    /// is full.
    pub async fn create(&self, make: impl FnOnce(&str) -> R) -> Option<String> {
        let mut rooms = self.rooms.lock().await;
        if rooms.len() >= self.max {
            return None;
        }
        let key = key::new();
        rooms.insert(key.clone(), make(&key));
        self.save(&rooms);
        Some(key)
    }

    /// Runs `f` against a room, having first told it the time. Saves
    /// afterwards, so this is for anything that changes the room.
    pub async fn with<T>(&self, key: &str, f: impl FnOnce(&mut R, u64) -> T) -> Option<T> {
        let mut rooms = self.rooms.lock().await;
        let now = now_ms();
        let room = rooms.get_mut(key)?;
        room.seen(now);
        room.tick(now);
        let out = f(room, now);
        self.save(&rooms);
        Some(out)
    }

    /// Runs `f` against a room without saving: for reads.
    pub async fn read<T>(&self, key: &str, f: impl FnOnce(&R, u64) -> T) -> Option<T> {
        let rooms = self.rooms.lock().await;
        Some(f(rooms.get(key)?, now_ms()))
    }

    /// The first room `f` says yes to, by key. Tools use this to look a
    /// room up by a key they handed out that is not the room's own.
    pub async fn find<T>(&self, f: impl Fn(&str, &R) -> Option<T>) -> Option<T> {
        let rooms = self.rooms.lock().await;
        rooms.iter().find_map(|(key, room)| f(key, room))
    }

    /// Runs `f` over every room and saves. For the migration a tool does
    /// at startup, when a room gained a field that older ones do not have.
    pub async fn each(&self, f: impl FnMut(&mut R)) {
        let mut rooms = self.rooms.lock().await;
        rooms.values_mut().for_each(f);
        self.save(&rooms);
    }

    pub(crate) async fn lock(&self) -> Held<'_, R> {
        self.rooms.lock().await
    }

    /// Saves the rooms as they stand. Called for you by everything here
    /// that changes one; public for a tool that changes a room from inside
    /// a [`crate::stream`] render closure, which holds the map itself.
    pub fn save(&self, rooms: &HashMap<String, R>) {
        let Ok(body) = serde_json::to_string(rooms) else {
            eprintln!("rooms: could not serialise, not saving");
            return;
        };
        // Through a temporary file: a half-written rooms.json read back
        // after a crash would lose every room rather than the last change.
        let temp = self.path.with_extension("tmp");
        let written = fs::File::create(&temp).and_then(|mut file| {
            file.write_all(body.as_bytes())?;
            file.sync_all()
        });
        if let Err(e) = written.and_then(|()| fs::rename(&temp, &self.path)) {
            eprintln!("rooms: could not write {}: {e}", self.path.display());
        }
    }

    /// Forgets expired rooms, for ever. Spawn it once at startup.
    pub async fn sweep(self: Arc<Self>, every: std::time::Duration) {
        loop {
            tokio::time::sleep(every).await;
            let now = now_ms();
            let mut rooms = self.rooms.lock().await;
            let before = rooms.len();
            rooms.retain(|_, room| !room.expired(now));
            if rooms.len() != before {
                self.save(&rooms);
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn len(&self) -> usize {
        self.rooms.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Toy {
        label: String,
        seen: u64,
        ticks: u64,
    }

    impl Room for Toy {
        fn seen(&mut self, now: u64) {
            self.seen = now;
        }
        fn tick(&mut self, _now: u64) -> bool {
            self.ticks += 1;
            true
        }
        fn expired(&self, now: u64) -> bool {
            self.seen + 1000 < now
        }
    }

    fn store(name: &str) -> (Arc<Rooms<Toy>>, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("rooms-test-{name}-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        (Rooms::load(&path, 3), path)
    }

    #[tokio::test]
    async fn a_room_comes_back_with_its_key_and_survives_a_reload() {
        let (rooms, path) = store("reload");
        let key = rooms
            .create(|k| Toy {
                label: k.to_string(),
                seen: 0,
                ticks: 0,
            })
            .await
            .expect("not full");
        let label = rooms.read(&key, |r, _| r.label.clone()).await;
        assert_eq!(label.as_deref(), Some(key.as_str()));

        let again: Arc<Rooms<Toy>> = Rooms::load(&path, 3);
        assert_eq!(
            again.read(&key, |r, _| r.label.clone()).await.as_deref(),
            Some(key.as_str())
        );
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_cap_holds() {
        let (rooms, path) = store("cap");
        for _ in 0..3 {
            assert!(
                rooms
                    .create(|_| Toy {
                        label: String::new(),
                        seen: 0,
                        ticks: 0
                    })
                    .await
                    .is_some()
            );
        }
        assert!(
            rooms
                .create(|_| Toy {
                    label: String::new(),
                    seen: 0,
                    ticks: 0
                })
                .await
                .is_none()
        );
        assert_eq!(rooms.len().await, 3);
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn writing_touches_the_room_and_reading_does_not() {
        let (rooms, path) = store("touch");
        let key = rooms
            .create(|_| Toy {
                label: String::new(),
                seen: 0,
                ticks: 0,
            })
            .await
            .unwrap();
        rooms.with(&key, |r, _| r.label = "set".into()).await;
        let (seen, ticks) = rooms.read(&key, |r, _| (r.seen, r.ticks)).await.unwrap();
        assert!(seen > 0, "with() should have told the room the time");
        assert_eq!(ticks, 1, "with() ticks once; read() does not tick at all");
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn an_unknown_key_is_simply_absent() {
        let (rooms, path) = store("absent");
        assert!(rooms.read("nope", |_, _| ()).await.is_none());
        assert!(rooms.with("nope", |_, _| ()).await.is_none());
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_room_can_be_found_by_a_key_it_handed_out() {
        let (rooms, path) = store("find");
        let key = rooms
            .create(|_| Toy {
                label: "player-token".into(),
                seen: 0,
                ticks: 0,
            })
            .await
            .unwrap();
        let found = rooms
            .find(|k, r| (r.label == "player-token").then(|| k.to_string()))
            .await;
        assert_eq!(found, Some(key));
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn every_room_can_be_walked_over_once_at_startup() {
        let (rooms, path) = store("each");
        for _ in 0..2 {
            rooms
                .create(|_| Toy {
                    label: String::new(),
                    seen: 0,
                    ticks: 0,
                })
                .await;
        }
        rooms.each(|room| room.label = "migrated".into()).await;

        let again: Arc<Rooms<Toy>> = Rooms::load(&path, 3);
        let labels = again.find(|_, room| Some(room.label.clone())).await;
        assert_eq!(labels.as_deref(), Some("migrated"), "and it was saved");
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_corrupt_file_starts_empty_rather_than_refusing_to_start() {
        let path =
            std::env::temp_dir().join(format!("rooms-test-corrupt-{}.json", std::process::id()));
        fs::write(&path, "{ this is not json").unwrap();
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 3);
        assert_eq!(rooms.len().await, 0);
        let _ = fs::remove_file(&path);
    }
}
