//! What each screen is sent.
//!
//! The facilitator sees everything. A player is sent their own answers,
//! and, once paired, where their partners stand on the statement they were
//! given: that is the conversation. Between rounds they are also sent the
//! few people who disagree with them most, and on what, so they can go
//! and find them without waiting for a round.

use std::collections::BTreeMap;

use rooms::lobby::Present;
use serde::Serialize;

use crate::game::{
    CLOSE_CAP, Game, Group, KUDOS_POINTS, MINUTES, Mode, PICKS, Phase, Player, SIDES, Scale, Score,
    Statement, UPDATE, spot,
};
use crate::statements::TOPICS;
use crate::warmup::{self, Stage, bits};

#[derive(Serialize)]
pub struct TopicChoice {
    key: &'static str,
    name: &'static str,
    size: usize,
    chosen: bool,
}

#[derive(Serialize)]
pub struct Said {
    name: String,
    before: i16,
    /// Only once the whole group is done, since partners guess blind.
    after: Option<i16>,
    /// Whether they have answered and guessed.
    done: bool,
}

#[derive(Serialize)]
pub struct GroupView<'a> {
    number: usize,
    /// Where the group meets: "🦊 Fox".
    spot: String,
    statement: &'a Statement,
    people: Vec<Said>,
    gap: i16,
    /// Everybody has answered and guessed, so the result can be shown.
    settled: bool,
    /// How much of the gap they closed, once settled.
    closed: Option<i16>,
    /// What that closing scored each of them: `closed`, up to `CLOSE_CAP`.
    points: Option<i32>,
}

#[derive(Serialize)]
pub struct RoundView<'a> {
    number: usize,
    groups: Vec<GroupView<'a>>,
    waiting: Vec<String>,
}

#[derive(Serialize)]
pub struct Who {
    /// What the host's Away button sends.
    id: u32,
    name: String,
    away: bool,
    answered: usize,
    of: usize,
    /// How many answers moved by at least `UPDATE` since they were given.
    updated: usize,
    moved: i32,
    lines: usize,
    score: Score,
}

/// One line of the board.
#[derive(Serialize)]
pub struct Leader {
    name: String,
    #[serde(flatten)]
    score: Score,
}

/// The board from these scores, best total first.
fn leaders(game: &Game, scores: &BTreeMap<String, Score>) -> Vec<Leader> {
    let mut out: Vec<Leader> = game
        .players()
        .filter_map(|p| {
            scores.get(p.key).map(|&score| Leader {
                name: p.name.to_string(),
                score,
            })
        })
        .filter(|l| l.score.total > 0)
        .collect();
    out.sort_by_key(|l| std::cmp::Reverse(l.score.total));
    out
}

/// A prize at the end of the evening, which can be shared.
#[derive(Serialize)]
pub struct Award {
    title: &'static str,
    why: &'static str,
    names: Vec<String>,
    /// The number behind it, in words.
    detail: String,
}

