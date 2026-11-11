//! What each screen is sent.
//!
//! The host's view contains everything. A player's view contains their own
//! ranges and nothing else about the question until the host reveals it.
//! The wall, which is projected for the whole room, gets less than either:
//! no answer and no ranges until the reveal, and then the ranges without
//! names. The views are built here rather than filtered in the page,
//! because anything sent to a page can be read there.

use rooms::lobby::Present;
use serde::Serialize;

use crate::game::{Bound, Change, Game, Pass, Queued, Round, Score, Shift, Stage, Stats};

#[derive(Serialize)]
pub struct Who<'a> {
    /// The player's [`Present::id`], to match them to `people`.
    id: u32,
    name: &'a str,
    /// Whether they gave a range in the first pass, and in the second.
    first: bool,
    second: bool,
}

#[derive(Serialize)]
pub struct Answer<'a> {
    name: &'a str,
    low: f64,
    high: f64,
    width: f64,
    /// Whether it contained the answer, where there is one.
    hit: Option<bool>,
}

#[derive(Serialize)]
pub struct Asked<'a> {
    question: &'a str,
    unit: &'a str,
    stage: Stage,
    answer: Option<f64>,
    /// The level it was asked at, which its numbers are judged against.
    confidence: u8,
    asked: u64,
}

#[derive(Serialize)]
pub struct Past<'a> {
    /// Its place in the room's list of questions, oldest first, which is
    /// how the host corrects its answer.
    index: usize,
    question: &'a str,
    unit: &'a str,
    answer: Option<f64>,
    confidence: u8,
    n: usize,
    hits: Option<usize>,
    /// How many contained it in the first pass, where there was one.
    hits_before: Option<usize>,
}

#[derive(Serialize)]
pub struct Host<'a> {
    join: &'a str,
    /// The wall's key, for the host's "Open the wall view" link.
    wall: &'a str,
    /// Who is in and whose phone is connected, for the roster.
    people: Vec<Present<'a>>,
    /// The level the next question will be asked at. The question on now
    /// carries its own, in `round`.
    confidence: u8,
    players: Vec<Who<'a>>,
    /// Who is away: not connected, and no range for this question or the
    /// one before. See `rooms::lobby::away`.
    away: Vec<u32>,
    round: Option<Asked<'a>>,
    /// The first pass, the second pass, and everybody's final ranges.
    before: Option<Stats>,
    after: Option<Stats>,
    standing: Option<Stats>,
    /// The comparison, once there are two passes to compare.
    shift: Option<Shift>,
    changes: Vec<Change>,
    /// Everybody's final range, lowest first.
    answers: Vec<Answer<'a>>,
    /// The ranges as a picture, once the question is revealed.
    reveal: Option<Reveal>,
    /// Newest first, the question on now excluded.
    history: Vec<Past<'a>>,
    /// The questions lined up, answers and all. The host's alone.
    queue: &'a [Queued],
    scores: Vec<Score>,
}

#[derive(Serialize)]
pub struct Player<'a> {
    name: &'a str,
    /// The room's level; the question carries the one it was asked at.
    confidence: u8,
    round: Option<Asked<'a>>,
    /// This player's two ranges.
    mine_before: Option<Bound>,
    mine_after: Option<Bound>,
    /// How many have answered the pass being taken. Before the reveal,
    /// this is all a player is told about anybody else.
    submitted: usize,
    players: usize,
    /// Everything from here down is only sent after the reveal.
    standing: Option<Stats>,
    before: Option<Stats>,
    shift: Option<Shift>,
    hit: Option<bool>,
    hit_before: Option<bool>,
    /// How this player's own two answers differed.
    mine_changed: Option<Change>,
    /// Everybody's ranges without names, theirs marked, after the reveal.
    reveal: Option<Reveal>,
    score: Option<Score>,
}

