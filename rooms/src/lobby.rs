//! Who is in a room: their names and keys, whose phone is connected, the
//! host taking somebody out, and the wall a room is projected on.
//!
//! Every tool here had the same forty lines of joining by name, copied
//! four times. This is them once, with three rules the copies lacked:
//!
//! - A name held by somebody whose phone is connected is refused: "That
//!   name is taken." Two people called Alex no longer become one person
//!   who sees the other's answers.
//! - A name held by somebody whose phone is *not* connected gets that
//!   person's key back, so a phone that lost its link can rejoin. The
//!   usual way back is the key the phone stored, which always works.
//! - The host can take somebody out.
//!
//! A tool keeps a [`Lobby`] in its room, as a field of its own, and
//! implements [`HasLobby`]:
//!
//! ```ignore
//! #[derive(Serialize, Deserialize)]
//! struct Game {
//!     // A field of its own, not `#[serde(flatten)]`: a flattened struct
//!     // sends every field of the room through serde's buffer, which
//!     // cannot read a map with number keys, and the room would save but
//!     // never load again. `default` is for rooms saved before the lobby.
//!     #[serde(default)]
//!     lobby: Lobby,
//!     rounds: BTreeMap<u32, Round>,
//!     last_seen: u64,
//! }
//!
//! impl Room for Game {
//!     fn keys(&self) -> Vec<&str> {
//!         self.lobby.keys().collect()
//!     }
//!     // Rooms saved with `join` and `players` at the top level, before
//!     // the lobby or while it was flattened, load into `lobby`.
//!     fn migrate(saved: &mut serde_json::Value) {
//!         lobby::migrate(saved);
//!     }
//!     // seen, expired, tick as before
//! }
//!
//! impl HasLobby for Game {
//!     fn lobby(&self) -> &Lobby { &self.lobby }
//!     fn lobby_mut(&mut self) -> &mut Lobby { &mut self.lobby }
//!     // arrived, left, wall_game, on_wall, removable as the game needs
//! }
//!
//! let app = Router::new()
//!     .merge(site::routes_under(FILES, base))
//!     .merge(lobby::routes(rooms.clone()))
//!     .route("/api/p/{key}/events", get(|State(app): State<App>, Path(key): Path<String>| {
//!         lobby::player_events(app.rooms, key, |game, key, now| view::player(game, key, now))
//!     }))
//!     .with_state(app);
//! ```
//!
//! Who is connected is not stored anywhere. Each phone keeps an event
//! stream open; while it is open, [`player_events`] marks the player seen
//! four times a second, and a player seen in the last [`PRESENT_MS`] is
//! connected. What the host and the wall are sent says only whether they
//! are, not when they were last seen, so that a view changes when somebody
//! comes or goes and not four times a second.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Room, Rooms, http, key};

/// The most people one room takes.
pub const MAX_PLAYERS: usize = 200;
/// The longest name, in characters.
pub const MAX_NAME: usize = 32;
/// A phone seen this recently is connected. Streams look every 250 ms, so
/// this allows for a slow tick or two and nothing more.
pub const PRESENT_MS: u64 = 3_000;

/// One person in the room.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    /// A number for them that is not a secret, unlike their key: the host
    /// takes somebody out by it. Given when they join, never reused in
    /// the room, and given to people in rooms saved without one when the
    /// room loads.
    #[serde(default)]
    pub id: u32,
    /// Their key: the `/p/<key>` link their phone keeps. Only they and
    /// the server know it; no view sends it to anybody else.
    pub key: String,
    pub name: String,
    /// Where the tool seated them, if it seats people: a team, a side, a
    /// country. See [`HasLobby::arrived`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<u32>,
    /// When their phone last had a stream open, or when they joined.
    #[serde(default)]
    pub seen: u64,
}

/// Everybody in a room, and the keys that let people in and let a screen
/// watch.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "Saved")]
pub struct Lobby {
    /// The key in the join link and the QR code.
    pub join: String,
    /// The key in the wall's link, `/w/<key>`. It grants nothing: the
    /// wall only shows what everybody in the room may see. It is not the
    /// host's key, because the wall is projected and an address bar on a
    /// projector can be photographed. Rooms saved before there was a wall
    /// are given one when they load.
    pub wall: String,
    /// In the order they joined.
    pub players: Vec<Player>,
    /// The [`Player::id`] the next person gets.
    next: u32,
}

/// A lobby as saved, which may predate wall keys and ids.
#[derive(Deserialize)]
struct Saved {
    #[serde(default = "key::new")]
    join: String,
    #[serde(default = "key::new")]
    wall: String,
    /// The hall saved these as `people`.
    #[serde(default, alias = "people")]
    players: Vec<Player>,
    #[serde(default)]
    next: u32,
}

impl From<Saved> for Lobby {
    fn from(saved: Saved) -> Lobby {
        let mut lobby = Lobby {
            join: saved.join,
            wall: saved.wall,
            players: saved.players,
            next: saved.next.max(1),
        };
        lobby.number();
        lobby
    }
}

impl Default for Lobby {
    fn default() -> Self {
        Self::new()
    }
}

/// Somebody as the host and the wall see them: no key.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Present<'a> {
    /// What the host's Remove button sends: see [`Player::id`].
    pub id: u32,
    pub name: &'a str,
    /// Whether their phone is connected now. Left out where who is
    /// connected is not to be shown: see [`OnWall::presence`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat: Option<u32>,
}

/// How much of the room the wall shows of the people in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum People {
    /// Everybody by name.
    Names,
    /// How many there are, and no names.
    Count,
    /// Nothing about them.
    Hidden,
}

/// What the wall shows besides the game's own part, which the game
/// decides with [`HasLobby::on_wall`]. The default shows everything.
///
/// Petrov Day, for one, hides who is connected while it is live, because
/// a phone dropping out tells the room which country is still up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnWall {
    /// The QR code and the join link.
    pub join: bool,
    /// The people: by name, as a count, or not at all.
    pub people: People,
    /// Whose phone is connected, and how many are.
    pub presence: bool,
    /// Something is under way: the game takes the screen, the QR code
    /// shrinks to a corner for latecomers, and the names give way to a
    /// count.
    pub busy: bool,
}

