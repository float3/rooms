use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn server(name: &str) -> Router {
    let dir = std::env::temp_dir().join(format!("hall-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    app_in(dir)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
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
    let (status, value) = call(app, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {value}");
    value
}

/// Opens a hall with Ada and Bo in it.
async fn hall(app: &Router) -> (String, String, String, String) {
    let keys = ok(app, "POST", "/api/rooms", None).await;
    let host = keys["host"].as_str().unwrap().to_string();
    let join = keys["join"].as_str().unwrap().to_string();
    let ada = ok(
        app,
        "POST",
        &format!("/api/j/{join}"),
        Some(json!({"name": "Ada"})),
    )
    .await["player"]
        .as_str()
        .unwrap()
        .to_string();
    let bo = ok(
        app,
        "POST",
        &format!("/api/j/{join}"),
        Some(json!({"name": "Bo"})),
    )
    .await["player"]
        .as_str()
        .unwrap()
        .to_string();
    (host, join, ada, bo)
}

/// The key at the end of a frame's link: `/numbers/p/<key>`.
fn key_in(url: &Value) -> String {
    url.as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn the_host_picks_a_game_and_everybody_is_in_it() {
    let app = server("pick");
    let (host, _, ada, _) = hall(&app).await;

    let view = ok(&app, "GET", &format!("/api/h/{host}"), None).await["view"].clone();
    assert!(view["playing"].is_null(), "a new hall is in the lobby");
    assert_eq!(view["people"], json!(["Ada", "Bo"]));
    assert_eq!(view["games"].as_array().unwrap().len(), 4);

    let view = ok(
        &app,
        "POST",
        &format!("/api/h/{host}/play"),
        Some(json!({"tool": "numbers"})),
    )
    .await["view"]
        .clone();
    assert_eq!(view["playing"]["id"], "numbers");
    let game_host = key_in(&view["playing"]["url"]);

    // The numbers room has both of them in it already.
    let game = ok(&app, "GET", &format!("/numbers/api/h/{game_host}"), None).await;
    assert_eq!(game["view"]["players"].as_array().unwrap().len(), 2);

    // Ada's phone is sent to her own page in it.
    let mine = ok(&app, "GET", &format!("/api/p/{ada}"), None).await["view"].clone();
    assert_eq!(mine["playing"]["title"], "Number games");
    let url = mine["playing"]["url"].as_str().unwrap();
    assert!(url.starts_with("/numbers/p/"), "{url}");
    let theirs = ok(
        &app,
        "GET",
        &format!("/numbers/api/p/{}", key_in(&mine["playing"]["url"])),
        None,
    )
    .await;
    assert_eq!(theirs["view"]["name"], "Ada");
}

#[tokio::test]
async fn going_back_to_a_game_goes_back_to_the_same_room() {
    let app = server("back");
    let (host, _, _, _) = hall(&app).await;
    let play = |tool: &'static str| {
        let app = app.clone();
        let host = host.clone();
        async move {
            ok(
                &app,
                "POST",
                &format!("/api/h/{host}/play"),
                Some(json!({"tool": tool})),
            )
            .await["view"]["playing"]["url"]
                .clone()
        }
    };
    let first = play("numbers").await;
    let other = play("estimate").await;
    assert!(other.as_str().unwrap().starts_with("/estimate/h/"));
    assert_eq!(play("numbers").await, first, "the scores so far are kept");

    let lobby = ok(
        &app,
        "POST",
        &format!("/api/h/{host}/play"),
        Some(json!({"tool": null})),
    )
    .await;
    assert!(lobby["view"]["playing"].is_null());
}

#[tokio::test]
async fn somebody_who_joins_during_a_game_is_put_straight_into_it() {
    let app = server("late");
    let (host, join, _, _) = hall(&app).await;
    let view = ok(
        &app,
        "POST",
        &format!("/api/h/{host}/play"),
        Some(json!({"tool": "updating"})),
    )
    .await;
    let game_host = key_in(&view["view"]["playing"]["url"]);

    let cy = ok(
        &app,
        "POST",
        &format!("/api/j/{join}"),
        Some(json!({"name": "Cy"})),
    )
    .await["player"]
        .as_str()
        .unwrap()
        .to_string();
    let mine = ok(&app, "GET", &format!("/api/p/{cy}"), None).await["view"].clone();
    assert!(
        mine["playing"]["url"]
            .as_str()
            .unwrap()
            .starts_with("/updating/p/")
    );
    let game = ok(&app, "GET", &format!("/updating/api/h/{game_host}"), None).await;
    assert_eq!(
        game["view"]["players"].as_array().map(Vec::len),
        Some(3),
        "{game}"
    );
}

#[tokio::test]
async fn the_same_name_joins_as_the_same_person() {
    let app = server("again");
    let (_, join, ada, _) = hall(&app).await;
    let again = ok(
        &app,
        "POST",
        &format!("/api/j/{join}"),
        Some(json!({"name": " Ada "})),
    )
    .await;
    assert_eq!(again["player"], ada.as_str());
    let count = ok(&app, "GET", &format!("/api/j/{join}"), None).await;
    assert_eq!(count["players"], 2);
}

#[tokio::test]
async fn a_game_this_server_does_not_play_is_refused() {
    let app = server("unknown");
    let (host, _, ada, _) = hall(&app).await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/h/{host}/play"),
        Some(json!({"tool": "chess"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/h/{ada}/play"),
        Some(json!({"tool": "numbers"})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a player's key does not run the room"
    );
}

#[tokio::test]
async fn every_game_is_served_under_its_own_path_with_its_links_moved_there() {
    let app = server("paths");
    for game in ["petrov", "numbers", "estimate", "updating"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{game}/p/somebody"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{game}");
        let page = response.into_body().collect().await.unwrap().to_bytes();
        let page = String::from_utf8(page.to_vec()).unwrap();
        assert!(page.contains(&format!("src=\"/{game}/app.js\"")), "{game}");
        assert!(page.contains(&format!("content=\"/{game}\"")), "{game}");
    }
}
