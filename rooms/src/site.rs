//! What every tool serves the same way: its page, the files the page
//! loads, the server's clock, QR codes, and the replies to a host action.
//!
//! A tool adds its own API on top:
//!
//! ```ignore
//! let app = Router::new()
//!     .merge(rooms::site::routes(rooms::site::Files {
//!         page: include_str!("../static/index.html"),
//!         icon: include_str!("../static/icon.svg"),
//!         script: include_str!(concat!(env!("OUT_DIR"), "/app.js")),
//!         stylesheet: include_str!(concat!(env!("OUT_DIR"), "/app.css")),
//!     }))
//!     .route("/api/rooms", post(open_room))
//!     .with_state(rooms);
//! rooms::serve(&addr, app).await;
//! ```

use axum::{
    Json, Router,
    body::Bytes,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;

use crate::{Room, Rooms, http, now_ms, qr};

/// The page and the files it loads, compiled into the binary.
#[derive(Clone, Copy)]
pub struct Files {
    pub page: &'static str,
    pub icon: &'static str,
    pub script: &'static str,
    pub stylesheet: &'static str,
}

/// One page, which picks its screen from the path: `/` opens a room,
/// `/h/<key>` runs it, `/j/<key>` joins it and `/p/<key>` is a player's.
/// Then the icon, the script and the stylesheet, the clock, and QR codes.
pub fn routes<S: Clone + Send + Sync + 'static>(files: Files) -> Router<S> {
    let page = move || async move { Html(files.page) };
    Router::new()
        .route("/", get(page))
        .route("/h/{key}", get(page))
        .route("/j/{key}", get(page))
        .route("/p/{key}", get(page))
        .route("/icon.svg", get(move || async move { icon(files.icon) }))
        .route("/app.js", get(move || async move { script(files.script) }))
        .route(
            "/app.css",
            get(move || async move { stylesheet(files.stylesheet) }),
        )
        .route("/api/time", get(time))
        .route("/api/qr", post(qr_code))
}

/// The compiled script. Not cached, so that a phone picks up a new
/// version on its next load.
pub fn script(body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

/// The stylesheet, likewise not cached.
pub fn stylesheet(body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

/// The icon, which changes rarely enough to cache for an hour.
pub fn icon(body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "max-age=3600"),
        ],
        body,
    )
        .into_response()
}

/// The server's clock, for a page that wants it before it has a view.
pub async fn time() -> Response {
    http::json(format!("{{\"now\":{}}}", now_ms()))
}

/// A link as a QR code. The page sends the link it wants, rather than
/// the server guessing at the host name it is served under.
pub async fn qr_code(body: Bytes) -> Response {
    match std::str::from_utf8(&body) {
        Ok(link) => qr::response(link),
        Err(_) => http::error(StatusCode::BAD_REQUEST, "That is not a link."),
    }
}

/// Opens a room and answers with its two keys: the host's, which the room
/// is stored under, and the one to hand out, which `join` reads from it.
pub async fn open<R: Room>(
    rooms: &Rooms<R>,
    make: impl FnOnce(&str) -> R,
    join: impl FnOnce(&R) -> String,
) -> Response {
    let Some(host) = rooms.create(make).await else {
        return http::full();
    };
    let join = rooms.read(&host, |room, _| join(room)).await;
    Json(serde_json::json!({ "host": host, "join": join })).into_response()
}

/// Applies a host action to a room and answers with the host's view.
///
/// Every host request answers with the whole view, so the screen that
/// pressed the button does not wait for its own event stream.
pub async fn act<R: Room, V: Serialize>(
    rooms: &Rooms<R>,
    key: &str,
    action: impl FnOnce(&mut R, u64) -> Result<(), &'static str>,
    view: impl FnOnce(&R) -> V,
) -> Response {
    let done = rooms
        .with(key, |room, now| {
            action(room, now).map(|()| http::envelope(now, &view(room)))
        })
        .await;
    match done {
        None => http::gone(),
        Some(Err(message)) => http::refused(message),
        Some(Ok(body)) => http::json(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    const FILES: Files = Files {
        page: "<p>page</p>",
        icon: "<svg/>",
        script: "go()",
        stylesheet: "p{}",
    };

    async fn get_one(path: &str) -> (StatusCode, String, String) {
        let app: Router = routes(FILES);
        let response = app
            .oneshot(axum::http::Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let kind = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        (status, kind, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn the_page_is_served_at_every_link_a_phone_might_be_handed() {
        for path in ["/", "/h/abc", "/j/abc", "/p/abc"] {
            let (status, kind, body) = get_one(path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(kind.starts_with("text/html"), "{path}: {kind}");
            assert_eq!(body, FILES.page);
        }
    }

    #[tokio::test]
    async fn the_files_go_out_with_their_types() {
        for (path, kind, body) in [
            ("/app.js", "text/javascript", FILES.script),
            ("/app.css", "text/css", FILES.stylesheet),
            ("/icon.svg", "image/svg+xml", FILES.icon),
        ] {
            let got = get_one(path).await;
            assert_eq!(got.0, StatusCode::OK, "{path}");
            assert!(got.1.starts_with(kind), "{path}: {}", got.1);
            assert_eq!(got.2, body);
        }
        let (_, _, clock) = get_one("/api/time").await;
        assert!(clock.starts_with("{\"now\":"), "{clock}");
    }
}
