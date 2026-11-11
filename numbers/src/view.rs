//! What each screen is sent.
//!
//! Nobody is sent anybody else's number until the host reveals the round.
//! That includes the host, whose screen may still be projected. Until the
//! reveal, every screen gets the rule, the range and a count.
//!
//! The wall, the screen meant for a projector, never pairs a name with a
//! number: after the reveal it shows the numbers as unnamed dots, and
//! names only on the top of the scoreboard.

use rooms::lobby::Present;
use serde::Serialize;

use crate::answer::Answers;
use crate::game::{Game, Outcome, Round, Score};

#[derive(Serialize)]
pub struct Who<'a> {
    /// The player's [`Present::id`], to match them to `people`.
    id: u32,
    name: &'a str,
    picked: bool,
}

#[derive(Serialize)]
pub struct Playing {
    /// The rule in words, which is all a screen needs.
    label: String,
    low: f64,
    high: f64,
    /// Whether the round is played with whole numbers.
    whole: bool,
    /// What a pick can be typed as.
    answers: Answers,
    open: bool,
    revealed: bool,
    asked: u64,
}

#[derive(Serialize)]
pub struct Past {
    label: String,
    target: Option<f64>,
    mean: Option<f64>,
    winners: Vec<String>,
    n: usize,
    /// On the newest of two or more revealed rounds in a row with the
    /// same rule and range, the target of each in the order they were
    /// played, the round being shown included once it is revealed: how
    /// the room moved, 33 → 21 → 12. Empty otherwise.
    targets: Vec<f64>,
}

#[derive(Serialize)]
pub struct Host<'a> {
    join: &'a str,
    /// The wall's key, for "Open the wall view".
    wall: &'a str,
    /// Everybody, with whose phone is connected.
    people: Vec<Present<'a>>,
    /// Everybody, with whether they have picked in this round.
    players: Vec<Who<'a>>,
    /// Who is away: not connected, and no pick in this round or the one
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
    /// This player's number, which they can change until the round
    /// closes.
    mine: Option<f64>,
    /// What this player typed, when the round takes expressions.
    mine_text: Option<String>,
    picked: usize,
    players: usize,
    outcome: Option<Outcome>,
    won: Option<bool>,
    score: Option<Score>,
}

impl Round {
    fn playing(&self) -> Playing {
        Playing {
            label: self.rule.label(),
            low: self.low,
            high: self.high,
            whole: self.whole(),
            answers: self.answers(),
            open: self.open,
            revealed: self.revealed,
            asked: self.asked,
        }
    }
}

