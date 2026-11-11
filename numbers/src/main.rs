//! Number games for a room of people.
//!
//! The host picks a rule and a range, everyone picks a number on their
//! phone, and the rule decides who won. No number is shown to anybody
//! until the host reveals the round.
//!
//! There are four kinds of link, and the key in each is the only
//! authority there is: `/h/<key>` runs the room, `/j/<key>` joins it,
//! `/p/<key>` belongs to one player, and `/w/<key>` is the wall.

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let addr = std::env::var("NUMBERS_ADDR").unwrap_or_else(|_| "127.0.0.1:8098".into());
    let state = std::env::var("NUMBERS_STATE").unwrap_or_else(|_| "rooms.json".into());
    let rooms = numbers::load(PathBuf::from(state));
    rooms::serve(&addr, numbers::router(rooms)).await;
}
