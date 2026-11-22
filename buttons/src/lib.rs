//! Red or blue for a room of people.
//!
//! Everybody presses one of two buttons on their phone. If more than a
//! share the host sets press blue, everybody who pressed one lives. If
//! not, only the ones who pressed red do. No press is shown to anybody
//! until the host reveals the round.
//!
//! There are four kinds of link, and the key in each is the only
//! authority there is: `/h/<key>` runs the room, `/j/<key>` joins it,
//! `/p/<key>` belongs to one player, and `/w/<key>` is the wall, a
//! read-only screen for a projector. Joining, who is connected, taking
//! somebody out and the wall are `rooms::lobby`'s.

pub mod game;
mod view;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    response::Response,
    routing::{get, post, put},
};
use rooms::{Rooms, http, lobby, now_ms, site};
use serde::Deserialize;

use game::{Colour, Game};

/// The page and its files. The script and stylesheet are compiled from
/// the TypeScript and CSS in `client/` by the build script.
const FILES: site::Files = site::Files {
    page: include_str!("../static/index.html"),
    icon: include_str!("../static/icon.svg"),
    script: include_str!(concat!(env!("OUT_DIR"), "/app.js")),
    stylesheet: include_str!(concat!(env!("OUT_DIR"), "/app.css")),
};

/// How many rooms at once. Limited because anyone can open one.
const MAX_ROOMS: usize = 2000;

pub type Shared = Arc<Rooms<Game>>;

/// Applies a host action to the room and returns the host's view, so the
/// screen that pressed the button does not wait for its own stream.
async fn act(
    rooms: &Rooms<Game>,
    key: &str,
    action: impl FnOnce(&mut Game, u64) -> Result<(), &'static str>,
) -> Response {
    site::act(rooms, key, action, |game, now| {
        http::envelope(now, &view::host(game, now))
    })
    .await
}

#[derive(Deserialize)]
struct Started {
    /// The share of the presses blue has to beat, in percent.
    percent: u32,
}

#[derive(Deserialize)]
struct Opened {
    open: bool,
}

#[derive(Deserialize)]
struct Pressed {
    colour: Colour,
}

async fn open_room(State(rooms): State<Shared>) -> Response {
    let now = now_ms();
    site::open(&rooms, |_| Game::new(now), |game| game.lobby.join.clone()).await
}

async fn host_state(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    match rooms
        .read(&key, |game, now| {
            http::envelope(now, &view::host(game, now))
        })
        .await
    {
        Some(body) => http::json(body),
        None => http::gone(),
    }
}

async fn host_events(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    rooms::one_at(rooms, key, |game, now| {
        serde_json::to_string(&view::host(game, now)).ok()
    })
}

async fn start(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(started): Json<Started>,
) -> Response {
    act(&rooms, &key, |game, now| game.start(started.percent, now)).await
}

async fn set_open(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(opened): Json<Opened>,
) -> Response {
    act(&rooms, &key, |game, _| game.set_open(opened.open)).await
}

async fn reveal(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    act(&rooms, &key, |game, _| game.reveal()).await
}

async fn again(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    act(&rooms, &key, |game, now| game.again(now)).await
}

/// A player's view as JSON, or `None` for a key that is nobody's.
fn player_json(game: &Game, key: &str, _now: u64) -> Option<String> {
    serde_json::to_string(&view::player(game, key)?).ok()
}

async fn player_state(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    lobby::player_state(&rooms, &key, player_json).await
}

async fn player_events(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    lobby::player_events(rooms, key, player_json).await
}

async fn press(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(pressed): Json<Pressed>,
) -> Response {
    lobby::act_as(
        &rooms,
        &key,
        |game, key, now| game.press(key, pressed.colour, now),
        |game, key, now| http::envelope(now, &view::player(game, key)),
    )
    .await
}

/// The site, served at the root.
pub fn router(rooms: Shared) -> Router {
    router_under(rooms, "")
}

/// The site, for mounting under `base` with `Router::nest`, which is how
/// the shared room serves it next to the other tools.
pub fn router_under(rooms: Shared, base: &str) -> Router {
    Router::new()
        .merge(site::routes_under(FILES, base))
        .merge(lobby::routes(rooms.clone()))
        .route("/api/rooms", post(open_room))
        .route("/api/h/{key}", get(host_state))
        .route("/api/h/{key}/events", get(host_events))
        .route("/api/h/{key}/start", post(start))
        .route("/api/h/{key}/open", put(set_open))
        .route("/api/h/{key}/reveal", post(reveal))
        .route("/api/h/{key}/again", post(again))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
        .route("/api/p/{key}/press", put(press))
        .with_state(rooms)
}

/// Loads the rooms kept at `state`, and deletes the ones nobody has
/// opened in a day as it goes.
pub fn load(state: impl Into<PathBuf>) -> Shared {
    let rooms: Shared = Rooms::load(state.into(), MAX_ROOMS);
    tokio::spawn(rooms.clone().sweep(rooms::SWEEP));
    rooms
}

