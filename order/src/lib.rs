//! One food order for a room of people.
//!
//! The host opens a room, says where the food is from and pastes a link
//! to the menu. Everybody opens the menu on their phone, adds what they
//! want, and the host's screen gathers it all: each person's items, the
//! same things counted together for reading out to the place, what it
//! comes to, who has not ordered yet and who has paid.
//!
//! There are four kinds of link, and the key in each is the only
//! authority there is: `/h/<key>` runs the room, `/j/<key>` joins it,
//! `/p/<key>` belongs to one player, and `/w/<key>` is the wall. Joining,
//! who is connected, taking somebody out and the wall are
//! `rooms::lobby`'s.

pub mod order;
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

use order::Order;

const FILES: site::Files = site::Files {
    page: include_str!("../static/index.html"),
    icon: include_str!("../static/icon.svg"),
    script: include_str!(concat!(env!("OUT_DIR"), "/app.js")),
    stylesheet: include_str!(concat!(env!("OUT_DIR"), "/app.css")),
};

/// How many rooms at once. Limited because anyone can open one.
const MAX_ROOMS: usize = 2000;

pub type Shared = Arc<Rooms<Order>>;

/// Applies a host action and answers with the host's view.
async fn act(
    rooms: &Rooms<Order>,
    key: &str,
    action: impl FnOnce(&mut Order, u64) -> Result<(), &'static str>,
) -> Response {
    site::act(rooms, key, action, |order, now| {
        http::envelope(now, &view::host(order, now))
    })
    .await
}

/// Applies a player's action and answers with their view.
async fn act_as(
    rooms: &Rooms<Order>,
    key: &str,
    action: impl FnOnce(&mut Order, &str, u64) -> Result<(), &'static str>,
) -> Response {
    lobby::act_as(rooms, key, action, |order, key, now| {
        http::envelope(now, &view::player(order, key))
    })
    .await
}

