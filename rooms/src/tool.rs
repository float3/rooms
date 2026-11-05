//! A tool as one of several in the same server, so that one room can move
//! between them without anyone scanning a new code.
//!
//! The shared room (`rooms-hall`) keeps the people and the one join link.
//! When its host picks a tool, it opens a room in that tool and joins
//! everybody into it by name. A tool only has to say how its rooms are
//! opened and joined, which is [`Joinable`], and [`Mount`] does the rest.

use std::{future::Future, pin::Pin, sync::Arc};

use crate::{Room, Rooms, now_ms};

/// A future that can be held behind a [`Tool`].
pub type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A room people join by giving a name.
pub trait Joinable: Room {
    /// A new, empty room.
    fn open(now: u64) -> Self;
    /// Adds somebody by name and answers with their player key. With a
    /// [`crate::lobby::Lobby`], this is `lobby::admit(self, name, None, now)`.
    fn join(&mut self, name: &str, now: u64) -> Result<String, &'static str>;

    /// Adds somebody the shared room seated here before, under `known`,
    /// the key this room gave them then. A room with a lobby answers with
    /// that key while it is still somebody's, so the shared room can move
    /// back to a game without tripping over "that name is taken": that is
    /// `lobby::admit(self, name, known, now)`. The default joins by name.
    fn rejoin(
        &mut self,
        known: Option<&str>,
        name: &str,
        now: u64,
    ) -> Result<String, &'static str> {
        let _ = known;
        self.join(name, now)
    }

    /// Takes the player with `key` out, because the shared room's host
    /// took them out of the room. Answers whether there was anybody to
    /// take out. The default keeps everybody.
    fn leave(&mut self, key: &str) -> bool {
        let _ = key;
        false
    }

    /// The key of this room's wall, `/w/<key>`, for the shared room to
    /// show on its own. The default is no wall.
    fn wall(&self) -> Option<&str> {
        None
    }
}

/// What the shared room needs from a tool.
pub trait Tool: Send + Sync {
    /// The path the tool is served under, without slashes: `numbers`.
    fn id(&self) -> &'static str;
    /// The tool's name, as the host's buttons show it.
    fn title(&self) -> &'static str;
    /// Opens a room and answers with its host key, or `None` when the
    /// tool holds as many rooms as it can.
    fn open(&self) -> Boxed<'_, Option<String>>;
    /// Joins `name` into the room run with `host`. `None` when that room
    /// is gone.
    fn join<'a>(
        &'a self,
        host: &'a str,
        name: &'a str,
    ) -> Boxed<'a, Option<Result<String, &'static str>>>;

    /// Joins `name` again, presenting `known`, the key the room gave them
    /// last time. See [`Joinable::rejoin`]. The default joins by name.
    fn rejoin<'a>(
        &'a self,
        host: &'a str,
        known: Option<&'a str>,
        name: &'a str,
    ) -> Boxed<'a, Option<Result<String, &'static str>>> {
        let _ = known;
        self.join(host, name)
    }

    /// Takes the player with `key` out of the room run with `host`. `None`
    /// when that room is gone. The default takes nobody out.
    fn leave<'a>(&'a self, host: &'a str, key: &'a str) -> Boxed<'a, Option<bool>> {
        let _ = (host, key);
        Box::pin(async { Some(false) })
    }

    /// The wall key of the room run with `host`, if it has a wall.
    fn wall<'a>(&'a self, host: &'a str) -> Boxed<'a, Option<String>> {
        let _ = host;
        Box::pin(async { None })
    }
}

/// A tool's store, as a [`Tool`].
pub struct Mount<R: Joinable> {
    pub id: &'static str,
    pub title: &'static str,
    pub rooms: Arc<Rooms<R>>,
}

