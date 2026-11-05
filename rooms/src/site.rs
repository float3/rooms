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

use crate::{Room, Rooms, http, now_ms, qr};
use axum::{
    Json, Router,
    body::Bytes,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};

/// The page and the files it loads, compiled into the binary.
#[derive(Clone, Copy)]
pub struct Files {
    pub page: &'static str,
    pub icon: &'static str,
    pub script: &'static str,
    pub stylesheet: &'static str,
}

/// One page, which picks its screen from the path: `/` opens a room,
/// `/h/<key>` runs it, `/j/<key>` joins it, `/p/<key>` is a player's and
/// `/w/<key>` is the wall, the read-only screen for a projector (see
/// [`crate::lobby`]).
/// Then the icon, the script and the stylesheet, the clock, and QR codes.
pub fn routes<S: Clone + Send + Sync + 'static>(files: Files) -> Router<S> {
    routes_under(files, "")
}

/// The same, for a tool served under a path such as `/numbers` rather
/// than at the root, which is how the shared room serves several tools
/// from one server. Mount the router with `Router::nest(base, ...)`.
///
/// The page's own links to `/...` are rewritten to `<base>/...`, and the
/// base is written into the page for the browser code to find.
pub fn routes_under<S: Clone + Send + Sync + 'static>(files: Files, base: &str) -> Router<S> {
    let page: &'static str = if base.is_empty() {
        files.page
    } else {
        // Once per tool when the server starts, so leaking it is fine.
        Box::leak(rebase(files.page, base).into_boxed_str())
    };
    let page = move || async move { Html(page) };
    Router::new()
        .route("/", get(page))
        .route("/h/{key}", get(page))
        .route("/j/{key}", get(page))
        .route("/p/{key}", get(page))
        .route("/w/{key}", get(page))
        .route("/icon.svg", get(move || async move { icon(files.icon) }))
        .route("/app.js", get(move || async move { script(files.script) }))
        .route(
            "/app.css",
            get(move || async move { stylesheet(files.stylesheet) }),
        )
        .route("/api/time", get(time))
        .route("/api/qr", post(qr_code))
}

/// The page with its links moved under `base`, and the base in a
/// `<meta name="rooms-base">` the browser code reads. Public for a tool
/// that serves its own page rather than through [`routes_under`].
pub fn rebase(page: &str, base: &str) -> String {
    let base = base.trim_end_matches('/');
    // A link to the home page goes to the base itself: a nested router
    // serves `/numbers` but not `/numbers/`.
    page.replace("href=\"/", &format!("href=\"{base}/"))
        .replace(&format!("href=\"{base}/\""), &format!("href=\"{base}\""))
        .replace("src=\"/", &format!("src=\"{base}/"))
        .replacen(
            "<head>",
            &format!("<head>\n<meta name=\"rooms-base\" content=\"{base}\">"),
            1,
        )
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
/// pressed the button does not wait for its own event stream. `reply`
/// renders it, usually `|room, now| http::envelope(now, &view::host(room))`:
/// a view tends to borrow from its room, which is why it is rendered there
/// rather than handed back.
pub async fn act<R: Room>(
    rooms: &Rooms<R>,
    key: &str,
    action: impl FnOnce(&mut R, u64) -> Result<(), &'static str>,
    reply: impl FnOnce(&R, u64) -> String,
) -> Response {
    let done = rooms
        .with(key, |room, now| {
            action(room, now).map(|()| reply(room, now))
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
        for path in ["/", "/h/abc", "/j/abc", "/p/abc", "/w/abc"] {
            let (status, kind, body) = get_one(path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(kind.starts_with("text/html"), "{path}: {kind}");
            assert_eq!(body, FILES.page);
        }
    }

    #[test]
    fn a_page_served_under_a_path_points_its_links_there() {
        let page = "<html><head><link href=\"/app.css\"></head><script src=\"/app.js\"></script><a href=\"https://x\"><a href=\"/\">";
        let moved = rebase(page, "/numbers");
        assert!(
            moved.contains("<a href=\"/numbers\">"),
            "home is the base itself: {moved}"
        );
        assert!(moved.contains("href=\"/numbers/app.css\""), "{moved}");
        assert!(moved.contains("src=\"/numbers/app.js\""), "{moved}");
        assert!(
            moved.contains("href=\"https://x\""),
            "other sites are left alone"
        );
        assert!(moved.contains("<meta name=\"rooms-base\" content=\"/numbers\">"));
    }

    #[tokio::test]
    async fn a_tool_nested_under_a_path_serves_its_page_there() {
        let app: Router = Router::new().nest("/numbers", routes_under(FILES, "/numbers"));
        for path in ["/numbers/h/abc", "/numbers/p/abc", "/numbers/app.js"] {
            let response = app
                .clone()
                .oneshot(axum::http::Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
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