#[derive(Deserialize)]
struct Described {
    #[serde(default)]
    place: String,
    #[serde(default)]
    menu: String,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct Opened {
    open: bool,
}

#[derive(Deserialize)]
struct Paid {
    /// The person's lobby id.
    id: u32,
    paid: bool,
}

#[derive(Deserialize)]
struct Added {
    what: String,
    /// In euros.
    #[serde(default)]
    price: Option<f64>,
}

#[derive(Deserialize)]
struct Which {
    item: u32,
}

async fn open_room(State(rooms): State<Shared>) -> Response {
    let now = now_ms();
    site::open(
        &rooms,
        |_| Order::new(now),
        |order| order.lobby.join.clone(),
    )
    .await
}

async fn host_state(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    match rooms
        .read(&key, |order, now| {
            http::envelope(now, &view::host(order, now))
        })
        .await
    {
        Some(body) => http::json(body),
        None => http::gone(),
    }
}

async fn host_events(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    rooms::one_at(rooms, key, |order, now| {
        serde_json::to_string(&view::host(order, now)).ok()
    })
}

async fn describe(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(said): Json<Described>,
) -> Response {
    act(&rooms, &key, |order, _| {
        order.describe(&said.place, &said.menu, &said.note)
    })
    .await
}

async fn set_open(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(opened): Json<Opened>,
) -> Response {
    act(&rooms, &key, |order, _| {
        order.set_open(opened.open);
        Ok(())
    })
    .await
}

async fn set_paid(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(paid): Json<Paid>,
) -> Response {
    act(&rooms, &key, |order, _| order.set_paid(paid.id, paid.paid)).await
}

async fn strike(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(which): Json<Which>,
) -> Response {
    act(&rooms, &key, |order, _| order.strike(which.item)).await
}

fn player_json(order: &Order, key: &str, _now: u64) -> Option<String> {
    serde_json::to_string(&view::player(order, key)?).ok()
}

async fn player_state(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    lobby::player_state(&rooms, &key, player_json).await
}

async fn player_events(State(rooms): State<Shared>, Path(key): Path<String>) -> Response {
    lobby::player_events(rooms, key, player_json).await
}

async fn add(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(added): Json<Added>,
) -> Response {
    act_as(&rooms, &key, |order, key, _| {
        order.add(key, &added.what, added.price)
    })
    .await
}

async fn take_off(
    State(rooms): State<Shared>,
    Path(key): Path<String>,
    Json(which): Json<Which>,
) -> Response {
    act_as(&rooms, &key, |order, key, _| {
        order.take_off(key, which.item)
    })
    .await
}

/// The site, served at the root.
pub fn router(rooms: Shared) -> Router {
    router_under(rooms, "")
}

/// The site, for mounting under `base` with `Router::nest`, which is how
/// the shared room serves it next to the games.
pub fn router_under(rooms: Shared, base: &str) -> Router {
    Router::new()
        .merge(site::routes_under(FILES, base))
        .merge(lobby::routes(rooms.clone()))
        .route("/api/rooms", post(open_room))
        .route("/api/h/{key}", get(host_state))
        .route("/api/h/{key}/events", get(host_events))
        .route("/api/h/{key}/place", put(describe))
        .route("/api/h/{key}/open", put(set_open))
        .route("/api/h/{key}/paid", put(set_paid))
        .route("/api/h/{key}/strike", post(strike))
        .route("/api/p/{key}", get(player_state))
        .route("/api/p/{key}/events", get(player_events))
        .route("/api/p/{key}/items", post(add))
        .route("/api/p/{key}/items/remove", post(take_off))
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
impl rooms::tool::Joinable for Order {
    fn open(now: u64) -> Self {
        Order::new(now)
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
            std::env::temp_dir().join(format!("order-test-{name}-{}.json", std::process::id()));
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
        let response = app.clone().oneshot(request).await.expect("an answer");
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

    /// A room from Da Mario with Ada and Bo in it.
    async fn ready(app: &Router) -> (String, String, String) {
        let keys = ok(app, "POST", "/api/rooms", None).await;
        let host = keys["host"].as_str().unwrap().to_string();
        let join = keys["join"].as_str().unwrap().to_string();
        ok(
            app,
            "PUT",
            &format!("/api/h/{host}/place"),
            Some(json!({"place": "Da Mario", "menu": "damario.example/menu", "note": "Ordering at 19:30"})),
        )
        .await;
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
        (host, players.remove(0), players.remove(0))
    }

    #[tokio::test]
    async fn everybody_orders_and_the_host_sees_it_all() {
        let (app, path) = app("all");
        let (host, ada, bo) = ready(&app).await;

        let mine = ok(
            &app,
            "POST",
            &format!("/api/p/{ada}/items"),
            Some(json!({"what": "Margherita", "price": 9.5})),
        )
        .await["view"]
            .clone();
        assert_eq!(mine["place"], "Da Mario");
        assert_eq!(mine["menu"], "https://damario.example/menu");
        assert_eq!(mine["mine"][0]["what"], "Margherita");
        assert_eq!(mine["total"], 950);
        ok(
            &app,
            "POST",
            &format!("/api/p/{bo}/items"),
            Some(json!({"what": "margherita"})),
        )
        .await;
        ok(
            &app,
            "POST",
            &format!("/api/p/{bo}/items"),
            Some(json!({"what": "Cola", "price": 2.5})),
        )
        .await;

        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["items"], 3);
        assert_eq!(view["total"], 1200);
        assert_eq!(view["unpriced"], 1);
        assert_eq!(view["tally"][0]["what"], "Margherita");
        assert_eq!(view["tally"][0]["count"], 2);
        assert_eq!(view["tally"][0]["names"], json!(["Ada", "Bo"]));
        assert_eq!(view["shares"][1]["name"], "Bo");
        assert_eq!(view["shares"][1]["unpriced"], true);
        assert_eq!(view["waiting"], json!([]));

        let theirs = ok(&app, "GET", &format!("/api/p/{bo}"), None).await["view"].clone();
        assert_eq!(theirs["others"][0]["name"], "Ada");
        assert_eq!(theirs["ordered"], 2);
        assert!(
            !theirs.to_string().contains(&ada),
            "nobody is sent another's key"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn once_the_order_is_in_nobody_changes_it_and_the_host_marks_who_paid() {
        let (app, path) = app("closed");
        let (host, ada, _) = ready(&app).await;
        let mine = ok(
            &app,
            "POST",
            &format!("/api/p/{ada}/items"),
            Some(json!({"what": "Calzone", "price": 11})),
        )
        .await["view"]
            .clone();
        let item = mine["mine"][0]["id"].clone();

        ok(
            &app,
            "PUT",
            &format!("/api/h/{host}/open"),
            Some(json!({"open": false})),
        )
        .await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/p/{ada}/items/remove"),
            Some(json!({ "item": item })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("gone in"));

        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        let id = view["shares"][0]["id"].clone();
        let view = ok(
            &app,
            "PUT",
            &format!("/api/h/{host}/paid"),
            Some(json!({"id": id, "paid": true})),
        )
        .await["view"]
            .clone();
        assert_eq!(view["shares"][0]["paid"], true);
        let mine = ok(&app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
        assert_eq!(mine["paid"], true);

        let view = ok(
            &app,
            "POST",
            &format!("/api/h/{host}/strike"),
            Some(json!({ "item": item })),
        )
        .await["view"]
            .clone();
        assert_eq!(view["items"], 0);
        assert_eq!(view["waiting"], json!(["Ada", "Bo"]));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_link_that_is_not_a_web_page_is_refused() {
        let (app, path) = app("menu");
        let (host, _, _) = ready(&app).await;
        let (status, _) = call(
            &app,
            "PUT",
            &format!("/api/h/{host}/place"),
            Some(json!({"place": "X", "menu": "javascript:alert(1)"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["menu"], "https://damario.example/menu");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn one_kind_of_key_does_not_work_as_another() {
        let (app, path) = app("keys");
        let (host, ada, _) = ready(&app).await;
        for uri in [format!("/api/h/{ada}"), format!("/api/p/{host}")] {
            let (status, _) = call(&app, "GET", &uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (status, _) = call(
            &app,
            "PUT",
            &format!("/api/h/{ada}/open"),
            Some(json!({"open": false})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "a player cannot run it");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn the_wall_shows_the_place_and_how_far_along_not_who_wants_what() {
        let (app, path) = app("wall");
        let (host, ada, _) = ready(&app).await;
        ok(
            &app,
            "POST",
            &format!("/api/p/{ada}/items"),
            Some(json!({"what": "Quattro formaggi"})),
        )
        .await;
        let wall = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"]["wall"]
            .as_str()
            .unwrap()
            .to_string();
        let view = ok(&app, "GET", &format!("/api/w/{wall}"), None).await["view"].clone();
        assert_eq!(view["game"]["place"], "Da Mario");
        assert_eq!(view["game"]["ordered"], 1);
        assert!(!view.to_string().contains("formaggi"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_room_survives_the_server_restarting() {
        let (app, path) = app("restart");
        let (host, ada, _) = ready(&app).await;
        ok(
            &app,
            "POST",
            &format!("/api/p/{ada}/items"),
            Some(json!({"what": "Funghi"})),
        )
        .await;
        let again = router(Rooms::load(&path, 4));
        let view = ok(&again, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
        assert_eq!(view["shares"][0]["items"][0]["what"], "Funghi");
        let _ = std::fs::remove_file(&path);
    }
}