fn history(game: &Game) -> Vec<Past> {
    let rounds = &game.rounds;
    let outcomes: Vec<Option<Outcome>> = rounds
        .iter()
        .map(|round| round.outcome(game.players()))
        .collect();
    // Round `b` carries on from round `a` before it: both came out, with
    // the same rule over the same range.
    let carries_on = |a: usize, b: usize| {
        outcomes[a].is_some() && outcomes[b].is_some() && rounds[a].same_as(&rounds[b])
    };
    let current = rounds.len().saturating_sub(1);
    (0..current)
        .rev()
        .filter_map(|i| {
            let outcome = outcomes[i].as_ref()?;
            let mut targets = Vec::new();
            // Only on the newest past round of a run, which then counts
            // the round being shown if that carries on from it.
            if i + 1 == current || !carries_on(i, i + 1) {
                let end = if carries_on(i, current) { current } else { i };
                let mut start = i;
                while start > 0 && carries_on(start - 1, start) {
                    start -= 1;
                }
                // A round nobody picked in has no target, and is passed over.
                let trail: Vec<f64> = (start..=end)
                    .filter_map(|k| outcomes[k].as_ref()?.target)
                    .collect();
                if trail.len() > 1 {
                    targets = trail;
                }
            }
            Some(Past {
                label: outcome.label.clone(),
                target: outcome.target,
                mean: outcome.mean,
                winners: outcome.winners.clone(),
                n: outcome.n,
                targets,
            })
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
                picked: round.is_some_and(|r| r.picks.contains_key(&player.key)),
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
    let outcome = round.and_then(|round| round.outcome(game.players()));
    Some(Player {
        name: &player.name,
        round: round.map(Round::playing),
        mine: round.and_then(|r| r.picks.get(key)).map(|pick| pick.value),
        mine_text: round
            .and_then(|r| r.picks.get(key))
            .and_then(|pick| pick.text.clone()),
        picked: round.map_or(0, |r| r.picks.len()),
        players: game.lobby.len(),
        won: outcome
            .as_ref()
            .map(|outcome| outcome.winners.contains(&player.name)),
        outcome,
        score: game
            .scores()
            .into_iter()
            .find(|score| score.name == player.name),
    })
}

/// One number on the wall's strip after the reveal: how many picked it,
/// and not who.
#[derive(Serialize)]
pub struct Spot {
    value: f64,
    count: usize,
    won: bool,
}

/// What the wall shows of a round once it has come out.
#[derive(Serialize)]
pub struct Shown {
    n: usize,
    target: Option<f64>,
    target_label: Option<&'static str>,
    mean: Option<f64>,
    median: Option<f64>,
    total: Option<f64>,
    /// How many won, without their names: their names would say what
    /// they picked.
    winners: usize,
    why: Option<String>,
    spots: Vec<Spot>,
}

/// Somebody on the top of the scoreboard. Wins and rounds, and no
/// average: after one round, a player's average is their number.
#[derive(Serialize)]
pub struct Top {
    name: String,
    wins: usize,
    /// Of those, the ones won alone.
    solo: usize,
    played: usize,
}

/// The game's part of the wall. See [`crate::game::Game`]'s `HasLobby`.
#[derive(Serialize)]
pub struct Wall {
    round: Option<Playing>,
    /// How many of the people in the room have picked in this round.
    picked: usize,
    players: usize,
    /// Only after the reveal.
    shown: Option<Shown>,
    /// The three with the most wins, once anybody has won.
    top: Vec<Top>,
}

pub fn wall(game: &Game) -> Wall {
    let round = game.round();
    let shown = round
        .and_then(|round| round.outcome(game.players()))
        .map(|outcome| {
            let mut spots: Vec<Spot> = Vec::new();
            for pick in &outcome.picks {
                match spots.iter_mut().find(|spot| spot.value == pick.value) {
                    Some(spot) => {
                        spot.count += 1;
                        spot.won |= pick.won;
                    }
                    None => spots.push(Spot {
                        value: pick.value,
                        count: 1,
                        won: pick.won,
                    }),
                }
            }
            Shown {
                n: outcome.n,
                target: outcome.target,
                target_label: outcome.target_label,
                mean: outcome.mean,
                median: outcome.median,
                total: outcome.total,
                winners: outcome.winners.len(),
                why: outcome.why,
                spots,
            }
        });
    Wall {
        round: round.map(Round::playing),
        picked: round.map_or(0, |round| {
            game.players()
                .iter()
                .filter(|p| round.picks.contains_key(&p.key))
                .count()
        }),
        players: game.lobby.len(),
        shown,
        top: game
            .scores()
            .into_iter()
            .filter(|score| score.wins > 0)
            .take(3)
            .map(|score| Top {
                name: score.name,
                wins: score.wins,
                solo: score.solo,
                played: score.played,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Rule;
    use rooms::lobby::admit;

    const THIRD: Rule = Rule::OfTheMean { num: 1, den: 3 };

    fn room() -> (Game, String, String) {
        let mut game = Game::new(1000);
        let ada = admit(&mut game, "Ada", None, 1000).unwrap();
        let bo = admit(&mut game, "Bo", None, 1000).unwrap();
        game.start(THIRD, 0.0, 10.0, 1000).unwrap();
        game.pick(&ada, 9.0, 1001).unwrap();
        game.pick(&bo, 1.0, 1001).unwrap();
        (game, ada, bo)
    }

    fn json<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("the view serializes")
    }

    #[test]
    fn nobody_sees_a_number_before_the_reveal_not_even_the_host() {
        let (mut game, ada, _) = room();
        let seen = json(&host(&game, 1000));
        assert!(
            !seen.contains("\"value\""),
            "the host screen is on the wall"
        );
        assert!(
            json(&player(&game, &ada).unwrap()).contains("\"mine\":9"),
            "a player still sees their own"
        );

        game.reveal().unwrap();
        assert!(json(&host(&game, 1000)).contains("\"value\":9"));
    }

    #[test]
    fn the_count_is_all_anyone_gets_until_then() {
        let (game, ada, _) = room();
        let view = player(&game, &ada).unwrap();
        assert_eq!((view.picked, view.players), (2, 2));
        assert!(view.outcome.is_none());
        assert!(view.won.is_none());
        let view = host(&game, 1000);
        assert!(view.players.iter().all(|who| who.picked));
        assert!(view.outcome.is_none());
    }

    #[test]
    fn the_numbers_and_the_winner_come_out_together() {
        let (mut game, ada, bo) = room();
        game.reveal().unwrap();
        let view = player(&game, &bo).unwrap();
        let outcome = view.outcome.expect("the round came out");
        assert_eq!(outcome.mean, Some(5.0));
        assert_eq!(outcome.target, Some(5.0 / 3.0));
        assert_eq!(outcome.winners, vec!["Bo"]);
        assert_eq!(view.won, Some(true));
        assert_eq!(player(&game, &ada).unwrap().won, Some(false));
    }

    #[test]
    fn a_round_still_being_played_is_not_in_the_history() {
        let (mut game, _, _) = room();
        game.reveal().unwrap();
        game.start(THIRD, 0.0, 10.0, 2000).unwrap();
        let view = host(&game, 1000);
        assert_eq!(view.history.len(), 1);
        assert_eq!(view.history[0].winners, vec!["Bo"]);
        assert!(view.outcome.is_none(), "the new round has not come out");
    }

    #[test]
    fn a_room_with_no_round_in_it_still_renders() {
        let mut game = Game::new(1000);
        let key = admit(&mut game, "Ada", None, 1000).unwrap();
        assert!(host(&game, 1000).round.is_none());
        let view = player(&game, &key).unwrap();
        assert!(view.round.is_none());
        assert!(view.mine.is_none());
    }

    #[test]
    fn a_key_that_is_not_in_the_room_has_no_view() {
        let (game, _, _) = room();
        assert!(player(&game, "nobody").is_none());
    }

    /// Plays another round of the room's rule: Ada and Bo pick `picks`.
    fn again(game: &mut Game, ada: &str, bo: &str, picks: [f64; 2], now: u64) {
        game.again(now).unwrap();
        game.pick(ada, picks[0], now + 1).unwrap();
        game.pick(bo, picks[1], now + 1).unwrap();
        game.reveal().unwrap();
    }

    fn targets(game: &Game) -> Vec<Vec<f64>> {
        history(game).into_iter().map(|past| past.targets).collect()
    }

    #[test]
    fn rounds_of_the_same_rule_in_a_row_show_how_the_target_moved() {
        let (mut game, ada, bo) = room();
        game.reveal().unwrap(); // a third of 5
        assert!(targets(&game).is_empty(), "nothing played before it");
        again(&mut game, &ada, &bo, [3.0, 3.0], 2000); // a third of 3
        assert_eq!(
            targets(&game),
            vec![vec![5.0 / 3.0, 1.0]],
            "with the round shown"
        );
        again(&mut game, &ada, &bo, [0.0, 3.0], 3000); // a third of 1.5
        assert_eq!(targets(&game), vec![vec![5.0 / 3.0, 1.0, 0.5], vec![]]);

        // A round not yet revealed is not in it.
        game.again(4000).unwrap();
        assert_eq!(
            targets(&game),
            vec![vec![5.0 / 3.0, 1.0, 0.5], vec![], vec![]]
        );
        game.reveal().unwrap();

        // Another range is another run, and the one before keeps its line,
        // passing over the round nobody picked in.
        game.start(THIRD, 0.0, 100.0, 5000).unwrap();
        game.pick(&ada, 30.0, 5001).unwrap();
        game.reveal().unwrap();
        let after = targets(&game);
        assert_eq!(after[0], vec![5.0 / 3.0, 1.0, 0.5]);
        assert!(after[1..].iter().all(Vec::is_empty), "{after:?}");
    }

    #[test]
    fn the_same_rule_again_only_after_the_reveal() {
        let (mut game, _, _) = room();
        assert!(game.again(2000).is_err(), "not over unseen numbers");
        game.reveal().unwrap();
        game.again(2000).unwrap();
        let (first, second) = (&game.rounds[0], &game.rounds[1]);
        assert!(second.same_as(first) && second.answers == first.answers);
        assert!(second.open && second.picks.is_empty());
    }

    #[test]
    fn away_is_not_connected_and_no_pick_in_this_round_or_the_last() {
        use rooms::lobby::{PRESENT_MS, away_ids, remove_away};
        let (mut game, ada, bo) = room();
        let cy = admit(&mut game, "Cy", None, 1000).unwrap();
        let id = |game: &Game, key: &str| game.lobby.player(key).unwrap().id;
        let later = 1000 + PRESENT_MS;
        assert_eq!(away_ids(&game, later), [id(&game, &cy)], "Cy never picked");
        game.start(THIRD, 0.0, 10.0, later).unwrap();
        game.pick(&ada, 5.0, later).unwrap();
        assert_eq!(
            away_ids(&game, later),
            [id(&game, &cy)],
            "Bo picked in the round before"
        );
        game.start(THIRD, 0.0, 10.0, later).unwrap();
        let away = away_ids(&game, later);
        assert_eq!(away, [id(&game, &bo), id(&game, &cy)]);
        assert!(json(&host(&game, later)).contains(&format!("\"away\":{}", json(&away))));
        // Connected is never away.
        game.lobby.touch(&bo, later);
        assert_eq!(away_ids(&game, later), [id(&game, &cy)]);

        assert_eq!(remove_away(&mut game, &away, later).unwrap().len(), 1);
        assert_eq!(game.lobby.len(), 2, "only Cy, who was still away");
    }
}
