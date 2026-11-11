//! One question, everyone's bounds, and what the room thought.
//!
//! The host asks a question and everybody gives a range they are
//! `confidence` per cent sure the answer falls in. The skill the game
//! tests is choosing the width of that range.
//!
//! The room then discusses the question and answers it again. The
//! difference between the two sets of bounds measures how much each
//! person was moved by the discussion: who narrowed, who widened, who did
//! not change, and whether the second answers were closer to the truth.

use std::collections::HashMap;

use rooms::lobby::{HasLobby, Lobby, OnWall, tidy};
use serde::{Deserialize, Serialize};

/// Somebody in the room. The lobby keeps them; see `rooms::lobby`.
pub use rooms::lobby::Player;

pub const MAX_ROUNDS: usize = 200;
pub const MAX_QUESTION: usize = 300;
pub const MAX_UNIT: usize = 24;

/// How sure players are meant to be. A fixed list rather than any number,
/// because the hit rate only means something if the whole room uses the
/// same one.
pub const CONFIDENCES: &[u8] = &[50, 80, 90, 98];

/// The level a room starts at.
pub const CONFIDENCE: u8 = 90;

fn default_confidence() -> u8 {
    CONFIDENCE
}

/// How far a question has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// Taking the first ranges, before the discussion.
    First,
    /// The room is discussing it. No bounds are being taken.
    Discussion,
    /// Taking the second ranges, after the discussion.
    Second,
    /// Everybody can see everything.
    Revealed,
}

impl Stage {
    /// Which set of bounds this stage is taking, if any.
    fn taking(self) -> Option<Pass> {
        match self {
            Stage::First => Some(Pass::First),
            Stage::Second => Some(Pass::Second),
            Stage::Discussion | Stage::Revealed => None,
        }
    }
}

/// Before the discussion, or after it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    First,
    Second,
}

/// One person's range for one question.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Bound {
    pub low: f64,
    pub high: f64,
    pub at: u64,
}

impl Bound {
    pub fn width(&self) -> f64 {
        self.high - self.low
    }

    /// The midpoint of the range. It moves when somebody changes their
    /// estimate; the width moves when they change how sure they are.
    pub fn middle(&self) -> f64 {
        (self.low + self.high) / 2.0
    }

    pub fn contains(&self, answer: f64) -> bool {
        self.low <= answer && answer <= self.high
    }
}

#[derive(Serialize, Deserialize)]
pub struct Round {
    pub question: String,
    /// What the number is in: "years", "km", "€". Shown beside the
    /// fields, so that half the room does not answer in thousands.
    pub unit: String,
    /// The correct answer. The host can enter it before asking or after
    /// seeing the bounds. Nobody else is sent it before the reveal.
    pub answer: Option<f64>,
    /// How sure the room was asked to be, fixed when the question was
    /// asked. Every number about this question is judged against it,
    /// whatever the room's level is now. Rooms saved before it was kept
    /// per question are given the room's level by `Room::migrate`.
    #[serde(default = "default_confidence")]
    pub confidence: u8,
    pub stage: Stage,
    pub asked: u64,
    /// Before the discussion, by player key.
    pub before: HashMap<String, Bound>,
    /// After it.
    pub after: HashMap<String, Bound>,
}

#[derive(Serialize, Deserialize)]
pub struct Game {
    /// Who is in, and the join and wall keys. The host's key is the one
    /// the game is stored under. A field of its own rather than
    /// flattened; rooms saved with `join` and `players` at the top level
    /// are moved into it by `Room::migrate`.
    #[serde(default)]
    pub lobby: Lobby,
    /// The level the next question is asked at. See `Round::confidence`.
    pub confidence: u8,
    /// Oldest first; the last one is the question the room is on.
    pub rounds: Vec<Round>,
    /// Questions the host has lined up, asked one at a time with "Ask
    /// next". Only the host is sent them, answers included.
    #[serde(default)]
    pub queue: Vec<Queued>,
    pub last_seen: u64,
}

impl rooms::Room for Game {
    fn seen(&mut self, now: u64) {
        self.last_seen = now;
    }

    /// A day after the room was last opened, which outlasts the evening
    /// it was used for.
    fn expired(&self, now: u64) -> bool {
        self.last_seen + rooms::DAY_MS < now
    }

    fn keys(&self) -> Vec<&str> {
        self.lobby.keys().collect()
    }

    /// Rooms saved before the lobby, and while it was flattened into the
    /// room, kept `join`, `wall` and `players` at the top level.
    fn migrate(saved: &mut serde_json::Value) {
        rooms::lobby::migrate(saved);
        // Questions asked before each kept its own level were all judged
        // against the room's, so that is the level they keep.
        let Some(room) = saved.as_object_mut() else {
            return;
        };
        let level = room
            .get("confidence")
            .cloned()
            .unwrap_or_else(|| CONFIDENCE.into());
        if let Some(rounds) = room.get_mut("rounds").and_then(|r| r.as_array_mut()) {
            for round in rounds.iter_mut().filter_map(|r| r.as_object_mut()) {
                round.entry("confidence").or_insert_with(|| level.clone());
            }
        }
    }
}

impl HasLobby for Game {
    fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    fn lobby_mut(&mut self) -> &mut Lobby {
        &mut self.lobby
    }

    /// Somebody the host took out. Their ranges go with them, so that
    /// nothing is kept under a key nobody holds; the room's numbers only
    /// ever counted the people in it.
    fn left(&mut self, player: &Player) {
        for round in &mut self.rounds {
            round.before.remove(&player.key);
            round.after.remove(&player.key);
        }
    }

    /// Gave a range, in either pass, to the question on now or the one
    /// before. Somebody who did not, and whose phone is not connected,
    /// is offered to the host as away.
    fn recent(&self, player: &Player) -> bool {
        self.rounds.iter().rev().take(2).any(|round| {
            round.before.contains_key(&player.key) || round.after.contains_key(&player.key)
        })
    }

    /// What the projector shows. See `view::wall`.
    fn wall_game(&self, _now: u64) -> serde_json::Value {
        serde_json::to_value(crate::view::wall(self)).unwrap_or_default()
    }

    /// Once a question is up, it takes the wall: the QR code shrinks to a
    /// corner for latecomers and the names give way to a count, so that
    /// the bars at the reveal fit a projector without scrolling.
    fn on_wall(&self, _now: u64) -> OnWall {
        OnWall {
            busy: self.round().is_some(),
            ..OnWall::default()
        }
    }
}

impl Game {
    pub fn new(now: u64) -> Game {
        Game {
            lobby: Lobby::new(),
            confidence: CONFIDENCE,
            rounds: Vec::new(),
            queue: Vec::new(),
            last_seen: now,
        }
    }

    pub fn round(&self) -> Option<&Round> {
        self.rounds.last()
    }