/// A room joined from the shared room, which joins everybody in it by
/// name.
impl rooms::tool::Joinable for Game {
    fn open(now: u64) -> Self {
        Game::new(now)
    }

    fn join(&mut self, name: &str, now: u64) -> Result<String, &'static str> {
        lobby::admit(self, name, None, now)
    }

    fn rejoin(
        &mut self,
        known: Option<&str>,
        name: &str,
        now: u64,
    ) -> Result<String, &'static str> {
        lobby::admit(self, name, known, now)
    }

    fn leave(&mut self, key: &str) -> bool {
        lobby::remove_key(self, key).is_some()
    }

    fn wall(&self) -> Option<&str> {
        Some(&self.lobby.wall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    fn app(name: &str) -> (Router, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("buttons-test-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        (router(Rooms::load(&path, 4)), path)
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
        .expect("the request is well formed");
        let response = app
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn ok(app: &Router, method: &str, uri: &str, body: Option<Value>) -> Value {
        let (status, body) = call(app, method, uri, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {body}");
        body
    }

    /// A room with two players and a round in which blue needs more
    /// than `percent`%.
    async fn ready(app: &Router, percent: u32) -> (String, String, String) {
        let keys = ok(app, "POST", "/api/rooms", None).await;
        let host = keys["host"].as_str().unwrap().to_string();
        let join = keys["join"].as_str().unwrap().to_string();
        let mut players = Vec::new();
        for name in ["Ada", "Bo"] {
            let body = ok(
                app,
                "POST",
                &format!("/api/j/{join}"),
                Some(json!({ "name": name })),
            )
            .await;
            players.push(body["player"].as_str().unwrap().to_string());
        }
        let started = ok(
            app,
            "POST",
            &format!("/api/h/{host}/start"),
            Some(json!({ "percent": percent })),
        )
        .await;
        assert_eq!(started["view"]["round"]["percent"], percent);
        (host, players.remove(0), players.remove(0))
    }

    async fn press(app: &Router, key: &str, colour: &str) -> Value {
        ok(
            app,
            "PUT",
            &format!("/api/p/{key}/press"),
            Some(json!({ "colour": colour })),
        )
        .await["view"]
            .clone()
    }

    #[tokio::test]
    async fn a_round_is_played_and_comes_out() {
        let (app, path) = app("round");
        let (host, ada, bo) = ready(&app, 40).await;
        assert_eq!(press(&app, &ada, "blue").await["mine"], "blue");
        press(&app, &bo, "red").await;

        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert!(view["outcome"].is_null(), "nothing before the reveal");

        let view = ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await["view"].clone();
        assert_eq!(view["outcome"]["blue"], 1);
        assert_eq!(view["outcome"]["blue_won"], true, "half beats 40%");
        assert_eq!(view["outcome"]["presses"][0]["name"], "Ada");
        assert_eq!(view["scores"][0]["lived"], 1);
        let mine = ok(&app, "GET", &format!("/api/p/{ada}"), None).await;
        assert_eq!(mine["view"]["lived"], true);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn nobody_is_sent_a_press_before_the_reveal() {
        let (app, path) = app("quiet");
        let (host, ada, bo) = ready(&app, 50).await;
        press(&app, &ada, "blue").await;

        let host_view = ok(&app, "GET", &format!("/api/h/{host}"), None)
            .await
            .to_string();
        assert!(!host_view.contains("\"colour\""), "{host_view}");
        let other = ok(&app, "GET", &format!("/api/p/{bo}"), None).await;
        assert!(other["view"]["mine"].is_null());
        assert_eq!(other["view"]["pressed"], 1, "a count and no more");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn what_is_not_a_button_or_a_share_is_refused() {
        let (app, path) = app("refused");
        let (host, ada, _) = ready(&app, 50).await;
        let (status, _) = call(
            &app,
            "PUT",
            &format!("/api/p/{ada}/press"),
            Some(json!({ "colour": "green" })),
        )
        .await;
        assert!(status.is_client_error(), "{status}");
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/h/{host}/start"),
            Some(json!({ "percent": 100 })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("less than 100%"));

        ok(
            &app,
            "PUT",
            &format!("/api/h/{host}/open"),
            Some(json!({ "open": false })),
        )
        .await;
        let (status, body) = call(
            &app,
            "PUT",
            &format!("/api/p/{ada}/press"),
            Some(json!({ "colour": "red" })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "This round is closed.");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_page_and_the_wall_are_served() {
        let (app, path) = app("page");
        let (host, _, _) = ready(&app, 50).await;
        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        let wall = view["wall"].as_str().unwrap();
        let seen = ok(&app, "GET", &format!("/api/w/{wall}"), None).await;
        assert_eq!(seen["view"]["game"]["round"]["percent"], 50);
        assert!(seen["view"]["game"]["shown"].is_null());
        let (status, _) = call(&app, "GET", "/", None).await;
        assert_eq!(status, StatusCode::OK);
        let _ = std::fs::remove_file(&path);
    }
}