/// Several small prizes rather than one: for listening, for being worth
/// listening to, for changing your own mind, for changing others', and
/// for a bingo.
fn awards(game: &Game) -> Vec<Award> {
    let scores = game.scores();
    let named = |key: &String| game.player(key).map(|p| p.name.to_string());
    let best = |value: &dyn Fn(&Score) -> i32| -> (i32, Vec<String>) {
        let top = scores.values().map(value).max().unwrap_or(0);
        let names = scores
            .iter()
            .filter(|(_, s)| top > 0 && value(s) == top)
            .filter_map(|(k, _)| named(k))
            .collect();
        (top, names)
    };
    let mut out = Vec::new();
    let mut add = |title, why, (top, names): (i32, Vec<String>), detail: String| {
        if top > 0 && !names.is_empty() {
            out.push(Award {
                title,
                why,
                names,
                detail,
            });
        }
    };
    let (top, names) = best(&|s| s.listening);
    add(
        "Best listener",
        "Guessed best where their partners would land",
        (top, names),
        format!("{top} points"),
    );
    let (top, names) = best(&|s| s.kudos as i32);
    add(
        "Most worth listening to",
        "Partners said they learned something from them",
        (top, names),
        format!("{top} × learned something"),
    );
    let (top, names) = best(&|s| s.best_closed);
    add(
        "Best conversation",
        "Closed the widest gap in one round",
        (top, names),
        format!("closed a gap of {top}"),
    );
    let (top, names) = best(&|s| s.opened);
    add(
        "Open mind",
        "Moved furthest towards the people they talked to",
        (top, names),
        format!("{top} points moved"),
    );
    let (top, names) = best(&|s| s.persuaded);
    add(
        "Most persuasive",
        "Partners moved furthest towards them",
        (top, names),
        format!("{top} points"),
    );
    if game.mode == Mode::Bingo {
        let top = game.players().map(|p| game.lines(p)).max().unwrap_or(0);
        let names = game
            .players()
            .filter(|p| top > 0 && game.lines(*p) == top)
            .map(|p| p.name.to_string())
            .collect();
        add(
            "Bingo",
            "Talked about a full line of their card",
            (top as i32, names),
            format!("{} {}", top, if top == 1 { "line" } else { "lines" }),
        );
    }
    out
}

/// One line of the warm-up board.
#[derive(Serialize)]
pub struct Calibrated {
    name: String,
    bits: f64,
    /// The Bayes bot, which is par.
    bot: bool,
}

/// Everybody's warm-up bits, best first, with the bot among them.
fn calibrated(game: &Game) -> Vec<Calibrated> {
    if !game.warmups.iter().any(|w| w.stage == Stage::Revealed) {
        return Vec::new();
    }
    let (board, bot) = warmup::board(&game.warmups);
    let mut out: Vec<Calibrated> = board
        .iter()
        .filter_map(|(key, &bits)| {
            Some(Calibrated {
                name: game.player(key)?.name.to_string(),
                bits,
                bot: false,
            })
        })
        .collect();
    out.push(Calibrated {
        name: "Bayes bot".into(),
        bits: bot,
        bot: true,
    });
    out.sort_by(|a, b| b.bits.total_cmp(&a.bits));
    out
}

/// How a warm-up question came out.
#[derive(Serialize)]
pub struct Outcome {
    truth: bool,
    /// The room's answers after their clues, for the wall to plot.
    answers: Vec<i16>,
    mean_before: Option<f64>,
    mean_after: Option<f64>,
    /// The bot's bits on this one.
    bot: Option<f64>,
    /// The room's average bits on this one.
    room: Option<f64>,
}

#[derive(Serialize)]
pub struct WarmHost<'a> {
    number: usize,
    setup: &'a str,
    claim: &'a str,
    prior: f64,
    stage: Stage,
    before: usize,
    after: usize,
    outcome: Option<Outcome>,
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let all: Vec<f64> = values.collect();
    (!all.is_empty()).then(|| all.iter().sum::<f64>() / all.len() as f64)
}

fn warm_host(game: &Game) -> Option<WarmHost<'_>> {
    let w = game.warmup()?;
    let truth = w.puzzle.truth;
    Some(WarmHost {
        number: w.number,
        setup: &w.puzzle.setup,
        claim: &w.puzzle.claim,
        prior: w.puzzle.prior,
        stage: w.stage,
        before: w.before.len(),
        after: w.after.len(),
        outcome: (w.stage == Stage::Revealed).then(|| Outcome {
            truth,
            answers: w.after.values().copied().collect(),
            mean_before: mean(w.before.values().map(|&v| v as f64)),
            mean_after: mean(w.after.values().map(|&v| v as f64)),
            bot: w.bot_bits(),
            room: mean(w.after.values().map(|&v| bits(v, truth))),
        }),
    })
}

#[derive(Serialize)]
pub struct Tally<'a> {
    statement: &'a Statement,
    n: usize,
    mean: f64,
    /// The standard deviation, on a common scale from -100 to 100.
    spread: f64,
    first_mean: f64,
    updated: usize,
}

