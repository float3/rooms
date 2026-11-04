//! Speed updating for a room of people.
//!
//! Everybody says where they stand on a set of statements, then talks to
//! whoever disagrees with them most, a few minutes at a time, and says
//! where they stand afterwards.
//!
//! Three kinds of link, and the key in each is the only authority there
//! is: `/h/<key>` runs the room, `/j/<key>` joins it, and `/p/<key>`
//! belongs to one player.

pub mod game;
mod statements;
mod view;
pub mod warmup;

use std::{
    io::Write,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use rooms::{Room, Rooms, http, now_ms, site};
use serde::Deserialize;
use tokio::sync::Mutex;

use game::{Game, Mode, Scale};

/// The page and its files. The script and stylesheet are compiled from
/// the TypeScript and CSS in `client/` by the build script.
const FILES: site::Files = site::Files {
    page: include_str!("../static/index.html"),
    icon: include_str!("../static/icon.svg"),
    script: include_str!(concat!(env!("OUT_DIR"), "/app.js")),
    stylesheet: include_str!(concat!(env!("OUT_DIR"), "/app.css")),
};

const MAX_ROOMS: usize = 2000;
/// Suggested statements are kept until somebody reads them; past this the
/// file takes no more, so nobody can fill the disk with it.
const MAX_SUGGESTIONS_BYTES: u64 = 1 << 20;

#[derive(Clone)]
struct App {
    rooms: Arc<Rooms<Game>>,
    suggestions: Arc<Mutex<PathBuf>>,
}

/// Applies a facilitator's action and answers with their whole view.
async fn act(
    app: &App,
    key: &str,
    action: impl FnOnce(&mut Game, u64) -> Result<(), &'static str>,
) -> Response {
    site::act(
        &app.rooms,
        key,
        |game, now| {
            game.tick(now);
            action(game, now)
        },
        |game, now| http::envelope(now, &view::host(game)),
    )
    .await
}

/// The same for a player: finds their room by their key, then acts.
async fn act_as(
    app: &App,
    key: &str,
    action: impl FnOnce(&mut Game, u64) -> Result<(), &'static str>,
) -> Response {
    let Some(room) = app
        .rooms
        .find(|room, game| game.player(key).map(|_| room.to_string()))
        .await
    else {
        return http::gone();
    };
    let done = app
        .rooms
        .with(&room, |game, now| {
            game.tick(now);
            action(game, now)?;
            view::player(game, key)
                .map(|v| http::envelope(now, &v))
                .ok_or("You are not in this room.")
        })
        .await;
    match done {
        None => http::gone(),
        Some(Err(message)) => http::refused(message),
        Some(Ok(body)) => http::json(body),
    }
}

#[derive(Deserialize)]
struct Setup {
    mode: Mode,
    #[serde(default)]
    scale: Scale,
    topics: Vec<String>,
    #[serde(default)]
    custom: String,
    side: u8,
}

#[derive(Deserialize)]
struct Minutes {
    minutes: u32,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Answer {
    id: u32,
    value: Option<i16>,
}

#[derive(Deserialize)]
struct Mark {
    id: u32,
    marked: bool,
}

#[derive(Deserialize)]
struct Pick {
    id: u32,
    picked: bool,
}

#[derive(Deserialize)]
struct Skip {
    id: u32,
}

#[derive(Deserialize)]
struct Guess {
    /// The partner, by name.
    name: String,
    value: i16,
}

#[derive(Deserialize)]
struct Kudos {
    name: String,
    given: bool,
}

#[derive(Deserialize)]
struct Warm {
    value: i16,
}

#[derive(Deserialize)]
struct Suggestion {
    text: String,
    #[serde(default)]
    chance: bool,
}

async fn open_room(State(app): State<App>) -> Response {
    let now = now_ms();
    site::open(&app.rooms, |_| Game::new(now), |game| game.join.clone()).await
}

async fn host_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |_, _| Ok(())).await
}

async fn host_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    rooms::one(app.rooms, key, |game| {
        serde_json::to_string(&view::host(game)).ok()
    })
}

async fn setup(State(app): State<App>, Path(key): Path<String>, Json(s): Json<Setup>) -> Response {
    act(&app, &key, |game, _| {
        game.setup(s.mode, s.scale, &s.topics, &s.custom, s.side)
    })
    .await
}

async fn minutes(
    State(app): State<App>,
    Path(key): Path<String>,
    Json(m): Json<Minutes>,
) -> Response {
    act(&app, &key, |game, _| game.set_minutes(m.minutes)).await
}

async fn round(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, now| game.start_round(now)).await
}