impl Round {
    fn asked(&self, to_host: bool) -> Asked<'_> {
        let revealed = self.stage == Stage::Revealed;
        Asked {
            question: &self.question,
            unit: &self.unit,
            stage: self.stage,
            // The host is sent the answer as soon as there is one.
            // Everybody else is sent it after the reveal.
            answer: self.answer.filter(|_| to_host || revealed),
            confidence: self.confidence,
            asked: self.asked,
        }
    }

    /// How many have answered the pass being taken. Once the question is
    /// over, how many answered it at all.
    fn in_so_far(&self, players: &[crate::game::Player]) -> usize {
        match self.stage {
            Stage::First | Stage::Discussion => self.taken(Pass::First),
            Stage::Second => self.taken(Pass::Second),
            Stage::Revealed => players
                .iter()
                .filter(|p| self.standing(&p.key).is_some())
                .count(),
        }
    }
}

/// The host's view, which holds everything about the question.
pub fn host(game: &Game, now: u64) -> Host<'_> {
    let players = &game.lobby.players;
    let round = game.round();
    let answers = round
        .map(|round| {
            let mut answers: Vec<Answer<'_>> = players
                .iter()
                .filter_map(|player| {
                    let bound = round.standing(&player.key)?;
                    Some(Answer {
                        name: &player.name,
                        low: bound.low,
                        high: bound.high,
                        width: bound.width(),
                        hit: round.answer.map(|answer| bound.contains(answer)),
                    })
                })
                .collect();
            answers.sort_by(|a, b| a.low.total_cmp(&b.low));
            answers
        })
        .unwrap_or_default();
    Host {
        join: &game.lobby.join,
        wall: &game.lobby.wall,
        people: game.lobby.roster(now),
        confidence: game.confidence,
        players: players
            .iter()
            .map(|player| Who {
                id: player.id,
                name: &player.name,
                first: round.is_some_and(|r| r.before.contains_key(&player.key)),
                second: round.is_some_and(|r| r.after.contains_key(&player.key)),
            })
            .collect(),
        away: rooms::lobby::away_ids(game, now),
        round: round.map(|round| round.asked(true)),
        before: round.and_then(|r| r.stats(players, Pass::First)),
        after: round.and_then(|r| r.stats(players, Pass::Second)),
        standing: round.and_then(|r| r.standing_stats(players)),
        shift: round.and_then(|r| r.shift(players)),
        changes: round.map(|r| r.changes(players)).unwrap_or_default(),
        answers,
        reveal: round
            .filter(|r| r.stage == Stage::Revealed)
            .map(|r| r.reveal(players, None)),
        history: game
            .rounds
            .iter()
            .enumerate()
            .rev()
            .skip(1)
            .map(|(index, round)| {
                let standing = round.standing_stats(players);
                Past {
                    index,
                    question: &round.question,
                    unit: &round.unit,
                    answer: round.answer,
                    confidence: round.confidence,
                    n: standing.as_ref().map_or(0, |stats| stats.n),
                    hits: standing.and_then(|stats| stats.hits),
                    hits_before: round.shift(players).and_then(|shift| shift.hits_before),
                }
            })
            .collect(),
        scores: game.scores(),
        queue: &game.queue,
    }
}

/// One player's own view. `key` is theirs.
pub fn player<'a>(game: &'a Game, key: &str) -> Option<Player<'a>> {
    let player = game.player(key)?;
    let players = &game.lobby.players;
    let round = game.round();
    let revealed = round.is_some_and(|r| r.stage == Stage::Revealed);
    let shown = round.filter(|_| revealed);
    let mine_before = round.and_then(|r| r.before.get(key)).copied();
    let mine_after = round.and_then(|r| r.after.get(key)).copied();
    let answer = round.and_then(|r| r.answer).filter(|_| revealed);
    let standing = round.and_then(|r| r.standing(key));
    Some(Player {
        name: &player.name,
        confidence: game.confidence,
        round: round.map(|round| round.asked(false)),
        mine_before,
        mine_after,
        submitted: round.map_or(0, |r| r.in_so_far(players)),
        players: players.len(),
        standing: shown.and_then(|r| r.standing_stats(players)),
        before: shown.and_then(|r| r.stats(players, Pass::First)),
        shift: shown.and_then(|r| r.shift(players)),
        hit: match (answer, standing) {
            (Some(answer), Some(bound)) => Some(bound.contains(answer)),
            _ => None,
        },
        hit_before: match (answer, mine_before) {
            (Some(answer), Some(bound)) => Some(bound.contains(answer)),
            _ => None,
        },
        mine_changed: shown.and_then(|r| {
            r.changes(players)
                .into_iter()
                .find(|change| change.name == player.name)
        }),
        reveal: shown.map(|r| r.reveal(players, Some(key))),
        // The revealed questions only. A tally that included the open
        // question would say whether this player's own range had hit.
        score: game
            .scores_revealed()
            .into_iter()
            .find(|score| score.name == player.name),
    })
}

