//! Speed updating for a room of people.
//!
//! Everybody says where they stand on a set of statements, then talks to
//! whoever disagrees with them most, a few minutes at a time, and says
//! where they stand afterwards.
//!
//! Four kinds of link, and the key in each is the only authority there
//! is: `/h/<key>` runs the room, `/j/<key>` joins it, `/p/<key>` belongs
//! to one player, and `/w/<key>` is the wall, a read-only screen for a
//! projector. Joining, who is connected, the host taking somebody out and
//! the wall's frame are `rooms::lobby`'s.

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
use rooms::{Room, Rooms, http, lobby, now_ms, site};
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
        |game, now| http::envelope(now, &view::host(game, now)),
    )
    .await
}

/// The same for a player, found by their key, answering with their view.
async fn act_as(
    app: &App,
    key: &str,
    action: impl FnOnce(&mut Game, &str, u64) -> Result<(), &'static str>,
) -> Response {
    lobby::act_as(
        &app.rooms,
        key,
        |game, key, now| {
            game.tick(now);
            action(game, key, now)
        },
        |game, key, now| http::envelope(now, &view::player(game, key)),
    )
    .await
}

/// A player's view as JSON, for their page and their stream.
fn mine(game: &Game, key: &str, _now: u64) -> Option<String> {
    serde_json::to_string(&view::player(game, key)?).ok()
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
struct Away {
    away: bool,
}

/// The host marking somebody away, by the id the host's screen has.
#[derive(Deserialize)]
struct AwayById {
    id: u32,
    away: bool,
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
    site::open(
        &app.rooms,
        |_| Game::new(now),
        |game| game.lobby.join.clone(),
    )
    .await
}

async fn host_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |_, _| Ok(())).await
}

async fn host_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    rooms::one_at(app.rooms, key, |game, now| {
        serde_json::to_string(&view::host(game, now)).ok()
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
    act_as(&app, &key, |game, key, _| game.guess(key, &g.name, g.value)).await
}

async fn kudos(State(app): State<App>, Path(key): Path<String>, Json(k): Json<Kudos>) -> Response {
    act_as(&app, &key, |game, key, _| game.kudos(key, &k.name, k.given)).await
}

async fn warm(State(app): State<App>, Path(key): Path<String>, Json(w): Json<Warm>) -> Response {
    act_as(&app, &key, |game, key, _| game.warm_answer(key, w.value)).await
}

async fn finish(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.finish()).await
}

async fn reopen(State(app): State<App>, Path(key): Path<String>) -> Response {
    act(&app, &key, |game, _| game.reopen()).await
}

async fn player_state(State(app): State<App>, Path(key): Path<String>) -> Response {
    lobby::player_state(&app.rooms, &key, mine).await
}

/// The player's stream. While it is open they count as connected, and it
/// ends a round on the clock even when the host's screen is closed.
async fn player_events(State(app): State<App>, Path(key): Path<String>) -> Response {
    lobby::player_events(app.rooms, key, mine).await
}

async fn answer(
    State(app): State<App>,
    Path(key): Path<String>,
    Json(a): Json<Answer>,
) -> Response {
    act_as(&app, &key, |game, key, _| game.answer(key, a.id, a.value)).await
}

async fn mark(State(app): State<App>, Path(key): Path<String>, Json(m): Json<Mark>) -> Response {
    act_as(&app, &key, |game, key, _| game.mark(key, m.id, m.marked)).await
}

async fn pick(State(app): State<App>, Path(key): Path<String>, Json(p): Json<Pick>) -> Response {
    act_as(&app, &key, |game, key, _| game.pick(key, p.id, p.picked)).await
}

async fn away(State(app): State<App>, Path(key): Path<String>, Json(a): Json<Away>) -> Response {
    act_as(&app, &key, |game, key, _| game.set_away(key, a.away)).await
}

async fn host_away(
    State(app): State<App>,
    Path(key): Path<String>,
    Json(a): Json<AwayById>,
) -> Response {
    act(&app, &key, |game, _| game.set_away_by_id(a.id, a.away)).await
}

