# rooms

[![CI](https://github.com/float3/rooms/actions/workflows/ci.yml/badge.svg)](https://github.com/float3/rooms/actions/workflows/ci.yml)

a rust library for tools where a host opens a room and other people join
it from their phones.

the host gets two keys, one to run the room and one to hand out. players
join with the second, usually by scanning a QR code off a screen. each
phone keeps an event stream open and redraws when the room changes. rooms
are kept in one JSON file, so restarting the server in the middle of an
evening does not lose what is in progress, and a room is deleted once
nobody has opened it for a day.

used by [petrov](https://github.com/float3/petrov),
[estimate](https://github.com/float3/estimate),
[numbers](https://github.com/float3/numbers) and
[updating](https://github.com/float3/updating).

## what is here

| | |
| --- | --- |
| `Rooms<R>` | a keyed store with a cap, saved to one JSON file, expired rooms deleted every few minutes |
| `rooms::stream` | server-sent events: one view per watcher, sent again only when it changes |
| `rooms::qr` | the join link as an SVG |
| `rooms::key` | 24 random alphanumeric characters |
| `rooms::http` | the JSON responses these tools answer with |
| `rooms::site` | the page and its files, the clock, QR codes, opening a room and answering a host action |
| `rooms::listen` | the socket systemd passed in, or a bound address; `rooms::serve` listens and serves |
| `rooms-client` | the TypeScript and CSS the pages share, and the build step that compiles them |

the tool decides what a room contains. implement `Room` on it:

```rust
impl Room for Lobby {
    fn seen(&mut self, now: u64) { self.seen = now; }
    fn expired(&self, now: u64) -> bool { self.seen + rooms::DAY_MS < now }
}
```

`tick` is for rooms that change without anyone acting, such as a round
that runs out of time. it returns whether anything changed, so a watcher
is only sent an update when there is one.

## the browser code

`rooms-client` is the second crate. it holds the TypeScript and the
stylesheet the three pages share: the event stream, the request helper,
number formatting, the QR code, and the styles for panels, tables and
buttons.

none of these repositories contain hand-written JavaScript. a tool keeps
its TypeScript in `client/`, calls `rooms_client::bundle("client")` from
its `build.rs`, and includes the output:

```rust
const APP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/app.js"));
const APP_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/app.css"));
```

`esbuild` compiles and bundles. `tsc` typechecks, and the build prints a
warning when `tsc` is missing rather than shipping TypeScript that nobody
has checked. `nix develop` provides both.

the stylesheet is written for a phone: a single column, buttons and
fields at least 48px tall, and 17px text in form fields, which is above
the size at which iOS zooms in when a field is tapped. the submit button
stays at the bottom of the screen while the page scrolls.

## keys

a room key is 24 random alphanumeric characters, and there are no
accounts. anyone with the host key can run the room; anyone with the join
key is in it. that is appropriate for a group of people who are in the
same room as each other, and for nothing else. do not put anything in a
room that would matter if a stranger guessed the key.

## Server-sent events rather than websockets

the traffic goes one way, it is plain HTTP, and the browser reconnects by
itself. watchers wake on a shared 250 ms boundary instead of 250 ms after
each one connected, so twenty phones get the same frame at the same
moment. an unchanged view is sent again every five seconds, which keeps
an idle proxy from closing the connection.

`stream` sets `X-Accel-Buffering: no`. without it nginx holds the events
until its buffer fills.

## the JSON file

a few hundred rooms of a few kilobytes each, written by one process a few
times a minute. writes go to a temporary file which is then renamed, so a
crash during a write loses the most recent change instead of every room.
a file that does not parse is treated as empty, because a tool that
refuses to start is worse than one that has lost a finished evening.

## under systemd

`rooms::listen` takes the socket a systemd `.socket` unit passes in
instead of binding one. the service can then run with
`PrivateNetwork = true` and `RestrictAddressFamilies = "none"`: a
compromised game server cannot open a connection to the internet or to
the other services on loopback. outside systemd it binds the address it
was given, so `cargo run` behaves as usual.

## development

```sh
nix develop --command cargo test
```
