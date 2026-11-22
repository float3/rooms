//! What each screen is sent.
//!
//! Nobody is sent anybody else's press until the host reveals the round.
//! That includes the host, whose screen may still be projected. Until the
//! reveal, every screen gets the share blue needs and a count.
//!
//! The wall, the screen meant for a projector, never pairs a name with a
//! button: after the reveal it shows the split and nothing about who.

use rooms::lobby::Present;
use serde::Serialize;

use crate::game::{Colour, Game, Outcome, Round, Score};

#[derive(Serialize)]
pub struct Who<'a> {
    /// The player's [`Present::id`], to match them to `people`.
    id: u32,
    name: &'a str,
    pressed: bool,
}

#[derive(Serialize)]
pub struct Playing {
    /// The share of the presses blue has to beat, in percent.
    percent: u32,
    open: bool,
    revealed: bool,
    asked: u64,
}

/// A round played earlier in the evening.
#[derive(Serialize)]
pub struct Past {
    percent: u32,
    n: usize,
    blue: usize,
    blue_won: Option<bool>,
}

#[derive(Serialize)]
pub struct Host<'a> {
    join: &'a str,
    /// The wall's key, for "Open the wall view".
    wall: &'a str,
    /// Everybody, with whose phone is connected.
    people: Vec<Present<'a>>,
    /// Everybody, with whether they have pressed in this round.
    players: Vec<Who<'a>>,
    /// Who is away: not connected, and no press in this round or the one
    /// before. See `rooms::lobby::away`.
    away: Vec<u32>,
    round: Option<Playing>,
    /// Only after the round has been revealed.
    outcome: Option<Outcome>,
    /// Newest first, excluding the round being played.
    history: Vec<Past>,
    scores: Vec<Score>,
}

#[derive(Serialize)]
pub struct Player<'a> {
    name: &'a str,
    round: Option<Playing>,
    /// This player's button, which they can change until the round
    /// closes.
    mine: Option<Colour>,
    pressed: usize,
    players: usize,
    outcome: Option<Outcome>,
    /// Whether this player lived, once the round is out and if they
    /// pressed a button in it.
    lived: Option<bool>,
    score: Option<Score>,
}

impl Round {
    fn playing(&self) -> Playing {
        Playing {
            percent: self.percent,
            open: self.open,
            revealed: self.revealed,
            asked: self.asked,
        }
    }
}

fn history(game: &Game) -> Vec<Past> {
    let current = game.rounds.len().saturating_sub(1);
    game.rounds[..current]
        .iter()
        .rev()
        .filter(|round| round.revealed)
        .map(|round| Past {
            percent: round.percent,
            n: round.presses.len(),
            blue: round.blue(),
            blue_won: round.blue_won(),
        })
        .collect()
}

pub fn host(game: &Game, now: u64) -> Host<'_> {
    let round = game.round();
    Host {
        join: &game.lobby.join,
        wall: &game.lobby.wall,
        people: game.lobby.roster(now),
        players: game
            .players()
            .iter()
            .map(|player| Who {
                id: player.id,
                name: &player.name,
                pressed: round.is_some_and(|r| r.presses.contains_key(&player.key)),
            })
            .collect(),
        away: rooms::lobby::away_ids(game, now),
        round: round.map(Round::playing),
        outcome: round.and_then(|round| round.outcome(game.players())),
        history: history(game),
        scores: game.scores(),
    }
}

pub fn player<'a>(game: &'a Game, key: &str) -> Option<Player<'a>> {
    let player = game.player(key)?;
    let round = game.round();
    let mine = round
        .and_then(|r| r.presses.get(key))
        .map(|press| press.colour);
    Some(Player {
        name: &player.name,
        round: round.map(Round::playing),
        mine,
        pressed: round.map_or(0, |r| r.presses.len()),
        players: game.lobby.len(),
        outcome: round.and_then(|round| round.outcome(game.players())),
        lived: round
            .filter(|round| round.revealed)
            .zip(mine)
            .map(|(round, colour)| round.lives(colour)),
        score: game
            .scores()
            .into_iter()
            .find(|score| score.name == player.name),
    })
}