    fn round_mut(&mut self) -> Result<&mut Round, &'static str> {
        self.rounds.last_mut().ok_or("No question has been asked.")
    }

    pub fn player(&self, key: &str) -> Option<&Player> {
        self.lobby.player(key)
    }

    /// Sets the level the next question is asked at. Refused while a
    /// question is open: the people who have sent a range sent it at the
    /// old level, and everyone else is reading it off their phone.
    pub fn set_confidence(&mut self, confidence: u8) -> Result<(), &'static str> {
        if !CONFIDENCES.contains(&confidence) {
            return Err("That is not one of the confidence levels.");
        }
        if confidence != self.confidence && self.round().is_some_and(|r| r.stage != Stage::Revealed)
        {
            return Err(
                "The level is fixed while a question is open. Change it before the next one.",
            );
        }
        self.confidence = confidence;
        Ok(())
    }

    /// Asks the next question. The one before it stops taking bounds but
    /// keeps everything it was given.
    pub fn ask(
        &mut self,
        question: &str,
        unit: &str,
        answer: Option<f64>,
        now: u64,
    ) -> Result<(), &'static str> {
        let question = tidy(question, MAX_QUESTION);
        if question.is_empty() {
            return Err("A question needs some text.");
        }
        if self.rounds.len() >= MAX_ROUNDS {
            return Err("That is as many questions as one room holds.");
        }
        if answer.is_some_and(|answer| !answer.is_finite()) {
            return Err("That answer is not a number.");
        }
        if let Some(round) = self.rounds.last_mut() {
            round.stage = Stage::Revealed;
        }
        self.rounds.push(Round {
            question,
            unit: tidy(unit, MAX_UNIT),
            answer,
            confidence: self.confidence,
            stage: Stage::First,
            asked: now,
            before: HashMap::new(),
            after: HashMap::new(),
        });
        Ok(())
    }

    /// Replaces the questions lined up. See `parse_queue`.
    pub fn set_queue(&mut self, queue: Vec<Queued>) -> Result<(), &'static str> {
        if queue.len() > MAX_ROUNDS {
            return Err("That is more questions than one room holds.");
        }
        self.queue = queue;
        Ok(())
    }

    /// Asks the first question lined up, as `ask` would, and takes it off
    /// the list.
    pub fn ask_next(&mut self, now: u64) -> Result<(), &'static str> {
        let next = self
            .queue
            .first()
            .cloned()
            .ok_or("No questions are lined up.")?;
        self.ask(&next.question, &next.unit, next.answer, now)?;
        self.queue.remove(0);
        Ok(())
    }

    /// Moves the question on: to the discussion, to the second ranges, or
    /// to the reveal. The host can also go back a stage for somebody who
    /// missed one. Nothing comes back out of the reveal, because by then
    /// everybody has seen the answer.
    pub fn move_to(&mut self, stage: Stage) -> Result<(), &'static str> {
        let round = self.round_mut()?;
        if round.stage == Stage::Revealed {
            return Err("This question is over.");
        }
        round.stage = stage;
        Ok(())
    }

    pub fn set_answer(&mut self, answer: Option<f64>) -> Result<(), &'static str> {
        if answer.is_some_and(|answer| !answer.is_finite()) {
            return Err("That answer is not a number.");
        }
        self.round_mut()?.answer = answer;
        Ok(())
    }

    /// Enters or corrects the answer of a question asked earlier, by its
    /// place in `rounds`, for a room that looked an answer up afterwards.
    pub fn set_answer_of(&mut self, index: usize, answer: Option<f64>) -> Result<(), &'static str> {
        if answer.is_some_and(|answer| !answer.is_finite()) {
            return Err("That answer is not a number.");
        }
        self.rounds
            .get_mut(index)
            .ok_or("That question is not here.")?
            .answer = answer;
        Ok(())
    }

    /// Shows everybody the answer and the room's numbers.
    pub fn reveal(&mut self) -> Result<(), &'static str> {
        self.round_mut()?.stage = Stage::Revealed;
        Ok(())
    }

    /// Takes one player's range, into whichever pass is being taken.
    /// Bounds given the wrong way round are swapped rather than refused,
    /// because the two fields get filled in either order.
    pub fn bound(
        &mut self,
        player: &str,
        low: f64,
        high: f64,
        now: u64,
    ) -> Result<(), &'static str> {
        if !low.is_finite() || !high.is_finite() {
            return Err("Both bounds need to be numbers.");
        }
        if self.player(player).is_none() {
            return Err("This link does not belong to anyone in the room.");
        }
        let round = self.round_mut()?;
        let Some(pass) = round.stage.taking() else {
            return Err(match round.stage {
                Stage::Discussion => "The room is talking this one over.",
                _ => "This question is closed.",
            });
        };
        let (low, high) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        let bound = Bound { low, high, at: now };
        match pass {
            Pass::First => round.before.insert(player.to_string(), bound),
            Pass::Second => round.after.insert(player.to_string(), bound),
        };
        Ok(())
    }
}

/// A question waiting its turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Queued {
    pub question: String,
    pub answer: Option<f64>,
    pub unit: String,
}

/// Reads a pasted list of questions, one per line, as
/// `question | answer | unit`. The answer and the unit may be left out
/// or left empty. Blank lines and lines starting with `#` are skipped.
/// The answer may have commas, spaces or underscores between thousands
/// and a typographic minus; its decimal separator is a point.
///
/// An error names the line, so the host can find it in what they pasted.
pub fn parse_queue(text: &str) -> Result<Vec<Queued>, String> {
    let mut queue = Vec::new();
    for (at, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = at + 1;
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() > 3 {
            return Err(format!(
                "Line {at} has more than three parts. Write it as: question | answer | unit."
            ));
        }
        let question = tidy(fields[0], MAX_QUESTION);
        if question.is_empty() {
            return Err(format!("Line {at} has no question before its first |."));
        }
        let answer = match fields.get(1).copied().unwrap_or("") {
            "" => None,
            given => {
                let plain: String = given
                    .chars()
                    .filter(|c| !matches!(c, ',' | '_' | ' ' | '\u{2009}' | '\u{202f}' | '\u{a0}'))
                    .map(|c| if c == '\u{2212}' { '-' } else { c })
                    .collect();
                match plain.parse::<f64>() {
                    Ok(answer) if answer.is_finite() => Some(answer),
                    _ => {
                        return Err(format!(
                            "Line {at}: “{given}” is not a number. Write the answer with a decimal point, as 1500000 or 1,500,000.5."
                        ));
                    }
                }
            }
        };
        queue.push(Queued {
            question,
            answer,
            unit: tidy(fields.get(2).copied().unwrap_or(""), MAX_UNIT),
        });
        if queue.len() > MAX_ROUNDS {
            return Err(format!("That is more than {MAX_ROUNDS} questions."));
        }
    }
    Ok(queue)
}

/// A number and the name of the player it came from.
#[derive(Serialize)]
pub struct Extreme {
    pub name: String,
    pub value: f64,
}

/// The room's numbers for one pass.
#[derive(Serialize)]
pub struct Stats {
    pub n: usize,
    /// The median lower bound and the median upper bound.
    pub median_low: f64,
    pub median_high: f64,
    pub mean_low: f64,
    pub mean_high: f64,
    /// The lowest lower bound and the highest upper bound.
    pub lowest: Extreme,
    pub highest: Extreme,
    /// The widest range and the narrowest.
    pub widest: Extreme,
    pub tightest: Extreme,
    pub median_width: f64,
    /// How many ranges contained the answer, once there is one.
    pub hits: Option<usize>,
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

fn sorted_by<T: Copy>(values: impl Iterator<Item = T>, of: impl Fn(T) -> f64) -> Vec<T> {
    let mut values: Vec<T> = values.collect();
    values.sort_by(|a, b| of(*a).total_cmp(&of(*b)));
    values
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

impl Round {
    fn ranges(&self, pass: Pass) -> &HashMap<String, Bound> {
        match pass {
            Pass::First => &self.before,
            Pass::Second => &self.after,
        }
    }

    /// Whether anybody answered this pass.
    pub fn taken(&self, pass: Pass) -> usize {
        self.ranges(pass).len()
    }

    /// The range that counts for this player: the one given after the
    /// discussion, or the first one if there was no second pass.
    pub fn standing(&self, key: &str) -> Option<Bound> {
        self.after
            .get(key)
            .or_else(|| self.before.get(key))
            .copied()
    }

    /// The bounds of one pass with names attached, in the order people
    /// joined.
    fn named<'a>(&'a self, players: &'a [Player], pass: Pass) -> Vec<(&'a str, Bound)> {
        let ranges = self.ranges(pass);
        players
            .iter()
            .filter_map(|p| ranges.get(&p.key).map(|b| (p.name.as_str(), *b)))
            .collect()
    }

    /// The same, over each person's final range.
    fn standings<'a>(&'a self, players: &'a [Player]) -> Vec<(&'a str, Bound)> {
        players
            .iter()
            .filter_map(|p| self.standing(&p.key).map(|b| (p.name.as_str(), b)))
            .collect()
    }