/// How many of the board the wall names.
const WALL_TOP: usize = 3;

/// One range in the reveal's picture, without whose it was.
#[derive(Serialize)]
pub struct Range {
    low: f64,
    high: f64,
    hit: Option<bool>,
    /// Their first range, where this one is their second, drawn faintly
    /// behind it.
    first: Option<(f64, f64)>,
    /// Whether it is the range of the phone it is sent to.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    mine: bool,
}

/// The two passes compared, as counts only.
#[derive(Serialize)]
pub struct Twice {
    /// People who answered both times.
    n: usize,
    /// Of those, how many changed their range.
    moved: usize,
    hits_before: Option<usize>,
    hits_after: Option<usize>,
}

/// The ranges against the answer, once a question is revealed: for the
/// wall, the host and each phone.
#[derive(Serialize)]
pub struct Reveal {
    answer: Option<f64>,
    /// Everybody's final range, by midpoint, with no names.
    ranges: Vec<Range>,
    /// The median lower and upper bound.
    middle: Option<(f64, f64)>,
    hits: Option<usize>,
    twice: Option<Twice>,
}

#[derive(Serialize)]
pub struct WallRound<'a> {
    question: &'a str,
    unit: &'a str,
    /// The level it was asked at.
    confidence: u8,
    stage: Stage,
    /// How many have sent the pass being taken, and how many are in.
    sent: usize,
    of: usize,
    /// Only once the host has revealed it.
    reveal: Option<Reveal>,
}

/// Somebody at the top of the board.
#[derive(Serialize)]
pub struct Top<'a> {
    name: &'a str,
    hits: usize,
    scored: usize,
    /// Their interval score; see `game::interval_cost`.
    cost: f64,
}

/// The game's part of the wall, which is projected for the whole room.
///
/// Before the reveal: the question, its unit, the stage and how many have
/// sent a range. Never the answer, and never a range, because either one
/// on the wall would anchor everybody's next answer. After the reveal:
/// the answer and the ranges as unnamed bars. Names only for the top of
/// the board, never next to a range.
#[derive(Serialize)]
pub struct Wall<'a> {
    /// The room's level, for the wall before the first question.
    confidence: u8,
    round: Option<WallRound<'a>>,
    top: Vec<Top<'a>>,
}

impl Round {
    /// The picture of a revealed question. `mine` marks one player's
    /// range, for their own phone.
    fn reveal(&self, players: &[crate::game::Player], mine: Option<&str>) -> Reveal {
        let mut ranges: Vec<Range> = players
            .iter()
            .filter_map(|p| {
                let bound = self.standing(&p.key)?;
                Some(Range {
                    low: bound.low,
                    high: bound.high,
                    hit: self.answer.map(|a| bound.contains(a)),
                    first: self
                        .after
                        .contains_key(&p.key)
                        .then(|| self.before.get(&p.key))
                        .flatten()
                        .map(|b| (b.low, b.high)),
                    mine: mine == Some(p.key.as_str()),
                })
            })
            .collect();
        let middle = |r: &Range| r.low + r.high;
        ranges.sort_by(|a, b| {
            middle(a)
                .total_cmp(&middle(b))
                .then(a.low.total_cmp(&b.low))
        });
        let standing = self.standing_stats(players);
        Reveal {
            answer: self.answer,
            ranges,
            middle: standing.as_ref().map(|s| (s.median_low, s.median_high)),
            hits: standing.and_then(|s| s.hits),
            twice: self.shift(players).map(|shift| Twice {
                n: shift.n,
                moved: shift.moved,
                hits_before: shift.hits_before,
                hits_after: shift.hits_after,
            }),
        }
    }
}