impl Default for OnWall {
    fn default() -> Self {
        OnWall {
            join: true,
            people: People::Names,
            presence: true,
            busy: false,
        }
    }
}

/// What the wall is sent. What [`OnWall`] allows of the room, and whatever
/// the tool decides is public ([`HasLobby::wall_game`]); never anybody's
/// answer.
#[derive(Debug, Serialize)]
pub struct Wall<'a> {
    /// The join key, for the QR code. The join link is on the wall
    /// anyway, so this tells a viewer nothing new. `None` hides the code.
    pub join: Option<&'a str>,
    /// Everybody by name, or `None` when the names are not shown.
    pub people: Option<Vec<Present<'a>>>,
    /// How many are in, or `None` when that is not shown either.
    pub count: Option<usize>,
    /// How many of them are connected, or `None` when that is not shown.
    pub connected: Option<usize>,
    /// See [`OnWall::busy`].
    pub busy: bool,
    pub game: Value,
}

/// Trims, collapses inner whitespace, and cuts to `max` characters rather
/// than bytes, so names in any script survive.
pub fn tidy(text: &str, max: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// Two names that people would take for the same name.
fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// What [`Lobby::let_in`] decided.
#[derive(Debug, PartialEq)]
pub enum Admitted {
    /// Somebody new, under a new key.
    New(String),
    /// Somebody already here, back under their key.
    Back(String),
}

impl Admitted {
    pub fn key(&self) -> &str {
        match self {
            Admitted::New(key) | Admitted::Back(key) => key,
        }
    }
}

impl Lobby {
    /// An empty room with fresh join and wall keys.
    pub fn new() -> Lobby {
        Lobby {
            join: key::new(),
            wall: key::new(),
            players: Vec::new(),
            next: 1,
        }
    }

    /// Gives everybody without an id, or with one somebody else has, an
    /// id of their own: people in rooms saved before ids, or pushed onto
    /// [`Lobby::players`] by hand.
    fn number(&mut self) {
        let mut taken = std::collections::HashSet::new();
        let highest = self.players.iter().map(|p| p.id).max().unwrap_or(0);
        self.next = self.next.max(highest.saturating_add(1)).max(1);
        for player in &mut self.players {
            if player.id == 0 || !taken.insert(player.id) {
                player.id = self.next;
                self.next += 1;
                taken.insert(player.id);
            }
        }
    }

    pub fn player(&self, key: &str) -> Option<&Player> {
        self.players.iter().find(|p| p.key == key)
    }

    /// The player with this [`Player::id`].
    pub fn by_id(&self, id: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id && id != 0)
    }

    /// The player with this name, however it is capitalised or spaced.
    pub fn named(&self, name: &str) -> Option<&Player> {
        let name = tidy(name, MAX_NAME);
        self.players.iter().find(|p| same(&p.name, &name))
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// The join key, the wall key and every player's key, for
    /// [`Room::keys`].
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        [self.join.as_str(), self.wall.as_str()]
            .into_iter()
            .chain(self.players.iter().map(|p| p.key.as_str()))
    }

    /// Whether `key`'s phone is connected.
    pub fn connected(&self, key: &str, now: u64) -> bool {
        self.player(key).is_some_and(|p| present(p, now))
    }

    /// Marks `key` as connected now. False when there is no such player,
    /// which is how a stream learns its player was taken out.
    pub fn touch(&mut self, key: &str, now: u64) -> bool {
        match self.players.iter_mut().find(|p| p.key == key) {
            Some(player) => {
                player.seen = player.seen.max(now);
                true
            }
            None => false,
        }
    }

    /// Everybody, without keys, in the order they joined, with whether
    /// each is connected.
    pub fn roster(&self, now: u64) -> Vec<Present<'_>> {
        self.players
            .iter()
            .map(|p| Present {
                id: p.id,
                name: &p.name,
                connected: Some(present(p, now)),
                seat: p.seat,
            })
            .collect()
    }

    /// Everybody, without keys and without saying who is connected.
    pub fn names(&self) -> Vec<Present<'_>> {
        self.players
            .iter()
            .map(|p| Present {
                id: p.id,
                name: &p.name,
                connected: None,
                seat: p.seat,
            })
            .collect()
    }

    /// [`Lobby::roster`] when `presence`, [`Lobby::names`] when not.
    pub fn people(&self, now: u64, presence: bool) -> Vec<Present<'_>> {
        if presence {
            self.roster(now)
        } else {
            self.names()
        }
    }

    /// Somebody new, under a fresh key and the next id, without the join
    /// rules or [`HasLobby::arrived`]: for moving people seated by an older
    /// version of a tool into its lobby. `None` for an empty name, a name
    /// already here, or a full room. They count as not connected.
    pub fn add(&mut self, name: &str, seat: Option<u32>) -> Option<&Player> {
        let name = tidy(name, MAX_NAME);
        if name.is_empty() || self.named(&name).is_some() || self.players.len() >= MAX_PLAYERS {
            return None;
        }
        self.push(name, seat, 0);
        self.players.last()
    }

    fn push(&mut self, name: String, seat: Option<u32>, seen: u64) -> String {
        let key = key::new();
        self.number();
        self.players.push(Player {
            id: self.next,
            key: key.clone(),
            name,
            seat,
            seen,
        });
        self.next += 1;
        key
    }

    /// The join rules, without seating anybody. Most tools want [`admit`],
    /// which also runs [`HasLobby::arrived`].
    ///
    /// `known` is a key the phone (or the hall) was given here before.
    /// If it is still somebody's, that is who this is, whatever name came
    /// with it. Otherwise the name decides: a new name is let in, a name
    /// whose phone is connected is refused, and a name whose phone is not
    /// gets that player's key back.
    pub fn let_in(
        &mut self,
        name: &str,
        known: Option<&str>,
        now: u64,
    ) -> Result<Admitted, &'static str> {
        if let Some(player) = known.and_then(|k| self.players.iter_mut().find(|p| p.key == k)) {
            player.seen = player.seen.max(now);
            return Ok(Admitted::Back(player.key.clone()));
        }
        let name = tidy(name, MAX_NAME);
        if name.is_empty() {
            return Err("Please give a name, so the room knows who you are.");
        }
        if let Some(player) = self.players.iter_mut().find(|p| same(&p.name, &name)) {
            if present(player, now) {
                return Err("That name is taken. Add an initial, or pick another.");
            }
            player.seen = now;
            return Ok(Admitted::Back(player.key.clone()));
        }
        if self.players.len() >= MAX_PLAYERS {
            return Err("This room is full.");
        }
        // Counted as connected from the moment they join, so a second
        // phone typing the same name in the next breath is refused before
        // the first one's stream has opened.
        Ok(Admitted::New(self.push(name, None, now)))
    }

    /// Takes somebody out of the lobby alone. Their phone's stream closes
    /// and their link stops working; the join link lets them back in as
    /// somebody new. A tool wants [`remove_key`], which also tells it.
    pub fn remove(&mut self, key: &str) -> Option<Player> {
        let at = self.players.iter().position(|p| p.key == key)?;
        Some(self.players.remove(at))
    }

    /// The wall's view: what `shows` allows of the room, with `game` as
    /// the tool's part of it.
    pub fn wall_view<'a>(&'a self, now: u64, shows: OnWall, game: Value) -> Wall<'a> {
        let counted = shows.people != People::Hidden;
        Wall {
            join: shows.join.then_some(self.join.as_str()),
            people: (shows.people == People::Names).then(|| self.people(now, shows.presence)),
            count: counted.then_some(self.players.len()),
            connected: (counted && shows.presence)
                .then(|| self.players.iter().filter(|p| present(p, now)).count()),
            busy: shows.busy,
            game,
        }
    }
}