    fn stats_of(&self, named: &[(&str, Bound)]) -> Option<Stats> {
        let n = named.len();
        if n == 0 {
            return None;
        }
        let lows = sorted_by(named.iter().map(|(_, b)| b.low), |low| low);
        let highs = sorted_by(named.iter().map(|(_, b)| b.high), |high| high);
        let widths = sorted_by(named.iter().map(|(_, b)| b.width()), |width| width);
        let by_low = sorted_by(named.iter().copied(), |(_, b)| b.low);
        let by_high = sorted_by(named.iter().copied(), |(_, b)| b.high);
        let by_width = sorted_by(named.iter().copied(), |(_, b)| b.width());
        let at = |name: &str, value: f64| Extreme {
            name: name.to_string(),
            value,
        };
        Some(Stats {
            n,
            median_low: median(&lows),
            median_high: median(&highs),
            mean_low: mean(&lows),
            mean_high: mean(&highs),
            lowest: at(by_low[0].0, by_low[0].1.low),
            highest: at(by_high[n - 1].0, by_high[n - 1].1.high),
            widest: at(by_width[n - 1].0, by_width[n - 1].1.width()),
            tightest: at(by_width[0].0, by_width[0].1.width()),
            median_width: median(&widths),
            hits: self
                .answer
                .map(|answer| named.iter().filter(|(_, b)| b.contains(answer)).count()),
        })
    }

    /// The numbers for one pass.
    pub fn stats(&self, players: &[Player], pass: Pass) -> Option<Stats> {
        self.stats_of(&self.named(players, pass))
    }

    /// The numbers over everybody's final range.
    pub fn standing_stats(&self, players: &[Player]) -> Option<Stats> {
        self.stats_of(&self.standings(players))
    }
}

/// One person's two answers to the same question, and the difference.
#[derive(Serialize, Clone)]
pub struct Change {
    pub name: String,
    pub before: Bound,
    pub after: Bound,
    /// How far the midpoint of their range moved, which is the change in
    /// their estimate rather than in their confidence.
    pub moved: f64,
    /// Negative when the second range is narrower than the first.
    pub width_change: f64,
    pub hit_before: Option<bool>,
    pub hit_after: Option<bool>,
}

/// Whose first answer the rest of the room moved towards.
#[derive(Serialize, Clone)]
pub struct Pull {
    pub name: String,
    /// How much closer everybody else's midpoint got to this person's
    /// first midpoint, averaged over the people who answered twice.
    pub towards: f64,
}

/// What changed between the two passes.
#[derive(Serialize)]
pub struct Shift {
    /// People who answered both times.
    pub n: usize,
    /// Of those, how many changed anything at all.
    pub moved: usize,
    pub narrower: usize,
    pub wider: usize,
    /// The median midpoint before and after, which shows which way the
    /// room moved.
    pub median_middle_before: f64,
    pub median_middle_after: f64,
    pub median_width_before: f64,
    pub median_width_after: f64,
    /// Whose midpoint moved furthest.
    pub biggest: Option<Change>,
    /// Whose first answer the rest of the room moved towards.
    pub pulled: Option<Pull>,
    /// How many ranges contained the answer, in each pass.
    pub hits_before: Option<usize>,
    pub hits_after: Option<usize>,
    /// How many people ended up closer to the answer than they started.
    pub towards: Option<usize>,
}

impl Shift {
    #[cfg(test)]
    fn pull_name(&self) -> Option<&str> {
        self.pulled.as_ref().map(|p| p.name.as_str())
    }
}

impl Round {
    /// Everybody who answered both times, and what changed.
    pub fn changes(&self, players: &[Player]) -> Vec<Change> {
        players
            .iter()
            .filter_map(|player| {
                let before = *self.before.get(&player.key)?;
                let after = *self.after.get(&player.key)?;
                Some(Change {
                    name: player.name.clone(),
                    before,
                    after,
                    moved: (after.middle() - before.middle()).abs(),
                    width_change: after.width() - before.width(),
                    hit_before: self.answer.map(|a| before.contains(a)),
                    hit_after: self.answer.map(|a| after.contains(a)),
                })
            })
            .collect()
    }

    /// Whose first answer the rest of the room moved towards between the
    /// two passes.
    ///
    /// For each person, take their first midpoint as a fixed point and
    /// measure how much closer everybody else's midpoint got to it,
    /// averaged over the people who answered twice. Their own movement is
    /// left out of their own score, so somebody cannot score by moving.
    /// `None` when fewer than two people answered twice, or when nobody's
    /// first answer drew the room in.
    fn pull(&self, players: &[Player], changes: &[Change]) -> Option<Pull> {
        if changes.len() < 2 {
            return None;
        }
        let mut best: Option<Pull> = None;
        for player in players {
            let Some(anchor) = self.before.get(&player.key).map(|b| b.middle()) else {
                continue;
            };
            let others: Vec<&Change> = changes
                .iter()
                .filter(|change| change.name != player.name)
                .collect();
            if others.is_empty() {
                continue;
            }
            let closer: f64 = others
                .iter()
                .map(|c| (c.before.middle() - anchor).abs() - (c.after.middle() - anchor).abs())
                .sum::<f64>()
                / others.len() as f64;
            if closer > 0.0 && best.as_ref().is_none_or(|b| closer > b.towards) {
                best = Some(Pull {
                    name: player.name.clone(),
                    towards: closer,
                });
            }
        }
        best
    }

    /// The comparison, or `None` if there was no second pass.
    pub fn shift(&self, players: &[Player]) -> Option<Shift> {
        let changes = self.changes(players);
        if changes.is_empty() {
            return None;
        }
        let middles = |pick: fn(&Change) -> Bound| {
            let mut values: Vec<f64> = changes.iter().map(|c| pick(c).middle()).collect();
            values.sort_by(f64::total_cmp);
            median(&values)
        };
        let widths = |pick: fn(&Change) -> Bound| {
            let mut values: Vec<f64> = changes.iter().map(|c| pick(c).width()).collect();
            values.sort_by(f64::total_cmp);
            median(&values)
        };
        let biggest = changes
            .iter()
            .max_by(|a, b| a.moved.total_cmp(&b.moved))
            .filter(|change| change.moved > 0.0)
            .cloned();
        Some(Shift {
            n: changes.len(),
            moved: changes
                .iter()
                .filter(|c| c.moved > 0.0 || c.width_change != 0.0)
                .count(),
            narrower: changes.iter().filter(|c| c.width_change < 0.0).count(),
            wider: changes.iter().filter(|c| c.width_change > 0.0).count(),
            median_middle_before: middles(|c| c.before),
            median_middle_after: middles(|c| c.after),
            median_width_before: widths(|c| c.before),
            median_width_after: widths(|c| c.after),
            biggest,
            pulled: self.pull(players, &changes),
            hits_before: self
                .answer
                .map(|a| changes.iter().filter(|c| c.before.contains(a)).count()),
            hits_after: self
                .answer
                .map(|a| changes.iter().filter(|c| c.after.contains(a)).count()),
            towards: self.answer.map(|answer| {
                changes
                    .iter()
                    .filter(|c| {
                        (c.after.middle() - answer).abs() < (c.before.middle() - answer).abs()
                    })
                    .count()
            }),
        })
    }
}

