//! The join link as something to point a phone at.
//!
//! Handing out a 24-character key by reading it aloud does not work in a
//! room with twenty people in it. Putting it on the wall does.

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use qrcode::{QrCode, render::svg};

/// A link that will not fit in a QR code anyone can scan.
pub const MAX_LEN: usize = 1000;

/// The link as an SVG, sized by the viewer rather than by us.
pub fn svg(link: &str) -> Result<String, &'static str> {
    if link.is_empty() {
        return Err("Nothing to encode.");
    }
    if link.len() > MAX_LEN {
        return Err("That link is too long to put in a QR code.");
    }
    let code = QrCode::new(link).map_err(|_| "That link will not fit in a QR code.")?;
    Ok(code
        .render()
        .min_dimensions(240, 240)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

/// The same, as a response: an image the page can drop straight into a
/// container.
pub fn response(link: &str) -> Response {
    match svg(link) {
        Ok(body) => (
            [
                (header::CONTENT_TYPE, "image/svg+xml"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            body,
        )
            .into_response(),
        Err(message) => crate::http::error(StatusCode::BAD_REQUEST, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_becomes_an_svg() {
        let out = svg("https://example.com/join/abc").expect("a short link encodes");
        assert!(
            out.starts_with("<?xml") || out.contains("<svg"),
            "{}",
            &out[..60]
        );
        assert!(out.contains("#000000"));
    }

    #[test]
    fn nothing_and_too_much_are_both_refused() {
        assert!(svg("").is_err());
        assert!(svg(&"x".repeat(MAX_LEN + 1)).is_err());
    }
}