/// A statement somebody picked, and how many did.
#[derive(Serialize)]
pub struct Picked<'a> {
    statement: &'a Statement,
    n: usize,
}

/// How the picking is going, for the wall.
#[derive(Serialize)]
pub struct Picking<'a> {
    /// How many people have picked at least one statement.
    pickers: usize,
    /// How many each person can pick.
    limit: usize,
    /// Every statement picked so far, the most picked first: the deck the
    /// room will play if the picks closed now.
    picked: Vec<Picked<'a>>,
}

fn picking(game: &Game) -> Option<Picking<'_>> {
    if game.phase != Phase::Picking {
        return None;
    }
    let mut picked: Vec<Picked> = game
        .pick_counts()
        .into_iter()
        .filter_map(|(id, n)| {
            Some(Picked {
                statement: game.statement(id)?,
                n,
            })
        })
        .collect();
    picked.sort_by_key(|p| (std::cmp::Reverse(p.n), p.statement.id));
    Some(Picking {
        pickers: game.players().filter(|p| !p.picks.is_empty()).count(),
        limit: PICKS,
        picked,
    })
}

#[derive(Serialize)]
pub struct HostView<'a> {
    join: &'a str,
    /// The key of the wall, `/w/<key>`, for the projector.
    wall: &'a str,
    /// Everybody, and whether their phone is connected, for the roster
    /// the host can take people out of.
    people: Vec<Present<'a>>,
    mode: Mode,
    scale: Scale,
    topics: Vec<TopicChoice>,
    custom: String,
    side: u8,
    sides: &'static [u8],
    minutes: u32,
    lengths: &'static [u32],
    deck: usize,
    locked: bool,
    phase: Phase,
    ends: u64,
    round: Option<RoundView<'a>>,
    rounds: usize,
    players: Vec<Who>,
    contested: Vec<Tally<'a>>,
    shifted: Vec<Tally<'a>>,
    /// What the most people have answered, which is where a free-for-all
    /// room is converging.
    popular: Vec<Tally<'a>>,
    answers: usize,
    updates: usize,
    leaders: Vec<Leader>,
    /// What one kudos is worth, to explain the board.
    kudos_points: i32,
    awards: Vec<Award>,
    warmup: Option<WarmHost<'a>>,
    /// How many warm-up questions have been asked.
    warmups: usize,
    calibrated: Vec<Calibrated>,
    /// In the pick mode, while people are picking.
    picking: Option<Picking<'a>>,
}

/// Somebody's name, or, for somebody the host took out after their group
/// had settled, what they are now.
fn name_of(game: &Game, key: &str) -> String {
    game.player(key)
        .map(|p| p.name.to_string())
        .unwrap_or_else(|| "(left)".into())
}

fn group_view<'a>(game: &'a Game, number: usize, group: &Group) -> Option<GroupView<'a>> {
    let statement = game.statement(group.statement)?;
    let settled = group.settled();
    let people: Vec<Said> = group
        .members
        .iter()
        .map(|m| Said {
            name: name_of(game, m),
            before: group.before.get(m).copied().unwrap_or(0),
            after: group.after.get(m).copied().filter(|_| settled),
            done: group.after.contains_key(m)
                && group
                    .members
                    .iter()
                    .filter(|o| *o != m)
                    .all(|o| group.guesses.get(m).is_some_and(|g| g.contains_key(o))),
        })
        .collect();
    let gap = people
        .iter()
        .flat_map(|a| {
            people
                .iter()
                .map(move |b| statement.gap(a.before, b.before))
        })
        .max()
        .unwrap_or(0);
    let (before, after) = group.gaps(statement);
    let closed = settled.then(|| (before - after).max(0));
    Some(GroupView {
        number,
        spot: spot(number - 1),
        statement,
        people,
        gap,
        settled,
        closed,
        points: closed.map(|c| i32::from(c).min(CLOSE_CAP)),
    })
}

