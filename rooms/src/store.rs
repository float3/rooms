//! Where the rooms are kept.
//!
//! One JSON file, rewritten whenever a room changes. The load it has to
//! take is a few hundred rooms of a few kilobytes each, written a few
//! times a minute by one process, which a file handles. Restarting the
//! server in the middle of an evening then does not lose the games in
//! progress.

use std::{collections::HashMap, fs, io::Write, path::PathBuf, sync::Arc};

use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{Mutex, MutexGuard};

use crate::{key, now_ms};

/// What a tool keeps in a room. The tool decides what that is; this trait
/// decides when it is deleted.
pub trait Room: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Somebody opened the room. The default does nothing, in which case
    /// the room expires on a schedule instead of on use.
    fn seen(&mut self, _now: u64) {}

    /// Move the room on to `now`: close a round that has run out of time,
    /// for example. Returns whether anything changed, so that watchers are
    /// only sent an update when there is one. The default suits a room
    /// that changes only when someone acts.
    fn tick(&mut self, _now: u64) -> bool {
        false
    }

    /// Whether this room can be deleted. Called every few minutes.
    fn expired(&self, now: u64) -> bool;

    /// Every key other than its own that this room answers to: its join
    /// key, its wall key, each player's key. [`Rooms`] keeps an index from
    /// these to the room, so that [`Rooms::locate`] finds a player's room
    /// without looking through every room. The default is none, which
    /// leaves the tool to [`Rooms::find`] as before.
    fn keys(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Brings one saved room up to date before it is read, for a room
    /// whose saved shape has changed: a field renamed, or moved into a
    /// struct of its own. The default leaves it as it is.
    ///
    /// A room that keeps a [`crate::lobby::Lobby`] as `lobby: Lobby` calls
    /// [`crate::lobby::migrate`] here, so that rooms saved with the
    /// lobby's fields at the top level, or before there was a lobby, load.
    fn migrate(_saved: &mut serde_json::Value) {}
}

pub struct Rooms<R: Room> {
    rooms: Mutex<HashMap<String, R>>,
    /// Each key a room hands out ([`Room::keys`]), to the room's own key.
    /// Rebuilt whenever the rooms are saved, which is whenever a request
    /// changes one. Always taken after `rooms`, never held across an await.
    index: std::sync::Mutex<HashMap<String, String>>,
    path: PathBuf,
    /// A limit, so that a script cannot fill the disk by opening rooms.
    max: usize,
}

type Held<'a, R> = MutexGuard<'a, HashMap<String, R>>;

fn index_of<R: Room>(rooms: &HashMap<String, R>) -> HashMap<String, String> {
    rooms
        .iter()
        .flat_map(|(own, room)| {
            room.keys()
                .into_iter()
                .map(move |key| (key.to_string(), own.clone()))
        })
        .collect()
}

