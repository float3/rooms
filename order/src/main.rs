//! One food order for a room of people, on a server of its own. The hall
//! serves the same thing under `/order`.

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let addr = std::env::var("ORDER_ADDR").unwrap_or_else(|_| "127.0.0.1:8104".into());
    let state = std::env::var("ORDER_STATE").unwrap_or_else(|_| "rooms.json".into());
    let rooms = order::load(PathBuf::from(state));
    rooms::serve(&addr, order::router(rooms)).await;
}