/// Which way somebody's hit count is off from what their level says it
/// should be.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Calibration {
    /// Fewer ranges contained the answer than the level says should:
    /// overconfident.
    Narrow,
    /// More did: underconfident.
    Wide,
    /// About as many as should, over enough questions to say so.
    Right,
}

/// Whether `hits` is about what `levels` (one per scored question, as a
/// fraction) say to expect. A count may be off by one question, or by
/// one standard deviation of the count if that is more, and still be
/// about right: with ten questions at 90%, 8 to 10 is. Fewer than three
/// questions are never called right, only clearly off.
fn calibration(hits: usize, levels: &[f64]) -> Option<Calibration> {
    let expected: f64 = levels.iter().sum();
    let spread = levels.iter().map(|p| p * (1.0 - p)).sum::<f64>().sqrt();
    let off = hits as f64 - expected;
    if off.abs() <= spread.max(1.0) {
        (levels.len() >= 3).then_some(Calibration::Right)
    } else if off < 0.0 {
        Some(Calibration::Narrow)
    } else {
        Some(Calibration::Wide)
    }
}

/// One range's interval score (Gneiting and Raftery, 2007), in its two
/// parts: the width of the range, and a penalty for missing of `2 / α`
/// times the distance it missed by, where `α = 1 − level`. The sum is
/// lowest, on average, for somebody whose ranges hold the answer as often
/// as the level says and are no wider than that needs, so neither a very
/// wide range nor a very narrow one pays.
///
/// `log` measures in powers of ten; otherwise both parts are divided by
/// `scale`. See `Round::cost_log` and `Round::cost_scale`.
pub fn interval_cost(bound: Bound, answer: f64, level: u8, log: bool, scale: f64) -> (f64, f64) {
    // 2 / α, as 200 / (100 − level) so that 90% is exactly 20.
    let penalty = 200.0 / f64::from(100 - level.min(99));
    let (low, high, x) = if log {
        (bound.low.log10(), bound.high.log10(), answer.log10())
    } else {
        (bound.low / scale, bound.high / scale, answer / scale)
    };
    let missed = if x < low {
        low - x
    } else if x > high {
        x - high
    } else {
        0.0
    };
    (high - low, penalty * missed)
}

/// One player's results over the evening.
#[derive(Serialize, Clone)]
pub struct Score {
    pub name: String,
    /// Questions answered.
    pub answered: usize,
    /// Of those, the ones with a known answer. A hit rate can only be
    /// counted over these.
    pub scored: usize,
    pub hits: usize,
    /// How many of the scored ranges should have contained the answer, at
    /// the level each question was asked at: 9 for ten questions at 90%.
    pub expected: f64,
    /// The level, when every scored question was asked at the same one.
    pub level: Option<u8>,
    pub calibration: Option<Calibration>,
    /// The interval score, averaged over the scored questions, in its two
    /// parts: what the width of the ranges cost, and what the misses
    /// cost. Lower is better. `None` until a question has an answer.
    pub width_cost: Option<f64>,
    pub miss_cost: Option<f64>,
    /// Questions where their range was narrower than the room's median
    /// range. Widths cannot be compared between questions; this count
    /// can.
    pub tighter: usize,
    /// Questions where they changed their range after the discussion.
    pub changed_mind: usize,
}

impl Score {
    /// The interval score: width and misses together.
    pub fn cost(&self) -> Option<f64> {
        Some(self.width_cost? + self.miss_cost?)
    }
}

impl Round {
    /// Whether this question is scored in powers of ten: when the answer
    /// and every bound given are above zero. A zero or a negative number
    /// anywhere in it puts the whole question on `cost_scale` instead, so
    /// that everybody on one question is scored the same way.
    fn cost_log(&self, players: &[Player]) -> bool {
        self.answer.is_some_and(|a| a > 0.0)
            && players
                .iter()
                .filter_map(|p| self.standing(&p.key))
                .all(|b| b.low > 0.0)
    }

    /// What a question not scored in powers of ten is measured in: the
    /// room's median first range, so that a question about millions and
    /// one about single digits count alike. For a room that gave only
    /// points, the size of the answer, and then one.
    fn cost_scale(&self, players: &[Player]) -> f64 {
        let width = self
            .stats(players, Pass::First)
            .or_else(|| self.standing_stats(players))
            .map_or(0.0, |stats| stats.median_width);
        let answer = self.answer.map_or(0.0, f64::abs);
        if width > 0.0 {
            width
        } else if answer > 0.0 {
            answer
        } else {
            1.0
        }
    }
}

impl Game {
    /// Everyone's results, lowest interval score first: see
    /// `interval_cost`. Answering "between nought and a billion" every
    /// time holds every answer and still loses on width; ranges too
    /// narrow to hold it lose on misses. Ties go to more hits. People
    /// with nothing scored yet come after, by name.
    pub fn scores(&self) -> Vec<Score> {
        self.scores_over(|_| true)
    }

    /// The same, over the revealed questions only. This is the version a
    /// player is sent: a tally that included the open question would tell
    /// them whether their own range had hit.
    pub fn scores_revealed(&self) -> Vec<Score> {
        self.scores_over(|round| round.stage == Stage::Revealed)
    }

