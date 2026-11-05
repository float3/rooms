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
//! A tool keeps a [`Lobby`] in its room and implements [`HasLobby`]:
//!
//! ```ignore
//! #[derive(Serialize, Deserialize)]
//! struct Game {
//!     // `flatten` keeps the saved field names, `join` and `players`, so
//!     // rooms saved before the lobby load as they are.
//!     #[serde(flatten)]
//!     lobby: Lobby,
//!     rounds: Vec<Round>,
//!     last_seen: u64,
//! }
//!
//! impl Room for Game {
//!     fn keys(&self) -> Vec<&str> {
//!         self.lobby.keys().collect()
//!     }
//!     // seen, expired, tick as before
//! }
//!
//! impl HasLobby for Game {
//!     fn lobby(&self) -> &Lobby { &self.lobby }
//!     fn lobby_mut(&mut self) -> &mut Lobby { &mut self.lobby }
//! }
//!
//! let app = Router::new()
//!     .merge(site::routes_under(FILES, base))
//!     .merge(lobby::routes::<Game, Shared>())
//!     .route("/api/p/{key}/events", get(|State(rooms): State<Shared>, Path(key): Path<String>| {
//!         lobby::player_events(rooms, key, |game, key, now| view::player(game, key, now))
//!     }));
//! ```
//!
//! Who is connected is not stored anywhere. Each phone keeps an event
//! stream open; while it is open, [`player_events`] marks the player seen
//! four times a second, and a player seen in the last [`PRESENT_MS`] is
//! connected.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{FromRef, Path, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Room, Rooms, http, key, stream};

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
#[derive(Debug, Serialize, Deserialize)]
pub struct Lobby {
    /// The key in the join link and the QR code.
    pub join: String,
    /// The key in the wall's link, `/w/<key>`. It grants nothing: the
    /// wall only shows what everybody in the room may see. It is not the
    /// host's key, because the wall is projected and an address bar on a
    /// projector can be photographed. Rooms saved before there was a wall
    /// are given one when they load.
    #[serde(default = "key::new")]
    pub wall: String,
    /// In the order they joined. The hall saved these as `people`.
    #[serde(default, alias = "people")]
    pub players: Vec<Player>,
}

impl Default for Lobby {
    fn default() -> Self {
        Self::new()
    }
}

/// Somebody as the host and the wall see them: no key.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Present<'a> {
    pub name: &'a str,
    /// Whether their phone is connected now.
    pub connected: bool,
    /// When it last was, in the server's milliseconds.
    pub seen: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat: Option<u32>,
}

/// What the wall is sent. Names and who is connected, and whatever the
/// tool decides is public ([`HasLobby::wall`]); never anybody's answer.
#[derive(Serialize)]
pub struct Wall<'a> {
    /// The join key, for the QR code. The join link is on the wall
    /// anyway, so this tells a viewer nothing new.
    pub join: &'a str,
    pub people: Vec<Present<'a>>,
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
        }
    }

    pub fn player(&self, key: &str) -> Option<&Player> {
        self.players.iter().find(|p| p.key == key)
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

    /// Everybody, without keys, in the order they joined.
    pub fn roster(&self, now: u64) -> Vec<Present<'_>> {
        self.players
            .iter()
            .map(|p| Present {
                name: &p.name,
                connected: present(p, now),
                seen: p.seen,
                seat: p.seat,
            })
            .collect()
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
        let key = key::new();
        self.players.push(Player {
            key: key.clone(),
            name,
            seat: None,
            // Counted as connected from the moment they join, so a second
            // phone typing the same name in the next breath is refused
            // before the first one's stream has opened.
            seen: now,
        });
        Ok(Admitted::New(key))
    }

    /// Takes somebody out. Their phone's stream closes and their link
    /// stops working; the join link lets them back in as somebody new.
    pub fn remove(&mut self, key: &str) -> Option<Player> {
        let at = self.players.iter().position(|p| p.key == key)?;
        Some(self.players.remove(at))
    }

    /// The wall's view, with `game` as the tool's part of it.
    pub fn wall_view<'a>(&'a self, now: u64, game: Value) -> Wall<'a> {
        Wall {
            join: &self.join,
            people: self.roster(now),
            game,
        }
    }
}

