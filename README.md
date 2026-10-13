# rooms

[![CI](https://github.com/float3/rooms/actions/workflows/ci.yml/badge.svg)](https://github.com/float3/rooms/actions/workflows/ci.yml)

The part every one of these tools needs: a room other people join from
their phones.

A host opens a room and gets two keys — one to run it, one to hand out.
Players join with the second, usually by scanning it off a screen. Every
phone holds an event stream open and sees the room change as it changes.
The rooms live in one JSON file so a restart does not throw an evening
away, and they are forgotten when nobody has looked at them for a while.

Used by [petrov](https://github.com/float3/petrov),
[estimation](https://github.com/float3/estimation) and
[number-games](https://github.com/float3/number-games).

## What it does and does not do

It does the parts that were going to be written three times:

| | |
| --- | --- |
| `Rooms<R>` | keyed store, capped, saved to one JSON file, swept when rooms go cold |
| `rooms::stream` | server-sent events: a view per watcher, resent only when it differs |
| `rooms::qr` | the join link as an SVG to put on the wall |
| `rooms::key` | 24 alphanumeric characters, which is the whole access model |
| `rooms::http` | the four responses all of these send |

What a room *is* belongs to the tool. Implement `Room` on it:

```rust
impl Room for Lobby {
    fn seen(&mut self, now: u64) { self.seen = now; }
    fn expired(&self, now: u64) -> bool { self.seen + rooms::DAY_MS < now }
}
```

`tick` is there for rooms that move on their own — a round that runs out
of time — and returns whether anything changed, so watchers are only woken
for something.

## Keys are the whole access model

A room key is 24 random alphanumeric characters and there are no accounts.
Anyone with the host key runs the room; anyone with the join key is in it.
That is enough for a room of people who are all looking at the same wall,
and it is not enough for anything else. Do not put something in a room you
would mind a stranger reading if they guessed 24 characters.

## Server-sent events, not websockets

Traffic only goes one way, it is plain HTTP so it survives whatever a
venue's wifi does to it, and the browser reconnects on its own. Watchers
wake on a shared 250 ms boundary rather than 250 ms after each one
connected, so twenty phones get the same frame at the same moment. An
unchanged view is resent every five seconds, which gives an idle proxy
something to not drop.

`stream` sets `X-Accel-Buffering: no`, without which nginx holds every
event until its buffer fills.

## One JSON file

Not a database and not trying to be: a few hundred rooms of a few
kilobytes, written by one process a handful of times a minute. Writes go
through a temporary file and a rename, so a crash mid-write loses the last
change rather than every room. A file that will not parse starts empty
rather than stopping the tool from booting — the alternative is refusing
to start because of an evening that is already over.

## Development

```sh
nix develop --command cargo test
```
