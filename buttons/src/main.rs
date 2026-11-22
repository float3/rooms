//! Red or blue for a room of people, on a server of its own. The hall
//! serves the same thing under `/buttons`.

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let addr = std::env::var("BUTTONS_ADDR").unwrap_or_else(|_| "127.0.0.1:8106".into());
    let state = std::env::var("BUTTONS_STATE").unwrap_or_else(|_| "rooms.json".into());
    let rooms = buttons::load(PathBuf::from(state));
    rooms::serve(&addr, buttons::router(rooms)).await;
}