async fn stop(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.stop()).await
}

async fn extend(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, now| game.extend(now)).await
}

async fn close_picks(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.close_picks()).await
}

async fn warmup(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.start_warmup()).await
}

async fn warmup_next(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.warmup_next()).await
}

async fn warmup_end(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.end_warmup()).await
}

async fn guess(State(app): State<App>, Path(key): Path<String>, Json(g): Json<Guess>) -> Response {
    act_as(&app, &key, |game, _| game.guess(&key, &g.name, g.value)).await
}

async fn kudos(State(app): State<App>, Path(key): Path<String>, Json(k): Json<Kudos>) -> Response {
    act_as(&app, &key, |game, _| game.kudos(&key, &k.name, k.given)).await
}

async fn warm(State(app): State<App>, Path(key): Path<String>, Json(w): Json<Warm>) -> Response {
    act_as(&app, &key, |game, _| game.warm_answer(&key, w.value)).await
}

async fn finish(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.finish()).await
}

async fn reopen(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.reopen()).await
}

async fn join(
    State(app): State<App>,
    Path(join): Path<String>,
    Json(named): Json<Named>,
) -> Response {
    let Some(key) = app.rooms.key_where(|game| game.join == join).await else {
        return http::gone();
    };
    match app
        .rooms
        .with(&key, |game, now| game.join(&named.name, now))
        .await
    {
        None => http::gone(),
        Some(Err(message)) => http::refused(message),
        Some(Ok(player)) => Json(serde_json::json!({ "player": player })).into_response(),
    }
}

async fn whose(State(app): State<App>, Path(join): Path<String>) -> Response {
    let Some(key) = app.rooms.key_where(|game| game.join == join).await else {
        return http::gone();
    };
    match app.rooms.read(&key, |game, _| game.players.len()).await {
        Some(players) => Json(serde_json::json!({ "players": players })).into_response(),
        None => http::gone(),
    }
}

async fn player_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    act_as(&app, &key, |_, _| Ok(())).await
}

async fn player_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    rooms::stream(app.rooms, move |held, now| {
        let game = held.values_mut().find(|game| game.player(&key).is_some())?;
        game.last_seen = now;
        // The clock ends a round even when the facilitator's screen is
        // closed. Saved by the next request, like everything a watcher sees.
        game.tick(now);
        serde_json::to_string(&view::player(game, &key)?).ok()
    })
}

async fn answer(
    State(app): State<App>,
    Path(key): Path<String>,
    Json(a): Json<Answer>,
) -> Response {
    let player = key.clone();
    act_as(&app, &key, move |game, _| {
        game.answer(&player, a.id, a.value)
    })
    .await
}

async fn mark(State(app): State<App>, Path(key): Path<String>, Json(m): Json<Mark>) -> Response {
    let player = key.clone();
    act_as(&app, &key, move |game, _| {
        game.mark(&player, m.id, m.marked)
    })
    .await
}

async fn pick(State(app): State<App>, Path(key): Path<String>, Json(p): Json<Pick>) -> Response {
    let player = key.clone();
    act_as(&app, &key, move |game, _| {
        game.pick(&player, p.id, p.picked)
    })
    .await
}

async fn skip(State(app): State<App>, Path(key): Path<String>, Json(s): Json<Skip>) -> Response {
    let player = key.clone();
    act_as(&app, &key, move |game, _| game.skip(&player, s.id)).await
}

/// Keeps a statement somebody would like added, one JSON line each, for
/// whoever runs the server to read. Nothing reads it back over the web.
async fn suggest(State(app): State<App>, Json(s): Json<Suggestion>) -> Response {
    let text = game::tidy(&s.text, game::MAX_STATEMENT);
    if text.chars().count() < 8 {
        return http::refused("Write the statement out, so it can be added as it is.");
    }
    let file = app.suggestions.lock().await;
    match keep(&file, &text, s.chance, now_ms()) {
        Ok(()) => Json(serde_json::json!({ "kept": true })).into_response(),
        Err(message) => http::error(StatusCode::SERVICE_UNAVAILABLE, message),
    }
}

fn keep(path: &FsPath, text: &str, chance: bool, now: u64) -> Result<(), &'static str> {
    let full = "There are a lot of suggestions waiting already. Try again another day.";
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_SUGGESTIONS_BYTES) {
        return Err(full);
    }
    let line = serde_json::json!({ "at": now, "text": text, "chance": chance }).to_string();
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| writeln!(f, "{line}"))
        .map_err(|_| "The suggestion could not be saved. Try again later.")
}

