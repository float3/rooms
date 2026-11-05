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
| `rooms::lobby` | who is in a room: names, keys, who is connected, the host taking somebody out, and the wall |
| `rooms::tool` | what a tool implements so the shared room can open its rooms and join people into them by name |
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

`keys` lists every key the room hands out besides its own: the join key,
the wall key, each player's key. the store keeps an index from them to
the room, so `Rooms::locate` finds a player's room in one lookup instead
of walking every room under the lock.

## the lobby

`rooms::lobby` is the joining, presence and kicking every game had its
own copy of. a game keeps a `Lobby` in its room as a field of its own
(`#[serde(default)] lobby: Lobby`, not flattened: serde's flatten cannot
read maps with number keys, so a flattened room saves but never loads),
calls `lobby::migrate` from `Room::migrate` so that rooms saved with
`join` and `players` at the top level load, and implements `HasLobby`.
`lobby::routes(rooms)` then serves joining, the join count, the host's
"remove" and the wall, and merges into a router with any state.

- a name held by somebody whose phone is connected is refused: "That
  name is taken." a name held by somebody whose phone has gone gets that
  player's key back, and a phone that kept its key always gets back in.
  names are compared without case or extra spaces.
- a player is connected while their event stream is open: the stream
  marks them seen four times a second, and the host sees a dot by each
  name.
- the host can take somebody out, by an id that is not their key. their
  stream closes, their link stops working, and their name is free again.
  `HasLobby::removable` can refuse it for now; `HasLobby::left` is told
  who left.
- the host can also take out everybody away at once
  (`/api/h/<key>/remove-away`): their phone is not connected and
  `HasLobby::recent` says they have not played in this round or the one
  before. the game says what counts; by default nobody is away. nobody
  leaves the count on their own.
- `HasLobby::arrived` is the game's hook for seating somebody new: deal
  them a card, put them on a team. the seat it returns is kept with them.
- the wall, `/w/<key>`, is a read-only page for a projector: a large QR
  code and the join link, who is in and connected, and whatever the game
  puts there with `HasLobby::wall_game`. `HasLobby::on_wall` lets the
  game hide the code, the names or who is connected, and say when it is
  busy and should have the screen. it has a key of its own, which grants
  nothing, because a projected address bar can be photographed: the
  host's key never goes on the wall. a wall left on does not keep a room
  alive.

a state file that will not load is never overwritten: it is copied to
`<file>.bad-<ms>`, the error is printed, and the rooms that did load are
kept.

`rooms-client` has the screens to go with it: `wall`, `roster` (with
a "Remove N away" button given `away`), `waitingLine` (the host's "18 of
25 picked · waiting on Ada, Bo and 3 others · 2 not connected"),
`shareLink`, `wallButton`, `keepAwake`, `showConnection` (a
"Reconnecting…" notice) and `confirmTap` (press again to confirm).
they bring their own styles, so a page without the shared stylesheet
can use them.

## one room for every game

`hall/` is a server that runs [petrov](https://github.com/float3/petrov),
[numbers](https://github.com/float3/numbers),
[estimate](https://github.com/float3/estimate) and
[updating](https://github.com/float3/updating) together, each under its
own path, with a room of its own on top. The room has one join link and
one QR code for the whole evening. The host picks a game; the hall opens
a room in it, or goes back to the one it opened earlier, and joins
everybody in by name. Each phone shows the current game in a frame, and
the frame follows the host to the next game. Somebody who joins in the
middle of a game is put straight into it.

```sh
nix develop --command cargo run -p rooms-hall
```

listens on `127.0.0.1:8103` (`HALL_ADDR`) and keeps the halls and every
game's rooms in the directory `HALL_STATE`, one JSON file each. the flake
has a NixOS module for it, `services.rooms-hall`, with the same sandbox
as the games on their own.

petrov is in it with a seat per person: each name goes to the country
with fewer people in it.

the hall's people are a lobby too, with a wall at `/w/<key>` that shows
the code, who is in, and the current game's own wall in a frame. the
host can take somebody out of the hall and every game it seated them in.
when a game will not take somebody, their phone says why instead of
waiting.

## the browser code

`rooms-client` is the second crate. it holds the TypeScript and the
stylesheet the pages share: the event stream, the request helper,
number formatting, the QR code, the lobby's screens, and the styles for
panels, tables and buttons.

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
