//! One room for an evening, with one join link, that plays whichever game
//! the host picks.
//!
//! Every game is served from this one server under its own path,
//! `/petrov`, `/numbers`, `/estimate` and `/updating`, exactly as it would be on its
//! own. The hall keeps the people and the one QR code. When the host picks
//! a game, the hall opens a room in it (or goes back to the one it opened
//! earlier) and joins everybody into it by name. Phones show the game in a
//! frame, and the frame moves when the host moves the room.

mod hall;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rooms::{
    Rooms, http, now_ms, site,
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
}

async fn open_hall(State(app): State<App>) -> Response {
    let now = now_ms();
    site::open(&app.halls, |_| Hall::new(now), |hall| hall.join.clone()).await
}

async fn host_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    let tools = app.tools.clone();
    match app
        .halls
        .read(&key, |hall, now| {
            http::envelope(now, &hall.host_view(&tools))
        })
        .await
    {
        Some(body) => http::json(body),
        None => http::gone(),
    }
}

async fn host_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    let tools = app.tools.clone();
    rooms::one(app.halls, key, move |hall| {
        serde_json::to_string(&hall.host_view(&tools)).ok()
    })
}

#[derive(Deserialize)]
struct Play {
    /// The game to move the room to, or nothing for the lobby.
    tool: Option<String>,
}

/// Moves the room to a game. Opens a room in that game the first time,
/// goes back to the same one after that, and joins everybody into it by
/// name, which is harmless for anybody already in.
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
    let Some((host, people)) = app
        .halls
        .read(&key, |hall, _| (hall.game_host(&id), hall.people()))
        .await
    else {
        return http::gone();
    };
    let seated = match seat_everybody(tool.as_ref(), host, &people).await {
        Ok(seated) => seated,
        Err(message) => return http::refused(message),
    };
    reply(&app, &key, move |hall| {
        hall.moved_to(&id, seated);
        Ok(())
    })
    .await
}

/// Joins everybody into `tool`'s room run with `host`, opening a room
/// when there is none yet or the old one has expired.
async fn seat_everybody(
    tool: &dyn Tool,
    host: Option<String>,
    people: &[(String, String)],
) -> Result<Seated, &'static str> {
    let mut host = host;
    // Twice at most: the room the hall remembers, then a fresh one if
    // that one has expired.
    for _ in 0..2 {
        let key = match host.take() {
            Some(key) => key,
            None => tool
                .open()
                .await
                .ok_or("That game is full tonight. Try again later.")?,
        };
        let mut players = Vec::new();
        let mut expired = false;
        for (person, name) in people {
            match tool.join(&key, name).await {
                Some(Ok(player)) => players.push((person.clone(), player)),
                Some(Err(message)) => return Err(message),
                None => {
                    expired = true;
                    break;
                }
            }
        }
        if !expired {
            return Ok(Seated { host: key, players });
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
    site::act(
        &app.halls,
        key,
        |hall, _| action(hall),
        |hall, now| http::envelope(now, &hall.host_view(&tools)),
    )
    .await
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

/// Joins the hall, and the game it is playing if there is one.
async fn join(
    State(app): State<App>,
    Path(join): Path<String>,
    Json(named): Json<Named>,
) -> Response {
    let Some(key) = app.halls.key_where(|hall| hall.join == join).await else {
        return http::gone();
    };
    let added = app
        .halls
        .with(&key, |hall, now| {
            hall.add(&named.name, now)
                .map(|(person, name)| (person, name, hall.current_game()))
        })
        .await;
    let (person, name, playing) = match added {
        None => return http::gone(),
        Some(Err(message)) => return http::refused(message),
        Some(Ok(added)) => added,
    };
    if let Some((id, host)) = playing
        && let Some(tool) = app.tool(&id)
        && let Some(Ok(player)) = tool.join(&host, &name).await
    {
        app.halls
            .with(&key, |hall, _| hall.seat(&id, &person, player))
            .await;
    }
    Json(serde_json::json!({ "player": person })).into_response()
}

/// How many people are in already, which the join page shows.
async fn how_many(State(app): State<App>, Path(join): Path<String>) -> Response {
    match app
        .halls
        .find(|_, hall| (hall.join == join).then(|| hall.people().len()))
        .await
    {
        Some(players) => Json(serde_json::json!({ "players": players })).into_response(),
        None => http::gone(),
    }
}

async fn player_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    let tools = app.tools.clone();
    match app
        .halls
        .find(|_, hall| {
            hall.player_view(&key, &tools)
                .map(|view| http::envelope(now_ms(), &view))
        })
        .await
    {
        Some(body) => http::json(body),
        None => http::gone(),
    }
}

/// A player's stream. Their hall is looked up once, when the phone
/// connects: looking for it on every tick walked every hall, four times a
/// second per phone, under the lock that every other watcher waits on.
async fn player_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    let Some(hall) = app
        .halls
        .find(|hall_key, hall| hall.has(&key).then(|| hall_key.to_string()))
        .await
    else {
        return http::gone();
    };
    let tools = app.tools.clone();
    rooms::one(app.halls, hall, move |hall| {
        serde_json::to_string(&hall.player_view(&key, &tools)?).ok()
    })
}

fn router(app: App, games: Router) -> Router {
    Router::new()
        .merge(site::routes(FILES))
        .route("/api/rooms", post(open_hall))
        .route("/api/h/{key}", get(host_state))
        .route("/api/h/{key}/events", get(host_events))
        .route("/api/h/{key}/play", post(play))
        .route("/api/j/{key}", get(how_many).post(join))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
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
        Arc::new(Mount {
            id: "numbers",
            title: "Number games",
            rooms: numbers,
        }),
        Arc::new(Mount {
            id: "estimate",
            title: "Estimation",
            rooms: estimate,
        }),
        Arc::new(Mount {
            id: "updating",
            title: "Speed updating",
            rooms: updating,
        }),
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