    fn scores_over(&self, counted: impl Fn(&Round) -> bool) -> Vec<Score> {
        let players = &self.lobby.players;
        // Each question's numbers once, rather than once per player.
        let rounds: Vec<(&Round, Option<Stats>, bool, f64)> = self
            .rounds
            .iter()
            .filter(|round| counted(round))
            .map(|round| {
                (
                    round,
                    round.standing_stats(players),
                    round.cost_log(players),
                    round.cost_scale(players),
                )
            })
            .collect();
        let mut scores: Vec<Score> = players
            .iter()
            .map(|player| {
                let mut score = Score {
                    name: player.name.clone(),
                    answered: 0,
                    scored: 0,
                    hits: 0,
                    expected: 0.0,
                    level: None,
                    calibration: None,
                    width_cost: None,
                    miss_cost: None,
                    tighter: 0,
                    changed_mind: 0,
                };
                let mut levels: Vec<u8> = Vec::new();
                let (mut width, mut miss) = (0.0, 0.0);
                for (round, stats, log, scale) in &rounds {
                    let Some(bound) = round.standing(&player.key) else {
                        continue;
                    };
                    score.answered += 1;
                    if let Some(answer) = round.answer {
                        score.scored += 1;
                        if bound.contains(answer) {
                            score.hits += 1;
                        }
                        levels.push(round.confidence);
                        let (w, m) = interval_cost(bound, answer, round.confidence, *log, *scale);
                        width += w;
                        miss += m;
                    }
                    if let Some(stats) = stats
                        && bound.width() < stats.median_width
                    {
                        score.tighter += 1;
                    }
                    if let (Some(before), Some(after)) =
                        (round.before.get(&player.key), round.after.get(&player.key))
                        && (before.low != after.low || before.high != after.high)
                    {
                        score.changed_mind += 1;
                    }
                }
                if score.scored > 0 {
                    let n = score.scored as f64;
                    let fractions: Vec<f64> =
                        levels.iter().map(|&l| f64::from(l) / 100.0).collect();
                    score.width_cost = Some(width / n);
                    score.miss_cost = Some(miss / n);
                    score.expected = levels.iter().map(|&l| f64::from(l)).sum::<f64>() / 100.0;
                    score.level = levels.iter().all(|&l| l == levels[0]).then_some(levels[0]);
                    score.calibration = calibration(score.hits, &fractions);
                }
                score
            })
            .filter(|score| score.answered > 0)
            .collect();
        scores.sort_by(|a, b| match (a.cost(), b.cost()) {
            (Some(x), Some(y)) => x
                .total_cmp(&y)
                .then(b.hits.cmp(&a.hits))
                .then(a.name.cmp(&b.name)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        });
        scores
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rooms::lobby::{MAX_NAME, PRESENT_MS, admit};

    fn room() -> (Game, Vec<String>) {
        let mut game = Game::new(1000);
        let keys = ["Ada", "Bo", "Cy", "Di"]
            .iter()
            .map(|name| admit(&mut game, name, None, 1000).expect("the room has space"))
            .collect();
        (game, keys)
    }

    #[test]
    fn the_room_gets_its_middle_its_edges_and_its_extremes() {
        let (mut game, keys) = room();
        game.ask("How tall is the Frauenkirche?", "m", Some(99.0), 1000)
            .unwrap();
        let ranges = [(10.0, 40.0), (20.0, 60.0), (30.0, 90.0), (100.0, 120.0)];
        for (key, (low, high)) in keys.iter().zip(ranges) {
            game.bound(key, low, high, 1001).unwrap();
        }
        let round = game.round().unwrap();
        let stats = round.stats(&game.lobby.players, Pass::First).unwrap();
        assert_eq!(stats.n, 4);
        assert_eq!(stats.median_low, 25.0, "lows 10 20 30 100");
        assert_eq!(stats.median_high, 75.0, "highs 40 60 90 120");
        assert_eq!(stats.mean_low, 40.0);
        assert_eq!(stats.mean_high, 77.5);
        assert_eq!(
            (stats.lowest.name.as_str(), stats.lowest.value),
            ("Ada", 10.0)
        );
        assert_eq!(
            (stats.highest.name.as_str(), stats.highest.value),
            ("Di", 120.0)
        );
        assert_eq!(
            (stats.widest.name.as_str(), stats.widest.value),
            ("Cy", 60.0),
            "widths 30 40 60 20"
        );
        assert_eq!(
            (stats.tightest.name.as_str(), stats.tightest.value),
            ("Di", 20.0)
        );
        assert_eq!(stats.median_width, 35.0);
        assert_eq!(stats.hits, Some(0), "99 is above every range but Di's");
    }

    #[test]
    fn a_range_that_ends_on_the_answer_counts_as_containing_it() {
        let (mut game, keys) = room();
        game.ask("When?", "", Some(50.0), 1000).unwrap();
        game.bound(&keys[0], 50.0, 80.0, 1001).unwrap(); // starts on it
        game.bound(&keys[1], 10.0, 50.0, 1001).unwrap(); // ends on it
        game.bound(&keys[2], 50.0, 50.0, 1001).unwrap(); // is it
        game.bound(&keys[3], 51.0, 80.0, 1001).unwrap(); // misses by one
        let stats = game
            .round()
            .unwrap()
            .stats(&game.lobby.players, Pass::First)
            .unwrap();
        assert_eq!(stats.hits, Some(3));
    }

    #[test]
    fn bounds_the_wrong_way_round_are_swapped_rather_than_refused() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        game.bound(&keys[0], 90.0, 10.0, 1001).unwrap();
        let bound = game.round().unwrap().before[&keys[0]];
        assert_eq!((bound.low, bound.high), (10.0, 90.0));
    }

    #[test]
    fn nothing_is_taken_while_the_room_is_talking() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 1001).unwrap();
        game.move_to(Stage::Discussion).unwrap();
        assert_eq!(
            game.bound(&keys[0], 5.0, 6.0, 1002),
            Err("The room is talking this one over.")
        );
        assert_eq!(game.round().unwrap().before[&keys[0]].high, 2.0);
    }

    #[test]
    fn the_second_pass_is_kept_apart_from_the_first() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        game.bound(&keys[0], 0.0, 100.0, 1001).unwrap();
        game.move_to(Stage::Discussion).unwrap();
        game.move_to(Stage::Second).unwrap();
        game.bound(&keys[0], 40.0, 60.0, 1002).unwrap();

        let round = game.round().unwrap();
        assert_eq!(round.before[&keys[0]].high, 100.0, "the first still stands");
        assert_eq!(round.after[&keys[0]].high, 60.0);
        assert_eq!(round.standing(&keys[0]).unwrap().high, 60.0);
        assert!(!round.after.is_empty(), "the room went round twice");
    }

    #[test]
    fn a_question_with_no_second_pass_stands_on_its_first() {
        let (mut game, keys) = room();
        game.ask("How many?", "", Some(50.0), 1000).unwrap();
        game.bound(&keys[0], 40.0, 60.0, 1001).unwrap();
        game.reveal().unwrap();
        let round = game.round().unwrap();
        assert!(round.after.is_empty(), "the room only went round once");
        assert!(round.shift(&game.lobby.players).is_none());
        assert_eq!(
            round.standing_stats(&game.lobby.players).unwrap().hits,
            Some(1)
        );
    }

    /// The host may know the answer when they ask, look it up while the
    /// room is arguing, or correct it after the reveal, and enter one for
    /// an earlier question once somebody has looked it up.
    #[test]
    fn the_answer_can_be_set_at_any_point() {
        let (mut game, keys) = room();
        game.ask("How many?", "", Some(10.0), 1000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 1001).unwrap();
        game.set_answer(Some(20.0)).unwrap();
        assert_eq!(game.round().unwrap().answer, Some(20.0));

        game.reveal().unwrap();
        game.set_answer(Some(30.0)).unwrap();
        assert_eq!(
            game.round().unwrap().answer,
            Some(30.0),
            "a wrong answer can be corrected after the reveal"
        );

        game.ask("Next", "", None, 2000).unwrap();
        game.set_answer(Some(40.0)).unwrap();
        assert_eq!(
            game.rounds[0].answer,
            Some(30.0),
            "setting the current one leaves it"
        );
        assert_eq!(game.rounds[1].answer, Some(40.0));

        game.set_answer_of(0, Some(35.0)).unwrap();
        assert_eq!(
            game.rounds[0].answer,
            Some(35.0),
            "an earlier answer looked up later can still be entered"
        );
        assert_eq!(game.rounds[1].answer, Some(40.0));
        assert!(game.set_answer_of(2, Some(1.0)).is_err());
        assert!(game.set_answer_of(0, Some(f64::NAN)).is_err());
    }

    #[test]
    fn a_question_takes_the_wall_once_one_is_asked() {
        let mut game = Game::new(1000);
        assert!(
            !game.on_wall(1000).busy,
            "the QR code has the wall at first"
        );
        game.ask("How many?", "", None, 1000).unwrap();
        assert!(game.on_wall(1000).busy);
        assert!(game.on_wall(1000).join, "latecomers can still scan it");
    }

    #[test]
    fn a_revealed_question_takes_nothing_more_and_cannot_be_reopened() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 1001).unwrap();
        game.reveal().unwrap();
        assert!(game.bound(&keys[0], 1.0, 1000.0, 1002).is_err());
        assert!(game.move_to(Stage::First).is_err());
    }

    #[test]
    fn asking_again_closes_the_last_question_and_keeps_its_answers() {
        let (mut game, keys) = room();
        game.ask("First", "", None, 1000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 1001).unwrap();
        game.ask("Second", "", None, 2000).unwrap();
        assert_eq!(game.rounds.len(), 2);
        assert_eq!(game.rounds[0].stage, Stage::Revealed);
        assert_eq!(game.rounds[0].before.len(), 1);
        assert_eq!(game.rounds[1].stage, Stage::First);
        assert!(game.rounds[1].before.is_empty());
    }

    #[test]
    fn the_shift_says_who_moved_and_which_way_the_room_went() {
        let (mut game, keys) = room();
        game.ask("How many?", "", Some(50.0), 1000).unwrap();
        // Ada narrows and stays put, Bo moves a long way towards 50,
        // Cy widens and away, Di does not change at all.
        let first = [(0.0, 100.0), (0.0, 10.0), (40.0, 60.0), (30.0, 70.0)];
        for (key, (low, high)) in keys.iter().zip(first) {
            game.bound(key, low, high, 1001).unwrap();
        }
        game.move_to(Stage::Second).unwrap();
        let second = [(25.0, 75.0), (40.0, 60.0), (0.0, 30.0), (30.0, 70.0)];
        for (key, (low, high)) in keys.iter().zip(second) {
            game.bound(key, low, high, 1002).unwrap();
        }
        game.reveal().unwrap();

        let round = game.round().unwrap();
        let shift = round.shift(&game.lobby.players).expect("two passes");
        assert_eq!(shift.n, 4);
        assert_eq!(shift.moved, 3, "Di did not touch theirs");
        assert_eq!(shift.narrower, 1, "only Ada's range got smaller");
        assert_eq!(shift.wider, 2, "Bo's and Cy's got bigger");
        assert_eq!(
            shift.hits_before,
            Some(3),
            "0-100, 40-60 and 30-70 all hold 50"
        );
        assert_eq!(shift.hits_after, Some(3), "Cy let go of it, Bo caught it");
        assert_eq!(shift.towards, Some(1), "only Bo ended up nearer the answer");
        assert_eq!(shift.biggest.as_ref().map(|c| c.name.as_str()), Some("Bo"));
        assert_eq!(shift.biggest.as_ref().map(|c| c.moved), Some(45.0));
        assert_eq!(shift.median_middle_before, 50.0);
        assert_eq!(shift.median_width_before, 30.0, "widths 100 10 20 40");
        assert_eq!(shift.median_width_after, 35.0, "widths 50 20 30 40");
    }

    #[test]
    fn the_room_moving_towards_one_persons_first_answer_is_counted() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        // Ada says 10, and the other three say 90, 92 and 94.
        for (key, (low, high)) in
            keys.iter()
                .zip([(5.0, 15.0), (85.0, 95.0), (87.0, 97.0), (89.0, 99.0)])
        {
            game.bound(key, low, high, 1001).unwrap();
        }
        game.move_to(Stage::Second).unwrap();
        // Ada holds; the other three come most of the way to her.
        for (key, (low, high)) in
            keys.iter()
                .zip([(5.0, 15.0), (15.0, 25.0), (17.0, 27.0), (19.0, 29.0)])
        {
            game.bound(key, low, high, 1002).unwrap();
        }
        game.reveal().unwrap();

        let shift = game.round().unwrap().shift(&game.lobby.players).unwrap();
        let pulled = shift.pull_name();
        assert_eq!(
            pulled,
            Some("Ada"),
            "the room came to Ada, not the other way"
        );
        assert_eq!(shift.biggest.as_ref().map(|c| c.name.as_str()), Some("Di"));
    }

    #[test]
    fn nobody_pulls_a_room_that_did_not_move_towards_them() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        game.bound(&keys[0], 0.0, 10.0, 1001).unwrap();
        game.bound(&keys[1], 20.0, 30.0, 1001).unwrap();
        game.move_to(Stage::Second).unwrap();
        // Both move further apart, so neither drew the other in.
        game.bound(&keys[0], -20.0, -10.0, 1002).unwrap();
        game.bound(&keys[1], 40.0, 50.0, 1002).unwrap();
        game.reveal().unwrap();
        assert!(
            game.round()
                .unwrap()
                .shift(&game.lobby.players)
                .unwrap()
                .pulled
                .is_none()
        );
    }

    #[test]
    fn moving_towards_the_answer_is_counted_only_when_there_is_one() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        game.bound(&keys[0], 0.0, 10.0, 1001).unwrap();
        game.move_to(Stage::Second).unwrap();
        game.bound(&keys[0], 40.0, 60.0, 1002).unwrap();
        let round = game.round().unwrap();
        let shift = round.shift(&game.lobby.players).unwrap();
        assert_eq!(shift.towards, None);
        assert_eq!(shift.hits_before, None);

        game.set_answer(Some(50.0)).unwrap();
        let shift = game.round().unwrap().shift(&game.lobby.players).unwrap();
        assert_eq!(shift.towards, Some(1));
        assert_eq!((shift.hits_before, shift.hits_after), (Some(0), Some(1)));
    }

    #[test]
    fn somebody_who_only_answered_the_second_time_is_not_a_change() {
        let (mut game, keys) = room();
        game.ask("How many?", "", None, 1000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 1001).unwrap();
        game.move_to(Stage::Second).unwrap();
        game.bound(&keys[1], 5.0, 6.0, 1002).unwrap();
        let round = game.round().unwrap();
        assert_eq!(
            round.changes(&game.lobby.players).len(),
            0,
            "nobody did both"
        );
        assert!(round.shift(&game.lobby.players).is_none());
        assert_eq!(
            round.standing_stats(&game.lobby.players).unwrap().n,
            2,
            "but both of them still count towards where the room stands"
        );
    }

    #[test]
    fn a_name_is_refused_while_its_phone_is_here_and_given_back_after() {
        let (mut game, keys) = room();
        assert!(
            admit(&mut game, "  ada  ", None, 2000).is_err(),
            "Ada joined a second ago, so this is somebody else"
        );
        assert_eq!(game.lobby.len(), 4);
        let gone = 1000 + PRESENT_MS;
        assert_eq!(admit(&mut game, "Ada", None, gone).unwrap(), keys[0]);
        assert_eq!(
            admit(&mut game, "Whoever", Some(&keys[1]), gone).unwrap(),
            keys[1],
            "a kept key gets back in"
        );
        assert_eq!(game.lobby.len(), 4);
    }

    #[test]
    fn somebody_taken_out_takes_their_ranges_with_them() {
        let (mut game, keys) = room();
        game.ask("One", "", Some(5.0), 1000).unwrap();
        game.bound(&keys[0], 1.0, 9.0, 1001).unwrap();
        game.bound(&keys[1], 1.0, 2.0, 1001).unwrap();
        let ada = game.player(&keys[0]).unwrap().id;
        assert_eq!(
            rooms::lobby::remove(&mut game, ada).map(|p| p.key),
            Ok(keys[0].clone())
        );
        let round = game.round().unwrap();
        assert!(!round.before.contains_key(&keys[0]));
        assert_eq!(round.standing_stats(&game.lobby.players).unwrap().n, 1);
        assert!(game.bound(&keys[0], 1.0, 2.0, 1002).is_err());
    }

    #[test]
    fn a_pasted_list_becomes_questions_with_answers_and_units() {
        let queue = parse_queue(
            "How tall is the Olympiaturm? | 291 | m\n\
             \n\
             # a comment\n\
             How many people live in Munich? | 1,510,000\n\
             Coldest day on record? | −31.6 | °C\n\
             Nobody knows this one\n\
             No answer yet |  | km\n",
        )
        .unwrap();
        let got: Vec<(&str, Option<f64>, &str)> = queue
            .iter()
            .map(|q| (q.question.as_str(), q.answer, q.unit.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("How tall is the Olympiaturm?", Some(291.0), "m"),
                ("How many people live in Munich?", Some(1_510_000.0), ""),
                ("Coldest day on record?", Some(-31.6), "°C"),
                ("Nobody knows this one", None, ""),
                ("No answer yet", None, "km"),
            ]
        );
    }

    #[test]
    fn a_line_that_cannot_be_read_is_named() {
        let error = parse_queue("Fine | 1\nHow far? | about ten | km").unwrap_err();
        assert!(error.starts_with("Line 2:"), "{error}");
        assert!(parse_queue(" | 5").unwrap_err().starts_with("Line 1"));
        assert!(parse_queue("a | 1 | m | extra").is_err());
        assert_eq!(parse_queue("\n\n").unwrap(), []);
    }

    #[test]
    fn asking_next_takes_the_first_question_off_the_list() {
        let (mut game, keys) = room();
        assert!(game.ask_next(1000).is_err(), "nothing is lined up");
        game.set_queue(parse_queue("One | 1 | m\nTwo | 2").unwrap())
            .unwrap();
        game.ask_next(1000).unwrap();
        game.bound(&keys[0], 0.0, 5.0, 1001).unwrap();
        let round = game.round().unwrap();
        assert_eq!(
            (round.question.as_str(), round.answer, round.unit.as_str()),
            ("One", Some(1.0), "m")
        );
        assert_eq!(game.queue.len(), 1);
        game.ask_next(2000).unwrap();
        assert_eq!(
            game.rounds[0].stage,
            Stage::Revealed,
            "like asking, it reveals the open one"
        );
        assert_eq!(game.round().unwrap().question, "Two");
        assert!(game.queue.is_empty());
    }

    #[test]
    fn a_nameless_player_is_turned_away() {
        let mut game = Game::new(1000);
        assert!(admit(&mut game, "   ", None, 1000).is_err());
    }

    #[test]
    fn nothing_but_a_question_is_asked() {
        let mut game = Game::new(1000);
        assert!(game.ask("  ", "", None, 1000).is_err());
        assert!(game.move_to(Stage::Second).is_err(), "there is no question");
        assert!(game.round().is_none());
    }

    #[test]
    fn the_scoreboard_counts_hits_and_who_went_narrow() {
        let (mut game, keys) = room();
        game.ask("One", "", Some(50.0), 1000).unwrap();
        game.bound(&keys[0], 40.0, 60.0, 1001).unwrap(); // hit, width 20
        game.bound(&keys[1], 0.0, 1000.0, 1001).unwrap(); // hit, width 1000
        game.bound(&keys[2], 0.0, 10.0, 1001).unwrap(); // miss, width 10
        game.ask("Two", "", None, 2000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 2001).unwrap();

        let scores = game.scores();
        assert_eq!(
            scores.len(),
            3,
            "Di answered nothing and is not on the board"
        );
        assert_eq!(scores[0].name, "Ada");
        assert_eq!(
            (scores[0].answered, scores[0].scored, scores[0].hits),
            (2, 1, 1)
        );
        assert_eq!(scores[0].tighter, 0, "20 is the median width, not under it");
        // A zero in the room puts the question on a straight scale, in
        // units of the median first width, 20. At 90% a miss costs 20
        // times the distance.
        assert_eq!(scores[0].cost(), Some(1.0), "20 wide, held it");
        assert_eq!(scores[1].name, "Cy");
        assert_eq!(
            (scores[1].width_cost, scores[1].miss_cost),
            (Some(0.5), Some(40.0)),
            "10 wide, missed by 40"
        );
        assert_eq!(scores[1].tighter, 1);
        assert_eq!(
            scores[2].name, "Bo",
            "a range of 0 to 1000 holds it and still loses"
        );
        assert_eq!(scores[2].cost(), Some(50.0));
    }

    /// Ten questions at 90%. One player answers a thousand times either
    /// side of the answer and holds all ten. The other answers a factor
    /// of two either side, holds nine, and misses one by a factor of two.
    #[test]
    fn calibrated_ranges_beat_ranges_wide_enough_to_hold_everything() {
        let (mut game, keys) = room();
        for i in 0..10 {
            let answer = 10f64.powi(i % 5 + 1);
            game.ask("How many?", "", Some(answer), 1000).unwrap();
            game.bound(&keys[0], answer / 1000.0, answer * 1000.0, 1001)
                .unwrap();
            let (low, high) = if i == 0 {
                (answer * 2.0, answer * 8.0)
            } else {
                (answer / 2.0, answer * 2.0)
            };
            game.bound(&keys[1], low, high, 1001).unwrap();
            game.reveal().unwrap();
        }
        let scores = game.scores();
        let (wide, tight) = (&scores[1], &scores[0]);
        assert_eq!((tight.name.as_str(), tight.hits), ("Bo", 9));
        assert_eq!((wide.name.as_str(), wide.hits), ("Ada", 10));
        let near = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
        assert!(near(wide.width_cost, 6.0), "six powers of ten wide");
        assert!(near(wide.miss_cost, 0.0));
        let two = 2f64.log10();
        assert!(near(
            tight.width_cost,
            (9.0 * 2.0 * two + 4f64.log10()) / 10.0
        ));
        assert!(
            near(tight.miss_cost, 20.0 * two / 10.0),
            "one miss by a factor of two"
        );
        assert_eq!(
            tight.calibration,
            Some(Calibration::Right),
            "9 of 10 at 90%"
        );
        assert_eq!(
            wide.calibration,
            Some(Calibration::Right),
            "10 of 10 is within one"
        );
        assert_eq!((tight.expected, tight.level), (9.0, Some(90)));
    }

    #[test]
    fn a_hit_count_well_short_of_the_level_is_too_narrow() {
        let (mut game, keys) = room();
        for i in 0..10 {
            game.ask("How many?", "", Some(100.0), 1000).unwrap();
            let (low, high) = if i < 7 { (90.0, 110.0) } else { (1.0, 2.0) };
            game.bound(&keys[0], low, high, 1001).unwrap();
            let (low, high) = if i < 3 { (90.0, 110.0) } else { (1.0, 2.0) };
            game.bound(&keys[1], low, high, 1001).unwrap();
            game.bound(&keys[2], 50.0, 200.0, 1001).unwrap();
            game.reveal().unwrap();
        }
        let scores = game.scores();
        let of = |name: &str| scores.iter().find(|s| s.name == name).unwrap();
        assert_eq!(of("Ada").hits, 7);
        assert_eq!(
            of("Ada").calibration,
            Some(Calibration::Narrow),
            "7 of 10 at 90%"
        );
        assert_eq!(of("Bo").calibration, Some(Calibration::Narrow));
        assert_eq!(of("Cy").calibration, Some(Calibration::Right));

        game.set_confidence(50).unwrap();
        for _ in 0..4 {
            game.ask("How many?", "", Some(100.0), 1000).unwrap();
            game.bound(&keys[3], 50.0, 200.0, 1001).unwrap();
            game.reveal().unwrap();
        }
        let di = game.scores().into_iter().find(|s| s.name == "Di").unwrap();
        assert_eq!((di.hits, di.expected), (4, 2.0));
        assert_eq!(di.calibration, Some(Calibration::Wide), "4 of 4 at 50%");
    }

    #[test]
    fn too_few_questions_are_not_called_about_right() {
        let (mut game, keys) = room();
        game.ask("How many?", "", Some(100.0), 1000).unwrap();
        game.bound(&keys[0], 90.0, 110.0, 1001).unwrap();
        game.reveal().unwrap();
        assert_eq!(game.scores()[0].calibration, None);
    }

    #[test]
    fn a_question_with_a_negative_answer_is_scored_on_the_rooms_scale() {
        let (mut game, keys) = room();
        game.ask("Coldest day?", "°C", Some(-30.0), 1000).unwrap();
        game.bound(&keys[0], -40.0, -20.0, 1001).unwrap(); // width 20, holds it
        game.bound(&keys[1], -10.0, 0.0, 1001).unwrap(); // width 10, misses by 20
        game.reveal().unwrap();
        let scores = game.scores();
        // The median first width is 15.
        assert_eq!(scores[0].name, "Ada");
        let near = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
        assert!(near(scores[0].cost(), 20.0 / 15.0));
        assert!(near(scores[1].width_cost, 10.0 / 15.0));
        assert!(near(scores[1].miss_cost, 20.0 * 20.0 / 15.0));
    }

    #[test]
    fn the_board_puts_people_with_nothing_scored_last() {
        let (mut game, keys) = room();
        game.ask("One", "", None, 1000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 1001).unwrap();
        game.ask("Two", "", Some(5.0), 2000).unwrap();
        game.bound(&keys[1], 1.0, 1000.0, 2001).unwrap();
        let scores = game.scores();
        assert_eq!(scores[0].name, "Bo");
        assert_eq!(scores[1].name, "Ada");
        assert_eq!(scores[1].cost(), None);
    }

    #[test]
    fn the_scoreboard_counts_who_changed_their_mind() {
        let (mut game, keys) = room();
        game.ask("One", "", None, 1000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 1001).unwrap();
        game.bound(&keys[1], 1.0, 2.0, 1001).unwrap();
        game.move_to(Stage::Second).unwrap();
        game.bound(&keys[0], 3.0, 4.0, 1002).unwrap();
        game.bound(&keys[1], 1.0, 2.0, 1002).unwrap();
        game.reveal().unwrap();

        let scores = game.scores();
        let ada = scores.iter().find(|s| s.name == "Ada").unwrap();
        let bo = scores.iter().find(|s| s.name == "Bo").unwrap();
        assert_eq!(ada.changed_mind, 1);
        assert_eq!(bo.changed_mind, 0, "the same range twice is not a change");
    }

    #[test]
    fn a_tally_of_the_revealed_questions_only_counts_those() {
        let (mut game, keys) = room();
        game.ask("One", "", Some(50.0), 1000).unwrap();
        game.bound(&keys[0], 40.0, 60.0, 1001).unwrap();
        assert!(
            game.scores_revealed().is_empty(),
            "nothing is revealed yet, so there is nothing to tally"
        );
        game.reveal().unwrap();
        assert_eq!(game.scores_revealed()[0].hits, 1);

        game.ask("Two", "", Some(10.0), 2000).unwrap();
        game.bound(&keys[0], 1.0, 2.0, 2001).unwrap();
        assert_eq!(game.scores_revealed()[0].scored, 1, "the open one is out");
        assert_eq!(game.scores()[0].scored, 2, "the host sees both");
    }

    #[test]
    fn a_question_nobody_has_answered_has_no_numbers() {
        let (mut game, _) = room();
        game.ask("One", "", None, 1000).unwrap();
        let round = game.round().unwrap();
        assert!(round.stats(&game.lobby.players, Pass::First).is_none());
        assert!(round.standing_stats(&game.lobby.players).is_none());
    }

    #[test]
    fn the_confidence_level_is_one_of_the_offered_ones() {
        let mut game = Game::new(1000);
        assert!(game.set_confidence(90).is_ok());
        assert!(game.set_confidence(73).is_err());
        assert_eq!(game.confidence, 90);
    }

    #[test]
    fn each_question_keeps_the_level_it_was_asked_at() {
        let mut game = Game::new(1000);
        game.set_confidence(50).unwrap();
        game.ask("One", "", None, 1000).unwrap();
        game.reveal().unwrap();
        game.set_confidence(98).unwrap();
        game.ask("Two", "", None, 2000).unwrap();
        assert_eq!(game.rounds[0].confidence, 50, "changing it later left it");
        assert_eq!(game.rounds[1].confidence, 98);
    }

    #[test]
    fn the_level_cannot_change_while_a_question_is_open() {
        let mut game = Game::new(1000);
        game.ask("One", "", None, 1000).unwrap();
        for stage in [Stage::First, Stage::Discussion, Stage::Second] {
            game.move_to(stage).unwrap();
            assert!(game.set_confidence(50).is_err(), "{stage:?}");
            assert!(
                game.set_confidence(90).is_ok(),
                "the same level is no change"
            );
        }
        assert_eq!(game.round().unwrap().confidence, 90);
        game.reveal().unwrap();
        game.set_confidence(50).unwrap();
        assert_eq!(
            game.round().unwrap().confidence,
            90,
            "the revealed one keeps it"
        );
        assert_eq!(game.confidence, 50, "and the next one is asked at it");
    }

    #[test]
    fn a_name_is_cut_to_length_in_characters() {
        let mut game = Game::new(1000);
        let key = admit(&mut game, &"ä".repeat(MAX_NAME + 10), None, 1000).unwrap();
        assert_eq!(game.player(&key).unwrap().name.chars().count(), MAX_NAME);
    }

    fn load(saved: &str) -> Game {
        let mut saved: serde_json::Value = serde_json::from_str(saved).unwrap();
        <Game as rooms::Room>::migrate(&mut saved);
        serde_json::from_value(saved).unwrap()
    }

    #[test]
    fn a_room_saved_before_the_lobby_loads() {
        let game = load(
            r#"{"join":"J","confidence":80,"players":[{"key":"K","name":"Ada"}],
            "rounds":[],"last_seen":5}"#,
        );
        assert_eq!(game.lobby.join, "J");
        assert_eq!(game.player("K").unwrap().name, "Ada");
        assert!(!game.lobby.wall.is_empty(), "and it is given a wall");
        assert_eq!(game.confidence, 80);
    }

    #[test]
    fn questions_saved_without_a_level_get_the_rooms() {
        let game = load(
            r#"{"lobby":{"join":"J","wall":"W","players":[]},"confidence":80,"rounds":[
            {"question":"Q","unit":"","answer":null,"stage":"revealed","asked":1,
            "before":{},"after":{}}],"last_seen":5}"#,
        );
        assert_eq!(game.round().unwrap().confidence, 80);
    }

    /// The shape the flattened lobby saved: `join`, `wall` and `players`
    /// beside the game's own fields, with `seen` on each player.
    #[test]
    fn a_room_saved_with_the_lobby_flattened_loads() {
        let game = load(
            r#"{"join":"J","wall":"W","players":[{"key":"K1","name":"Ada","seen":3},
            {"key":"K2","name":"ada","seen":4}],"confidence":98,"rounds":[{"question":"Q",
            "unit":"m","answer":5.0,"stage":"revealed","asked":1,
            "before":{"K1":{"low":1.0,"high":9.0,"at":2}},"after":{}}],"last_seen":5}"#,
        );
        assert_eq!(
            (game.lobby.join.as_str(), game.lobby.wall.as_str()),
            ("J", "W")
        );
        assert_eq!(game.lobby.len(), 2);
        let (ada, other) = (game.player("K1").unwrap(), game.player("K2").unwrap());
        assert_ne!(ada.id, other.id, "each is given an id to be removed by");
        assert_eq!(game.round().unwrap().before["K1"].high, 9.0);
        assert_eq!(game.scores()[0].hits, 1);
    }

    #[test]
    fn a_room_saved_now_loads_as_it_was_saved() {
        let (mut game, keys) = room();
        game.ask("Q", "", Some(5.0), 1000).unwrap();
        game.bound(&keys[1], 1.0, 9.0, 1001).unwrap();
        let again = load(&serde_json::to_string(&game).unwrap());
        assert_eq!(again.lobby.join, game.lobby.join);
        assert_eq!(again.lobby.len(), 4);
        assert_eq!(again.round().unwrap().before[&keys[1]].low, 1.0);
    }
}