/// The site, served at the root. Suggested statements are added to the
/// file at `suggestions`.
pub fn router(rooms: Arc<Rooms<Game>>, suggestions: PathBuf) -> Router {
    router_under(rooms, suggestions, "")
}

/// The site, for mounting under `base` with `Router::nest`, which is how
/// the shared room serves it next to the other tools.
pub fn router_under(rooms: Arc<Rooms<Game>>, suggestions: PathBuf, base: &str) -> Router {
    let app = App {
        rooms,
        suggestions: Arc::new(Mutex::new(suggestions)),
    };
    Router::new()
        .merge(site::routes_under(FILES, base))
        .route("/api/suggest", post(suggest))
        .route("/api/rooms", post(open_room))
        .route("/api/h/{key}", get(host_state))
        .route("/api/h/{key}/events", get(host_events))
        .route("/api/h/{key}/setup", put(setup))
        .route("/api/h/{key}/minutes", put(minutes))
        .route("/api/h/{key}/round", post(round))
        .route("/api/h/{key}/stop", post(stop))
        .route("/api/h/{key}/extend", post(extend))
        .route("/api/h/{key}/finish", post(finish))
        .route("/api/h/{key}/reopen", post(reopen))
        .route("/api/h/{key}/picks/close", post(close_picks))
        .route("/api/h/{key}/warmup", post(warmup))
        .route("/api/h/{key}/warmup/next", post(warmup_next))
        .route("/api/h/{key}/warmup/end", post(warmup_end))
        .route("/api/j/{key}", get(whose).post(join))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
        .route("/api/p/{key}/answer", put(answer))
        .route("/api/p/{key}/mark", put(mark))
        .route("/api/p/{key}/pick", put(pick))
        .route("/api/p/{key}/skip", put(skip))
        .route("/api/p/{key}/guess", put(guess))
        .route("/api/p/{key}/kudos", put(kudos))
        .route("/api/p/{key}/warm", put(warm))
        .with_state(app)
}

/// A room joined from the shared room, which joins everybody in it by
/// name.
impl rooms::tool::Joinable for Game {
    fn open(now: u64) -> Self {
        Game::new(now)
    }