/// How many of a player's answers moved far enough to count as an update,
/// and how far they moved in all, on the shared scale of `Statement::gap`.
fn moved(game: &Game, player: Player<'_>) -> (usize, i32) {
    player
        .answers
        .iter()
        .filter_map(|(id, now)| {
            let then = player.first.get(id)?;
            Some(game.statement(*id)?.gap(*now, *then))
        })
        .fold((0, 0), |(n, sum), d| {
            (n + (d >= UPDATE) as usize, sum + d as i32)
        })
}

/// The room's answers to one statement. Means are on the statement's own
/// scale; the spread is on a common one, -100 to 100, so that spreads
/// compare across the two kinds.
fn tally<'a>(game: &'a Game, statement: &'a Statement) -> Option<Tally<'a>> {
    let scale = |v: i16| -> f64 { statement.norm(v) * 100.0 };
    let now: Vec<f64> = game
        .players()
        .filter_map(|p| p.answers.get(&statement.id).map(|&v| scale(v)))
        .collect();
    if now.is_empty() {
        return None;
    }
    let firsts: Vec<f64> = game
        .players()
        .filter_map(|p| p.first.get(&statement.id).map(|&v| scale(v)))
        .collect();
    let n = now.len() as f64;
    let mean = now.iter().sum::<f64>() / n;
    let spread = (now.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    let first_mean = firsts.iter().sum::<f64>() / firsts.len().max(1) as f64;
    let updated = game
        .players()
        .filter(
            |p| match (p.first.get(&statement.id), p.answers.get(&statement.id)) {
                (Some(a), Some(b)) => statement.gap(*a, *b) >= UPDATE,
                _ => false,
            },
        )
        .count();
    // Back on the statement's own scale, for showing.
    let back = |v: f64| {
        if statement.chance {
            (v + 100.0) / 2.0
        } else {
            v / 100.0 * 3.0 + 4.0
        }
    };
    Some(Tally {
        statement,
        n: now.len(),
        mean: back(mean),
        spread,
        first_mean: back(first_mean),
        updated,
    })
}

pub fn host(game: &Game, now: u64) -> HostView<'_> {
    let round = game
        .rounds
        .last()
        .filter(|_| matches!(game.phase, Phase::Talking | Phase::Updating))
        .map(|r| RoundView {
            number: game.rounds.len(),
            groups: r
                .groups
                .iter()
                .enumerate()
                .filter_map(|(i, g)| group_view(game, i + 1, g))
                .collect(),
            waiting: r.waiting.iter().map(|k| name_of(game, k)).collect(),
        });
    let scores = game.scores();
    let players: Vec<Who> = game
        .players()
        .map(|p| {
            let (updated, moved) = moved(game, p);
            Who {
                id: p.id,
                name: p.name.to_string(),
                away: p.away,
                answered: p.answers.len(),
                of: game.hand(p).len(),
                updated,
                moved,
                lines: game.lines(p),
                score: scores.get(p.key).copied().unwrap_or_default(),
            }
        })
        .collect();
    let tallies: Vec<Tally> = game.deck.iter().filter_map(|s| tally(game, s)).collect();
    let mut contested: Vec<&Tally> = tallies.iter().filter(|t| t.n >= 2).collect();
    contested.sort_by(|a, b| b.spread.total_cmp(&a.spread));
    let contested_ids: Vec<u32> = contested.iter().take(5).map(|t| t.statement.id).collect();
    let mut shifted: Vec<&Tally> = tallies.iter().filter(|t| t.updated > 0).collect();
    shifted.sort_by_key(|t| std::cmp::Reverse(t.updated));
    let shifted_ids: Vec<u32> = shifted.iter().take(5).map(|t| t.statement.id).collect();
    let mut popular: Vec<&Tally> = tallies.iter().filter(|t| t.n >= 2).collect();
    popular.sort_by_key(|t| std::cmp::Reverse(t.n));
    let popular_ids: Vec<u32> = popular.iter().take(5).map(|t| t.statement.id).collect();
    let pick = |ids: &[u32]| -> Vec<Tally> {
        ids.iter()
            .filter_map(|&id| game.statement(id).and_then(|s| tally(game, s)))
            .collect()
    };
    HostView {
        join: &game.lobby.join,
        wall: &game.lobby.wall,
        people: game.lobby.roster(now),
        mode: game.mode,
        scale: game.scale,
        topics: TOPICS
            .iter()
            .map(|t| TopicChoice {
                key: t.key,
                name: t.name,
                size: t.statements.len(),
                chosen: game.topics.iter().any(|k| k == t.key),
            })
            .collect(),
        custom: game.custom.join("\n"),
        side: game.side,
        sides: SIDES,
        minutes: game.minutes,
        lengths: MINUTES,
        deck: game.deck.len(),
        locked: game.locked(),
        phase: game.phase,
        ends: game.ends,
        round,
        rounds: game.rounds.len(),
        answers: game.players().map(|p| p.answers.len()).sum(),
        updates: game.players().map(|p| moved(game, p).0).sum(),
        players,
        leaders: leaders(game, &game.scores()),
        kudos_points: KUDOS_POINTS,
        awards: awards(game),
        warmup: warm_host(game),
        warmups: game.warmups.len(),
        calibrated: calibrated(game),
        picking: picking(game),
        contested: pick(&contested_ids),
        shifted: pick(&shifted_ids),
        popular: pick(&popular_ids),
    }
}

