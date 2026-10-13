//! A room that other people join from their phones.
//!
//! The host gets two keys, one to run the room and one to hand out.
//! Players join with the second, usually by scanning a QR code off a
//! screen. Each phone keeps an event stream open and redraws when the room
//! changes. Rooms are kept in one JSON file, so restarting the server does
//! not lose what is in progress, and a room is deleted once nobody has
//! opened it for a day.
//!
//! The tool decides what a room contains. Implement [`Room`] on it and
//! [`Rooms`] stores it.
//!
//! ```no_run
//! use rooms::{Room, Rooms};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Quiz {
//!     question: String,
//!     seen: u64,
//! }
//!
//! impl Room for Quiz {
//!     fn seen(&mut self, now: u64) {
//!         self.seen = now;
//!     }
//!     fn expired(&self, now: u64) -> bool {
//!         self.seen + rooms::DAY_MS < now
//!     }
//! }
//!
//! # async fn example() {
//! let rooms: std::sync::Arc<Rooms<Quiz>> = Rooms::load("rooms.json", 2000);
//! let key = rooms
//!     .create(|_key| Quiz { question: String::new(), seen: rooms::now_ms() })
//!     .await
//!     .expect("room store is not full");
//! # }
//! ```

pub mod http;
pub mod key;
mod listen;
pub mod qr;
mod store;
mod stream;

pub use listen::{listen, where_it_listens};
pub use store::{Room, Rooms};
pub use stream::{one, stream};

use std::time::{SystemTime, UNIX_EPOCH};

pub const HOUR_MS: u64 = 60 * 60 * 1000;
pub const DAY_MS: u64 = 24 * HOUR_MS;

/// Milliseconds since the epoch. Phones and the server disagree about the
/// time by enough to matter when a round is counting down, so every view
/// these tools send is stamped with this and the phone works from the
/// difference rather than from its own clock.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before 1970")
        .as_millis() as u64
}
