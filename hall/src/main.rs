//! One room for an evening, with one join link, that plays whichever game
//! the host picks.
//!
//! Every game is served from this one server under its own path,
//! `/petrov`, `/numbers`, `/estimate` and `/updating`, exactly as it would be on its
//! own. The hall keeps the people and the one QR code. When the host picks
//! a game, the hall opens a room in it (or goes back to the one it opened
//! earlier) and joins everybody into it by name. Phones show the game in a
//! frame, and the frame moves when the host moves the room.
//!
//! The people are a `rooms::lobby`: a name already held by a connected
//! phone is refused, the host can take somebody out, and the wall at
//! `/w/<key>` shows the code, who is in, and the game's own wall.

mod hall;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{FromRef, Path, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rooms::{
    Rooms, http,
    lobby::{self, OnWall},
    now_ms, site,
    tool::{Mount, Tool},
};
use serde::Deserialize;

use hall::{Hall, Seated};

const FILES: site::Files = site::Files {
    page: include_str!("../static/index.html"),
    icon: include_str!("../static/icon.svg"),
    script: include_str!(concat!(env!("OUT_DIR"), "/app.js")),
    stylesheet: include_str!(concat!(env!("OUT_DIR"), "/app.css")),
};

/// How many halls at once. Limited because anyone can open one.
const MAX_HALLS: usize = 2000;

#[derive(Clone)]
struct App {
    halls: Arc<Rooms<Hall>>,
    tools: Arc<Vec<Arc<dyn Tool>>>,
}

impl App {
    fn tool(&self, id: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|tool| tool.id() == id).cloned()
    }

    /// What the game the hall `key` is playing lets the room see of its
    /// people right now. Everything, in the lobby or when the game's room
    /// has gone.
    async fn game_on_wall(&self, key: &str) -> OnWall {
        let playing = self
            .halls
            .read(key, |hall, _| hall.current_game())
            .await
            .flatten();
        let Some((id, host)) = playing else {
            return OnWall::default();
        };
        match self.tool(&id) {
            Some(tool) => tool.on_wall(&host).await.unwrap_or_default(),
            None => OnWall::default(),
        }
    }
}

/// So that the lobby's own handlers can be routed here.
impl FromRef<App> for Arc<Rooms<Hall>> {
    fn from_ref(app: &App) -> Self {
        app.halls.clone()
    }
}

async fn open_hall(State(app): State<App>) -> Response {
    let now = now_ms();
    site::open(
        &app.halls,
        |_| Hall::new(now),
        |hall| hall.lobby.join.clone(),
    )
    .await
}

async fn host_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    let game = app.game_on_wall(&key).await;
    let tools = app.tools.clone();
    match app
        .halls
        .look(&key, |hall, now| {
            http::envelope(now, &hall.host_view(now, &tools, game))
        })
        .await
    {
        Some(body) => http::json(body),
        None => http::gone(),
    }
}

/// The host's stream. Each tick asks the game being played what it lets
/// the room see, which takes that game's lock, so this is a
/// [`rooms::poll`] rather than a [`rooms::one_at`].
async fn host_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    rooms::poll(move |_| {
        let app = app.clone();
        let key = key.clone();
        async move {
            let game = app.game_on_wall(&key).await;
            let tools = app.tools.clone();
            app.halls
                .watching(&key, |hall, now| {
                    serde_json::to_string(&hall.host_view(now, &tools, game)).ok()
                })
                .await
                .flatten()
        }
    })
}

#[derive(Deserialize)]
struct Play {
    /// The game to move the room to, or nothing for the lobby.
    tool: Option<String>,
}

/// Moves the room to a game. Opens a room in that game the first time,
/// goes back to the same one after that, and joins everybody into it,
/// presenting the key each of them was given there before.
async fn play(State(app): State<App>, Path(key): Path<String>, Json(play): Json<Play>) -> Response {
    let Some(id) = play.tool else {
        return reply(&app, &key, |hall| {
            hall.current = None;
            Ok(())
        })
        .await;
    };
    let Some(tool) = app.tool(&id) else {
        return http::refused("That game is not one this server plays.");
    };
    let Some((session, people)) = app
        .halls
        .read(&key, |hall, _| (hall.session(&id), hall.people()))
        .await
    else {
        return http::gone();
    };
    let seated = match seat_everybody(tool.as_ref(), session, &people).await {
        Ok(seated) => seated,
        Err(message) => return http::refused(message),
    };
    reply(&app, &key, move |hall| {
        hall.moved_to(&id, seated);
        Ok(())
    })
    .await
}