#[derive(Serialize)]
pub struct Square<'a> {
    statement: &'a Statement,
    answer: Option<i16>,
    first: Option<i16>,
    marked: bool,
    /// In the pick mode, whether this player picked it.
    picked: bool,
    /// In the free mode, how many other people have answered it.
    count: usize,
    /// In the free mode, and only once this player has answered it: who
    /// else did, and what they said.
    others: Vec<Seen>,
}

#[derive(Serialize)]
pub struct Seen {
    name: String,
    answer: i16,
}

#[derive(Serialize)]
pub struct Partner {
    name: String,
    answer: i16,
    /// Where they stand now: only once this player has guessed it and
    /// said where they stand themselves.
    after: Option<i16>,
    /// Whether they are done with the round too, so `after` is final.
    done: bool,
    /// This player's guess at it.
    guess: Option<i16>,
    /// Whether this player said they learned something from them.
    kudos: bool,
}

#[derive(Serialize)]
pub struct Mine<'a> {
    number: usize,
    spot: String,
    statement: &'a Statement,
    partners: Vec<Partner>,
    before: i16,
    after: Option<i16>,
    /// Whether this player is done with the round: their answer and
    /// guesses are in, and the answer stays as the round's.
    done: bool,
}

#[derive(Serialize)]
pub struct Opposite<'a> {
    name: String,
    statement: &'a Statement,
    mine: i16,
    theirs: i16,
    gap: i16,
}

#[derive(Serialize)]
pub struct PlayerView<'a> {
    name: String,
    mode: Mode,
    side: u8,
    phase: Phase,
    ends: u64,
    round: usize,
    squares: Vec<Square<'a>>,
    topics: Vec<(&'static str, &'static str)>,
    group: Option<Mine<'a>>,
    waiting: bool,
    /// Sitting rounds out: see [`crate::game::Sheet::away`].
    away: bool,
    opposites: Vec<Opposite<'a>>,
    lines: usize,
    updated: usize,
    moved: i32,
    players: usize,
    score: Score,
    /// In the free mode, the quick-fire feed's next statements, best first.
    next: Vec<u32>,
    /// In the pick mode, how many statements each person can pick.
    picks: usize,
    /// Their place on the board, once they have one.
    rank: Option<usize>,
    /// The board's top three, shown when the evening is over.
    winners: Vec<Leader>,
    /// The prizes, when the evening is over.
    awards: Vec<Award>,
    warmup: Option<WarmMine<'a>>,
    /// Their warm-up bits so far, and the bot's.
    calibration: Option<(f64, f64)>,
}

/// How a warm-up question came out for one player.
#[derive(Serialize)]
pub struct WarmResult {
    truth: bool,
    bits_before: Option<f64>,
    bits_after: Option<f64>,
    /// Where Bayes would be after their clue, from the true prior...
    ideal: f64,
    /// ...and from where they said they were before it.
    ideal_from_yours: Option<f64>,
}

