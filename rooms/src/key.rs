//! Room keys.
//!
//! A key is the only thing standing between a stranger and the room, so it
//! is long enough that guessing one is not worth anybody's afternoon, and
//! made of characters that survive being read off a screen and typed in.

use rand::{RngExt, distr::Alphanumeric, rng};

/// Long enough that there is nothing to gain by guessing, short enough to
/// fit in a QR code that scans from across a room.
pub const LEN: usize = 24;

/// A fresh key of `len` alphanumeric characters.
pub fn random(len: usize) -> String {
    let mut rng = rng();
    (0..len).map(|_| rng.sample(Alphanumeric) as char).collect()
}

/// A key of the usual length.
pub fn new() -> String {
    random(LEN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_are_the_right_shape_and_do_not_repeat() {
        let keys: HashSet<String> = (0..500).map(|_| new()).collect();
        assert_eq!(keys.len(), 500, "two keys collided in 500 tries");
        for key in &keys {
            assert_eq!(key.chars().count(), LEN);
            assert!(key.chars().all(|c| c.is_ascii_alphanumeric()), "{key}");
        }
    }

    #[test]
    fn a_length_can_be_asked_for() {
        assert_eq!(random(8).chars().count(), 8);
        assert_eq!(random(0), "");
    }
}