/// Joins everybody into `tool`'s room, opening one when there is none
/// yet or the old one has expired. Somebody the game will not take is
/// noted with the reason, and everybody else is still seated.
async fn seat_everybody(
    tool: &dyn Tool,
    session: Option<(String, std::collections::HashMap<String, String>)>,
    people: &[(String, String)],
) -> Result<Seated, &'static str> {
    let mut session = session;
    // Twice at most: the room the hall remembers, then a fresh one if
    // that one has expired.
    for _ in 0..2 {
        let (host, known) = match session.take() {
            Some(session) => session,
            None => (
                tool.open()
                    .await
                    .ok_or("That game is full tonight. Try again later.")?,
                Default::default(),
            ),
        };
        let mut seated = Seated {
            host: host.clone(),
            players: Vec::new(),
            refused: Vec::new(),
            wall: None,
        };
        let mut expired = false;
        for (person, name) in people {
            let known = known.get(person).map(String::as_str);
            match tool.rejoin(&host, known, name).await {
                Some(Ok(player)) => seated.players.push((person.clone(), player)),
                Some(Err(message)) => seated.refused.push((person.clone(), message.into())),
                None => {
                    expired = true;
                    break;
                }
            }
        }
        if !expired {
            seated.wall = tool.wall(&host).await;
            return Ok(seated);
        }
    }
    Err("That game could not be opened. Try again.")
}

/// Changes the hall and answers with the host's view.
async fn reply(
    app: &App,
    key: &str,
    action: impl FnOnce(&mut Hall) -> Result<(), &'static str>,
) -> Response {
    let tools = app.tools.clone();
    let game = app.game_on_wall(key).await;
    site::act(
        &app.halls,
        key,
        |hall, _| action(hall),
        |hall, now| http::envelope(now, &hall.host_view(now, &tools, game)),
    )
    .await
}

/// Joins the hall by its rules, then the game it is playing if there is
/// one. If the game will not take them, their phone is told why.
async fn join(
    State(app): State<App>,
    Path(join): Path<String>,
    Json(joining): Json<lobby::Joining>,
) -> Response {
    let Some(key) = lobby::room_of(&app.halls, &join).await else {
        return http::gone();
    };
    let added = app
        .halls
        .with(&key, |hall, now| {
            if hall.lobby.join != join {
                return None;
            }
            Some(
                lobby::admit(hall, &joining.name, joining.key.as_deref(), now).map(|person| {
                    let name = hall.lobby.player(&person).map(|p| p.name.clone());
                    let playing = hall
                        .current_game()
                        .map(|(id, host)| (hall.seat_in(&id, &person), id, host));
                    (person, name.unwrap_or_default(), playing)
                }),
            )
        })
        .await
        .flatten();
    let (person, name, playing) = match added {
        None => return http::gone(),
        Some(Err(message)) => return http::refused(message),
        Some(Ok(added)) => added,
    };
    if let Some((known, id, host)) = playing
        && let Some(tool) = app.tool(&id)
    {
        let seated = match tool.rejoin(&host, known.as_deref(), &name).await {
            Some(Ok(player)) => Ok(player),
            Some(Err(message)) => Err(message.to_string()),
            None => Err("That game has closed. Ask the host to pick it again.".to_string()),
        };
        app.halls
            .with(&key, |hall, _| hall.seat(&id, &host, &person, seated))
            .await;
    }
    Json(serde_json::json!({ "player": person })).into_response()
}

/// Takes somebody out of the hall, by the id the host's list gave them,
/// and out of every game it seated them in, so their phone stops and their
/// name is free again. The games are not asked: somebody the host has
/// taken out of the room is out of every game in it.
async fn remove(
    State(app): State<App>,
    Path(key): Path<String>,
    Json(removing): Json<lobby::Removing>,
) -> Response {
    let removed = app
        .halls
        .with(&key, |hall, _| {
            let places = hall
                .lobby
                .by_id(removing.id)
                .map(|p| hall.places(&p.key))
                .unwrap_or_default();
            lobby::remove(hall, removing.id).map(|_| places)
        })
        .await;
    let places = match removed {
        None => return http::gone(),
        Some(Err(message)) => return http::refused(message),
        Some(Ok(places)) => places,
    };
    for (id, host, player) in places {
        if let Some(tool) = app.tool(&id) {
            tool.leave(&host, &player).await;
        }
    }
    reply(&app, &key, |_| Ok(())).await
}

async fn player_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    let tools = app.tools.clone();
    lobby::player_state(&app.halls, &key, |hall, key, _| {
        serde_json::to_string(&hall.player_view(key, &tools)?).ok()
    })
    .await
}

