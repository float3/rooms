//! Number games for a room of people.
//!
//! The host picks a rule and a range, everyone picks a number on their
//! phone, and the rule decides who won. No number is shown to anybody
//! until the host reveals the round.
//!
//! There are four kinds of link, and the key in each is the only
//! authority there is: `/h/<key>` runs the room, `/j/<key>` joins it,
//! `/p/<key>` belongs to one player, and `/w/<key>` is the wall, a
//! read-only screen for a projector. Joining, who is connected, taking
//! somebody out and the wall are `rooms::lobby`'s.

pub mod answer;
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

use answer::Answers;
use game::{Game, Rule};

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
    #[serde(flatten)]
    rule: Rule,
    /// Whole numbers, decimals or expressions; by default, what the rule
    /// takes.
    #[serde(default)]
    answers: Option<Answers>,
    low: f64,
    high: f64,
}

#[derive(Deserialize)]
struct Opened {
    open: bool,
}

#[derive(Deserialize)]
struct Picked {
    /// What the player typed, read as the round's kind of answer.
    #[serde(default)]
    text: Option<String>,
    /// A number already read, as older pages send.
    #[serde(default)]
    value: Option<f64>,
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
    act(&rooms, &key, |game, now| {
        game.start_with(
            started.rule,
            started.answers,
            started.low,
            started.high,
            now,
        )
    })
    .await
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

async fn pick(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(picked): Json<Picked>,
) -> Response {
    lobby::act_as(
        &rooms,
        &key,
        |game, key, now| match (&picked.text, picked.value) {
            (Some(text), _) => game.pick_text(key, text, now).map(|_| ()),
            (None, Some(value)) => game.pick(key, value, now),
            (None, None) => Err("Type a number first."),
        },
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
        .route("/api/p/{key}/pick", put(pick))
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
            std::env::temp_dir().join(format!("numbers-test-{name}-{}.json", std::process::id()));
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

    /// A room with two players and a round of nought to ten, a third of
    /// the average.
    async fn ready(app: &Router) -> (String, String, String) {
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
        ok(
            app,
            "POST",
            &format!("/api/h/{host}/start"),
            Some(json!({"rule": "of-the-mean", "num": 1, "den": 3, "low": 0, "high": 10})),
        )
        .await;
        (host, players.remove(0), players.remove(0))
    }

    #[tokio::test]
    async fn an_expression_is_read_on_the_server_and_shown_as_typed() {
        let (app, path) = app("expression");
        let keys = ok(&app, "POST", "/api/rooms", None).await;
        let host = keys["host"].as_str().unwrap().to_string();
        let join = keys["join"].as_str().unwrap().to_string();
        let ada = ok(
            &app,
            "POST",
            &format!("/api/j/{join}"),
            Some(json!({ "name": "Ada" })),
        )
        .await["player"]
            .as_str()
            .unwrap()
            .to_string();
        let started = ok(
            &app,
            "POST",
            &format!("/api/h/{host}/start"),
            Some(json!({"rule": "of-the-mean", "num": 2, "den": 3, "answers": "expression", "low": 0, "high": 1000})),
        )
        .await;
        assert_eq!(started["view"]["round"]["answers"], "expression");

        let (status, _) = call(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({"text": "1/0"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let view = ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({"text": "2^10 - 24"})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["mine"], 1000.0);
        assert_eq!(view["mine_text"], "2^10 - 24");

        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await;
        assert_eq!(view["view"]["outcome"]["picks"][0]["text"], "2^10 - 24");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_round_is_played_and_comes_out() {
        let (app, path) = app("round");
        let (host, ada, bo) = ready(&app).await;

        let view = ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({"value": 9})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["mine"], 9.0);
        assert_eq!(view["round"]["label"], "closest to 1/3 of the average");
        ok(
            &app,
            "PUT",
            &format!("/api/p/{bo}/pick"),
            Some(json!({"value": 1})),
        )
        .await;

        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert!(view["outcome"].is_null(), "nothing before the reveal");

        let view = ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await["view"].clone();
        assert_eq!(view["outcome"]["mean"], 5.0);
        assert_eq!(view["outcome"]["winners"][0], "Bo");
        assert_eq!(view["outcome"]["picks"][0]["value"], 1.0);
        assert_eq!(view["scores"][0]["name"], "Bo");
        assert_eq!(view["scores"][0]["wins"], 1);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn nobody_is_sent_a_number_before_the_reveal() {
        let (app, path) = app("quiet");
        let (host, ada, bo) = ready(&app).await;
        ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({"value": 7})),
        )
        .await;

        let host_view = ok(&app, "GET", &format!("/api/h/{host}"), None)
            .await
            .to_string();
        assert!(!host_view.contains("\"value\""), "{host_view}");
        let other = ok(&app, "GET", &format!("/api/p/{bo}"), None).await;
        assert!(other["view"]["mine"].is_null());
        assert_eq!(other["view"]["picked"], 1, "a count and no more");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_number_outside_the_range_is_refused() {
        let (app, path) = app("range");
        let (_, ada, _) = ready(&app).await;
        let (status, body) = call(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({"value": 11})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(
            body["error"],
            "That number is outside the range for this round."
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn every_rule_can_be_started() {
        let (app, path) = app("rules");
        let keys = ok(&app, "POST", "/api/rooms", None).await;
        let host = keys["host"].as_str().unwrap().to_string();
        for rule in [
            json!({"rule": "of-the-mean", "num": 2, "den": 3, "low": 0, "high": 100}),
            json!({"rule": "of-the-median", "num": 1, "den": 2, "low": 0, "high": 100}),
            json!({"rule": "lowest-unique", "low": 1, "high": 20}),
            json!({"rule": "highest-unique", "low": 1, "high": 20}),
            json!({"rule": "under-the-mean", "low": 0, "high": 100}),
        ] {
            let view =
                ok(&app, "POST", &format!("/api/h/{host}/start"), Some(rule)).await["view"].clone();
            assert!(view["round"]["label"].is_string());
        }
        let (status, _) = call(
            &app,
            "POST",
            &format!("/api/h/{host}/start"),
            Some(json!({"rule": "of-the-mean", "num": 5, "den": 3, "low": 0, "high": 10})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "5/3 of the average runs away");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_host_plays_the_same_rule_again_after_the_reveal() {
        let (app, path) = app("again");
        let (host, ada, _) = ready(&app).await;
        let again = format!("/api/h/{host}/again");
        let (status, _) = call(&app, "POST", &again, None).await;
        assert_eq!(status, StatusCode::CONFLICT, "not before the reveal");
        ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({"value": 6})),
        )
        .await;
        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        let view = ok(&app, "POST", &again, None).await["view"].clone();
        assert_eq!(view["round"]["label"], "closest to 1/3 of the average");
        assert_eq!(
            (&view["round"]["low"], &view["round"]["high"]),
            (&json!(0.0), &json!(10.0))
        );
        assert_eq!(view["round"]["revealed"], false);
        let (status, _) = call(&app, "POST", &format!("/api/h/{ada}/again"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "a player cannot");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn one_kind_of_key_does_not_work_as_another() {
        let (app, path) = app("keys");
        let (host, ada, _) = ready(&app).await;
        for uri in [
            format!("/api/h/{ada}"),
            format!("/api/p/{host}"),
            "/api/h/notakey".to_string(),
            "/api/p/notakey".to_string(),
            "/api/j/notakey".to_string(),
        ] {
            let (status, _) = call(&app, "GET", &uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (status, _) = call(&app, "POST", &format!("/api/h/{ada}/reveal"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "a player cannot reveal");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_room_survives_the_server_restarting() {
        let (app, path) = app("restart");
        let (host, ada, _) = ready(&app).await;
        ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({"value": 4})),
        )
        .await;
        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;

        let again = router(Rooms::load(&path, 4));
        let view = ok(&again, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["outcome"]["picks"][0]["value"], 4.0);

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
    async fn a_name_in_use_is_refused_and_a_kept_key_gets_back_in() {
        let (app, path) = app("names");
        let keys = ok(&app, "POST", "/api/rooms", None).await;
        let join = keys["join"].as_str().unwrap().to_string();
        let at = format!("/api/j/{join}");
        let ada = ok(&app, "POST", &at, Some(json!({ "name": "Ada" }))).await["player"]
            .as_str()
            .unwrap()
            .to_string();

        // Ada has only just joined, so her phone counts as connected.
        let (status, body) = call(&app, "POST", &at, Some(json!({ "name": " ada " }))).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(
            body["error"],
            "That name is taken. Add an initial, or pick another."
        );

        let back = ok(
            &app,
            "POST",
            &at,
            Some(json!({ "name": "Ada", "key": ada })),
        )
        .await;
        assert_eq!(back["player"], ada.as_str(), "the kept key is hers");
        assert_eq!(ok(&app, "GET", &at, None).await["players"], 1);

        let _ = std::fs::remove_file(&path);
    }

    /// The host takes `name` out, by the id the host's view gives them,
    /// and the server answers with nothing.
    async fn remove(app: &Router, host: &str, name: &str) {
        let view = ok(app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        let id = view["people"]
            .as_array()
            .unwrap()
            .iter()
            .find(|person| person["name"] == name)
            .expect("they are in")["id"]
            .clone();
        let (status, body) = call(
            app,
            "POST",
            &format!("/api/h/{host}/remove"),
            Some(json!({ "id": id })),
        )
        .await;
        assert_eq!((status, body), (StatusCode::NO_CONTENT, Value::Null));
    }

    #[tokio::test]
    async fn somebody_the_host_takes_out_is_gone() {
        let (app, path) = app("remove");
        let (host, ada, bo) = ready(&app).await;
        for (key, value) in [(&ada, 9), (&bo, 1)] {
            ok(
                &app,
                "PUT",
                &format!("/api/p/{key}/pick"),
                Some(json!({ "value": value })),
            )
            .await;
        }

        remove(&app, &host, "Ada").await;
        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["people"].as_array().unwrap().len(), 1);
        assert_eq!(view["people"][0]["name"], "Bo");

        for (method, uri, body) in [
            ("GET", format!("/api/p/{ada}"), None),
            ("GET", format!("/api/p/{ada}/events"), None),
            (
                "PUT",
                format!("/api/p/{ada}/pick"),
                Some(json!({ "value": 2 })),
            ),
        ] {
            let (status, _) = call(&app, method, &uri, body).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        }

        // Her number no longer counts in the round being played.
        let view = ok(&app, "GET", &format!("/api/p/{bo}"), None).await["view"].clone();
        assert_eq!((&view["picked"], &view["players"]), (&json!(1), &json!(1)));
        let view = ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await["view"].clone();
        assert_eq!(view["outcome"]["n"], 1);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn taking_somebody_out_after_a_reveal_does_not_change_who_won() {
        let (app, path) = app("remove-after");
        let (host, ada, bo) = ready(&app).await;
        for (key, value) in [(&ada, 1), (&bo, 9)] {
            ok(
                &app,
                "PUT",
                &format!("/api/p/{key}/pick"),
                Some(json!({ "value": value })),
            )
            .await;
        }
        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        remove(&app, &host, "Ada").await;
        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["outcome"]["winners"], json!(["Ada"]));
        assert_eq!(view["outcome"]["mean"], 5.0);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_wall_shows_counts_and_never_whose_number_is_whose() {
        let (app, path) = app("wall");
        let (host, ada, bo) = ready(&app).await;
        let hosting = ok(&app, "GET", &format!("/api/h/{host}"), None).await;
        let wall = hosting["view"]["wall"].as_str().unwrap().to_string();
        let join = hosting["view"]["join"].as_str().unwrap().to_string();
        ok(
            &app,
            "PUT",
            &format!("/api/p/{ada}/pick"),
            Some(json!({ "value": 7.25 })),
        )
        .await;

        let seen = ok(&app, "GET", &format!("/api/w/{wall}"), None).await;
        assert_eq!(seen["view"]["busy"], true, "a round has the wall");
        assert_eq!(seen["view"]["connected"], 2);
        let game = &seen["view"]["game"];
        assert_eq!(game["round"]["label"], "closest to 1/3 of the average");
        assert_eq!((&game["picked"], &game["players"]), (&json!(1), &json!(2)));
        assert!(game["shown"].is_null());
        let text = seen.to_string();
        for secret in [host.as_str(), &ada, &bo, "7.25", "\"value\""] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        for key in [&host, &join, &ada] {
            let (status, _) = call(&app, "GET", &format!("/api/w/{key}"), None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{key} is not the wall's");
        }

        ok(
            &app,
            "PUT",
            &format!("/api/p/{bo}/pick"),
            Some(json!({ "value": 1 })),
        )
        .await;
        ok(&app, "POST", &format!("/api/h/{host}/reveal"), None).await;
        let game = ok(&app, "GET", &format!("/api/w/{wall}"), None).await["view"]["game"].clone();
        let shown = &game["shown"];
        assert_eq!(shown["winners"], 1);
        assert_eq!(
            shown["spots"],
            json!([
                { "value": 1.0, "count": 1, "won": true },
                { "value": 7.25, "count": 1, "won": false },
            ])
        );
        assert_eq!(
            game["top"],
            json!([{ "name": "Bo", "wins": 1, "solo": 1, "played": 1 }])
        );
        let text = game.to_string();
        assert!(
            !text.contains("Ada"),
            "names only on the top of the board: {text}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_shared_room_rejoins_by_key_and_can_take_somebody_out() {
        use rooms::tool::Joinable;
        let mut game = Game::new(1000);
        let ada = game.join("Ada", 1000).unwrap();
        assert!(game.join("Ada", 1001).is_err(), "her phone is still here");
        assert_eq!(game.rejoin(Some(&ada), "Ada", 1001), Ok(ada.clone()));
        assert_eq!(game.wall(), Some(game.lobby.wall.as_str()));
        assert!(game.leave(&ada));
        assert!(!game.leave(&ada));
        assert!(game.player(&ada).is_none());
    }
}
