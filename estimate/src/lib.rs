//! The estimation game for a room of people.
//!
//! The host opens a room, puts the join link on the wall, and asks a
//! question. Each phone shows two fields. The host's screen shows the
//! room's bounds and, once the answer has been entered, how many of the
//! ranges contained it.
//!
//! There are four kinds of link, and the key in each is the only
//! authority there is: `/h/<key>` runs the room, `/j/<key>` joins it,
//! `/p/<key>` belongs to one player, and `/w/<key>` is the wall, a
//! read-only screen for the projector. Joining, the host taking somebody
//! out, and the wall's routes come from `rooms::lobby`.

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

use game::{Game, Stage};

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

/// Applies a host action to the room and returns the host's view.
///
/// Every host request answers with the whole view, so the screen that
/// pressed the button does not wait for its own event stream.
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
struct Asked {
    question: String,
    #[serde(default)]
    unit: String,
    /// The host can enter the answer before asking or afterwards.
    #[serde(default)]
    answer: Option<f64>,
}

/// A pasted list of questions: see `game::parse_queue`.
#[derive(Deserialize)]
struct Lined {
    text: String,
}

#[derive(Deserialize)]
struct Staged {
    stage: Stage,
}

#[derive(Deserialize)]
struct Answered {
    answer: Option<f64>,
    /// Which question, by its place in the room's list, oldest first.
    /// Missing means the one the room is on.
    #[serde(default)]
    question: Option<usize>,
}

#[derive(Deserialize)]
struct Confidence {
    confidence: u8,
}