fn present(player: &Player, now: u64) -> bool {
    now.saturating_sub(player.seen) < PRESENT_MS
}

/// Moves a lobby saved with its fields at the top level of the room, as
/// `join`, `wall` and `players` (or the hall's `people`), into a `lobby`
/// field. That is how rooms were saved before the lobby, and while the
/// lobby was `#[serde(flatten)]`ed into them. Call it from
/// [`Room::migrate`]. A room that already has a `lobby`, or never had a
/// `join`, is left alone: `#[serde(default)]` on the field gives the
/// second a fresh lobby.
///
/// A tool whose old `players` held more than a lobby's player does, such
/// as each player's answers, takes what it needs from them first: after
/// this they are in `lobby.players`, where only the lobby's fields of
/// them are read.
pub fn migrate(saved: &mut Value) {
    let Some(room) = saved.as_object_mut() else {
        return;
    };
    if room.contains_key("lobby") || !room.contains_key("join") {
        return;
    }
    let mut lobby = serde_json::Map::new();
    for field in ["join", "wall", "players", "people", "next"] {
        if let Some(value) = room.remove(field) {
            lobby.insert(field.to_string(), value);
        }
    }
    room.insert("lobby".into(), Value::Object(lobby));
}

/// A room with a [`Lobby`] in it.
pub trait HasLobby: Room {
    fn lobby(&self) -> &Lobby;
    fn lobby_mut(&mut self) -> &mut Lobby;

    /// Somebody new has been let in under `key`. Set up what the tool
    /// keeps for them, such as dealing a card, and answer with their seat,
    /// such as a team, or `None` for a tool without seats. Their seat is
    /// kept in the lobby, so this can count who is seated where from
    /// [`Lobby::players`]. An error takes them back out and is shown on
    /// their phone.
    ///
    /// Called once per person. Somebody rejoining keeps their seat.
    fn arrived(&mut self, _key: &str, _now: u64) -> Result<Option<u32>, &'static str> {
        Ok(None)
    }

    /// `player` has been taken out, and is no longer in the lobby. Forget
    /// what the tool keeps for them, or keep it for the scores: the tool
    /// decides. Called by [`remove_key`] and [`remove`].
    fn left(&mut self, _player: &Player) {}

    /// Whether the host may take `player` out now, from the host's screen
    /// ([`remove`], `POST /api/h/<key>/remove`). An error is shown to the
    /// host. The default always may. The shared room takes people out
    /// through [`remove_key`], which does not ask: somebody it has taken
    /// out of the room is out of every game.
    fn removable(&self, _player: &Player) -> Result<(), &'static str> {
        Ok(())
    }

    /// The tool's part of the wall: what the projector shows besides the
    /// QR code and the names. Everybody in the room can see the wall, so
    /// put nothing on it the players should not see yet. `Null`, the
    /// default, shows nothing.
    fn wall_game(&self, _now: u64) -> Value {
        Value::Null
    }

    /// What else the wall shows: the QR code, the names, who is connected,
    /// and whether the game should have the screen. The default shows
    /// everything and is never busy. The shared room's wall and its host's
    /// list of people follow the game it is playing in this too.
    fn on_wall(&self, _now: u64) -> OnWall {
        OnWall::default()
    }
}

/// Lets somebody in by the lobby's rules and seats them. Answers with
/// their key. This is what `Joinable::join` and `Joinable::rejoin` call.
pub fn admit<R: HasLobby>(
    room: &mut R,
    name: &str,
    known: Option<&str>,
    now: u64,
) -> Result<String, &'static str> {
    match room.lobby_mut().let_in(name, known, now)? {
        Admitted::Back(key) => Ok(key),
        Admitted::New(key) => match room.arrived(&key, now) {
            Ok(seat) => {
                if let Some(player) = room.lobby_mut().players.iter_mut().find(|p| p.key == key) {
                    player.seat = seat;
                }
                Ok(key)
            }
            Err(message) => {
                room.lobby_mut().remove(&key);
                Err(message)
            }
        },
    }
}

/// Takes the player with `key` out and tells the tool
/// ([`HasLobby::left`]). Answers with who they were, or `None` if nobody
/// has that key. This is what `Joinable::leave` calls.
pub fn remove_key<R: HasLobby>(room: &mut R, key: &str) -> Option<Player> {
    let player = room.lobby_mut().remove(key)?;
    room.left(&player);
    Some(player)
}

/// The host takes somebody out by their [`Player::id`], which is what the
/// host's screen knows them by, if the tool allows it now
/// ([`HasLobby::removable`]). Answers with who they were.
pub fn remove<R: HasLobby>(room: &mut R, id: u32) -> Result<Player, &'static str> {
    let player = room
        .lobby()
        .by_id(id)
        .ok_or("That person is not in the room any more.")?;
    room.removable(player)?;
    let key = player.key.clone();
    remove_key(room, &key).ok_or("That person is not in the room any more.")
}

