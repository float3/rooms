//! Speed updating for a room of people.
//!
//! Everybody says where they stand on a set of statements, then talks to
//! whoever disagrees with them most, a few minutes at a time, and says
//! where they stand afterwards.
//!
//! Three kinds of link, and the key in each is the only authority there
//! is: `/h/<key>` runs the room, `/j/<key>` joins it, and `/p/<key>`
//! belongs to one player.

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let addr = std::env::var("UPDATING_ADDR").unwrap_or_else(|_| "127.0.0.1:8102".into());
    let state = std::env::var("UPDATING_STATE").unwrap_or_else(|_| "rooms.json".into());
    let suggestions =
        std::env::var("UPDATING_SUGGESTIONS").unwrap_or_else(|_| "suggestions.jsonl".into());
    let rooms = updating::load(PathBuf::from(state));
    rooms::serve(&addr, updating::router(rooms, PathBuf::from(suggestions))).await;
}
