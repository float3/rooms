//! Sending the room to every phone watching it.
//!
//! Server-sent events rather than websockets. The traffic goes one way, it
//! is plain HTTP, and the browser reconnects by itself.
//!
//! A watcher is sent the room as it is now, and then nothing until it
//! differs. A room with twenty phones on it and nothing happening costs
//! one heartbeat every five seconds.

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
/// stutter, slow enough to cost nothing.
const TICK_MS: u64 = 250;
/// Send an unchanged view again this often, to keep a proxy from closing
/// an idle connection.
const HEARTBEAT_MS: u64 = 5_000;

/// How long until the next tick boundary. Watchers wake on the boundary
/// instead of `TICK_MS` after each one connected, so twenty phones get the
/// same frame at the same moment.
fn until_tick(now: u64) -> u64 {
    TICK_MS - now % TICK_MS
}

async fn next_tick() {
    tokio::time::sleep(Duration::from_millis(until_tick(now_ms()))).await;
}

/// Watches rooms and sends a view whenever it changes.
///
/// `render` is called with every room and the time. It returns the view as
/// JSON, or `None` to close the connection, which is how a watcher of a
/// deleted room is let go.
///
/// Watching never saves, however much `render` changes: a room with twenty
/// phones on it would otherwise rewrite the state file eighty times a
/// second. Requests change rooms, and requests save them. [`one`] is the
/// exception, because a room that moves on its own moves while somebody is
/// watching it.
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
                    break; // the phone closed the connection
                }
                last = view;
                last_sent = now;
            }
            next_tick().await;
        }
    });
    (
        // nginx buffers proxied responses by default, which holds every
        // event until the buffer fills. This header turns that off.
        [(header::HeaderName::from_static("x-accel-buffering"), "no")],
        Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
    )
        .into_response()
}

/// Watches one room, ticks it, and renders it, closing the connection if
/// the room is gone.
///
/// `render` returns the view as JSON. That is the tool's job rather than
/// this crate's, because a view usually borrows from the room it
/// describes. Saves when [`Room::tick`] reports a change, and not
/// otherwise.
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
