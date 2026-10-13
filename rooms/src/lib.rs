//! The part every one of these tools needs: a room other people join from
//! their phones.
//!
//! A host opens a room and gets back two keys — one to run it, one to hand
//! out. Players join with the second, usually by scanning it off a screen.
//! Everyone's phone holds an event stream open and sees the room change as
//! it changes. The rooms live in one JSON file so a restart does not throw
//! an evening away, and they are forgotten when nobody has looked at them
//! for a while.
//!
//! What a room *is* belongs to the tool. Implement [`Room`] on whatever
//! that is and [`Rooms`] keeps them.
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
pub mod qr;
mod store;
mod stream;

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