#[derive(Deserialize)]
struct Bounds {
    low: f64,
    high: f64,
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

async fn ask(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(asked): Json<Asked>,
) -> Response {
    act(&rooms, &key, |game, now| {
        game.ask(&asked.question, &asked.unit, asked.answer, now)
    })
    .await
}

/// Replaces the questions lined up with the pasted list. A line that
/// cannot be read is refused by its number, and the list is left as it
/// was.
async fn set_queue(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(lined): Json<Lined>,
) -> Response {
    match game::parse_queue(&lined.text) {
        Ok(queue) => act(&rooms, &key, |game, _| game.set_queue(queue)).await,
        Err(message) => http::refused(&message),
    }
}

/// Asks the first question lined up.
async fn ask_next(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    act(&rooms, &key, |game, now| game.ask_next(now)).await
}

/// Moves the question on: to the discussion, to the second ranges, or
/// back a step for somebody who missed one.
async fn set_stage(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(staged): Json<Staged>,
) -> Response {
    act(&rooms, &key, |game, _| game.move_to(staged.stage)).await
}

async fn set_answer(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(answered): Json<Answered>,
) -> Response {
    act(&rooms, &key, |game, _| match answered.question {
        Some(index) => game.set_answer_of(index, answered.answer),
        None => game.set_answer(answered.answer),
    })
    .await
}

async fn reveal(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    act(&rooms, &key, |game, _| game.reveal()).await
}

async fn set_confidence(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(level): Json<Confidence>,
) -> Response {
    act(&rooms, &key, |game, _| {
        game.set_confidence(level.confidence)
    })
    .await
}

/// A player's view as JSON, or `None` for a key that is not in the room.
fn player_json(game: &Game, key: &str, _now: u64) -> Option<String> {
    serde_json::to_string(&view::player(game, key)?).ok()
}

async fn player_state(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    lobby::player_state(&rooms, &key, player_json).await
}

async fn player_events(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    lobby::player_events(rooms, key, player_json).await
}

async fn bounds(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(bounds): Json<Bounds>,
) -> Response {
    lobby::act_as(
        &rooms,
        &key,
        |game, key, now| game.bound(key, bounds.low, bounds.high, now),
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
        .route("/api/h/{key}/ask", post(ask))
        .route("/api/h/{key}/queue", put(set_queue))
        .route("/api/h/{key}/next", post(ask_next))
        .route("/api/h/{key}/stage", put(set_stage))
        .route("/api/h/{key}/answer", put(set_answer))
        .route("/api/h/{key}/reveal", post(reveal))
        .route("/api/h/{key}/confidence", put(set_confidence))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
        .route("/api/p/{key}/bounds", put(bounds))
        .with_state(rooms)
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

/// Loads the rooms kept at `state`, and deletes the ones nobody has
/// opened in a day as it goes.
pub fn load(state: impl Into<PathBuf>) -> Arc<Rooms<Game>> {
    let rooms = Rooms::load(state.into(), MAX_ROOMS);
    tokio::spawn(rooms.clone().sweep(rooms::SWEEP));
    rooms
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

    /// A router with its own state file, which the test deletes after.
    fn app(name: &str) -> (Router, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("estimate-test-{name}-{}.json", std::process::id()));
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
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    async fn ok(app: &Router, method: &str, uri: &str, body: Option<Value>) -> Value {
        let (status, body) = call(app, method, uri, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {body}");
        body
    }

    /// A room with one player in it and one question open.
    async fn ready(app: &Router) -> (String, String) {
        let keys = ok(app, "POST", "/api/rooms", None).await;
        let host = keys["host"].as_str().expect("a host key").to_string();
        let join = keys["join"].as_str().expect("a join key").to_string();
        let player = ok(
            app,
            "POST",
            &format!("/api/j/{join}"),
            Some(json!({"name": "Ada"})),
        )
        .await["player"]
            .as_str()
            .expect("a player key")
            .to_string();
        ok(
            app,
            "POST",
            &format!("/api/h/{host}/ask"),
            Some(json!({"question": "How many lamps?", "unit": "lamps", "answer": 1234567})),
        )
        .await;
        (host, player)
    }

    #[tokio::test]
    async fn a_question_goes_round_the_room_and_comes_back_as_numbers() {
        let (app, path) = app("round");
        let (host, player) = ready(&app).await;

        let view = ok(
            &app,
            "PUT",
            &format!("/api/p/{player}/bounds"),
            Some(json!({"low": 2000, "high": 3000})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["mine_before"]["low"], 2000.0);
        assert_eq!(view["round"]["question"], "How many lamps?");
        assert_eq!(view["submitted"], 1);

        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["standing"]["n"], 1);
        assert_eq!(view["standing"]["median_low"], 2000.0);
        assert_eq!(view["standing"]["hits"], 0);
        assert_eq!(view["answers"][0]["name"], "Ada");
        assert_eq!(view["players"][0]["first"], true);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_player_is_not_sent_the_answer_until_it_is_revealed() {
        let (app, path) = app("reveal");
        let (host, player) = ready(&app).await;
        ok(
            &app,
            "PUT",
            &format!("/api/p/{player}/bounds"),
            Some(json!({"low": 1000000, "high": 2000000})),
        )
        .await;

        let before = ok(&app, "GET", &format!("/api/p/{player}"), None).await;
        assert!(
            !before.to_string().contains("1234567"),
            "the answer went out early: {before}"
        );

        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        let after = ok(&app, "GET", &format!("/api/p/{player}"), None).await["view"].clone();
        assert_eq!(after["round"]["answer"], 1234567.0);
        assert_eq!(after["hit"], true);
        assert_eq!(after["standing"]["n"], 1);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_room_answers_twice_and_the_change_is_counted() {
        let (app, path) = app("twice");
        let (host, player) = ready(&app).await;
        ok(
            &app,
            "PUT",
            &format!("/api/p/{player}/bounds"),
            Some(json!({"low": 0, "high": 10})),
        )
        .await;

        ok(
            &app,
            "PUT",
            &format!("/api/h/{host}/stage"),
            Some(json!({"stage": "discussion"})),
        )
        .await;
        let (status, body) = call(
            &app,
            "PUT",
            &format!("/api/p/{player}/bounds"),
            Some(json!({"low": 1, "high": 2})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "The room is talking this one over.");

        ok(
            &app,
            "PUT",
            &format!("/api/h/{host}/stage"),
            Some(json!({"stage": "second"})),
        )
        .await;
        let view = ok(
            &app,
            "PUT",
            &format!("/api/p/{player}/bounds"),
            Some(json!({"low": 1000000, "high": 2000000})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["mine_before"]["high"], 10.0, "the first one stands");
        assert_eq!(view["mine_after"]["high"], 2000000.0);

        let view = ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await["view"].clone();
        assert_eq!(view["shift"]["n"], 1);
        assert_eq!(view["shift"]["moved"], 1);
        assert_eq!(view["shift"]["hits_before"], 0);
        assert_eq!(view["shift"]["hits_after"], 1, "1234567 is in the second");
        assert_eq!(view["shift"]["towards"], 1);
        assert_eq!(view["scores"][0]["changed_mind"], 1);

        let mine = ok(&app, "GET", &format!("/api/p/{player}"), None).await["view"].clone();
        assert_eq!(mine["mine_changed"]["after"]["low"], 1000000.0);
        assert_eq!(mine["hit"], true);
        assert_eq!(mine["hit_before"], false);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_level_is_fixed_while_a_question_is_open() {
        let (app, path) = app("level");
        let (host, _) = ready(&app).await;
        let uri = format!("/api/h/{host}/confidence");
        let (status, body) = call(&app, "PUT", &uri, Some(json!({"confidence": 50}))).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        let view = ok(&app, "PUT", &uri, Some(json!({"confidence": 50}))).await["view"].clone();
        assert_eq!(view["confidence"], 50);
        assert_eq!(
            view["round"]["confidence"], 90,
            "the revealed one keeps its level"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_pasted_list_is_asked_one_at_a_time() {
        let (app, path) = app("queue");
        let (host, _) = ready(&app).await;
        let uri = format!("/api/h/{host}/queue");
        let (status, body) = call(
            &app,
            "PUT",
            &uri,
            Some(json!({"text": "One | 1\nTwo | lots"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body["error"].as_str().unwrap().starts_with("Line 2"),
            "{body}"
        );

        let view = ok(
            &app,
            "PUT",
            &uri,
            Some(json!({"text": "One | 1 | m\nTwo | 2"})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["queue"][1]["question"], "Two");
        let view = ok(&app, "POST", &format!("/api/h/{host}/next"), None).await["view"].clone();
        assert_eq!(view["round"]["question"], "One");
        assert_eq!(view["round"]["unit"], "m");
        assert_eq!(view["history"][0]["question"], "How many lamps?");
        assert_eq!(view["queue"].as_array().map(Vec::len), Some(1));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_revealed_question_takes_no_more_bounds() {
        let (app, path) = app("closed");
        let (host, player) = ready(&app).await;
        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        let (status, body) = call(
            &app,
            "PUT",
            &format!("/api/p/{player}/bounds"),
            Some(json!({"low": 1, "high": 2})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "This question is closed.");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn one_kind_of_key_does_not_work_as_another() {
        let (app, path) = app("keys");
        let (host, player) = ready(&app).await;
        for uri in [
            format!("/api/h/{player}"),
            format!("/api/p/{host}"),
            "/api/h/notakey".to_string(),
            "/api/p/notakey".to_string(),
            "/api/j/notakey".to_string(),
            "/api/w/notakey".to_string(),
        ] {
            let (status, _) = call(&app, "GET", &uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (status, _) = call(&app, "POST", &format!("/api/h/{player}/reveal"), None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a player cannot run the room"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_page_is_served_at_every_link_a_phone_might_be_handed() {
        let (app, path) = app("pages");
        for uri in ["/", "/h/abc", "/j/abc", "/p/abc", "/w/abc"] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_room_survives_the_server_restarting() {
        let (app, path) = app("restart");
        let (host, player) = ready(&app).await;
        ok(
            &app,
            "PUT",
            &format!("/api/p/{player}/bounds"),
            Some(json!({"low": 1, "high": 9})),
        )
        .await;

        let again = router(Rooms::load(&path, 4));
        let view = ok(&again, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["standing"]["n"], 1, "the room came back off disk");
        assert_eq!(view["answers"][0]["name"], "Ada");

        let _ = std::fs::remove_file(&path);
    }

    /// A state file as the server wrote it before this version: one
    /// room saved with the lobby flattened into it, and one saved before
    /// the lobby, with no wall key.
    #[tokio::test]
    async fn rooms_saved_in_the_older_shapes_load_and_work() {
        let path = std::env::temp_dir().join(format!(
            "estimate-test-old-shapes-{}.json",
            std::process::id()
        ));
        let now = now_ms();
        let flat = json!({
            "join": "JOIN1", "wall": "WALL1", "confidence": 90, "last_seen": now,
            "players": [{"key": "P1", "name": "Ada", "seen": now}],
            "rounds": [{"question": "How many?", "unit": "", "answer": 5.0,
                "stage": "first", "asked": now,
                "before": {"P1": {"low": 1.0, "high": 9.0, "at": now}}, "after": {}}],
        });
        let older = json!({
            "join": "JOIN2", "confidence": 50, "last_seen": now, "rounds": [],
            "players": [{"key": "P2", "name": "Bo"}],
        });
        std::fs::write(&path, json!({"HOST1": flat, "HOST2": older}).to_string()).unwrap();
        let app = router(Rooms::load(&path, 4));

        let view = ok(&app, "GET", "/api/h/HOST1", None).await["view"].clone();
        assert_eq!(view["standing"]["n"], 1);
        assert_eq!(view["people"][0]["name"], "Ada");
        assert_eq!(
            ok(&app, "GET", "/api/p/P1", None).await["view"]["name"],
            "Ada"
        );
        assert_eq!(ok(&app, "GET", "/api/j/JOIN1", None).await["players"], 1);
        let wall = ok(&app, "GET", "/api/w/WALL1", None).await;
        assert_eq!(wall["view"]["game"]["round"]["question"], "How many?");

        let view = ok(&app, "GET", "/api/h/HOST2", None).await["view"].clone();
        assert_eq!(view["confidence"], 50);
        assert_eq!(view["people"][0]["name"], "Bo");
        let wall = view["wall"].as_str().expect("given a wall key").to_string();
        ok(&app, "GET", &format!("/api/w/{wall}"), None).await;

        let kept = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry.file_name().to_string_lossy().starts_with(&format!(
                    "{}.bad-",
                    path.file_name().unwrap().to_string_lossy()
                ))
            });
        assert!(!kept, "nothing failed to load");
        let _ = std::fs::remove_file(&path);
    }

    /// The join key of the room `host` runs, and its wall key.
    async fn keys_of(app: &Router, host: &str) -> (String, String) {
        let view = ok(app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        (
            view["join"].as_str().expect("a join key").to_string(),
            view["wall"].as_str().expect("a wall key").to_string(),
        )
    }

    #[tokio::test]
    async fn a_name_in_use_is_refused_and_a_kept_key_gets_back_in() {
        let (app, path) = app("names");
        let (host, ada) = ready(&app).await;
        let (join, _) = keys_of(&app, &host).await;

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/j/{join}"),
            Some(json!({"name": " ada "})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "Ada is still here: {body}");
        assert_eq!(
            body["error"],
            "That name is taken. Add an initial, or pick another."
        );

        let back = ok(
            &app,
            "POST",
            &format!("/api/j/{join}"),
            Some(json!({"name": "Ada", "key": ada})),
        )
        .await;
        assert_eq!(back["player"], ada.as_str(), "the phone kept its key");

        let count = ok(&app, "GET", &format!("/api/j/{join}"), None).await;
        assert_eq!(count["players"], 1);
        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["people"][0]["name"], "Ada");
        assert!(
            !view["people"].to_string().contains(&ada),
            "the roster carries no keys"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_player_the_host_removes_is_gone() {
        let (app, path) = app("remove");
        let (host, ada) = ready(&app).await;
        ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/bounds"),
            Some(json!({"low": 1, "high": 2})),
        )
        .await;

        let id = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"]["people"][0]["id"]
            .clone();
        let (status, _) = call(
            &app,
            "POST",
            &format!("/api/h/{ada}/remove"),
            Some(json!({ "id": id })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "a player cannot remove");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/h/{host}/remove"),
            Some(json!({ "id": id })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(body, Value::Null, "nothing but the status");
        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["people"], json!([]));

        let (status, _) = call(&app, "GET", &format!("/api/p/{ada}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(&app, "GET", &format!("/api/p/{ada}/events"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            &app,
            "PUT",
            &format!("/api/p/{ada}/bounds"),
            Some(json!({"low": 1, "high": 2})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["standing"], Value::Null, "their range went with them");
        assert_eq!(view["answers"], json!([]));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_wall_tells_the_room_nothing_until_the_reveal() {
        let (app, path) = app("wall");
        let (host, ada) = ready(&app).await;
        let (join, wall) = keys_of(&app, &host).await;
        ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/bounds"),
            Some(json!({"low": 1000001, "high": 1999999})),
        )
        .await;

        for stage in ["discussion", "second"] {
            let body = ok(&app, "GET", &format!("/api/w/{wall}"), None).await;
            let text = body.to_string();
            for secret in ["1234567", "1000001", "1999999", &host, &ada] {
                assert!(!text.contains(secret), "{secret} on the wall: {text}");
            }
            assert_eq!(body["view"]["join"], join.as_str(), "for the QR code");
            assert_eq!(body["view"]["game"]["round"]["question"], "How many lamps?");
            ok(
                &app,
                "PUT",
                &format!("/api/h/{host}/stage"),
                Some(json!({"stage": stage})),
            )
            .await;
        }

        // Only the wall's own key opens the wall, and it runs nothing.
        for key in [&host, &join, &ada] {
            let (status, _) = call(&app, "GET", &format!("/api/w/{key}"), None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{key}");
        }
        for (method, uri) in [
            ("POST", format!("/api/h/{wall}/reveal")),
            ("GET", format!("/api/h/{wall}")),
            ("GET", format!("/api/p/{wall}")),
        ] {
            let (status, _) = call(&app, method, &uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        }

        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        let game = ok(&app, "GET", &format!("/api/w/{wall}"), None).await["view"]["game"].clone();
        let reveal = &game["round"]["reveal"];
        assert_eq!(reveal["answer"], 1234567.0);
        assert_eq!(reveal["ranges"][0]["low"], 1000001.0);
        assert_eq!(reveal["ranges"][0]["hit"], true);
        assert_eq!(reveal["ranges"][0].get("name"), None, "no names on ranges");
        assert_eq!(game["top"][0]["name"], "Ada");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_link_becomes_a_qr_code() {
        let (app, path) = app("qr");
        let request = Request::builder()
            .method("POST")
            .uri("/api/qr")
            .body(Body::from("https://estimate.rationality-munich.com/j/abc"))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&bytes).contains("<svg"));
        let _ = std::fs::remove_file(&path);
    }
}