fn present(player: &Player, now: u64) -> bool {
    now.saturating_sub(player.seen) < PRESENT_MS
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

    /// Somebody has been taken out by the host. Forget what the tool
    /// keeps for them, or keep it for the scores: the tool decides.
    fn left(&mut self, _key: &str) {}

    /// The tool's part of the wall: what the projector shows besides the
    /// QR code and the names. Everybody in the room can see the wall, so
    /// put nothing on it the players should not see yet. `Null`, the
    /// default, shows nothing.
    fn wall(&self, _now: u64) -> Value {
        Value::Null
    }
}

/// Lets somebody in by the lobby's rules and seats them. Answers with
/// their key. This is what `Joinable::join` should call.
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

/// Takes somebody out by name, which is all the host's screen knows them
/// by, and tells the tool. Answers with the key they had.
pub fn remove<R: HasLobby>(room: &mut R, name: &str) -> Result<String, &'static str> {
    let key = room
        .lobby()
        .named(name)
        .map(|p| p.key.clone())
        .ok_or("Nobody by that name is in the room.")?;
    room.lobby_mut().remove(&key);
    room.left(&key);
    Ok(key)
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

/// A player's view, once: for the page's first draw. `render` is given
/// the room, the player's key and the time, and answers with the view as
/// JSON, or `None` for a key the room does not know.
pub async fn player_state<R: HasLobby>(
    rooms: &Rooms<R>,
    key: &str,
    render: impl FnOnce(&R, &str, u64) -> Option<String>,
) -> Response {
    let Some(at) = room_of(rooms, key).await else {
        return http::gone();
    };
    match rooms
        .read(&at, |room, now| {
            room.lobby().player(key)?;
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
    stream(rooms, move |held, now| {
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
/// what `reply` renders, usually the player's view in an envelope.
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
            room.lobby().player(key)?;
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

/// What a host sends to take somebody out.
#[derive(Deserialize)]
pub struct Named {
    pub name: String,
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

/// `POST /api/h/<host>/remove` with `{name}`: takes them out and answers
/// with everybody left, as `{now, view: {people}}`. The host's own stream
/// brings the rest of its view.
pub async fn kick<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(host): Path<String>,
    Json(named): Json<Named>,
) -> Response {
    let done = rooms
        .with(&host, |room, now| {
            remove(room, &named.name).map(|_| {
                http::envelope(
                    now,
                    &serde_json::json!({ "people": room.lobby().roster(now) }),
                )
            })
        })
        .await;
    match done {
        None => http::gone(),
        Some(Err(message)) => http::refused(message),
        Some(Ok(body)) => http::json(body),
    }
}

fn wall_json<R: HasLobby>(room: &R, now: u64) -> Option<String> {
    serde_json::to_string(&room.lobby().wall_view(now, room.wall(now))).ok()
}

/// `GET /api/w/<wall>`: the wall's view, once.
pub async fn wall_state<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(wall): Path<String>,
) -> Response {
    let Some(at) = room_of(&rooms, &wall).await else {
        return http::gone();
    };
    match rooms
        .read(&at, |room, now| {
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

/// `GET /api/w/<wall>/events`: the wall's stream.
pub async fn wall_events<R: HasLobby>(
    State(rooms): State<Arc<Rooms<R>>>,
    Path(wall): Path<String>,
) -> Response {
    let Some(at) = room_of(&rooms, &wall).await else {
        return http::gone();
    };
    crate::one_at(rooms, at, move |room, now| {
        if room.lobby().wall != wall {
            return None;
        }
        wall_json(room, now)
    })
}

/// The lobby's routes, for a tool whose state is, or holds, its
/// `Arc<Rooms<R>>` (implement `FromRef` for a state of your own):
///
/// - `GET /api/j/{key}` and `POST /api/j/{key}`: how many are in, and joining
/// - `POST /api/h/{key}/remove`: the host takes somebody out
/// - `GET /api/w/{key}` and `/api/w/{key}/events`: the wall
///
/// The page at `/w/{key}` is in [`crate::site::routes`].
pub fn routes<R, S>() -> Router<S>
where
    R: HasLobby,
    S: Clone + Send + Sync + 'static,
    Arc<Rooms<R>>: FromRef<S>,
{
    Router::new()
        .route("/api/j/{key}", get(count::<R>).post(join::<R>))
        .route("/api/h/{key}/remove", post(kick::<R>))
        .route("/api/w/{key}", get(wall_state::<R>))
        .route("/api/w/{key}/events", get(wall_events::<R>))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;

    #[derive(Serialize, Deserialize)]
    struct Toy {
        #[serde(flatten)]
        lobby: Lobby,
        /// Which team each player is on, as a tool would keep it.
        #[serde(default)]
        dealt: Vec<String>,
        #[serde(default)]
        secret: u32,
    }

    impl Room for Toy {
        fn expired(&self, _now: u64) -> bool {
            false
        }
        fn keys(&self) -> Vec<&str> {
            self.lobby.keys().collect()
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
        fn left(&mut self, key: &str) {
            self.dealt.retain(|k| k != key);
        }
        fn wall(&self, _now: u64) -> Value {
            serde_json::json!({ "round": 1 })
        }
    }

    fn toy() -> Toy {
        Toy {
            lobby: Lobby::new(),
            dealt: Vec::new(),
            secret: 7,
        }
    }

    const T: u64 = 1_000_000;

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
            [("Ada", false), ("Bo", true)]
        );
        assert!(!room.lobby.connected(&ada, now));
        let json = serde_json::to_string(&roster).unwrap();
        assert!(
            !json.contains(&ada) && !json.contains(&bo),
            "no keys: {json}"
        );
    }

    #[test]
    fn the_host_takes_somebody_out() {
        let mut room = toy();
        let ada = admit(&mut room, "Ada", None, T).unwrap();
        admit(&mut room, "Bo", None, T).unwrap();
        assert_eq!(remove(&mut room, "ada"), Ok(ada.clone()));
        assert!(room.lobby.player(&ada).is_none());
        assert!(!room.dealt.contains(&ada), "and the tool forgot them");
        assert!(!room.lobby.touch(&ada, T), "so their stream closes");
        assert!(remove(&mut room, "Ada").is_err());
        let back = admit(&mut room, "Ada", Some(&ada), T).unwrap();
        assert_ne!(back, ada, "the old key no longer gets in");
    }

    #[test]
    fn rooms_saved_before_the_lobby_load_into_it() {
        // A numbers or estimate room as it was saved, and the hall's.
        let old = r#"{"join":"J","players":[{"key":"K","name":"Ada"}],"secret":3}"#;
        let room: Toy = serde_json::from_str(old).unwrap();
        assert_eq!(room.lobby.join, "J");
        assert_eq!(room.lobby.player("K").unwrap().name, "Ada");
        assert_eq!(room.lobby.wall.len(), key::LEN, "a wall key is made");
        assert_eq!(room.secret, 3);
        let hall = r#"{"join":"J","people":[{"key":"K","name":"Ada"}]}"#;
        let room: Toy = serde_json::from_str(hall).unwrap();
        assert_eq!(room.lobby.len(), 1);
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

    #[tokio::test]
    async fn the_routes_join_count_remove_and_show_the_wall() {
        let path = std::env::temp_dir().join(format!("rooms-lobby-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let rooms: Arc<Rooms<Toy>> = Rooms::load(&path, 4);
        let host = rooms.create(|_| toy()).await.unwrap();
        let (join, wall) = rooms
            .read(&host, |r, _| (r.lobby.join.clone(), r.lobby.wall.clone()))
            .await
            .unwrap();
        let app: Router = routes::<Toy, Arc<Rooms<Toy>>>().with_state(rooms.clone());

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
            Some(serde_json::json!({"name": "Ada"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "the wall key cannot remove");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/h/{host}/remove"),
            Some(serde_json::json!({"name": "Ada"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["view"]["people"], serde_json::json!([]));
        assert_eq!(rooms.locate(&ada), None);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_player_stream_marks_them_connected_and_closes_when_they_are_removed() {
        let path =
            std::env::temp_dir().join(format!("rooms-lobby-stream-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
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

        rooms
            .with(&host, |r, _| remove(r, "Ada"))
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
        let frame = body.frame().await.unwrap().unwrap();
        String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap()
    }
}
