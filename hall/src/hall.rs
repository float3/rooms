//! The room itself: who is in it, which game it is playing, and each
//! person's key in every game it has played.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use rooms::tool::Tool;
use serde::{Deserialize, Serialize};

pub const MAX_PEOPLE: usize = 200;
pub const MAX_NAME: usize = 32;

#[derive(Serialize, Deserialize)]
pub struct Person {
    pub key: String,
    pub name: String,
}

/// The room a hall opened in one game.
#[derive(Serialize, Deserialize, Default)]
pub struct Session {
    /// The host key of the game's room.
    pub host: String,
    /// Each person's player key in it, by their key in the hall.
    pub players: HashMap<String, String>,
}

/// Everybody joined into a game's room.
pub struct Seated {
    pub host: String,
    /// (key in the hall, key in the game)
    pub players: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
pub struct Hall {
    pub join: String,
    pub people: Vec<Person>,
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
}

fn tidy(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME)
        .collect()
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
}

#[derive(Serialize)]
pub struct HostView<'a> {
    join: &'a str,
    people: Vec<&'a str>,
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
            join: rooms::key::new(),
            people: Vec::new(),
            current: None,
            games: BTreeMap::new(),
            seen: now,
        }
    }

    /// Everybody, as (key, name).
    pub fn people(&self) -> Vec<(String, String)> {
        self.people
            .iter()
            .map(|p| (p.key.clone(), p.name.clone()))
            .collect()
    }

    pub fn has(&self, key: &str) -> bool {
        self.people.iter().any(|p| p.key == key)
    }

    pub fn game_host(&self, id: &str) -> Option<String> {
        self.games.get(id).map(|session| session.host.clone())
    }

    /// The game being played and its host key.
    pub fn current_game(&self) -> Option<(String, String)> {
        let id = self.current.clone()?;
        let host = self.game_host(&id)?;
        Some((id, host))
    }

    /// Adds somebody and answers with their key and their name as kept. A
    /// name already in the room answers with that person's key, so a
    /// phone that lost its link can join again as the same person.
    pub fn add(&mut self, name: &str, now: u64) -> Result<(String, String), &'static str> {
        let name = tidy(name);
        if name.is_empty() {
            return Err("Please give a name, so the room knows who you are.");
        }
        if let Some(person) = self.people.iter().find(|p| p.name == name) {
            return Ok((person.key.clone(), name));
        }
        if self.people.len() >= MAX_PEOPLE {
            return Err("This room is full.");
        }
        let key = rooms::key::new();
        self.people.push(Person {
            key: key.clone(),
            name: name.clone(),
        });
        self.seen = now;
        Ok((key, name))
    }

    /// Records somebody's key in a game.
    pub fn seat(&mut self, id: &str, person: &str, player: String) {
        if let Some(session) = self.games.get_mut(id) {
            session.players.insert(person.to_string(), player);
        }
    }

    /// Moves the room to a game everybody has been joined into. A game
    /// room that had to be opened afresh replaces the old one.
    pub fn moved_to(&mut self, id: &str, seated: Seated) {
        let session = self.games.entry(id.to_string()).or_default();
        if session.host != seated.host {
            *session = Session {
                host: seated.host,
                players: HashMap::new(),
            };
        }
        session.players.extend(seated.players);
        self.current = Some(id.to_string());
    }

    pub fn host_view(&self, tools: &[Arc<dyn Tool>]) -> HostView<'_> {
        HostView {
            join: &self.join,
            people: self.people.iter().map(|p| p.name.as_str()).collect(),
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
                id,
            }),
        }
    }

    pub fn player_view(&self, key: &str, tools: &[Arc<dyn Tool>]) -> Option<PlayerView<'_>> {
        let person = self.people.iter().find(|p| p.key == key)?;
        Some(PlayerView {
            name: &person.name,
            people: self.people.len(),
            playing: self.current.as_ref().map(|id| Playing {
                id: id.clone(),
                title: title(tools, id),
                url: self
                    .games
                    .get(id)
                    .and_then(|session| session.players.get(key))
                    .map(|player| format!("/{id}/p/{player}")),
            }),
        })
    }
}
