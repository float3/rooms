//! The room itself: who is in it, which game it is playing, and each
//! person's key in every game it has played.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use rooms::lobby::{self, HasLobby, Lobby, OnWall, Player, Present};
use rooms::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The room a hall opened in one game.
#[derive(Serialize, Deserialize, Default)]
pub struct Session {
    /// The host key of the game's room.
    pub host: String,
    /// Each person's player key in it, by their key in the hall.
    pub players: HashMap<String, String>,
    /// Why the game would not take somebody, by their key in the hall,
    /// so their phone can say so instead of waiting.
    #[serde(default)]
    pub refused: HashMap<String, String>,
    /// The game room's wall key, for a game that has a wall.
    #[serde(default)]
    pub wall: Option<String>,
}

/// Everybody joined into a game's room, or turned away from it.
pub struct Seated {
    pub host: String,
    /// (key in the hall, key in the game)
    pub players: Vec<(String, String)>,
    /// (key in the hall, why the game would not take them)
    pub refused: Vec<(String, String)>,
    pub wall: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Hall {
    /// The join key, the wall key and the people. Saved as `join` and
    /// `people` at the top level before the lobby, and as `join`, `wall`
    /// and `players` while it was flattened in; both still load, through
    /// `Room::migrate`.
    #[serde(default)]
    pub lobby: Lobby,
    /// The game being played, by its id.
    pub current: Option<String>,
    /// Every game this hall has played, so that going back to one picks up
    /// its scores where they were.
    pub games: BTreeMap<String, Session>,
    pub seen: u64,
}

impl rooms::Room for Hall {
    fn seen(&mut self, now: u64) {
        self.seen = now;
    }

    fn expired(&self, now: u64) -> bool {
        self.seen + rooms::DAY_MS < now
    }

    fn keys(&self) -> Vec<&str> {
        self.lobby.keys().collect()
    }

    fn migrate(saved: &mut Value) {
        lobby::migrate(saved);
    }
}

impl HasLobby for Hall {
    fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    fn lobby_mut(&mut self) -> &mut Lobby {
        &mut self.lobby
    }

    /// Forgets their place in every game. Taking them out of the games
    /// themselves is the handler's job, because that waits on the games.
    fn left(&mut self, player: &Player) {
        for session in self.games.values_mut() {
            session.players.remove(&player.key);
            session.refused.remove(&player.key);
        }
    }
}

#[derive(Serialize)]
pub struct Game {
    id: &'static str,
    title: &'static str,
}

#[derive(Serialize)]
pub struct Playing {
    id: String,
    title: &'static str,
    /// The page to show in the frame. For a player who could not be
    /// joined into the game, nothing.
    url: Option<String>,
    /// Why the game would not take this player.
    #[serde(skip_serializing_if = "Option::is_none")]
    refused: Option<String>,
}

#[derive(Serialize)]
pub struct HostView<'a> {
    join: &'a str,
    /// The wall's key: its link grants nothing, so it can be projected.
    wall: &'a str,
    people: Vec<Present<'a>>,
    games: Vec<Game>,
    playing: Option<Playing>,
}

#[derive(Serialize)]
pub struct PlayerView<'a> {
    name: &'a str,
    people: usize,
    playing: Option<Playing>,
}

fn title(tools: &[Arc<dyn Tool>], id: &str) -> &'static str {
    tools
        .iter()
        .find(|tool| tool.id() == id)
        .map_or("A game", |tool| tool.title())
}

impl Hall {
    pub fn new(now: u64) -> Hall {
        Hall {
            lobby: Lobby::new(),
            current: None,
            games: BTreeMap::new(),
            seen: now,
        }
    }

    /// Everybody, as (key, name).
    pub fn people(&self) -> Vec<(String, String)> {
        self.lobby
            .players
            .iter()
            .map(|p| (p.key.clone(), p.name.clone()))
            .collect()
    }

    /// The game being played and its host key.
    pub fn current_game(&self) -> Option<(String, String)> {
        let id = self.current.clone()?;
        let host = self.games.get(&id)?.host.clone();
        Some((id, host))
    }

    /// The room the hall opened in game `id`, and each person's key in
    /// it, for joining them in again.
    pub fn session(&self, id: &str) -> Option<(String, HashMap<String, String>)> {
        self.games
            .get(id)
            .map(|session| (session.host.clone(), session.players.clone()))
    }