impl<R: Room> Rooms<R> {
    /// Rooms from `path`, or no rooms if the file is missing.
    ///
    /// A file that will not parse, or a room in it that will not, is not
    /// thrown away: the file is copied to `<path>.bad-<ms>` and the error
    /// printed, and the server starts with every room that did load.
    /// Refusing to start would be worse than starting without a room or
    /// two; overwriting the only copy of them would be worse still.
    pub fn load(path: impl Into<PathBuf>, max: usize) -> Arc<Self> {
        let path = path.into();
        let rooms = match fs::read_to_string(&path) {
            Ok(text) => parse(&path, &text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => {
                eprintln!("rooms: could not read {}: {e}", path.display());
                set_aside(&path);
                HashMap::new()
            }
        };
        let index = std::sync::Mutex::new(index_of(&rooms));
        Arc::new(Rooms {
            rooms: Mutex::new(rooms),
            index,
            path,
            max,
        })
    }

    /// Opens a room and returns its key. `make` is given the key, because
    /// a room usually stores links to itself. `None` when the store is
    /// full.
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

    /// Runs `f` against a room, after telling it the time. Saves
    /// afterwards, so use it for anything that changes the room.
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

    /// Runs `f` against a room as it is, without moving it on to now and
    /// without saving. A screen's first draw wants [`Rooms::look`].
    pub async fn read<T>(&self, key: &str, f: impl FnOnce(&R, u64) -> T) -> Option<T> {
        let rooms = self.rooms.lock().await;
        Some(f(rooms.get(key)?, now_ms()))
    }

    /// Moves a room on to now ([`Room::tick`]) and runs `f` against it,
    /// saving only if the tick changed it. For a screen's first draw: a
    /// round that ran out while nobody was watching is shown closed.
    ///
    /// Unlike [`Rooms::with`], this does not count as somebody opening
    /// the room ([`Room::seen`]), so a screen that only shows the room, a
    /// projector left on, does not keep it from expiring.
    pub async fn look<T>(&self, key: &str, f: impl FnOnce(&R, u64) -> T) -> Option<T> {
        let mut rooms = self.rooms.lock().await;
        let now = now_ms();
        let room = rooms.get_mut(key)?;
        let changed = room.tick(now);
        let out = f(room, now);
        if changed {
            self.save(&rooms);
        }
        Some(out)
    }

    /// What a stream such as [`crate::one_at`] does with a room each tick,
    /// for a stream built with [`crate::poll`]: counts as somebody using
    /// the room ([`Room::seen`]), moves it on to now, runs `f`, and saves
    /// only if the tick changed it.
    pub async fn watching<T>(&self, key: &str, f: impl FnOnce(&R, u64) -> T) -> Option<T> {
        let mut rooms = self.rooms.lock().await;
        let now = now_ms();
        let room = rooms.get_mut(key)?;
        room.seen(now);
        let changed = room.tick(now);
        let out = f(room, now);
        if changed {
            self.save(&rooms);
        }
        Some(out)
    }

    /// The first room `f` returns something for. Tools use this to find a
    /// room by a key they handed out that is not the room's own key.
    pub async fn find<T>(&self, f: impl Fn(&str, &R) -> Option<T>) -> Option<T> {
        let rooms = self.rooms.lock().await;
        rooms.iter().find_map(|(key, room)| f(key, room))
    }

    /// The room that handed out `key`, by the index of [`Room::keys`]. One
    /// lookup, instead of [`Rooms::find`] walking every room under the
    /// lock that every request and stream waits on.
    pub fn locate(&self, key: &str) -> Option<String> {
        self.index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
    }

    /// The key of the first room `f` holds for: the room a join link
    /// belongs to, say, when the room is stored under the host's key.
    pub async fn key_where(&self, f: impl Fn(&R) -> bool) -> Option<String> {
        self.find(|key, room| f(room).then(|| key.to_string()))
            .await
    }

    /// Runs `f` over every room and saves. For a migration at startup,
    /// when a room has gained a field that older saved ones lack.
    pub async fn each(&self, f: impl FnMut(&mut R)) {
        let mut rooms = self.rooms.lock().await;
        rooms.values_mut().for_each(f);
        self.save(&rooms);
    }

    pub(crate) async fn lock(&self) -> Held<'_, R> {
        self.rooms.lock().await
    }

    /// Saves the rooms as they are. Everything here that changes a room
    /// calls it already. It is public for a tool that changes a room from
    /// inside a [`crate::stream`] render closure, which holds the map.
    pub fn save(&self, rooms: &HashMap<String, R>) {
        // Cheaper than the serialising below, and keeps the index right
        // whatever changed.
        *self
            .index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = index_of(rooms);
        let Ok(body) = serde_json::to_string(rooms) else {
            eprintln!("rooms: could not serialise, not saving");
            return;
        };
        // Through a temporary file. A half-written rooms.json read back
        // after a crash would lose every room instead of the last change.
        let temp = self.path.with_extension("tmp");
        let written = fs::File::create(&temp).and_then(|mut file| {
            file.write_all(body.as_bytes())?;
            file.sync_all()
        });
        if let Err(e) = written.and_then(|()| fs::rename(&temp, &self.path)) {
            eprintln!("rooms: could not write {}: {e}", self.path.display());
        }
    }