pub fn wall(game: &Game) -> Wall<'_> {
    let players = &game.lobby.players;
    let round = game.round();
    let revealed = round.is_some_and(|r| r.stage == Stage::Revealed);
    let scores = if revealed {
        game.scores_revealed()
    } else {
        Vec::new()
    };
    Wall {
        confidence: game.confidence,
        round: round.map(|r| WallRound {
            question: &r.question,
            unit: &r.unit,
            confidence: r.confidence,
            stage: r.stage,
            sent: r.in_so_far(players),
            of: players.len(),
            reveal: revealed.then(|| r.reveal(players, None)),
        }),
        top: scores
            .iter()
            .filter(|score| score.scored > 0)
            .take(WALL_TOP)
            .filter_map(|score| {
                // Borrowed from the lobby rather than the score, which
                // owns its copy.
                let name = players.iter().find(|p| p.name == score.name)?;
                Some(Top {
                    name: &name.name,
                    hits: score.hits,
                    scored: score.scored,
                    cost: score.cost()?,
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rooms::lobby::admit;

    /// A number that could not appear in the view by accident.
    const ANSWER: f64 = 1_234_567.0;

    fn room() -> (Game, String) {
        let mut game = Game::new(1000);
        let key = admit(&mut game, "Ada", None, 1000).unwrap();
        game.ask("How many?", "people", Some(ANSWER), 1000).unwrap();
        game.bound(&key, 1.0, 2.0, 1001).unwrap();
        (game, key)
    }

    fn json<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("the view serializes")
    }

    #[test]
    fn the_answer_does_not_reach_a_player_before_the_reveal() {
        let (mut game, key) = room();
        let before = json(&player(&game, &key).unwrap());
        assert!(
            !before.contains("1234567"),
            "the answer was sent out early: {before}"
        );
        assert!(
            json(&host(&game, 1000)).contains("1234567"),
            "the host has it"
        );

        game.reveal().unwrap();
        assert!(json(&player(&game, &key).unwrap()).contains("1234567"));
    }

    #[test]
    fn nor_does_what_anybody_else_said() {
        let (mut game, key) = room();
        let other = admit(&mut game, "Bo", None, 1000).unwrap();
        game.bound(&other, 500.0, 600.0, 1001).unwrap();
        let view = player(&game, &key).unwrap();
        assert_eq!(view.submitted, 2, "only the count");
        assert_eq!(view.players, 2);
        let text = json(&view);
        assert!(!text.contains("Bo"), "another player's name: {text}");
        assert!(!text.contains("500"), "another player's range: {text}");
    }

    #[test]
    fn a_players_own_tally_waits_for_the_reveal_too() {
        let (game, key) = room();
        let view = player(&game, &key).unwrap();
        assert!(
            view.score.is_none(),
            "a tally over an open question says whether the range hit"
        );
    }

    #[test]
    fn the_room_is_shown_once_it_is_revealed() {
        let (mut game, key) = room();
        game.reveal().unwrap();
        let view = player(&game, &key).unwrap();
        assert!(view.standing.is_some());
        assert_eq!(view.hit, Some(false), "1 to 2 does not contain 1234567");
        assert_eq!(view.score.map(|s| s.scored), Some(1));
    }

    #[test]
    fn a_player_is_sent_their_own_two_ranges_and_what_changed() {
        let (mut game, key) = room();
        game.move_to(Stage::Second).unwrap();
        game.bound(&key, 1_000_000.0, 2_000_000.0, 1002).unwrap();

        let view = player(&game, &key).unwrap();
        assert_eq!(view.mine_before.map(|b| b.high), Some(2.0));
        assert_eq!(view.mine_after.map(|b| b.high), Some(2_000_000.0));
        assert!(
            view.mine_changed.is_none(),
            "what changed waits for the reveal, like everything else"
        );

        game.reveal().unwrap();
        let view = player(&game, &key).unwrap();
        let change = view.mine_changed.expect("both ranges are in");
        assert_eq!(change.before.high, 2.0);
        assert_eq!(change.after.low, 1_000_000.0);
        assert_eq!(view.hit, Some(true), "the second range caught it");
        assert_eq!(view.hit_before, Some(false), "the first did not");
    }

    #[test]
    fn the_host_sees_both_passes_apart_and_where_the_room_ended_up() {
        let (mut game, key) = room();
        let other = admit(&mut game, "Bo", None, 1000).unwrap();
        game.bound(&other, 10.0, 20.0, 1001).unwrap();
        game.move_to(Stage::Second).unwrap();
        game.bound(&key, 5.0, 6.0, 1002).unwrap();

        let view = host(&game, 1000);
        assert_eq!(view.before.as_ref().map(|s| s.n), Some(2));
        assert_eq!(view.after.as_ref().map(|s| s.n), Some(1), "only Ada again");
        assert_eq!(
            view.standing.as_ref().map(|s| s.n),
            Some(2),
            "Bo's first range still stands for Bo"
        );
        assert_eq!(view.answers.len(), 2);
        assert_eq!(view.answers[0].name, "Ada", "5 to 6 is the lowest");
        assert!(view.players[0].second);
        assert!(!view.players[1].second);
    }

    #[test]
    fn the_host_sees_the_ranges_lowest_first() {
        let (mut game, _) = room();
        let other = admit(&mut game, "Bo", None, 1000).unwrap();
        game.bound(&other, -5.0, 0.0, 1001).unwrap();
        let view = host(&game, 1000);
        assert_eq!(view.answers.len(), 2);
        assert_eq!(view.answers[0].name, "Bo");
        assert_eq!(view.answers[0].hit, Some(false));
        assert_eq!(view.players.len(), 2);
        assert!(view.players.iter().all(|who| who.first));
    }

    #[test]
    fn a_room_with_no_question_in_it_still_renders() {
        let mut game = Game::new(1000);
        let key = admit(&mut game, "Ada", None, 1000).unwrap();
        let view = host(&game, 1000);
        assert!(view.round.is_none());
        assert!(view.answers.is_empty());
        assert!(view.history.is_empty());
        let view = player(&game, &key).unwrap();
        assert!(view.round.is_none());
        assert!(view.mine_before.is_none());
    }

    #[test]
    fn every_screen_judges_a_question_by_the_level_it_was_asked_at() {
        let (mut game, key) = room();
        game.reveal().unwrap();
        game.set_confidence(50).unwrap();
        assert_eq!(host(&game, 1000).round.map(|r| r.confidence), Some(90));
        assert_eq!(
            player(&game, &key).unwrap().round.map(|r| r.confidence),
            Some(90)
        );
        assert_eq!(wall(&game).round.map(|r| r.confidence), Some(90));
        game.ask("Next", "", None, 2000).unwrap();
        let view = host(&game, 1000);
        assert_eq!(view.round.map(|r| r.confidence), Some(50));
        assert_eq!(view.history[0].confidence, 90);
    }

    #[test]
    fn a_phone_is_sent_the_picture_after_the_reveal_with_its_own_range_marked() {
        let (mut game, ada) = room();
        let bo = admit(&mut game, "Bo", None, 1000).unwrap();
        game.bound(&bo, 1_000_000.0, 2_000_000.0, 1001).unwrap();
        game.move_to(Stage::Second).unwrap();
        game.bound(&bo, 1_100_000.0, 1_900_000.0, 1002).unwrap();
        assert!(
            player(&game, &ada).unwrap().reveal.is_none(),
            "not before it"
        );
        assert!(host(&game, 1000).reveal.is_none());

        game.reveal().unwrap();
        let view = player(&game, &ada).unwrap();
        let reveal = view.reveal.as_ref().expect("after it");
        let mine: Vec<bool> = reveal.ranges.iter().map(|r| r.mine).collect();
        assert_eq!(mine, [true, false], "Ada's is 1 to 2, the lower one");
        assert_eq!(reveal.ranges[0].first, None, "Ada only answered once");
        assert_eq!(reveal.ranges[1].first, Some((1_000_000.0, 2_000_000.0)));
        let ranges = json(&reveal.ranges);
        assert!(!ranges.contains("Bo") && !ranges.contains(&bo), "{ranges}");

        let host = host(&game, 1000);
        let reveal = host.reveal.expect("the host has it too");
        assert!(
            reveal.ranges.iter().all(|r| !r.mine),
            "nothing is the host's"
        );
        let wall = wall(&game);
        let reveal = wall.round.and_then(|r| r.reveal).unwrap();
        assert!(reveal.ranges.iter().all(|r| !r.mine));
    }

    #[test]
    fn the_questions_lined_up_reach_the_host_and_nobody_else() {
        let (mut game, key) = room();
        game.set_queue(crate::game::parse_queue("Secret question | 7654321").unwrap())
            .unwrap();
        assert!(json(&host(&game, 1000)).contains("7654321"));
        for text in [json(&player(&game, &key).unwrap()), json(&wall(&game))] {
            assert!(
                !text.contains("7654321") && !text.contains("Secret"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_key_that_is_not_in_the_room_has_no_view() {
        let (game, _) = room();
        assert!(player(&game, "nobody").is_none());
    }

    #[test]
    fn the_history_holds_every_question_but_the_one_on_now() {
        let (mut game, key) = room();
        game.ask("Second", "", None, 2000).unwrap();
        game.bound(&key, 3.0, 4.0, 2001).unwrap();
        let view = host(&game, 1000);
        assert_eq!(view.round.map(|r| r.question), Some("Second"));
        assert_eq!(view.history.len(), 1);
        assert_eq!(view.history[0].question, "How many?");
        assert_eq!(view.history[0].hits, Some(0));
    }

    #[test]
    fn the_wall_shows_no_answer_and_no_range_before_the_reveal() {
        let (mut game, ada) = room();
        let bo = admit(&mut game, "Bo", None, 1000).unwrap();
        game.bound(&bo, 777_001.0, 777_002.0, 1001).unwrap();
        for stage in [Stage::First, Stage::Discussion, Stage::Second] {
            game.move_to(stage).unwrap();
            let text = json(&wall(&game));
            for secret in ["1234567", "777001", "777002", "Ada", "Bo", &ada, &bo] {
                assert!(!text.contains(secret), "{secret} on the wall: {text}");
            }
            assert!(text.contains("How many?"), "{text}");
        }
        let view = wall(&game);
        let round = view.round.expect("a question is on");
        assert_eq!(
            (round.sent, round.of),
            (0, 2),
            "nobody has sent a second range"
        );
        assert!(round.reveal.is_none());
        assert!(view.top.is_empty());
    }

    #[test]
    fn the_wall_shows_the_ranges_without_names_after_the_reveal() {
        let (mut game, _) = room();
        let bo = admit(&mut game, "Bo", None, 1000).unwrap();
        game.bound(&bo, 1_000_000.0, 2_000_000.0, 1001).unwrap();
        game.reveal().unwrap();
        let view = wall(&game);
        let reveal = view.round.as_ref().and_then(|r| r.reveal.as_ref()).unwrap();
        assert_eq!(reveal.answer, Some(ANSWER));
        assert_eq!(reveal.ranges.len(), 2);
        assert_eq!(reveal.ranges[0].high, 2.0, "by midpoint");
        assert_eq!(reveal.ranges[1].hit, Some(true));
        assert_eq!(reveal.hits, Some(1));
        let ranges = json(&reveal.ranges);
        assert!(
            !ranges.contains("Ada") && !ranges.contains("Bo"),
            "{ranges}"
        );
        let top: Vec<_> = view.top.iter().map(|t| t.name).collect();
        assert_eq!(top, ["Bo", "Ada"], "the board, by name, best first");
    }

    #[test]
    fn away_is_not_connected_and_no_range_for_this_question_or_the_last() {
        use rooms::lobby::{PRESENT_MS, away_ids, remove_away};
        let (mut game, ada) = room();
        let bo = admit(&mut game, "Bo", None, 1000).unwrap();
        let id = |game: &Game, key: &str| game.lobby.player(key).unwrap().id;
        let later = 1000 + PRESENT_MS;
        assert_eq!(
            away_ids(&game, later),
            [id(&game, &bo)],
            "Bo never answered"
        );
        game.ask("And now?", "", None, later).unwrap();
        assert_eq!(away_ids(&game, later), [id(&game, &bo)], "Ada did, before");
        game.ask("And then?", "", None, later).unwrap();
        let away = away_ids(&game, later);
        assert_eq!(away, [id(&game, &ada), id(&game, &bo)]);
        assert!(json(&host(&game, later)).contains(&format!("\"away\":{}", json(&away))));
        game.lobby.touch(&bo, later);
        assert_eq!(
            away_ids(&game, later),
            [id(&game, &ada)],
            "connected is here"
        );
        assert_eq!(remove_away(&mut game, &away, later).unwrap().len(), 1);
        assert_eq!(game.lobby.len(), 1, "only Ada, who was still away");
    }
}