#[derive(Serialize)]
pub struct WarmMine<'a> {
    number: usize,
    setup: &'a str,
    claim: &'a str,
    stage: Stage,
    before: Option<i16>,
    after: Option<i16>,
    /// Their clue, once the clues are out.
    clue: Option<String>,
    result: Option<WarmResult>,
}

fn warm_mine<'a>(game: &'a Game, key: &str) -> Option<WarmMine<'a>> {
    let w = game.warmup()?;
    let before = w.before.get(key).copied();
    let after = w.after.get(key).copied();
    let truth = w.puzzle.truth;
    Some(WarmMine {
        number: w.number,
        setup: &w.puzzle.setup,
        claim: &w.puzzle.claim,
        stage: w.stage,
        before,
        after,
        clue: (w.stage != Stage::Before).then(|| w.clue(key)),
        result: (w.stage == Stage::Revealed).then(|| {
            let (ideal, ideal_from_yours) = w.ideal(key);
            WarmResult {
                truth,
                bits_before: before.map(|v| bits(v, truth)),
                bits_after: after.map(|v| bits(v, truth)),
                ideal,
                ideal_from_yours,
            }
        }),
    })
}

/// How many statements ahead the quick-fire feed is sent.
const FEED: usize = 8;

/// How many people a player is shown to go and find.
const OPPOSITES: usize = 5;

pub fn player<'a>(game: &'a Game, key: &str) -> Option<PlayerView<'a>> {
    let me = game.player(key)?;
    let free = game.mode == Mode::Free;
    let squares = game
        .hand(me)
        .into_iter()
        .filter_map(|id| {
            let answer = me.answers.get(&id).copied();
            let mut others: Vec<Seen> = Vec::new();
            let mut count = 0;
            if free {
                for p in game.players().filter(|p| p.key != key) {
                    let Some(&theirs) = p.answers.get(&id) else {
                        continue;
                    };
                    count += 1;
                    // Not before answering: knowing who said what would
                    // anchor the answer.
                    if answer.is_some() {
                        others.push(Seen {
                            name: p.name.to_string(),
                            answer: theirs,
                        });
                    }
                }
            }
            Some(Square {
                statement: game.statement(id)?,
                answer,
                first: me.first.get(&id).copied(),
                marked: me.marked.contains(&id),
                picked: me.picks.contains(&id),
                count,
                others,
            })
        })
        .collect();
    let group = game.group_of(key).and_then(|(i, g)| {
        let guesses = g.guesses.get(key);
        let done = g.done(key);
        let kudos = g.kudos.get(key);
        Some(Mine {
            number: i + 1,
            spot: spot(i),
            statement: game.statement(g.statement)?,
            partners: g
                .members
                .iter()
                .filter(|m| *m != key)
                .map(|m| {
                    let guess = guesses.and_then(|gs| gs.get(m)).copied();
                    Partner {
                        name: name_of(game, m),
                        answer: g.before.get(m).copied().unwrap_or(0),
                        after: g.after.get(m).copied().filter(|_| done),
                        done: g.done(m),
                        guess,
                        kudos: kudos.is_some_and(|k| k.contains(m)),
                    }
                })
                .collect(),
            before: g.before.get(key).copied().unwrap_or(0),
            after: g.after.get(key).copied(),
            done,
        })
    });
    let waiting = matches!(game.phase, Phase::Talking | Phase::Updating) && group.is_none();
    let opposites = if matches!(game.phase, Phase::Talking | Phase::Warmup) {
        Vec::new()
    } else {
        let mut found: Vec<Opposite> = game
            .players()
            .filter(|p| p.key != key && !p.away)
            .filter_map(|p| {
                let (id, gap) = game.best_between(me, p)?;
                Some(Opposite {
                    name: p.name.to_string(),
                    statement: game.statement(id)?,
                    mine: me.answers[&id],
                    theirs: p.answers[&id],
                    gap,
                })
            })
            .filter(|o| o.gap > 0)
            .collect();
        found.sort_by_key(|o| std::cmp::Reverse(o.gap));
        found.truncate(OPPOSITES);
        found
    };
    let (updated, moved) = moved(game, me);
    let board = leaders(game, &game.scores());
    let rank = board.iter().position(|l| l.name == me.name).map(|i| i + 1);
    let score = game.scores().get(key).copied().unwrap_or_default();
    let finished = game.phase == Phase::Finished;
    let winners = if finished {
        board.into_iter().take(3).collect()
    } else {
        Vec::new()
    };
    let calibration = calibrated(game);
    let calibration = calibration
        .iter()
        .find(|c| !c.bot && c.name == me.name)
        .zip(calibration.iter().find(|c| c.bot))
        .map(|(mine, bot)| (mine.bits, bot.bits));
    Some(PlayerView {
        name: me.name.to_string(),
        mode: game.mode,
        side: game.side,
        phase: game.phase,
        ends: game.ends,
        round: game.rounds.len(),
        squares,
        topics: TOPICS
            .iter()
            .map(|t| (t.key, t.name))
            .chain([("custom", "From the facilitator")])
            .collect(),
        group,
        waiting,
        away: me.away,
        opposites,
        lines: game.lines(me),
        updated,
        moved,
        players: game.lobby.len(),
        score,
        rank,
        winners,
        awards: if finished { awards(game) } else { Vec::new() },
        warmup: warm_mine(game, key),
        calibration,
        picks: PICKS,
        next: if free {
            game.feed(me, FEED)
        } else {
            Vec::new()
        },
    })
}