async fn skip(State(app): State<App>, Path(key): Path<String>, Json(s): Json<Skip>) -> Response {
    act_as(&app, &key, |game, key, _| game.skip(key, s.id)).await
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
        rooms: rooms.clone(),
        suggestions: Arc::new(Mutex::new(suggestions)),
    };
    Router::new()
        .merge(site::routes_under(FILES, base))
        // Joining, the host taking somebody out, and the wall.
        .merge(lobby::routes(rooms))
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
        .route("/api/h/{key}/away", put(host_away))
        .route("/api/h/{key}/picks/close", post(close_picks))
        .route("/api/h/{key}/warmup", post(warmup))
        .route("/api/h/{key}/warmup/next", post(warmup_next))
        .route("/api/h/{key}/warmup/end", post(warmup_end))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
        .route("/api/p/{key}/answer", put(answer))
        .route("/api/p/{key}/mark", put(mark))
        .route("/api/p/{key}/pick", put(pick))
        .route("/api/p/{key}/skip", put(skip))
        .route("/api/p/{key}/away", put(away))
        .route("/api/p/{key}/guess", put(guess))
        .route("/api/p/{key}/kudos", put(kudos))
        .route("/api/p/{key}/warm", put(warm))
        .with_state(app)
}

/// A room joined from the shared room, which joins everybody in it by
/// name, and back in by the key this room gave them before.
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

        // Bo cannot see where Ada landed until Bo has guessed it and said
        // where Bo stands.
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
        assert!(
            bo_view["group"]["partners"][0]["after"].is_null(),
            "not on the guess alone"
        );
        assert_eq!(bo_view["group"]["partners"][0]["guess"], 50);
        assert_eq!(bo_view["group"]["done"], false);
        let (status, _) = call(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/guess"),
            Some(json!({"name": "Ada", "value": 40})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "a guess is final");
        let bo_view = answer(&t.app, &bo, 0, 20).await["view"].clone();
        assert_eq!(bo_view["group"]["done"], true);
        assert_eq!(
            bo_view["group"]["partners"][0]["after"], 40,
            "now Bo sees it"
        );
        assert_eq!(
            bo_view["group"]["partners"][0]["done"], false,
            "Ada is not done"
        );

        // Done, Bo's answer still changes, and the round's stays.
        let bo_view = answer(&t.app, &bo, 0, 40).await["view"].clone();
        assert_eq!(bo_view["group"]["after"], 20, "the round's answer is kept");
        let square = bo_view["squares"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["statement"]["id"] == 0)
            .unwrap()
            .clone();
        assert_eq!(square["answer"], 40, "and the answer itself moves");

        // Ada has answered and not guessed, so does not see Bo's yet, and
        // can still change hers.
        let view = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert!(view["group"]["partners"][0]["after"].is_null());
        assert_eq!(
            answer(&t.app, &ada, 0, 45).await["view"]["group"]["after"],
            45
        );
        answer(&t.app, &ada, 0, 40).await;

        // Once everybody has answered and guessed, the wall shows it.
        let view = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/guess"),
            Some(json!({"name": "Bo", "value": 15})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["group"]["partners"][0]["after"], 20);
        assert_eq!(view["group"]["partners"][0]["done"], true);
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
        assert_eq!(group["people"][1]["after"], 20, "Bo's, as it was when done");
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
            "/w/abc",
            "/app.js",
            "/app.css",
            "/icon.svg",
        ] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = t.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
    }

    async fn answer(app: &Router, key: &str, id: u32, value: i16) -> Value {
        ok(
            app,
            "PUT",
            &format!("/api/p/{key}/answer"),
            Some(json!({"id": id, "value": value})),
        )
        .await
    }

    async fn join(app: &Router, join: &str, body: Value) -> (StatusCode, Value) {
        call(app, "POST", &format!("/api/j/{join}"), Some(body)).await
    }

    async fn join_key(app: &Router, host: &str) -> String {
        ok(app, "GET", &format!("/api/h/{host}"), None).await["view"]["join"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// The id the host's screen knows somebody by.
    async fn id_of(app: &Router, host: &str, name: &str) -> u64 {
        let view = ok(app, "GET", &format!("/api/h/{host}"), None).await;
        view["view"]["people"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    #[tokio::test]
    async fn a_name_in_use_is_refused_and_a_kept_key_gets_back_in() {
        let t = test("names");
        let (host, ada, _) = ready(&t.app).await;
        let code = join_key(&t.app, &host).await;

        let (status, body) = join(&t.app, &code, json!({"name": " ada "})).await;
        assert_eq!(status, StatusCode::CONFLICT, "Ada's phone is connected");
        assert!(body["error"].as_str().unwrap().contains("taken"));

        let (status, body) = join(&t.app, &code, json!({"name": "Ada", "key": ada})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["player"],
            ada.as_str(),
            "her own key, kept by her phone"
        );

        let (status, body) = join(&t.app, &code, json!({"name": "Cy"})).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(body["player"], ada.as_str());
        let count = ok(&t.app, "GET", &format!("/api/j/{code}"), None).await;
        assert_eq!(count["players"], 3);

        let host_view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        let people = host_view["people"].as_array().unwrap();
        assert_eq!(people.len(), 3);
        assert_eq!(people[0]["name"], "Ada");
        assert_eq!(people[0]["connected"], true);
        assert!(
            !host_view["people"].to_string().contains(&ada),
            "the roster carries no keys"
        );
    }

    #[tokio::test]
    async fn somebody_the_host_removed_is_gone_and_not_paired_again() {
        let t = test("remove");
        let (host, ada, bo) = ready(&t.app).await;
        answer(&t.app, &ada, 0, 90).await;
        answer(&t.app, &bo, 0, 10).await;

        let (status, body) = call(
            &t.app,
            "POST",
            &format!("/api/h/{host}/remove"),
            Some(json!({"id": id_of(&t.app, &host, "Bo").await})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(body.is_null(), "the host's stream brings the view");
        let view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["people"][0]["name"], "Ada");
        assert_eq!(view["people"].as_array().unwrap().len(), 1);

        let (status, _) = call(&t.app, "GET", &format!("/api/p/{bo}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "Bo's link is dead");
        let (status, _) = call(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/answer"),
            Some(json!({"id": 1, "value": 50})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "and answers nothing");
        let (status, _) = call(
            &t.app,
            "POST",
            &format!("/api/h/{ada}/remove"),
            Some(json!({"id": 1})),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a player cannot remove anybody"
        );

        let code = join_key(&t.app, &host).await;
        let (_, body) = join(&t.app, &code, json!({"name": "Cy"})).await;
        let cy = body["player"].as_str().unwrap().to_string();
        answer(&t.app, &cy, 0, 40).await;
        let view = ok(&t.app, "POST", &format!("/api/h/{host}/round"), None).await["view"].clone();
        assert_eq!(view["round"]["groups"].as_array().unwrap().len(), 1);
        assert_eq!(view["round"]["groups"][0]["people"][0]["name"], "Ada");
        assert_eq!(view["round"]["groups"][0]["people"][1]["name"], "Cy");
        assert!(
            !view.to_string().contains("\"Bo\""),
            "Bo is not paired or listed"
        );
        assert_eq!(view["answers"], 2, "and Bo's answer no longer counts");

        let (status, body) = join(&t.app, &code, json!({"name": "Bo"})).await;
        assert_eq!(status, StatusCode::OK, "the name is free again");
        assert_ne!(body["player"], bo.as_str(), "under a new key");
    }

    #[tokio::test]
    async fn somebody_away_is_not_paired_until_they_are_back() {
        let t = test("away");
        let (host, ada, bo) = ready(&t.app).await;
        let code = join_key(&t.app, &host).await;
        let (_, body) = join(&t.app, &code, json!({"name": "Cy"})).await;
        let cy = body["player"].as_str().unwrap().to_string();
        answer(&t.app, &ada, 0, 90).await;
        answer(&t.app, &bo, 0, 10).await;
        answer(&t.app, &cy, 0, 30).await;

        // Bo says so on the phone, and is not suggested to Ada either.
        let view = ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/away"),
            Some(json!({"away": true})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["away"], true);
        let mine = ok(&t.app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert!(!mine["opposites"].to_string().contains("\"Bo\""));
        let host_view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(host_view["players"][1]["away"], true);

        let view = ok(&t.app, "POST", &format!("/api/h/{host}/round"), None).await["view"].clone();
        let groups = view["round"]["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert!(
            !view["round"].to_string().contains("\"Bo\""),
            "Bo sits it out"
        );
        let bo_view = ok(&t.app, "GET", &format!("/api/p/{bo}"), None).await["view"].clone();
        assert!(bo_view["group"].is_null());
        ok(&t.app, "POST", &format!("/api/h/{host}/stop"), None).await;

        // Answering brings Bo back.
        let bo_view = answer(&t.app, &bo, 1, 50).await["view"].clone();
        assert_eq!(bo_view["away"], false);

        // The host marks Cy away by the id on the host's screen, and a
        // player's key cannot.
        let id = id_of(&t.app, &host, "Cy").await;
        let (status, _) = call(
            &t.app,
            "PUT",
            &format!("/api/h/{ada}/away"),
            Some(json!({"id": id, "away": true})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let view = ok(
            &t.app,
            "PUT",
            &format!("/api/h/{host}/away"),
            Some(json!({"id": id, "away": true})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["players"][2]["away"], true);
        let cy_view = ok(&t.app, "GET", &format!("/api/p/{cy}"), None).await["view"].clone();
        assert_eq!(cy_view["away"], true, "and Cy's phone says so");
        let view = ok(&t.app, "POST", &format!("/api/h/{host}/round"), None).await["view"].clone();
        assert!(
            !view["round"].to_string().contains("\"Cy\""),
            "Cy sits it out"
        );
        ok(&t.app, "POST", &format!("/api/h/{host}/stop"), None).await;

        // Back by the host, or by pressing it again.
        ok(
            &t.app,
            "PUT",
            &format!("/api/h/{host}/away"),
            Some(json!({"id": id, "away": false})),
        )
        .await;
        let cy_view = ok(&t.app, "GET", &format!("/api/p/{cy}"), None).await["view"].clone();
        assert_eq!(cy_view["away"], false);
    }

    #[tokio::test]
    async fn somebody_removed_mid_round_leaves_their_partner_able_to_finish() {
        let t = test("remove-mid");
        let (host, ada, bo) = ready(&t.app).await;
        answer(&t.app, &ada, 0, 90).await;
        answer(&t.app, &bo, 0, 10).await;
        ok(&t.app, "POST", &format!("/api/h/{host}/round"), None).await;
        ok(
            &t.app,
            "POST",
            &format!("/api/h/{host}/remove"),
            Some(json!({"id": id_of(&t.app, &host, "Bo").await})),
        )
        .await;
        ok(&t.app, "POST", &format!("/api/h/{host}/stop"), None).await;
        answer(&t.app, &ada, 0, 70).await;
        let host_view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        let group = &host_view["round"]["groups"][0];
        assert_eq!(group["people"].as_array().unwrap().len(), 1);
        assert_eq!(group["settled"], true, "Ada has nobody left to guess");
    }

    #[tokio::test]
    async fn the_wall_shows_nobodys_answer_until_their_group_has_settled() {
        let t = test("wall");
        let (host, ada, bo) = ready(&t.app).await;
        let host_view = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        let wall = host_view["wall"].as_str().unwrap().to_string();
        let code = host_view["join"].as_str().unwrap().to_string();
        answer(&t.app, &ada, 0, 93).await;
        answer(&t.app, &bo, 0, 7).await;

        let secrets = [host.as_str(), ada.as_str(), bo.as_str()];
        let look = |view: &Value| {
            let all = view.to_string();
            for secret in secrets {
                assert!(!all.contains(secret), "a key on the wall: {all}");
            }
            // Without the clock, whose digits could be anything.
            let mut game = view["view"]["game"].clone();
            game.as_object_mut().unwrap().remove("ends");
            let text = game.to_string();
            for name in ["Ada", "Bo"] {
                assert!(!text.contains(name), "a name in the game's part: {text}");
            }
            assert!(!text.contains("93"), "an answer on the wall: {text}");
            game
        };

        let view = ok(&t.app, "GET", &format!("/api/w/{wall}"), None).await;
        assert_eq!(view["view"]["join"], code.as_str(), "the code to join by");
        assert_eq!(view["view"]["people"].as_array().unwrap().len(), 2);
        assert_eq!(view["view"]["busy"], false, "the code has the screen");
        let game = look(&view);
        assert_eq!(game["people"], 2);
        assert_eq!(game["answering"], 2);
        assert!(game["round"].is_null());

        ok(&t.app, "POST", &format!("/api/h/{host}/round"), None).await;
        let view = ok(&t.app, "GET", &format!("/api/w/{wall}"), None).await;
        assert_eq!(view["view"]["busy"], true, "and the round has it now");
        let game = look(&view);
        let round = &game["round"];
        assert_eq!(round["number"], 1);
        assert_eq!(round["on"], true);
        assert_eq!(round["meetings"][0]["spot"], "🦊 Fox");
        assert_eq!(round["meetings"][0]["statement"]["id"], 0);
        assert_eq!(round["people"], 2);
        assert_eq!(round["done"], 0);
        assert!(round["results"].as_array().unwrap().is_empty());

        // Ada has answered and guessed; Bo has not, so nothing is out.
        ok(&t.app, "POST", &format!("/api/h/{host}/stop"), None).await;
        answer(&t.app, &ada, 0, 60).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/guess"),
            Some(json!({"name": "Bo", "value": 20})),
        )
        .await;
        let game = look(&ok(&t.app, "GET", &format!("/api/w/{wall}"), None).await);
        assert_eq!(game["round"]["done"], 1);
        assert!(game["round"]["results"].as_array().unwrap().is_empty());
        assert!(!game.to_string().contains("60"), "nor Ada's new answer");

        answer(&t.app, &bo, 0, 30).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{bo}/guess"),
            Some(json!({"name": "Ada", "value": 50})),
        )
        .await;
        let game = ok(&t.app, "GET", &format!("/api/w/{wall}"), None).await["view"]["game"].clone();
        let result = &game["round"]["results"][0];
        assert_eq!(result["before"], json!([7, 93]), "sorted, so nobody's");
        assert_eq!(result["after"], json!([30, 60]));
        assert_eq!(result["closed"], 112, "172 apart, then 60");
        assert!(game["board"][0]["name"].is_string());
        assert!(
            game["board"][0].get("listening").is_none(),
            "the board is names and totals"
        );

        // The wall key runs nothing, and the other keys open no wall.
        for uri in [format!("/api/h/{wall}"), format!("/api/p/{wall}")] {
            let (status, _) = call(&t.app, "GET", &uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (status, _) = call(
            &t.app,
            "POST",
            &format!("/api/h/{wall}/remove"),
            Some(json!({"id": 1})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        for key in [&host, &code, &ada] {
            let (status, _) = call(&t.app, "GET", &format!("/api/w/{key}"), None).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn the_wall_keeps_a_warm_up_answer_until_it_is_revealed() {
        let t = test("wall-warm");
        let (host, ada, _) = ready(&t.app).await;
        let wall = ok(&t.app, "GET", &format!("/api/h/{host}"), None).await["view"]["wall"]
            .as_str()
            .unwrap()
            .to_string();
        ok(&t.app, "POST", &format!("/api/h/{host}/warmup"), None).await;
        ok(
            &t.app,
            "PUT",
            &format!("/api/p/{ada}/warm"),
            Some(json!({"value": 83})),
        )
        .await;
        ok(&t.app, "POST", &format!("/api/h/{host}/warmup/next"), None).await;
        let game = ok(&t.app, "GET", &format!("/api/w/{wall}"), None).await["view"]["game"].clone();
        let warm = &game["warmup"];
        assert_eq!(warm["stage"], "clue");
        assert_eq!(warm["before"], 1);
        assert!(warm["outcome"].is_null(), "no truth, no answers");
        assert!(!game.to_string().contains("truth"));
        assert!(!warm.to_string().contains("answers"));
        assert!(!game.to_string().contains("clue\":\""), "nobody's clue");

        ok(&t.app, "POST", &format!("/api/h/{host}/warmup/next"), None).await;
        let game = ok(&t.app, "GET", &format!("/api/w/{wall}"), None).await["view"]["game"].clone();
        assert!(game["warmup"]["outcome"]["truth"].is_boolean());
        assert!(!game["warmup"]["board"].as_array().unwrap().is_empty());
    }

    /// A room with Ada's answer of 64 to statement 3 saved, the saved
    /// file, and what it says.
    async fn saved_with_an_answer(t: &Test) -> (String, String, Value) {
        let (host, ada, _) = ready(&t.app).await;
        answer(&t.app, &ada, 3, 64).await;
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&t.files[0]).unwrap()).unwrap();
        (host, ada, saved)
    }

    /// Restarts on the saved file and checks that the room came back
    /// whole: both people, Ada's answer, a wall key, and still playable.
    /// Then restarts once more, on what that saved.
    async fn restarts_whole(t: &Test, host: &str, ada: &str) {
        for restart in 0..2 {
            let again = router(Rooms::load(&t.files[0], 4), t.files[1].clone());
            let view = ok(&again, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
            assert_eq!(view["answers"], 1 + restart, "restart {restart}");
            let people = view["people"].as_array().unwrap();
            assert_eq!(people.len(), 2);
            assert_ne!(people[0]["id"], people[1]["id"], "each given an id");
            assert!(
                view["wall"].as_str().unwrap().len() > 10,
                "given a wall key"
            );
            let mine = ok(&again, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
            let answered = |id: u32| {
                mine["squares"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|s| s["statement"]["id"] == id)
                    .unwrap()["answer"]
                    .clone()
            };
            assert_eq!(answered(3), 64);
            if restart == 1 {
                assert_eq!(answered(4), 10, "what was answered after the first");
            }
            answer(&again, ada, 4, 10).await;
            assert!(
                !std::fs::read_to_string(&t.files[0])
                    .unwrap()
                    .contains("\"sheets\":{}"),
                "the sheets were kept"
            );
        }
    }

    #[tokio::test]
    async fn a_room_saves_and_loads_again() {
        let t = test("save");
        let (host, ada, saved) = saved_with_an_answer(&t).await;
        assert!(saved[&host]["lobby"]["players"].is_array());
        assert!(saved[&host]["sheets"][&ada]["answers"]["3"] == 64);
        restarts_whole(&t, &host, &ada).await;
    }

    #[tokio::test]
    async fn a_room_saved_with_the_lobby_flattened_loads() {
        let t = test("flat-save");
        let (host, ada, mut saved) = saved_with_an_answer(&t).await;
        // As #17 saved rooms: the lobby's fields at the top level beside
        // `sheets`, and its players without ids.
        let room = saved[&host].as_object_mut().unwrap();
        let Value::Object(lobby) = room.remove("lobby").unwrap() else {
            panic!("a lobby");
        };
        for (field, value) in lobby {
            if field != "next" {
                room.insert(field, value);
            }
        }
        for p in room["players"].as_array_mut().unwrap() {
            p.as_object_mut().unwrap().remove("id");
        }
        std::fs::write(&t.files[0], saved.to_string()).unwrap();
        restarts_whole(&t, &host, &ada).await;
    }

    #[tokio::test]
    async fn a_room_saved_before_the_lobby_loads_with_its_answers() {
        let t = test("old-save");
        let (host, ada, mut saved) = saved_with_an_answer(&t).await;
        // Back to how rooms were saved before: each player's card and
        // answers inside their entry in `players` at the top level, and
        // no wall key or ids.
        let room = saved[&host].as_object_mut().unwrap();
        let sheets = room.remove("sheets").unwrap();
        let Value::Object(lobby) = room.remove("lobby").unwrap() else {
            panic!("a lobby");
        };
        room.insert("join".into(), lobby["join"].clone());
        let mut players = lobby["players"].clone();
        for p in players.as_array_mut().unwrap() {
            let p = p.as_object_mut().unwrap();
            p.remove("seen");
            p.remove("id");
            let sheet = sheets[p["key"].as_str().unwrap()].as_object().unwrap();
            p.extend(sheet.clone());
        }
        room.insert("players".into(), players);
        assert!(saved.to_string().contains("\"answers\":{\"3\":64}"));
        std::fs::write(&t.files[0], saved.to_string()).unwrap();
        restarts_whole(&t, &host, &ada).await;
    }
}