/// A player's stream. Their hall is found once, through the index, when
/// the phone connects, rather than by walking every hall on every tick.
async fn player_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    let tools = app.tools.clone();
    lobby::player_events(app.halls, key, move |hall, key, _| {
        serde_json::to_string(&hall.player_view(key, &tools)?).ok()
    })
    .await
}

/// The hall's wall as JSON, moved on to now, or `None` when `wall` is not
/// the wall key of the hall `key`. Looking does not keep the hall alive.
async fn wall_json(app: &App, key: &str, wall: &str) -> Option<String> {
    let game = app.game_on_wall(key).await;
    app.halls
        .look(key, |hall, now| {
            if hall.lobby.wall != wall {
                return None;
            }
            let view = hall
                .lobby
                .wall_view(now, hall.on_wall(game), hall.wall_part(&app.tools));
            serde_json::to_string(&view).ok()
        })
        .await
        .flatten()
}

async fn wall_state(State(app): State<App>, Path(wall): Path<String>) -> Response {
    let Some(key) = lobby::room_of(&app.halls, &wall).await else {
        return http::gone();
    };
    let now = now_ms();
    match wall_json(&app, &key, &wall).await {
        Some(view) => http::json(format!("{{\"now\":{now},\"view\":{view}}}")),
        None => http::gone(),
    }
}

/// The wall's stream, which follows what the game being played lets the
/// room see. A wall left on does not keep the hall alive.
async fn wall_events(State(app): State<App>, Path(wall): Path<String>) -> Response {
    let Some(key) = lobby::room_of(&app.halls, &wall).await else {
        return http::gone();
    };
    rooms::poll(move |_| {
        let app = app.clone();
        let key = key.clone();
        let wall = wall.clone();
        async move { wall_json(&app, &key, &wall).await }
    })
}

fn router(app: App, games: Router) -> Router {
    Router::new()
        .merge(site::routes(FILES))
        .route("/api/rooms", post(open_hall))
        .route("/api/h/{key}", get(host_state))
        .route("/api/h/{key}/events", get(host_events))
        .route("/api/h/{key}/play", post(play))
        .route("/api/h/{key}/remove", post(remove))
        .route("/api/j/{key}", get(lobby::count::<Hall>).post(join))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
        .route("/api/w/{key}", get(wall_state))
        .route("/api/w/{key}/events", get(wall_events))
        .with_state(app)
        .merge(games)
}

/// Every game, each under its own path and with its own state file in
/// `dir`, and the same games as the hall sees them.
fn games(dir: &std::path::Path) -> (Router, Vec<Arc<dyn Tool>>) {
    let petrov = petrov::load(
        dir.join("petrov.json"),
        dir.join("petrov-groups.json"),
        "petrov",
    )
    .expect("the petrov brand exists");
    let numbers = numbers::load(dir.join("numbers.json"));
    let estimate = estimate::load(dir.join("estimate.json"));
    let updating = updating::load(dir.join("updating.json"));
    let routes = Router::new()
        .nest("/petrov", petrov::router_under(petrov.clone(), "/petrov"))
        .nest(
            "/numbers",
            numbers::router_under(numbers.clone(), "/numbers"),
        )
        .nest(
            "/estimate",
            estimate::router_under(estimate.clone(), "/estimate"),
        )
        .nest(
            "/updating",
            updating::router_under(updating.clone(), dir.join("suggestions.jsonl"), "/updating"),
        );
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(petrov),
        // On the lobby, so the hall follows each game's `HasLobby::on_wall`.
        Arc::new(Mount::with_lobby("numbers", "Number games", numbers)),
        Arc::new(Mount::with_lobby("estimate", "Estimation", estimate)),
        Arc::new(Mount::with_lobby("updating", "Speed updating", updating)),
    ];
    (routes, tools)
}

/// The whole server, keeping its state in `dir`.
fn app_in(dir: PathBuf) -> Router {
    let (routes, tools) = games(&dir);
    let halls = Rooms::load(dir.join("halls.json"), MAX_HALLS);
    tokio::spawn(halls.clone().sweep(rooms::SWEEP));
    router(
        App {
            halls,
            tools: Arc::new(tools),
        },
        routes,
    )
}

#[tokio::main]
async fn main() {
    let addr = std::env::var("HALL_ADDR").unwrap_or_else(|_| "127.0.0.1:8103".into());
    let dir = PathBuf::from(std::env::var("HALL_STATE").unwrap_or_else(|_| ".".into()));
    std::fs::create_dir_all(&dir).expect("can make the state directory");
    rooms::serve(&addr, app_in(dir)).await;
}

#[cfg(test)]
mod tests;
