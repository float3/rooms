//! Sending the room to every phone watching it.
//!
//! Server-sent events rather than websockets. The traffic goes one way, it
//! is plain HTTP, and the browser reconnects by itself.
//!
//! A watcher is sent the room as it is now, and then nothing until it
//! differs. A room with twenty phones on it and nothing happening costs
//! one heartbeat every five seconds.

use std::{collections::HashMap, convert::Infallible, future::Future, sync::Arc, time::Duration};

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
    // Shared with each tick's future. Only ever taken by the one task
    // that runs this stream, so it is never waited on.
    let render = Arc::new(std::sync::Mutex::new(render));
    poll(move |now| {
        let rooms = rooms.clone();
        let render = render.clone();
        async move {
            let mut held = rooms.lock().await;
            let render = render.lock().unwrap_or_else(|p| p.into_inner());
            render(&mut held, now)
        }
    })
}

/// [`stream`], for a view that has to wait on something besides one
/// store: a room in another tool, say, as the shared room's wall waits on
/// the game it is showing. `next` is called four times a second with the
/// time and answers with the view as JSON, or `None` to close the
/// connection. As with [`stream`], an unchanged view is not sent again
/// except as a heartbeat.
///
/// Do not hold a lock across the whole of `next` that the rest of the
/// server waits on; take it, render, and let it go.
pub fn poll<F, Fut>(mut next: F) -> Response
where
    F: FnMut(u64) -> Fut + Send + 'static,
    Fut: Future<Output = Option<String>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(4);
    tokio::spawn(async move {
        let mut last = String::new();
        let mut last_sent = 0;
        loop {
            // The phone has gone. Stop now rather than at the next send,
            // so that whoever it was watching for stops counting as
            // connected (see `lobby`).
            if tx.is_closed() {
                break;
            }
            let now = now_ms();
            let Some(view) = next(now).await else { break };
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
    one_at(rooms, key, move |room, _| render(room))
}

/// [`one`], for a view that depends on the time: who is connected, how
/// long is left. The watcher counts as somebody using the room
/// ([`Room::seen`]) for as long as it watches.
pub fn one_at<R: Room>(
    rooms: Arc<Rooms<R>>,
    key: String,
    render: impl Fn(&R, u64) -> Option<String> + Send + 'static,
) -> Response {
    watch_one(rooms, key, true, render)
}

/// [`one_at`], for a screen that only shows the room, such as a wall on a
/// projector. It does not count as using the room, so a projector left
/// on overnight does not keep a finished room alive: once the room
/// expires, the stream closes.
pub fn one_unseen<R: Room>(
    rooms: Arc<Rooms<R>>,
    key: String,
    render: impl Fn(&R, u64) -> Option<String> + Send + 'static,
) -> Response {
    watch_one(rooms, key, false, render)
}

fn watch_one<R: Room>(
    rooms: Arc<Rooms<R>>,
    key: String,
    seen: bool,
    render: impl Fn(&R, u64) -> Option<String> + Send + 'static,
) -> Response {
    stream(rooms.clone(), move |held, now| {
        let room = held.get_mut(&key)?;
        if seen {
            room.seen(now);
        }
        if room.tick(now) {
            let view = render(room, now);
            rooms.save(held);
            return view;
        }
        render(room, now)
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