    /// `person`'s key in game `id`, if they were seated there.
    pub fn seat_in(&self, id: &str, person: &str) -> Option<String> {
        self.games.get(id)?.players.get(person).cloned()
    }

    /// Records somebody's key in a game, or why the game would not take
    /// them. Only if the game is still on the room `host`: the host may
    /// have moved on, or the room been replaced, while they were joined.
    pub fn seat(&mut self, id: &str, host: &str, person: &str, seated: Result<String, String>) {
        if self.lobby.player(person).is_none() {
            return; // taken out in the meantime
        }
        let Some(session) = self.games.get_mut(id).filter(|s| s.host == host) else {
            return;
        };
        match seated {
            Ok(player) => {
                session.refused.remove(person);
                session.players.insert(person.to_string(), player);
            }
            Err(message) => {
                session.refused.insert(person.to_string(), message);
            }
        }
    }

    /// Moves the room to a game everybody has been joined into. A game
    /// room that had to be opened afresh replaces the old one.
    pub fn moved_to(&mut self, id: &str, seated: Seated) {
        let session = self.games.entry(id.to_string()).or_default();
        if session.host != seated.host {
            *session = Session {
                host: seated.host,
                ..Session::default()
            };
        }
        session.wall = seated.wall;
        for (person, player) in seated.players {
            session.refused.remove(&person);
            session.players.insert(person, player);
        }
        for (person, message) in seated.refused {
            session.refused.insert(person, message);
        }
        self.current = Some(id.to_string());
    }

    /// Where somebody is seated in each game: what to take them out of
    /// when the host removes them. (game id, game host, player key)
    pub fn places(&self, person: &str) -> Vec<(String, String, String)> {
        self.games
            .iter()
            .filter_map(|(id, session)| {
                let player = session.players.get(person)?;
                Some((id.clone(), session.host.clone(), player.clone()))
            })
            .collect()
    }

    /// The host's view. `game` is what the game being played lets the
    /// room see of its people ([`OnWall`]): while it hides who is
    /// connected, so does the host's list here, because the hall's host
    /// screen is often the one on the wall.
    pub fn host_view(&self, now: u64, tools: &[Arc<dyn Tool>], game: OnWall) -> HostView<'_> {
        HostView {
            join: &self.lobby.join,
            wall: &self.lobby.wall,
            people: self.lobby.people(now, game.presence),
            games: tools
                .iter()
                .map(|tool| Game {
                    id: tool.id(),
                    title: tool.title(),
                })
                .collect(),
            playing: self.current_game().map(|(id, host)| Playing {
                title: title(tools, &id),
                url: Some(format!("/{id}/h/{host}")),
                refused: None,
                id,
            }),
        }
    }

    pub fn player_view(&self, key: &str, tools: &[Arc<dyn Tool>]) -> Option<PlayerView<'_>> {
        let person = self.lobby.player(key)?;
        Some(PlayerView {
            name: &person.name,
            people: self.lobby.len(),
            playing: self.current.as_ref().map(|id| {
                let session = self.games.get(id);
                Playing {
                    id: id.clone(),
                    title: title(tools, id),
                    url: session
                        .and_then(|session| session.players.get(key))
                        .map(|player| format!("/{id}/p/{player}")),
                    refused: session.and_then(|session| session.refused.get(key).cloned()),
                }
            }),
        })
    }

    /// What the hall's wall shows of the room, given what the game being
    /// played allows (`game`, from the game's `HasLobby::on_wall`): the
    /// game's rules, and busy whenever a game is being played, so that the
    /// game's frame has the screen.
    pub fn on_wall(&self, game: OnWall) -> OnWall {
        OnWall {
            busy: game.busy || self.current.is_some(),
            ..game
        }
    }

    /// The hall's part of its wall: the game being played, and that
    /// game's own wall to show in a frame, if it has one. What a game puts
    /// on its wall is the game's to decide; see
    /// `rooms::lobby::HasLobby::wall_game`.
    pub fn wall_part(&self, tools: &[Arc<dyn Tool>]) -> Value {
        match self.current.as_ref() {
            None => Value::Null,
            Some(id) => serde_json::json!({
                "id": id,
                "title": title(tools, id),
                "frame": self
                    .games
                    .get(id)
                    .and_then(|session| session.wall.as_ref())
                    .map(|wall| format!("/{id}/w/{wall}")),
            }),
        }
    }
}
