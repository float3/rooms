//! Everyone's phone, watching the same room.
//!
//! Server-sent events rather than websockets: the traffic only goes one
//! way, it is plain HTTP so it survives whatever the venue's wifi does to
//! it, and the browser reconnects on its own.
//!
//! Each watcher is sent the room as it looks now, and then nothing until it
//! looks different. A room with twenty phones on it and nothing happening
//! costs a heartbeat every five seconds.

use std::{collections::HashMap, convert::Infallible, sync::Arc, time::Duration};

use axum::{
    http::header,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    now_ms,
    store::{Room, Rooms},
};

/// How often to look at the room. Fast enough that a countdown does not
/// stutter, slow enough to be free.
const TICK_MS: u64 = 250;
/// Resend an unchanged view this often, so a proxy that drops idle
/// connections has something to not drop.
const HEARTBEAT_MS: u64 = 5_000;

/// How long until the next tick boundary. Watchers wake on the boundary
/// rather than `TICK_MS` after whenever each of them happened to connect,
/// so twenty phones get the same frame at the same moment.
fn until_tick(now: u64) -> u64 {
    TICK_MS - now % TICK_MS
}

async fn next_tick() {
    tokio::time::sleep(Duration::from_millis(until_tick(now_ms()))).await;
}

/// Watches rooms and sends a view whenever it changes.
///
/// `render` is called with every room and the time, and returns the view as
/// JSON — or `None` to hang up, which is how a watcher of a room that has
/// been swept is let go.
///
/// Watching saves nothing, however much `render` touches: a room with
/// twenty phones on it would otherwise rewrite the state file eighty times
/// a second. What changes a room is a request, and that is what saves it.
/// [`one`] is the exception, because a room that moves on its own moves
/// under a watcher.
pub fn stream<R: Room>(
    rooms: Arc<Rooms<R>>,
    render: impl Fn(&mut HashMap<String, R>, u64) -> Option<String> + Send + 'static,
) -> Response {
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(4);
    tokio::spawn(async move {
        let mut last = String::new();
        let mut last_sent = 0;
        loop {
            let now = now_ms();
            let view = {
                let mut held = rooms.lock().await;
                render(&mut held, now)
            };
            let Some(view) = view else { break };
            if view != last || now.saturating_sub(last_sent) >= HEARTBEAT_MS {
                let body = format!("{{\"now\":{now},\"view\":{view}}}");
                if tx.send(Ok(Event::default().data(body))).await.is_err() {
                    break; // the phone went away
                }
                last = view;
                last_sent = now;
            }
            next_tick().await;
        }
    });
    (
        // nginx buffers proxied responses by default, which holds every
        // event until the buffer fills; this is the header that stops it.
        [(header::HeaderName::from_static("x-accel-buffering"), "no")],
        Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
    )
        .into_response()
}

/// The common case: watch one room, tick it, and render it, hanging up if
/// it is gone.
///
/// `render` returns the view as JSON, which is the tool's job rather than
/// this crate's because a view usually borrows from the room it describes.
/// Saves when [`Room::tick`] says the room moved, and otherwise not.
pub fn one<R: Room>(
    rooms: Arc<Rooms<R>>,
    key: String,
    render: impl Fn(&R) -> Option<String> + Send + 'static,
) -> Response {
    stream(rooms.clone(), move |held, now| {
        let room = held.get_mut(&key)?;
        room.seen(now);
        if room.tick(now) {
            let view = render(room);
            rooms.save(held);
            return view;
        }
        render(room)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_watcher_wakes_on_the_same_boundary() {
        for now in [0, 1, 124, 125, 249, 250, 251, 1_762_000_000_123] {
            let wait = until_tick(now);
            assert!((1..=TICK_MS).contains(&wait), "now={now} wait={wait}");
            assert_eq!(
                (now + wait) % TICK_MS,
                0,
                "now={now} does not land on a tick"
            );
        }
    }
}