/// The room `key` belongs to: through the index, or, for a tool whose
/// [`Room::keys`] does not list its lobby's keys, by looking.
pub async fn room_of<R: HasLobby>(rooms: &Rooms<R>, key: &str) -> Option<String> {
    if let Some(room) = rooms.locate(key) {
        return Some(room);
    }
    let found = rooms
        .find(|own, room| {
            let lobby = room.lobby();
            (lobby.join == key || lobby.wall == key || lobby.player(key).is_some())
                .then(|| own.to_string())
        })
        .await;
    if found.is_some() {
        eprintln!("rooms: a room's keys are not indexed; list the lobby's in Room::keys");
    }
    found
}

fn envelope_raw(now: u64, view: &str) -> String {
    format!("{{\"now\":{now},\"view\":{view}}}")
}

/// A player's view, once: for the page's first draw, and for a join page
/// asking whether a kept key still works. `render` is given the room, the
/// player's key and the time, and answers with the view as JSON, or
/// `None` for a key the room does not know. The room is moved on to now
/// first, so the view is not stale, and the player counts as connected.
pub async fn player_state<R: HasLobby>(
    rooms: &Rooms<R>,
    key: &str,
    render: impl FnOnce(&R, &str, u64) -> Option<String>,
) -> Response {
    let Some(at) = room_of(rooms, key).await else {
        return http::gone();
    };
    match rooms
        .with(&at, |room, now| {
            if !room.lobby_mut().touch(key, now) {
                return None;
            }
            render(room, key, now).map(|view| envelope_raw(now, &view))
        })
        .await
        .flatten()
    {
        Some(body) => http::json(body),
        None => http::gone(),
    }
}

/// A player's stream. Their room is found once, when the phone connects,
/// not on every tick. While it is open they count as connected; when the
/// host takes them out it closes.
pub async fn player_events<R: HasLobby>(
    rooms: Arc<Rooms<R>>,
    key: String,
    render: impl Fn(&R, &str, u64) -> Option<String> + Send + 'static,
) -> Response {
    let Some(at) = room_of(&rooms, &key).await else {
        return http::gone();
    };
    let saver = rooms.clone();
    crate::stream(rooms, move |held, now| {
        let room = held.get_mut(&at)?;
        if !room.lobby_mut().touch(&key, now) {
            return None;
        }
        room.seen(now);
        let changed = room.tick(now);
        let view = render(room, &key, now);
        if changed {
            saver.save(held);
        }
        view
    })
}

/// A player acts: finds their room, applies `action`, and answers with
/// what `reply` renders, usually the player's view in an envelope. A
/// player who acts is connected, whatever their stream is doing.
pub async fn act_as<R: HasLobby>(
    rooms: &Rooms<R>,
    key: &str,
    action: impl FnOnce(&mut R, &str, u64) -> Result<(), &'static str>,
    reply: impl FnOnce(&R, &str, u64) -> String,
) -> Response {
    let Some(at) = room_of(rooms, key).await else {
        return http::gone();
    };
    let done = rooms
        .with(&at, |room, now| {
            if !room.lobby_mut().touch(key, now) {
                return None;
            }
            Some(action(room, key, now).map(|()| reply(room, key, now)))
        })
        .await
        .flatten();
    match done {
        None => http::gone(),
        Some(Err(message)) => http::refused(message),
        Some(Ok(body)) => http::json(body),
    }
}

/// What a phone sends to join.
#[derive(Deserialize)]
pub struct Joining {
    pub name: String,
    /// The key this phone was given here before, if it kept one.
    #[serde(default)]
    pub key: Option<String>,
}

/// What a host sends to take somebody out: their [`Player::id`], from
/// [`Present::id`].
#[derive(Deserialize)]
pub struct Removing {
    pub id: u32,
}

/// `GET /api/j/<join>`: how many are in, which the join page shows.
pub async fn count<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(join): Path<String>,
) -> Response {
    let Some(at) = room_of(&rooms, &join).await else {
        return http::gone();
    };
    match rooms
        .read(&at, |room, _| {
            (room.lobby().join == join).then(|| room.lobby().len())
        })
        .await
        .flatten()
    {
        Some(players) => Json(serde_json::json!({ "players": players })).into_response(),
        None => http::gone(),
    }
}

/// `POST /api/j/<join>` with `{name, key?}`: answers `{player: <key>}`.
pub async fn join<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(join): Path<String>,
    Json(joining): Json<Joining>,
) -> Response {
    let Some(at) = room_of(&rooms, &join).await else {
        return http::gone();
    };
    let done = rooms
        .with(&at, |room, now| {
            (room.lobby().join == join)
                .then(|| admit(room, &joining.name, joining.key.as_deref(), now))
        })
        .await
        .flatten();
    match done {
        None => http::gone(),
        Some(Err(message)) => http::refused(message),
        Some(Ok(player)) => Json(serde_json::json!({ "player": player })).into_response(),
    }
}

/// `POST /api/h/<host>/remove` with `{id}`: takes them out, if the tool
/// allows it now. Answers 204 and nothing else: the host's own stream
/// brings its view, which is the tool's to render and may not show who is
/// connected.
pub async fn kick<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(host): Path<String>,
    Json(removing): Json<Removing>,
) -> Response {
    match rooms.with(&host, |room, _| remove(room, removing.id)).await {
        None => http::gone(),
        Some(Err(message)) => http::refused(message),
        Some(Ok(_)) => StatusCode::NO_CONTENT.into_response(),
    }
}

/// The wall's view of a room as JSON: [`HasLobby::on_wall`] decides what
/// of the room is shown, [`HasLobby::wall_game`] the game's part.
pub fn wall_json<R: HasLobby>(room: &R, now: u64) -> Option<String> {
    let view = room
        .lobby()
        .wall_view(now, room.on_wall(now), room.wall_game(now));
    serde_json::to_string(&view).ok()
}

/// `GET /api/w/<wall>`: the wall's view, once, moved on to now. Looking
/// does not keep the room alive.
pub async fn wall_state<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(wall): Path<String>,
) -> Response {
    let Some(at) = room_of(&rooms, &wall).await else {
        return http::gone();
    };
    match rooms
        .look(&at, |room, now| {
            (room.lobby().wall == wall)
                .then(|| wall_json(room, now).map(|view| envelope_raw(now, &view)))
                .flatten()
        })
        .await
        .flatten()
    {
        Some(body) => http::json(body),
        None => http::gone(),
    }
}