/// What the wall shows of a round once it has come out: the split, which
/// says nothing of who.
#[derive(Serialize)]
pub struct Shown {
    n: usize,
    blue: usize,
    blue_won: Option<bool>,
}

/// The game's part of the wall. See [`crate::game::Game`]'s `HasLobby`.
/// No scoreboard: who lived through a round says what they pressed.
#[derive(Serialize)]
pub struct Wall {
    round: Option<Playing>,
    /// How many of the people in the room have pressed in this round.
    pressed: usize,
    players: usize,
    /// Only after the reveal.
    shown: Option<Shown>,
}

pub fn wall(game: &Game) -> Wall {
    let round = game.round();
    Wall {
        round: round.map(Round::playing),
        pressed: round.map_or(0, |round| {
            game.players()
                .iter()
                .filter(|p| round.presses.contains_key(&p.key))
                .count()
        }),
        players: game.lobby.len(),
        shown: round.filter(|round| round.revealed).map(|round| Shown {
            n: round.presses.len(),
            blue: round.blue(),
            blue_won: round.blue_won(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rooms::lobby::admit;

    /// Ada pressed blue and Bo red, with blue needing more than half.
    fn room() -> (Game, String, String) {
        let mut game = Game::new(1000);
        let ada = admit(&mut game, "Ada", None, 1000).unwrap();
        let bo = admit(&mut game, "Bo", None, 1000).unwrap();
        game.start(50, 1000).unwrap();
        game.press(&ada, Colour::Blue, 1001).unwrap();
        game.press(&bo, Colour::Red, 1001).unwrap();
        (game, ada, bo)
    }

    fn json<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("the view serializes")
    }

    #[test]
    fn nobody_sees_a_press_before_the_reveal_not_even_the_host() {
        let (mut game, ada, bo) = room();
        for seen in [json(&host(&game, 1000)), json(&wall(&game))] {
            assert!(!seen.contains("\"blue\""), "{seen}");
            assert!(!seen.contains("\"colour\""), "{seen}");
        }
        let view = player(&game, &bo).unwrap();
        assert_eq!((view.pressed, view.players), (2, 2), "a count and no more");
        assert!(view.outcome.is_none() && view.lived.is_none());
        assert!(
            json(&player(&game, &ada).unwrap()).contains("\"mine\":\"blue\""),
            "a player still sees their own"
        );

        game.reveal().unwrap();
        assert!(json(&host(&game, 1000)).contains("\"colour\":\"blue\""));
    }

    #[test]
    fn the_split_and_who_lived_come_out_together() {
        let (mut game, ada, bo) = room();
        game.reveal().unwrap();
        let view = player(&game, &ada).unwrap();
        let outcome = view.outcome.expect("the round came out");
        assert_eq!((outcome.n, outcome.blue), (2, 1));
        assert_eq!(outcome.blue_won, Some(false), "half is not more than half");
        assert_eq!(view.lived, Some(false));
        assert_eq!(player(&game, &bo).unwrap().lived, Some(true));
    }

    #[test]
    fn the_wall_shows_the_split_and_never_a_name() {
        let (mut game, _, _) = room();
        game.reveal().unwrap();
        let seen = json(&wall(&game));
        assert!(seen.contains("\"blue\":1"), "{seen}");
        assert!(!seen.contains("Ada") && !seen.contains("Bo"), "{seen}");
    }

    #[test]
    fn a_round_still_being_played_is_not_in_the_history() {
        let (mut game, _, _) = room();
        game.reveal().unwrap();
        game.start(30, 2000).unwrap();
        let view = host(&game, 1000);
        assert_eq!(view.history.len(), 1);
        assert_eq!(view.history[0].percent, 50);
        assert!(view.outcome.is_none(), "the new round has not come out");
    }

    #[test]
    fn a_room_with_no_round_in_it_still_renders() {
        let mut game = Game::new(1000);
        let key = admit(&mut game, "Ada", None, 1000).unwrap();
        assert!(host(&game, 1000).round.is_none());
        let view = player(&game, &key).unwrap();
        assert!(view.round.is_none() && view.mine.is_none());
        assert!(player(&game, "nobody").is_none());
    }
}
