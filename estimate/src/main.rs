//! The estimation game for a room of people.
//!
//! The host opens a room, puts the join link on the wall, and asks a
//! question. Each phone shows two fields. The host's screen shows the
//! room's bounds and, once the answer has been entered, how many of the
//! ranges contained it.
//!
//! There are three kinds of link, and the key in each is the only
//! authority there is: `/h/<key>` runs the room, `/j/<key>` joins it, and
//! `/p/<key>` belongs to one player.

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let addr = std::env::var("ESTIMATE_ADDR").unwrap_or_else(|_| "127.0.0.1:8097".into());
    let state = std::env::var("ESTIMATE_STATE").unwrap_or_else(|_| "rooms.json".into());
    let rooms = estimate::load(PathBuf::from(state));
    rooms::serve(&addr, estimate::router(rooms)).await;
}