    /// Deletes expired rooms. Spawn it once at startup.
    pub async fn sweep(self: Arc<Self>, every: std::time::Duration) {
        loop {
            tokio::time::sleep(every).await;
            self.sweep_once(now_ms()).await;
        }
    }

    async fn sweep_once(&self, now: u64) {
        let mut rooms = self.rooms.lock().await;
        let before = rooms.len();
        rooms.retain(|_, room| !room.expired(now));
        if rooms.len() != before {
            self.save(&rooms);
        }
    }

    #[cfg(test)]
    pub(crate) async fn len(&self) -> usize {
        self.rooms.lock().await.len()
    }
}

/// The rooms in a saved file. Each room is read on its own, through
/// [`Room::migrate`], so that one that will not parse costs that room and
/// not the others; either way the file is kept (see [`set_aside`]).
fn parse<R: Room>(path: &std::path::Path, text: &str) -> HashMap<String, R> {
    let saved: HashMap<String, serde_json::Value> = match serde_json::from_str(text) {
        Ok(saved) => saved,
        Err(e) => {
            eprintln!("rooms: {} will not parse: {e}", path.display());
            set_aside(path);
            return HashMap::new();
        }
    };
    let mut rooms = HashMap::with_capacity(saved.len());
    let mut lost = 0;
    for (key, mut room) in saved {
        R::migrate(&mut room);
        match serde_json::from_value(room) {
            Ok(room) => {
                rooms.insert(key, room);
            }
            Err(e) => {
                lost += 1;
                eprintln!("rooms: a room in {} will not load: {e}", path.display());
            }
        }
    }
    if lost > 0 {
        set_aside(path);
    }
    rooms
}

