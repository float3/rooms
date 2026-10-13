//! The handful of responses every one of these tools sends.

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// A refusal the page can show as it stands.
pub fn error(status: StatusCode, message: &str) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], {
        let escaped = serde_json::to_string(message).unwrap_or_else(|_| "\"\"".into());
        format!("{{\"error\":{escaped}}}")
    })
        .into_response()
}

/// A room key that is not a room: wrong, or old enough to have been swept.
pub fn gone() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "That room is not here. It may have finished, or the link may be wrong.",
    )
}

/// A view, stamped with the server's clock. Phones work from the difference
/// between that and their own rather than trusting either.
pub fn envelope<T: Serialize>(now: u64, view: &T) -> String {
    let body = serde_json::to_string(view).unwrap_or_else(|_| "null".into());
    format!("{{\"now\":{now},\"view\":{body}}}")
}

/// An already-serialised body, sent as JSON.
pub fn json(body: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

/// A view in an envelope, sent as JSON.
pub fn view<T: Serialize>(now: u64, view: &T) -> Response {
    json(envelope(now, view))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_carries_the_clock_and_the_view() {
        assert_eq!(
            envelope(7, &serde_json::json!({"a": 1})),
            r#"{"now":7,"view":{"a":1}}"#
        );
    }

    #[test]
    fn an_error_message_with_a_quote_in_it_is_still_json() {
        let body = error(StatusCode::BAD_REQUEST, "say \"no\"");
        assert_eq!(body.status(), StatusCode::BAD_REQUEST);
        // The message is escaped rather than closing the string early.
        let escaped = serde_json::to_string("say \"no\"").unwrap();
        assert!(format!("{{\"error\":{escaped}}}").contains(r#"say \"no\""#));
    }
}