// ----------------------------------------------------------------- wall
//
// The wall is projected, so everybody in the room sees it, whoever they
// were paired with. It shows where groups meet and what they talk about,
// how far along the room is, and, once a group has settled, how its
// statement came out, as unnamed answers in order of size. Names only for
// the top of the board and the prizes. Never anybody's answer by name,
// and nothing before the guesses that depend on it are in.

/// The top of a board: a name and a number, nothing else.
#[derive(Serialize)]
pub struct Top {
    name: String,
    total: i32,
}

/// Where a group meets and what it talks about. Not who is in it: their
/// phones tell them that.
#[derive(Serialize)]
pub struct Meeting<'a> {
    spot: String,
    statement: &'a Statement,
}

/// How one statement came out in a round, once every group that talked
/// about it has settled. The answers are sorted, so that they say nothing
/// about who gave which.
#[derive(Serialize)]
pub struct Settled<'a> {
    statement: &'a Statement,
    groups: usize,
    before: Vec<i16>,
    after: Vec<i16>,
    /// How far the groups closed, on average, on the shared scale.
    closed: i16,
}

#[derive(Serialize)]
pub struct WallRound<'a> {
    number: usize,
    /// Whether the round is still on: talking, or saying where they stand
    /// now. Otherwise this is the last round, between rounds.
    on: bool,
    meetings: Vec<Meeting<'a>>,
    /// How many people are in a group, and how many of them have said
    /// where they stand now and guessed where their partners do.
    people: usize,
    done: usize,
    /// How many could not be paired.
    waiting: usize,
    results: Vec<Settled<'a>>,
}

#[derive(Serialize)]
pub struct WallWarm<'a> {
    number: usize,
    setup: &'a str,
    claim: &'a str,
    stage: Stage,
    before: usize,
    after: usize,
    /// Only once the answer is out.
    outcome: Option<Outcome>,
    /// The top of the warm-up board, the bot among it, once revealed.
    board: Vec<Calibrated>,
}

#[derive(Serialize)]
pub struct WallView<'a> {
    phase: Phase,
    mode: Mode,
    /// When the round being talked ends, by the server's clock.
    ends: u64,
    /// How many are in, and how many of them have answered anything.
    people: usize,
    answering: usize,
    answers: usize,
    round: Option<WallRound<'a>>,
    picking: Option<Picking<'a>>,
    warmup: Option<WallWarm<'a>>,
    board: Vec<Top>,
    /// The prizes, once the evening is over.
    awards: Vec<Award>,
}