impl<R: Joinable> Tool for Mount<R> {
    fn id(&self) -> &'static str {
        self.id
    }

    fn title(&self) -> &'static str {
        self.title
    }

    fn open(&self) -> Boxed<'_, Option<String>> {
        Box::pin(async move {
            let now = now_ms();
            self.rooms.create(|_| R::open(now)).await
        })
    }

    fn join<'a>(
        &'a self,
        host: &'a str,
        name: &'a str,
    ) -> Boxed<'a, Option<Result<String, &'static str>>> {
        Box::pin(async move {
            self.rooms
                .with(host, |room, now| room.join(name, now))
                .await
        })
    }

    fn rejoin<'a>(
        &'a self,
        host: &'a str,
        known: Option<&'a str>,
        name: &'a str,
    ) -> Boxed<'a, Option<Result<String, &'static str>>> {
        Box::pin(async move {
            self.rooms
                .with(host, |room, now| room.rejoin(known, name, now))
                .await
        })
    }

    fn leave<'a>(&'a self, host: &'a str, key: &'a str) -> Boxed<'a, Option<bool>> {
        Box::pin(async move { self.rooms.with(host, |room, _| room.leave(key)).await })
    }

    fn wall<'a>(&'a self, host: &'a str) -> Boxed<'a, Option<String>> {
        Box::pin(async move {
            self.rooms
                .read(host, |room, _| room.wall().map(str::to_string))
                .await
                .flatten()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Toy {
        names: Vec<String>,
    }

    impl Room for Toy {
        fn expired(&self, _now: u64) -> bool {
            false
        }
    }

    impl Joinable for Toy {
        fn open(_now: u64) -> Self {
            Toy { names: Vec::new() }
        }
        fn join(&mut self, name: &str, _now: u64) -> Result<String, &'static str> {
            if !self.names.iter().any(|n| n == name) {
                self.names.push(name.to_string());
            }
            Ok(format!("player-{name}"))
        }
    }

    #[tokio::test]
    async fn a_mounted_tool_opens_rooms_and_joins_people_into_them() {
        let path = std::env::temp_dir().join(format!("rooms-tool-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let tool: Box<dyn Tool> = Box::new(Mount {
            id: "toy",
            title: "Toy",
            rooms: Rooms::<Toy>::load(&path, 4),
        });
        let host = tool.open().await.expect("there is room");
        assert_eq!(tool.join(&host, "Ada").await, Some(Ok("player-Ada".into())));
        assert_eq!(tool.join(&host, "Ada").await, Some(Ok("player-Ada".into())));
        assert_eq!(tool.join("not-a-room", "Ada").await, None);
        assert_eq!(
            tool.rejoin(&host, Some("player-Ada"), "Ada").await,
            Some(Ok("player-Ada".into())),
            "a tool without rejoin joins by name"
        );
        assert_eq!(tool.leave(&host, "player-Ada").await, Some(false));
        assert_eq!(tool.wall(&host).await, None);
        let _ = std::fs::remove_file(&path);
    }

    #[derive(Serialize, Deserialize)]
    struct WithLobby {
        #[serde(flatten)]
        lobby: crate::lobby::Lobby,
    }

    impl Room for WithLobby {
        fn expired(&self, _now: u64) -> bool {
            false
        }
        fn keys(&self) -> Vec<&str> {
            self.lobby.keys().collect()
        }
    }

    impl crate::lobby::HasLobby for WithLobby {
        fn lobby(&self) -> &crate::lobby::Lobby {
            &self.lobby
        }
        fn lobby_mut(&mut self) -> &mut crate::lobby::Lobby {
            &mut self.lobby
        }
    }

    impl Joinable for WithLobby {
        fn open(_now: u64) -> Self {
            WithLobby {
                lobby: crate::lobby::Lobby::new(),
            }
        }
        fn join(&mut self, name: &str, now: u64) -> Result<String, &'static str> {
            crate::lobby::admit(self, name, None, now)
        }
        fn rejoin(
            &mut self,
            known: Option<&str>,
            name: &str,
            now: u64,
        ) -> Result<String, &'static str> {
            crate::lobby::admit(self, name, known, now)
        }
        fn leave(&mut self, key: &str) -> bool {
            self.lobby.remove(key).is_some()
        }
        fn wall(&self) -> Option<&str> {
            Some(&self.lobby.wall)
        }
    }

    #[tokio::test]
    async fn a_tool_with_a_lobby_is_rejoined_by_key_and_can_be_left() {
        let path =
            std::env::temp_dir().join(format!("rooms-tool-lobby-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let rooms = Rooms::<WithLobby>::load(&path, 4);
        let tool: Box<dyn Tool> = Box::new(Mount {
            id: "toy",
            title: "Toy",
            rooms: rooms.clone(),
        });
        let host = tool.open().await.unwrap();
        let ada = tool.join(&host, "Ada").await.unwrap().unwrap();
        assert!(
            tool.join(&host, "Ada").await.unwrap().is_err(),
            "Ada has only just joined, so her name is taken"
        );
        assert_eq!(
            tool.rejoin(&host, Some(&ada), "Ada").await,
            Some(Ok(ada.clone())),
            "but the hall, which holds her key, gets her back"
        );
        let wall = tool.wall(&host).await.expect("a wall");
        assert_eq!(rooms.locate(&wall), Some(host.clone()));
        assert_eq!(tool.leave(&host, &ada).await, Some(true));
        assert_eq!(tool.leave(&host, &ada).await, Some(false));
        assert_eq!(tool.leave("gone", &ada).await, None);
        let _ = std::fs::remove_file(&path);
    }
}