/// `GET /api/w/<wall>/events`: the wall's stream. A wall left on does not
/// keep the room alive; when the room expires, the stream closes.
pub async fn wall_events<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(wall): Path<String>,
) -> Response {
    let Some(at) = room_of(&rooms, &wall).await else {
        return http::gone();
    };
    crate::one_unseen(rooms, at, move |room, now| {
        if room.lobby().wall != wall {
            return None;
        }
        wall_json(room, now)
    })
}

/// The lobby's routes, with `rooms` as their state, so that they merge
/// into a router with any state of the tool's own:
///
/// - `GET /api/j/{key}` and `POST /api/j/{key}`: how many are in, and joining
/// - `POST /api/h/{key}/remove`: the host takes somebody out
/// - `GET /api/w/{key}` and `/api/w/{key}/events`: the wall
///
/// ```ignore
/// let app = Router::new()
///     .route("/api/h/{key}", get(host_state))
///     .merge(lobby::routes(app_state.games.clone()))
///     .with_state(app_state);
/// ```
///
/// The page at `/w/{key}` is in [`crate::site::routes`].
pub fn routes<R: HasLobby, S: Clone + Send + Sync + 'static>(rooms: Arc<Rooms<R>>) -> Router<S> {
    Router::new()
        .route("/api/j/{key}", get(count::<R>).post(join::<R>))
        .route("/api/h/{key}/remove", post(kick::<R>))
        .route("/api/w/{key}", get(wall_state::<R>))
        .route("/api/w/{key}/events", get(wall_events::<R>))
        .with_state(rooms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, header},
    };
    use std::collections::BTreeMap;
    use tower::ServiceExt;

    #[derive(Serialize, Deserialize)]
    struct Toy {
        #[serde(default)]
        lobby: Lobby,
        /// Which team each player is on, as a tool would keep it.
        #[serde(default)]
        dealt: Vec<String>,
        /// Who left, by name, as `left` was told.
        #[serde(default)]
        gone: Vec<String>,
        #[serde(default)]
        secret: u32,
        /// Number keys, which a flattened lobby made unloadable.
        #[serde(default)]
        rounds: BTreeMap<u32, String>,
        #[serde(default)]
        seen: u64,
        #[serde(default)]
        ticks: u64,
        /// Whether the game is under way, which hides who is connected.
        #[serde(default)]
        live: bool,
    }

    impl Room for Toy {
        fn seen(&mut self, now: u64) {
            self.seen = now;
        }
        fn tick(&mut self, _now: u64) -> bool {
            self.ticks += 1;
            false
        }
        fn expired(&self, _now: u64) -> bool {
            false
        }
        fn keys(&self) -> Vec<&str> {
            self.lobby.keys().collect()
        }
        fn migrate(saved: &mut Value) {
            migrate(saved);
        }
    }

    impl HasLobby for Toy {
        fn lobby(&self) -> &Lobby {
            &self.lobby
        }
        fn lobby_mut(&mut self) -> &mut Lobby {
            &mut self.lobby
        }
        /// Two teams, kept even, as Petrov Day's countries are.
        fn arrived(&mut self, key: &str, _now: u64) -> Result<Option<u32>, &'static str> {
            if self.lobby.named("Nobody").is_some_and(|p| p.key == key) {
                return Err("Not that name.");
            }
            self.dealt.push(key.to_string());
            let first = self
                .lobby
                .players
                .iter()
                .filter(|p| p.seat == Some(0))
                .count();
            let second = self
                .lobby
                .players
                .iter()
                .filter(|p| p.seat == Some(1))
                .count();
            Ok(Some(u32::from(second < first)))
        }
        fn left(&mut self, player: &Player) {
            assert!(
                self.lobby.player(&player.key).is_none(),
                "told once they are out"
            );
            self.dealt.retain(|k| *k != player.key);
            self.gone.push(player.name.clone());
        }
        fn removable(&self, _player: &Player) -> Result<(), &'static str> {
            if self.live {
                return Err("Not while the game is live.");
            }
            Ok(())
        }
        fn wall_game(&self, _now: u64) -> Value {
            serde_json::json!({ "round": 1 })
        }
        fn on_wall(&self, _now: u64) -> OnWall {
            if self.live {
                OnWall {
                    join: false,
                    people: People::Count,
                    presence: false,
                    busy: true,
                }
            } else {
                OnWall::default()
            }
        }
    }

    fn toy() -> Toy {
        Toy {
            lobby: Lobby::new(),
            dealt: Vec::new(),
            gone: Vec::new(),
            secret: 7,
            rounds: BTreeMap::new(),
            seen: 0,
            ticks: 0,
            live: false,
        }
    }

    const T: u64 = 1_000_000;

    fn temp(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("rooms-lobby-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_name_held_by_a_connected_phone_is_refused() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        assert_eq!(
            admit(&mut room, " ada ", None, T + 1_000),
            Err("That name is taken. Add an initial, or pick another."),
            "the same name, however it is typed, while Ada is here"
        );
        room.lobby.touch(&ada, T + 2_000);
        assert!(admit(&mut room, "ADA", None, T + 4_000).is_err());
        assert_eq!(room.lobby.len(), 1);
        assert_eq!(
            admit(&mut room, "Ada B", None, T).map(|k| k != ada),
            Ok(true)
        );
    }

    #[test]
    fn a_name_whose_phone_has_gone_gets_its_key_back() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        let later = T + PRESENT_MS;
        assert!(!room.lobby.connected(&ada, later));
        assert_eq!(admit(&mut room, "Ada", None, later), Ok(ada.clone()));
        assert_eq!(room.dealt.len(), 1, "a rejoin is not dealt in twice");
        assert_eq!(room.lobby.len(), 1);
    }

    #[test]
    fn a_kept_key_always_gets_back_in() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        assert_eq!(admit(&mut room, "Ada", Some(&ada), T + 1), Ok(ada.clone()));
        assert_eq!(
            admit(&mut room, "Whatever", Some(&ada), T + 1),
            Ok(ada.clone()),
            "the key decides, not the name"
        );
        assert!(admit(&mut room, "Bo", Some("not-a-key"), T).is_ok());
        assert_eq!(room.lobby.len(), 2);
    }

    #[test]
    fn empty_and_full_are_refused() {
        let mut room = toy();
        assert!(admit(&mut room, "   ", None, T).is_err());
        for i in 0..MAX_PLAYERS {
            admit(&mut room, &format!("P{i}"), None, T).unwrap();
        }
        assert_eq!(
            admit(&mut room, "One more", None, T),
            Err("This room is full.")
        );
    }

    #[test]
    fn the_tool_seats_people_and_can_turn_them_away() {
        let mut room = toy();
        for name in ["A", "B", "C", "D"] {
            admit(&mut room, name, None, T).unwrap();
        }
        let seats: Vec<_> = room.lobby.players.iter().map(|p| p.seat).collect();
        assert_eq!(seats, [Some(0), Some(1), Some(0), Some(1)]);

        assert_eq!(admit(&mut room, "Nobody", None, T), Err("Not that name."));
        assert!(room.lobby.named("Nobody").is_none(), "taken back out");
    }

    #[test]
    fn presence_follows_the_stream() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        let bo = admit(&mut room, "Bo", None, T).unwrap();
        let now = T + 10_000;
        assert!(room.lobby.touch(&bo, now));
        assert!(!room.lobby.touch("nobody", now));
        let roster = room.lobby.roster(now);
        assert_eq!(
            roster
                .iter()
                .map(|p| (p.name, p.connected))
                .collect::<Vec<_>>(),
            [("Ada", Some(false)), ("Bo", Some(true))]
        );
        assert!(!room.lobby.connected(&ada, now));
        let json = serde_json::to_string(&roster).unwrap();
        assert!(
            !json.contains(&ada) && !json.contains(&bo),
            "no keys: {json}"
        );
    }

    #[test]
    fn what_is_sent_about_people_changes_when_they_come_and_go_and_not_every_tick() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        let wall = |room: &Toy, now| wall_json(room, now).unwrap();
        let host = |room: &Toy, now| serde_json::to_string(&room.lobby.roster(now)).unwrap();
        let (first_wall, first_host) = (wall(&room, T), host(&room, T));
        // Her stream marks her seen four times a second.
        for tick in 1..=8 {
            let now = T + tick * 250;
            room.lobby.touch(&ada, now);
            assert_eq!(wall(&room, now), first_wall, "the wall at tick {tick}");
            assert_eq!(host(&room, now), first_host, "the host at tick {tick}");
        }
        // Her phone goes: that is a change, once.
        let gone = T + 2_000 + PRESENT_MS;
        assert_ne!(wall(&room, gone), first_wall);
        assert_eq!(wall(&room, gone), wall(&room, gone + 10_000));
    }

    #[test]
    fn people_have_ids_that_are_not_their_keys_and_are_not_reused() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        let bo = admit(&mut room, "Bo", None, T).unwrap();
        let ids: Vec<u32> = room.lobby.players.iter().map(|p| p.id).collect();
        assert_eq!(ids, [1, 2]);
        remove_key(&mut room, &bo).unwrap();
        let cy = admit(&mut room, "Cy", None, T).unwrap();
        assert_eq!(room.lobby.player(&cy).unwrap().id, 3, "Bo's id stays his");
        assert_eq!(room.lobby.by_id(1).unwrap().key, ada);
        assert!(room.lobby.by_id(2).is_none());
        assert!(room.lobby.by_id(0).is_none());
    }

    #[test]
    fn the_host_takes_somebody_out_by_id_and_the_tool_is_told_who() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        admit(&mut room, "Bo", None, T).unwrap();
        let id = room.lobby.player(&ada).unwrap().id;
        assert_eq!(remove(&mut room, id).map(|p| p.key), Ok(ada.clone()));
        assert!(room.lobby.player(&ada).is_none());
        assert!(!room.dealt.contains(&ada), "and the tool forgot them");
        assert_eq!(room.gone, ["Ada"], "told who, after they were out");
        assert!(!room.lobby.touch(&ada, T), "so their stream closes");
        assert!(remove(&mut room, id).is_err());
        let back = admit(&mut room, "Ada", Some(&ada), T).unwrap();
        assert_ne!(back, ada, "the old key no longer gets in");
    }

    #[test]
    fn the_tool_can_refuse_the_host_but_not_the_shared_room() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        room.live = true;
        let id = room.lobby.player(&ada).unwrap().id;
        assert_eq!(
            remove(&mut room, id).map(|p| p.key),
            Err("Not while the game is live.")
        );
        assert!(room.lobby.player(&ada).is_some());
        assert_eq!(
            remove_key(&mut room, &ada).map(|p| p.name),
            Some("Ada".into())
        );
        assert_eq!(room.gone, ["Ada"]);
        assert!(remove_key(&mut room, &ada).is_none());
    }

    #[test]
    fn rooms_saved_before_the_lobby_or_with_it_flattened_load_into_it() {
        let migrated = |text: &str| {
            let mut saved: Value = serde_json::from_str(text).unwrap();
            <Toy as Room>::migrate(&mut saved);
            serde_json::from_value::<Toy>(saved).unwrap()
        };
        // A numbers or estimate room as it was saved.
        let room = migrated(r#"{"join":"J","players":[{"key":"K","name":"Ada"}],"secret":3}"#);
        assert_eq!(room.lobby.join, "J");
        assert_eq!(room.lobby.player("K").unwrap().name, "Ada");
        assert_eq!(room.lobby.player("K").unwrap().id, 1, "given an id");
        assert_eq!(room.lobby.wall.len(), key::LEN, "a wall key is made");
        assert_eq!(room.secret, 3);
        // The hall's.
        let room = migrated(r#"{"join":"J","people":[{"key":"K","name":"Ada"}]}"#);
        assert_eq!(room.lobby.len(), 1);
        // With the lobby flattened in, as rooms#18 saved them.
        let room = migrated(
            r#"{"join":"J","wall":"W","players":[{"key":"K","name":"Ada","seen":5}],"secret":4}"#,
        );
        assert_eq!((room.lobby.wall.as_str(), room.secret), ("W", 4));
        // Before there was any join key: a fresh lobby.
        let room = migrated(r#"{"secret":5}"#);
        assert!(room.lobby.is_empty() && room.lobby.join.len() == key::LEN);
        // And as it is saved now, untouched.
        let now = serde_json::to_string(&room).unwrap();
        let again = migrated(&now);
        assert_eq!(again.lobby.join, room.lobby.join);
    }

    #[test]
    fn an_old_room_with_two_people_one_name_apart_in_case_can_lose_either() {
        // Saved before names were compared ignoring case, so both got in.
        let mut saved: Value = serde_json::from_str(
            r#"{"join":"J","players":[{"key":"K1","name":"alex"},{"key":"K2","name":"Alex"}]}"#,
        )
        .unwrap();
        migrate(&mut saved);
        let mut room: Toy = serde_json::from_value(saved).unwrap();
        let ids: Vec<u32> = room.lobby.roster(0).iter().map(|p| p.id).collect();
        assert_eq!(ids, [1, 2]);
        assert_eq!(remove(&mut room, 2).unwrap().key, "K2", "the second Alex");
        assert_eq!(room.lobby.players[0].key, "K1");
    }

    #[tokio::test]
    async fn a_room_with_number_keys_survives_a_save_and_a_load() {
        let path = temp("numbered");
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let host = rooms
            .create(|_| {
                let mut room = toy();
                room.rounds.insert(1, "first".into());
                room.rounds.insert(20, "twentieth".into());
                room
            })
            .await
            .unwrap();
        let ada = rooms
            .with(&host, |room, now| admit(room, "Ada", None, now))
            .await
            .unwrap()
            .unwrap();

        let again: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let (rounds, name, id) = again
            .read(&host, |room, _| {
                let ada = room.lobby.player(&ada).unwrap();
                (room.rounds.clone(), ada.name.clone(), ada.id)
            })
            .await
            .expect("the room came back");
        assert_eq!(rounds.get(&20).map(String::as_str), Some("twentieth"));
        assert_eq!((name.as_str(), id), ("Ada", 1));
        assert_eq!(again.locate(&ada), Some(host.clone()), "indexed again");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_wall_shows_what_the_game_allows() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        admit(&mut room, "Bo", None, T - PRESENT_MS).unwrap();
        let open: Value = serde_json::from_str(&wall_json(&room, T).unwrap()).unwrap();
        assert_eq!(open["join"], room.lobby.join.as_str());
        assert_eq!(open["people"][0]["name"], "Ada");
        assert_eq!(open["people"][0]["connected"], true);
        assert_eq!(open["count"], 2);
        assert_eq!(open["connected"], 1);
        assert_eq!(open["busy"], false);
        assert_eq!(open["game"]["round"], 1);

        room.live = true;
        let text = wall_json(&room, T).unwrap();
        let live: Value = serde_json::from_str(&text).unwrap();
        assert!(live["join"].is_null(), "no QR code: {text}");
        assert!(live["people"].is_null(), "no names: {text}");
        assert_eq!(live["count"], 2);
        assert!(live["connected"].is_null(), "nobody's connection: {text}");
        assert_eq!(live["busy"], true);
        assert!(!text.contains("Ada") && !text.contains(&ada), "{text}");

        // Names without who is connected.
        let names = room.lobby.wall_view(
            T,
            OnWall {
                presence: false,
                ..OnWall::default()
            },
            Value::Null,
        );
        let text = serde_json::to_string(&names).unwrap();
        assert!(text.contains("Ada"), "{text}");
        assert!(
            !text.contains("connected\":true")
                && !text.contains("connected\":false")
                && text.contains("connected\":null"),
            "{text}"
        );
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let request = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The lobby's routes in a tool whose state is its own, which holds
    /// the rooms rather than being them.
    #[derive(Clone)]
    struct App {
        rooms: Arc<Rooms<Toy>>,
        name: &'static str,
    }

    #[tokio::test]
    async fn the_routes_join_count_remove_and_show_the_wall() {
        let path = temp("routes");
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let host = rooms.create(|_| toy()).await.unwrap();
        let (join, wall) = rooms
            .read(&host, |r, _| (r.lobby.join.clone(), r.lobby.wall.clone()))
            .await
            .unwrap();
        let state = App {
            rooms: rooms.clone(),
            name: "toy",
        };
        let app: Router = Router::new()
            .route(
                "/api/name",
                get(|State(app): State<App>| async move { app.name }),
            )
            .merge(routes(state.rooms.clone()))
            .with_state(state);
        let (status, _) = call(&app, "GET", "/api/name", None).await;
        assert_eq!(status, StatusCode::OK, "the tool's own routes still work");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/j/{join}"),
            Some(serde_json::json!({"name": "Ada"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let ada = body["player"].as_str().unwrap().to_string();
        assert_eq!(rooms.locate(&ada), Some(host.clone()), "indexed");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/j/{join}"),
            Some(serde_json::json!({"name": "ada"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(
            body["error"],
            "That name is taken. Add an initial, or pick another."
        );

        let (_, body) = call(&app, "GET", &format!("/api/j/{join}"), None).await;
        assert_eq!(body["players"], 1);

        let (status, body) = call(&app, "GET", &format!("/api/w/{wall}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["view"]["join"], join.as_str());
        assert_eq!(body["view"]["people"][0]["name"], "Ada");
        assert_eq!(body["view"]["game"]["round"], 1);
        let id = body["view"]["people"][0]["id"].as_u64().unwrap();
        let text = body.to_string();
        assert!(!text.contains(&ada) && !text.contains(&host), "{text}");
        assert!(!text.contains("secret"), "{text}");

        // Neither the join key nor the host key opens the wall, and the
        // wall key runs nothing.
        for key in [&join, &host, &ada] {
            let (status, _) = call(&app, "GET", &format!("/api/w/{key}"), None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{key}");
        }
        let (status, _) = call(
            &app,
            "POST",
            &format!("/api/h/{wall}/remove"),
            Some(serde_json::json!({"id": id})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "the wall key cannot remove");

        // While the game says not now, the host cannot take anybody out.
        rooms.with(&host, |r, _| r.live = true).await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/h/{host}/remove"),
            Some(serde_json::json!({"id": id})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"], "Not while the game is live.");
        assert!(body.get("view").is_none(), "and is told nothing else");
        rooms.with(&host, |r, _| r.live = false).await;

        let request = Request::post(format!("/api/h/{host}/remove"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::json!({"id": id}).to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 10)
            .await
            .unwrap();
        assert!(bytes.is_empty(), "no partial view for the page to take");
        assert_eq!(rooms.locate(&ada), None);
        let (status, _) = call(
            &app,
            "POST",
            &format!("/api/h/{host}/remove"),
            Some(serde_json::json!({"id": id})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "nobody left to take out");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn first_draws_are_moved_on_to_now_and_a_wall_keeps_nothing_alive() {
        let path = temp("first-draw");
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let host = rooms.create(|_| toy()).await.unwrap();
        let ada = rooms
            .with(&host, |r, now| admit(r, "Ada", None, now - PRESENT_MS))
            .await
            .unwrap()
            .unwrap();
        let wall = rooms
            .read(&host, |r, _| r.lobby.wall.clone())
            .await
            .unwrap();
        let (ticks, seen) = rooms.read(&host, |r, _| (r.ticks, r.seen)).await.unwrap();
        let app: Router = routes(rooms.clone());

        let (status, _) = call(&app, "GET", &format!("/api/w/{wall}"), None).await;
        assert_eq!(status, StatusCode::OK);
        let after = rooms.read(&host, |r, _| (r.ticks, r.seen)).await.unwrap();
        assert_eq!(
            after,
            (ticks + 1, seen),
            "ticked, and not counted as a visit"
        );

        let response = wall_events(State(rooms.clone()), Path(wall.clone())).await;
        let mut body = response.into_body();
        http_frame(&mut body).await;
        http_frame(&mut body).await;
        assert_eq!(
            rooms.read(&host, |r, _| r.seen).await.unwrap(),
            seen,
            "a wall left on does not keep the room alive"
        );
        drop(body);

        let response = player_state(&rooms, &ada, |r, key, _| {
            Some(format!("{}", r.lobby.player(key)?.id))
        })
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let (ticked, connected) = rooms
            .read(&host, |r, now| (r.ticks, r.lobby.connected(&ada, now)))
            .await
            .unwrap();
        assert!(ticked > ticks + 1, "the player's first draw ticked too");
        assert!(connected, "and they count as here");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_player_who_acts_counts_as_connected() {
        let path = temp("act");
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let host = rooms.create(|_| toy()).await.unwrap();
        let ada = rooms
            .with(&host, |r, now| admit(r, "Ada", None, now - 2 * PRESENT_MS))
            .await
            .unwrap()
            .unwrap();
        let away = |rooms: Arc<Rooms<Toy>>, ada: String| async move {
            !rooms
                .read(&host_of(&rooms, &ada).await, |r, now| {
                    r.lobby.connected(&ada, now)
                })
                .await
                .unwrap()
        };
        assert!(away(rooms.clone(), ada.clone()).await);
        let response = act_as(
            &rooms,
            &ada,
            |r, _, _| {
                r.secret += 1;
                Ok(())
            },
            |r, _, _| r.secret.to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!away(rooms.clone(), ada.clone()).await);
        let _ = std::fs::remove_file(&path);
    }

    async fn host_of(rooms: &Rooms<Toy>, key: &str) -> String {
        room_of(rooms, key).await.unwrap()
    }

    #[tokio::test]
    async fn a_wall_stream_sends_nothing_new_while_nothing_changes() {
        let path = temp("dedupe");
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let host = rooms.create(|_| toy()).await.unwrap();
        let ada = rooms
            .with(&host, |r, now| admit(r, "Ada", None, now))
            .await
            .unwrap()
            .unwrap();
        let wall = rooms
            .read(&host, |r, _| r.lobby.wall.clone())
            .await
            .unwrap();
        // Ada's phone is connected throughout, which bumps her `seen`.
        let phone = player_events(rooms.clone(), ada.clone(), |r, key, _| {
            Some(format!("\"{}\"", r.lobby.player(key)?.name))
        })
        .await;
        let mut phone = phone.into_body();
        http_frame(&mut phone).await;

        let response = wall_events(State(rooms.clone()), Path(wall)).await;
        let mut body = response.into_body();
        let first = http_frame(&mut body).await;
        assert!(first.contains("\"connected\":1"), "{first}");
        let next = tokio::time::timeout(std::time::Duration::from_millis(1_500), async {
            http_frame(&mut body).await
        })
        .await;
        assert!(
            next.is_err(),
            "nothing is sent while nothing changes: {next:?}"
        );
        drop(phone);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_player_stream_marks_them_connected_and_closes_when_they_are_removed() {
        let path = temp("stream");
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let host = rooms.create(|_| toy()).await.unwrap();
        let ada = rooms
            .with(&host, |r, now| admit(r, "Ada", None, now - PRESENT_MS))
            .await
            .unwrap()
            .unwrap();
        assert!(
            !rooms
                .read(&host, |r, now| r.lobby.connected(&ada, now))
                .await
                .unwrap()
        );

        let response = player_events(rooms.clone(), ada.clone(), |r, key, _| {
            Some(format!("\"{}\"", r.lobby.player(key)?.name))
        })
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        let first = http_frame(&mut body).await;
        assert!(first.contains(r#""view":"Ada""#), "{first}");
        assert!(
            rooms
                .read(&host, |r, now| r.lobby.connected(&ada, now))
                .await
                .unwrap()
        );

        let key = ada.clone();
        rooms
            .with(&host, move |r, _| remove_key(r, &key))
            .await
            .unwrap()
            .unwrap();
        // The stream ends rather than carrying on with nothing.
        let ended = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            use http_body_util::BodyExt;
            while let Some(frame) = body.frame().await {
                if frame.is_err() {
                    break;
                }
            }
        })
        .await;
        assert!(ended.is_ok(), "the stream of a removed player closes");

        let missing = player_events(rooms.clone(), "nobody".into(), |_, _, _| None).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let _ = std::fs::remove_file(&path);
    }

    async fn http_frame(body: &mut Body) -> String {
        use http_body_util::BodyExt;
        loop {
            let frame = body.frame().await.unwrap().unwrap();
            if let Ok(data) = frame.into_data() {
                let text = String::from_utf8(data.to_vec()).unwrap();
                // Keep-alive comments are not views.
                if text.starts_with("data:") {
                    return text;
                }
            }
        }
    }
}