/// How many people a board on the wall names.
const WALL_TOP: usize = 5;

fn wall_round(game: &Game) -> Option<WallRound<'_>> {
    let on = matches!(game.phase, Phase::Talking | Phase::Updating);
    if !on && game.phase != Phase::Answering {
        return None;
    }
    let round = game.rounds.last()?;
    let mut results: Vec<Settled> = Vec::new();
    let mut statements: Vec<u32> = round.groups.iter().map(|g| g.statement).collect();
    statements.sort_unstable();
    statements.dedup();
    for id in statements {
        let groups: Vec<&Group> = round.groups.iter().filter(|g| g.statement == id).collect();
        if !groups.iter().all(|g| g.settled()) {
            continue;
        }
        let Some(statement) = game.statement(id) else {
            continue;
        };
        let mut before: Vec<i16> = groups
            .iter()
            .flat_map(|g| g.before.values())
            .copied()
            .collect();
        let mut after: Vec<i16> = groups
            .iter()
            .flat_map(|g| g.members.iter().filter_map(|m| g.landed(m)))
            .collect();
        before.sort_unstable();
        after.sort_unstable();
        let closed = groups
            .iter()
            .map(|g| {
                let (b, a) = g.gaps(statement);
                (b - a).max(0) as i32
            })
            .sum::<i32>()
            / groups.len().max(1) as i32;
        results.push(Settled {
            statement,
            groups: groups.len(),
            before,
            after,
            closed: closed as i16,
        });
    }
    let people = round.groups.iter().map(|g| g.members.len()).sum();
    let done = round
        .groups
        .iter()
        .map(|g| {
            g.members
                .iter()
                .filter(|m| {
                    g.after.contains_key(*m)
                        && g.members
                            .iter()
                            .filter(|o| o != m)
                            .all(|o| g.guesses.get(*m).is_some_and(|gs| gs.contains_key(o)))
                })
                .count()
        })
        .sum();
    Some(WallRound {
        number: game.rounds.len(),
        on,
        meetings: if on {
            round
                .groups
                .iter()
                .enumerate()
                .filter_map(|(i, g)| {
                    Some(Meeting {
                        spot: spot(i),
                        statement: game.statement(g.statement)?,
                    })
                })
                .collect()
        } else {
            Vec::new()
        },
        people,
        done,
        waiting: round.waiting.len(),
        results,
    })
}

fn wall_warm(game: &Game) -> Option<WallWarm<'_>> {
    let w = game.warmup()?;
    let revealed = w.stage == Stage::Revealed;
    let mut outcome = warm_host(game)?.outcome;
    if let Some(o) = outcome.as_mut() {
        o.answers.sort_unstable();
    }
    Some(WallWarm {
        number: w.number,
        setup: &w.puzzle.setup,
        claim: &w.puzzle.claim,
        stage: w.stage,
        before: w.before.len(),
        after: w.after.len(),
        outcome,
        board: if revealed {
            let mut board = calibrated(game);
            board.truncate(WALL_TOP + 1);
            board
        } else {
            Vec::new()
        },
    })
}

/// The game's part of the wall, `/w/<key>`: see above for what it may
/// show.
pub fn wall(game: &Game) -> WallView<'_> {
    let finished = game.phase == Phase::Finished;
    WallView {
        phase: game.phase,
        mode: game.mode,
        ends: game.ends,
        people: game.lobby.len(),
        answering: game.players().filter(|p| !p.answers.is_empty()).count(),
        answers: game.players().map(|p| p.answers.len()).sum(),
        round: wall_round(game),
        picking: picking(game),
        warmup: wall_warm(game),
        board: leaders(game, &game.revealed_scores())
            .into_iter()
            .take(WALL_TOP)
            .map(|l| Top {
                name: l.name,
                total: l.score.total,
            })
            .collect(),
        awards: if finished { awards(game) } else { Vec::new() },
    }
}
