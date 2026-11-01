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
    /// Adds somebody by name and answers with their player key. A name
    /// already in the room answers with that player's key, so joining
    /// twice is harmless.
    fn join(&mut self, name: &str, now: u64) -> Result<String, &'static str>;
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
        let _ = std::fs::remove_file(&path);
    }
}