    fn join(&mut self, name: &str, now: u64) -> Result<String, &'static str> {
        Game::join(self, name, now)
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
        http::{Request, header},
    };
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    struct Test {
        app: Router,
        files: [PathBuf; 2],
    }

    impl Drop for Test {
        fn drop(&mut self) {
            for f in &self.files {
                let _ = std::fs::remove_file(f);
            }
        }
    }

    fn test(name: &str) -> Test {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let state = dir.join(format!("updating-test-{name}-{id}.json"));
        let ideas = dir.join(format!("updating-test-{name}-{id}.jsonl"));
        let _ = std::fs::remove_file(&state);
        let _ = std::fs::remove_file(&ideas);
        let app = router(Rooms::load(&state, 4), ideas.clone());
        Test {
            app,
            files: [state, ideas],
        }
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
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
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

    /// An open room with two people in it.
    async fn ready(app: &Router) -> (String, String, String) {
        let keys = ok(app, "POST", "/api/rooms", None).await;
        let host = keys["host"].as_str().unwrap().to_string();
        let join = keys["join"].as_str().unwrap().to_string();
        ok(
            app,
            "PUT",
            &format!("/api/h/{host}/setup"),
            Some(json!({"mode": "open", "topics": ["mind"], "side": 4})),
        )
        .await;
        let mut players = Vec::new();
        for name in ["Ada", "Bo"] {
            let body = ok(
                app,
                "POST",
                &format!("/api/j/{join}"),
                Some(json!({"name": name})),
            )
            .await;
            players.push(body["player"].as_str().unwrap().to_string());
        }
        (host, players[0].clone(), players[1].clone())
    }

    #[tokio::test]
    async fn two_people_who_disagree_are_paired_and_see_each_other() {
        let t = test("pair");
        let (host, ada, bo) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 0, "value": 90})),
        )
        .await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/answer"),
            Some(json!({"id": 0, "value": 10})),
        )
        .await;

        let view = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(view["opposites"][0]["name"], "Bo", "Bo is the one to find");
        assert_eq!(view["opposites"][0]["gap"], 160);

        let host_view =
            ok(&t.app, "POST", &format!("/api/h/{host}/round"), None).await["view"].clone();
        assert_eq!(host_view["phase"], "talking");
        assert_eq!(host_view["round"]["groups"][0]["gap"], 160);

        let view = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(view["group"]["partners"][0]["name"], "Bo");
        assert_eq!(view["group"]["partners"][0]["answer"], 10);
        assert_eq!(view["group"]["before"], 90);

        ok(&t.app, "POST", &format!("/api/h/{host}/stop"), None).await;
        let view = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 0, "value": 40})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["group"]["after"], 40);
        assert_eq!(view["updated"], 1);
        let host_view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(host_view["updates"], 1);
        assert!(
            host_view["round"]["groups"][0]["people"][0]["after"].is_null(),
            "the wall waits until both have guessed"
        );

        // Bo cannot see where Ada landed until Bo has guessed it.
        let bo_view = ok(&t.app, "GET", &format!("/api/p/{bo}"), None).await["view"].clone();
        assert!(bo_view["group"]["partners"][0]["after"].is_null());
        let bo_view = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/guess"),
            Some(json!({"name": "Ada", "value": 50})),
        )
        .await["view"]
            .clone();
        assert_eq!(bo_view["group"]["partners"][0]["after"], 40);
        assert_eq!(bo_view["group"]["partners"][0]["guess"], 50);
        let (status, _) = call(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/guess"),
            Some(json!({"name": "Ada", "value": 40})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "a guess is final");

        // Once everybody has answered and guessed, the wall shows it.
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/guess"),
            Some(json!({"name": "Bo", "value": 15})),
        )
        .await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/answer"),
            Some(json!({"id": 0, "value": 20})),
        )
        .await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/kudos"),
            Some(json!({"name": "Bo", "given": true})),
        )
        .await;
        let host_view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        let group = &host_view["round"]["groups"][0];
        assert_eq!(group["settled"], true);
        assert_eq!(group["people"][0]["after"], 40);
        assert_eq!(group["spot"], "🦊 Fox");
        // 160 apart before, 40 after: 120 closed.
        assert_eq!(group["closed"], 120);
        let bo_score = host_view["leaders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "Bo")
            .unwrap()
            .clone();
        assert_eq!(bo_score["kudos"], 1);
    }

    #[tokio::test]
    async fn a_warm_up_question_goes_before_clue_then_after_then_the_answer() {
        let t = test("warmup");
        let (host, ada, _) = ready(&t.app).await;
        let view = ok(&t.app, "POST", &format!("/api/h/{host}/warmup"), None).await["view"].clone();
        assert_eq!(view["phase"], "warmup");
        assert_eq!(view["warmup"]["stage"], "before");
        assert!(view["warmup"]["outcome"].is_null(), "no answer yet");

        let mine = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/warm"),
            Some(json!({"value": 50})),
        )
        .await["view"]["warmup"]
            .clone();
        assert_eq!(mine["before"], 50);
        assert!(mine["clue"].is_null(), "no clue before the clues are dealt");
        let (status, _) = call(&t.app, "POST", &format!("/api/h/{host}/round"), None).await;
        assert_eq!(status, StatusCode::CONFLICT, "no round during a warm-up");

        ok(&t.app, "POST", &format!("/api/h/{host}/warmup/next"), None).await;
        let mine = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/warm"),
            Some(json!({"value": 70})),
        )
        .await["view"]["warmup"]
            .clone();
        assert_eq!(mine["after"], 70);
        assert!(mine["clue"].as_str().unwrap().ends_with('.'));
        assert!(mine["result"].is_null());

        let view =
            ok(&t.app, "POST", &format!("/api/h/{host}/warmup/next"), None).await["view"].clone();
        assert!(view["warmup"]["outcome"]["truth"].is_boolean());
        assert_eq!(
            view["calibrated"].as_array().unwrap().len(),
            2,
            "Ada and the bot"
        );
        let mine = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert!(mine["warmup"]["result"]["bits_after"].is_number());
        assert!(mine["calibration"].is_array());

        let view =
            ok(&t.app, "POST", &format!("/api/h/{host}/warmup/end"), None).await["view"].clone();
        assert_eq!(view["phase"], "answering");
        assert_eq!(view["warmups"], 1);
    }

    #[tokio::test]
    async fn a_player_is_not_sent_other_peoples_answers_beyond_their_partners() {
        let t = test("private");
        let (host, ada, bo) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 3, "value": 17})),
        )
        .await;
        let view = ok(&t.app, "GET", &format!("/api/p/{bo}"), None).await;
        assert!(
            view["view"]["opposites"].as_array().unwrap().is_empty(),
            "nothing in common yet"
        );
        let (status, _) = call(&t.app, "GET", &format!("/api/h/{ada}"), None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a player key does not run the room"
        );
        let (status, _) = call(&t.app, "GET", &format!("/api/p/{host}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn in_the_free_mode_who_answered_is_shown_only_after_answering() {
        let t = test("free");
        let (host, ada, bo) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/h/{host}/setup"),
            Some(json!({"mode": "free", "topics": ["mind"], "side": 4})),
        )
        .await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/answer"),
            Some(json!({"id": 2, "value": 23})),
        )
        .await;

        let before = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(
            before["squares"][2]["count"], 1,
            "Ada sees that somebody answered"
        );
        assert!(
            before["squares"][2]["others"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !before.to_string().contains("\"answer\":23"),
            "but not what or who"
        );

        let after = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 2, "value": 60})),
        )
        .await["view"]
            .clone();
        assert_eq!(after["squares"][2]["others"][0]["name"], "Bo");
        assert_eq!(after["squares"][2]["others"][0]["answer"], 23);
        assert_eq!(
            after["opposites"][0]["name"], "Bo",
            "and Bo is the one to go and find"
        );
    }

    #[tokio::test]
    async fn in_the_pick_mode_the_room_plays_what_people_picked() {
        let t = test("pick");
        let (host, ada, bo) = ready(&t.app).await;
        let view = ok(
            &t.app,
            "PUT",
            &format!("/api/h/{host}/setup"),
            Some(json!({"mode": "pick", "topics": ["mind"], "side": 4})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["phase"], "picking");
        let pool = view["deck"].as_u64().unwrap();

        let mine = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(mine["phase"], "picking");
        assert_eq!(mine["picks"], 3);
        assert_eq!(mine["squares"].as_array().unwrap().len() as u64, pool);
        let (status, _) = call(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 0, "value": 50})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "no answers while picking");

        for (who, id) in [(&ada, 5), (&ada, 2), (&bo, 2)] {
            ok(
                &t.app,
                "PUT",
                &format!("/api/p/{who}/pick"),
                Some(json!({"id": id, "picked": true})),
            )
            .await;
        }
        let view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["picking"]["pickers"], 2);
        assert_eq!(view["picking"]["picked"][0]["n"], 2);
        assert_eq!(view["picking"]["picked"][0]["statement"]["id"], 2);
        let (status, _) = call(&t.app, "POST", &format!("/api/h/{host}/round"), None).await;
        assert_eq!(status, StatusCode::CONFLICT, "no round while picking");

        let view =
            ok(&t.app, "POST", &format!("/api/h/{host}/picks/close"), None).await["view"].clone();
        assert_eq!(view["phase"], "answering");
        assert_eq!(view["deck"], 2);
        assert!(view["picking"].is_null());

        let mine = ok(&t.app, "GET", &format!("/api/p/{bo}"), None).await["view"].clone();
        let squares = mine["squares"].as_array().unwrap();
        assert_eq!(squares.len(), 2);
        assert_eq!(squares[0]["picked"], true, "Bo's pick, the most picked");
        assert_eq!(squares[1]["picked"], false);
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/answer"),
            Some(json!({"id": 1, "value": 30})),
        )
        .await;
    }

    #[tokio::test]
    async fn suggestions_are_kept_on_disk_and_never_served() {
        let t = test("suggest");
        ok(
            &t.app,
            "POST",
            "/api/suggest",
            Some(json!({"text": "  Octopuses   are conscious. ", "chance": false})),
        )
        .await;
        let (status, _) = call(&t.app, "POST", "/api/suggest", Some(json!({"text": "hi"}))).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let kept = std::fs::read_to_string(&t.files[1]).unwrap();
        assert_eq!(kept.lines().count(), 1);
        assert!(kept.contains("\"text\":\"Octopuses are conscious.\""));
        let (status, _) = call(&t.app, "GET", "/api/suggest", None).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn the_setup_is_refused_once_people_have_answered() {
        let t = test("lock");
        let (host, ada, _) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 0, "value": 0})),
        )
        .await;
        let (status, body) = call(
            &t.app,
            "PUT",
            &format!("/api/h/{host}/setup"),
            Some(json!({"mode": "bingo", "topics": ["ea"], "side": 3})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("started answering")
        );
    }

    #[tokio::test]
    async fn a_room_survives_the_server_restarting() {
        let t = test("restart");
        let (host, ada, _) = ready(&t.app).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/answer"),
            Some(json!({"id": 1, "value": 20})),
        )
        .await;
        let again = router(Rooms::load(&t.files[0], 4), t.files[1].clone());
        let view = ok(&again, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["answers"], 1);
    }

    #[tokio::test]
    async fn the_page_is_served_at_every_link() {
        let t = test("pages");
        for uri in [
            "/",
            "/h/abc",
            "/j/abc",
            "/p/abc",
            "/app.js",
            "/app.css",
            "/icon.svg",
        ] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = t.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
    }
}