/// Copies a file that would not load to `<path>.bad-<ms>`, so that the
/// next save, which rewrites `path`, does not destroy the only copy.
fn set_aside(path: &std::path::Path) {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".bad-{}", now_ms()));
    let aside = PathBuf::from(name);
    match fs::copy(path, &aside) {
        Ok(_) => eprintln!("rooms: kept the unreadable file as {}", aside.display()),
        Err(e) => eprintln!("rooms: could not keep a copy of {}: {e}", path.display()),
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

    #[derive(Serialize, Deserialize)]
    struct Keyed {
        handed: Vec<String>,
    }

    impl Room for Keyed {
        fn expired(&self, _now: u64) -> bool {
            self.handed.is_empty()
        }
        fn keys(&self) -> Vec<&str> {
            self.handed.iter().map(String::as_str).collect()
        }
    }

    #[tokio::test]
    async fn a_room_is_located_by_a_key_it_handed_out_without_a_search() {
        let path =
            std::env::temp_dir().join(format!("rooms-test-index-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        let rooms: Arc<Rooms<Keyed>> = Rooms::load(&path, 3);
        let own = rooms
            .create(|_| Keyed {
                handed: vec!["a".into()],
            })
            .await
            .unwrap();
        assert_eq!(rooms.locate("a"), Some(own.clone()));
        assert_eq!(rooms.locate("b"), None);

        rooms.with(&own, |r, _| r.handed = vec!["b".into()]).await;
        assert_eq!(rooms.locate("a"), None, "a key given back is forgotten");
        assert_eq!(rooms.locate("b"), Some(own.clone()));

        let again: Arc<Rooms<Keyed>> = Rooms::load(&path, 3);
        assert_eq!(again.locate("b"), Some(own.clone()), "and rebuilt on load");

        rooms.with(&own, |r, _| r.handed.clear()).await;
        rooms.sweep_once(now_ms()).await;
        assert_eq!(rooms.len().await, 0);
        assert_eq!(rooms.locate("b"), None, "and a swept room is forgotten");
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

    fn aside(path: &std::path::Path) -> Vec<PathBuf> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|entry| {
                let found = entry.ok()?.path();
                let file = found.file_name()?.to_string_lossy().into_owned();
                file.starts_with(&format!("{name}.bad-")).then_some(found)
            })
            .collect()
    }

    fn clean(path: &std::path::Path) {
        let _ = fs::remove_file(path);
        for kept in aside(path) {
            let _ = fs::remove_file(kept);
        }
    }

    #[tokio::test]
    async fn a_corrupt_file_starts_empty_and_is_kept_rather_than_overwritten() {
        let path =
            std::env::temp_dir().join(format!("rooms-test-corrupt-{}.json", std::process::id()));
        clean(&path);
        fs::write(&path, "{ this is not json").unwrap();
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 3);
        assert_eq!(rooms.len().await, 0);
        let kept = aside(&path);
        assert_eq!(kept.len(), 1, "set aside once");
        assert_eq!(fs::read_to_string(&kept[0]).unwrap(), "{ this is not json");
        rooms
            .create(|_| Toy {
                label: String::new(),
                seen: 0,
                ticks: 0,
            })
            .await;
        assert_eq!(
            fs::read_to_string(&kept[0]).unwrap(),
            "{ this is not json",
            "the next save leaves the copy alone"
        );
        clean(&path);
    }

    #[tokio::test]
    async fn one_bad_room_costs_that_room_and_the_file_is_kept() {
        let path =
            std::env::temp_dir().join(format!("rooms-test-one-bad-{}.json", std::process::id()));
        clean(&path);
        let text = r#"{"good":{"label":"a","seen":1,"ticks":0},"bad":{"label":7}}"#;
        fs::write(&path, text).unwrap();
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 3);
        assert_eq!(rooms.len().await, 1);
        assert!(rooms.read("good", |_, _| ()).await.is_some());
        let kept = aside(&path);
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read_to_string(&kept[0]).unwrap(), text);
        clean(&path);
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Renamed {
        label: String,
    }

    impl Room for Renamed {
        fn expired(&self, _now: u64) -> bool {
            false
        }
        fn migrate(saved: &mut serde_json::Value) {
            if let Some(room) = saved.as_object_mut()
                && let Some(old) = room.remove("name")
            {
                room.insert("label".into(), old);
            }
        }
    }

    #[tokio::test]
    async fn a_room_saved_in_an_older_shape_is_migrated_as_it_loads() {
        let path =
            std::env::temp_dir().join(format!("rooms-test-migrate-{}.json", std::process::id()));
        clean(&path);
        fs::write(&path, r#"{"k":{"name":"old"}}"#).unwrap();
        let rooms: Arc<Rooms<Renamed>> = Rooms::load(&path, 3);
        assert_eq!(
            rooms.read("k", |r, _| r.label.clone()).await.as_deref(),
            Some("old")
        );
        assert!(aside(&path).is_empty(), "nothing was lost");
        clean(&path);
    }

    #[tokio::test]
    async fn looking_ticks_without_counting_as_a_visit() {
        let (rooms, path) = store("look");
        let key = rooms
            .create(|_| Toy {
                label: String::new(),
                seen: 0,
                ticks: 0,
            })
            .await
            .unwrap();
        let ticks = rooms.look(&key, |r, _| r.ticks).await;
        assert_eq!(ticks, Some(1), "moved on before it was read");
        let seen = rooms.read(&key, |r, _| r.seen).await;
        assert_eq!(seen, Some(0), "and not counted as somebody opening it");
        let again: Arc<Rooms<Toy>> = Rooms::load(&path, 3);
        assert_eq!(again.read(&key, |r, _| r.ticks).await, Some(1), "saved");
        let _ = fs::remove_file(&path);
    }
}
