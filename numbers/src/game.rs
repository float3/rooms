//! Everyone picks a number, and the rule decides who wins.
//!
//! Under every rule here, what a number is worth depends on what everybody
//! else picked. The oldest of them, closest to a fraction of the average,
//! is the Keynesian beauty contest.

use std::collections::{HashMap, HashSet};

use rooms::lobby::{HasLobby, Lobby, OnWall, tidy};
use serde::{Deserialize, Serialize};

use crate::answer::{Answers, MAX_TEXT};

/// Somebody in the room. The lobby keeps who is in.
pub use rooms::lobby::Player;

pub const MAX_ROUNDS: usize = 200;
/// The widest range a round can be played over, which is far wider than
/// the nought to a hundred these are usually played on.
pub const LIMIT: f64 = 1_000_000.0;

/// What decides the winner.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "kebab-case")]
pub enum Rule {
    /// Closest to `num`/`den` of the average, usually 1/3 or 2/3.
    OfTheMean { num: u32, den: u32 },
    /// Closest to `num`/`den` of the middle number.
    OfTheMedian { num: u32, den: u32 },
    /// The lowest number nobody else picked. Whole numbers only, and
    /// some rounds have no winner.
    LowestUnique,
    /// The highest number nobody else picked.
    HighestUnique,
    /// The highest number still below the average.
    UnderTheMean,
    /// The number the most people picked. Nobody wins if no two people
    /// picked the same number.
    MostPopular,
    /// The number furthest from anybody else's, measured by the distance
    /// to the nearest other pick.
    Loneliest,
    /// The highest number wins, but only if everybody's numbers add up
    /// to no more than the limit. Over the limit, nobody wins.
    UnderTheLimit { limit: f64 },
}

impl Rule {
    /// The rule as it is shown to the room.
    pub fn label(self) -> String {
        match self {
            Rule::OfTheMean { num: 1, den: 1 } => "closest to the average".into(),
            Rule::OfTheMean { num, den } => format!("closest to {num}/{den} of the average"),
            Rule::OfTheMedian { num: 1, den: 1 } => "closest to the middle number".into(),
            Rule::OfTheMedian { num, den } => {
                format!("closest to {num}/{den} of the middle number")
            }
            Rule::LowestUnique => "the lowest number nobody else picked".into(),
            Rule::HighestUnique => "the highest number nobody else picked".into(),
            Rule::UnderTheMean => "the highest number below the average".into(),
            Rule::MostPopular => "the number the most people picked".into(),
            Rule::Loneliest => "the number furthest from everybody else's".into(),
            Rule::UnderTheLimit { limit } => {
                format!(
                    "the highest number, if everybody's numbers together come to no more than {limit}"
                )
            }
        }
    }

    /// Whether picks have to be whole numbers. Matching another pick, or
    /// avoiding one, needs a countable set of numbers to pick from.
    pub fn whole_only(self) -> bool {
        matches!(
            self,
            Rule::LowestUnique | Rule::HighestUnique | Rule::MostPopular
        )
    }

    /// Whether the rule has a number to aim at, so that how far off a
    /// pick was means something. The average under "the highest number
    /// below the average" is a line to stay under, not a number to hit.
    pub fn aims(self) -> bool {
        matches!(self, Rule::OfTheMean { .. } | Rule::OfTheMedian { .. })
    }

    fn valid(self) -> Result<(), &'static str> {
        match self {
            Rule::OfTheMean { num, den } | Rule::OfTheMedian { num, den } => {
                if den == 0 || num == 0 || num > den || den > 9 {
                    Err("That fraction is not one of the ones this plays.")
                } else {
                    Ok(())
                }
            }
            Rule::UnderTheLimit { limit } => {
                if limit.is_finite() && limit > 0.0 {
                    Ok(())
                } else {
                    Err("The limit has to be a number above nought.")
                }
            }
            _ => Ok(()),
        }
    }
}

/// A number as it reads in a sentence: 110, 2.5, never 110.00000001.
fn tidy_number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    format!("{rounded}")
}

fn mean(picks: &[f64]) -> f64 {
    picks.iter().sum::<f64>() / picks.len() as f64
}

fn median(picks: &[f64]) -> f64 {
    let mut sorted = picks.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// How close two numbers have to be to count as equal. Picks are typed on
/// a phone, so this only matters when a target lands exactly between two
/// of them.
const SAME: f64 = 1e-9;

#[derive(Clone, Serialize, Deserialize)]
pub struct Pick {
    pub value: f64,
    pub at: u64,
    /// What was typed, in a round played with expressions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Who picked it, as they were called then. A round that has come
    /// out keeps the number of somebody the host later took out, under
    /// this name, so taking somebody out does not change who won.
    #[serde(default)]
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct Round {
    pub rule: Rule,
    pub low: f64,
    pub high: f64,
    /// What a pick can be typed as. Rounds from before this was chosen
    /// have none, and take what their rule did then.
    #[serde(default)]
    pub answers: Option<Answers>,
    pub open: bool,
    /// Until this is true, nobody is sent anybody's pick, including the
    /// host, whose screen is often the one on the wall.
    pub revealed: bool,
    pub asked: u64,
    /// By player key.
    pub picks: HashMap<String, Pick>,
}

#[derive(Serialize, Deserialize)]
pub struct Game {
    /// Who is in, and the join and wall keys. Rooms saved with `join`
    /// and `players` at the top are moved in here as they load: see
    /// `Room::migrate`.
    #[serde(default)]
    pub lobby: Lobby,
    pub rounds: Vec<Round>,
    pub last_seen: u64,
}

impl rooms::Room for Game {
    fn seen(&mut self, now: u64) {
        self.last_seen = now;
    }

    fn expired(&self, now: u64) -> bool {
        self.last_seen + rooms::DAY_MS < now
    }

    fn keys(&self) -> Vec<&str> {
        self.lobby.keys().collect()
    }

    fn migrate(saved: &mut serde_json::Value) {
        rooms::lobby::migrate(saved);
    }
}

impl HasLobby for Game {
    fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    fn lobby_mut(&mut self) -> &mut Lobby {
        &mut self.lobby
    }

    /// Somebody taken out stops counting in the round still being
    /// played. Rounds that have come out keep their number, so the
    /// winners and the scores already shown stay as they were.
    fn left(&mut self, player: &Player) {
        if let Some(round) = self.rounds.last_mut().filter(|round| !round.revealed) {
            round.picks.remove(&player.key);
        }
    }

    /// Picked in the round being played or the one before. Somebody
    /// who has not, and whose phone is not connected, is offered to the
    /// host as away.
    fn recent(&self, player: &Player) -> bool {
        self.rounds
            .iter()
            .rev()
            .take(2)
            .any(|round| round.picks.contains_key(&player.key))
    }

    fn wall_game(&self, _now: u64) -> serde_json::Value {
        serde_json::to_value(crate::view::wall(self)).unwrap_or_default()
    }

    /// Once rounds are being played, the round has the wall: the code
    /// shrinks to a corner for anybody late, and the names give way to a
    /// count.
    fn on_wall(&self, _now: u64) -> OnWall {
        OnWall {
            busy: !self.rounds.is_empty(),
            ..OnWall::default()
        }
    }
}

impl Game {
    pub fn new(now: u64) -> Game {
        Game {
            lobby: Lobby::new(),
            rounds: Vec::new(),
            last_seen: now,
        }
    }

    pub fn round(&self) -> Option<&Round> {
        self.rounds.last()
    }

    fn round_mut(&mut self) -> Result<&mut Round, &'static str> {
        self.rounds.last_mut().ok_or("No round has been started.")
    }

    pub fn player(&self, key: &str) -> Option<&Player> {
        self.lobby.player(key)
    }

    /// The players, in the order they joined.
    pub fn players(&self) -> &[Player] {
        &self.lobby.players
    }

    /// Starts a round, with the answers its rule takes by default.
    pub fn start(&mut self, rule: Rule, low: f64, high: f64, now: u64) -> Result<(), &'static str> {
        self.start_with(rule, None, low, high, now)
    }

    /// Starts a round. The round before it stops taking picks.
    pub fn start_with(
        &mut self,
        rule: Rule,
        answers: Option<Answers>,
        low: f64,
        high: f64,
        now: u64,
    ) -> Result<(), &'static str> {
        rule.valid()?;
        if rule.whole_only() && answers == Some(Answers::Decimal) {
            return Err("This rule is played with whole numbers, so it cannot take decimals.");
        }
        if !low.is_finite() || !high.is_finite() {
            return Err("The range needs two numbers.");
        }
        if low >= high {
            return Err("The range has to go upwards.");
        }
        if low < -LIMIT || high > LIMIT {
            return Err("That range is wider than this will play over.");
        }
        if rule.whole_only() && (low.fract() != 0.0 || high.fract() != 0.0) {
            return Err("This rule is played with whole numbers, so the range needs to be whole.");
        }
        if self.rounds.len() >= MAX_ROUNDS {
            return Err("That is as many rounds as one room holds.");
        }
        if let Some(round) = self.rounds.last_mut() {
            round.open = false;
        }
        self.rounds.push(Round {
            rule,
            low,
            high,
            answers,
            open: true,
            revealed: false,
            asked: now,
            picks: HashMap::new(),
        });
        Ok(())
    }

    pub fn set_open(&mut self, open: bool) -> Result<(), &'static str> {
        let round = self.round_mut()?;
        if round.revealed {
            return Err("This round is over.");
        }
        round.open = open;
        Ok(())
    }

    /// Shows everybody the numbers and the winner, and stops taking
    /// picks.
    pub fn reveal(&mut self) -> Result<(), &'static str> {
        let round = self.round_mut()?;
        round.open = false;
        round.revealed = true;
        Ok(())
    }

    /// Starts the round just revealed over again: the same rule, range
    /// and kind of answer, so a room can see where a rule takes it.
    pub fn again(&mut self, now: u64) -> Result<(), &'static str> {
        let round = self.round().ok_or("No round has been played yet.")?;
        if !round.revealed {
            return Err("Reveal this round first.");
        }
        let (rule, answers, low, high) = (round.rule, round.answers, round.low, round.high);
        self.start_with(rule, answers, low, high, now)
    }

    pub fn pick(&mut self, player: &str, value: f64, now: u64) -> Result<(), &'static str> {
        self.put(player, value, None, now)
    }

    /// Picks what a player typed, read as the round's kind of answer.
    pub fn pick_text(&mut self, player: &str, text: &str, now: u64) -> Result<f64, &'static str> {
        let answers = self.round().ok_or("No round has been started.")?.answers();
        let value = answers.read(text)?;
        let kept = (answers == Answers::Expression).then(|| tidy(text, MAX_TEXT));
        self.put(player, value, kept, now)?;
        Ok(value)
    }

    fn put(
        &mut self,
        player: &str,
        value: f64,
        text: Option<String>,
        now: u64,
    ) -> Result<(), &'static str> {
        if !value.is_finite() {
            return Err("That is not a number.");
        }
        let name = self
            .player(player)
            .ok_or("This link does not belong to anyone in the room.")?
            .name
            .clone();
        let round = self.round_mut()?;
        if !round.open {
            return Err("This round is closed.");
        }
        if value < round.low || value > round.high {
            return Err("That number is outside the range for this round.");
        }
        if round.whole() && value.fract() != 0.0 {
            return Err("This round is played with whole numbers.");
        }
        round.picks.insert(
            player.to_string(),
            Pick {
                value,
                at: now,
                text,
                name,
            },
        );
        Ok(())
    }
}

/// What one player picked, with their name. Only built for a revealed
/// round.
#[derive(Serialize)]
pub struct Chosen {
    pub name: String,
    pub value: f64,
    /// What they typed, when the round took expressions.
    pub text: Option<String>,
    pub won: bool,
}

/// The result of a round.
#[derive(Serialize)]
pub struct Outcome {
    pub n: usize,
    /// `None` when nobody picked.
    pub mean: Option<f64>,
    pub median: Option<f64>,
    /// The number the rule measures against: the one to aim at, or for
    /// "under the average", the one to stay under.
    pub target: Option<f64>,
    /// What `target` is, in words.
    pub target_label: Option<&'static str>,
    /// Everybody's numbers added up, for the limit rule.
    pub total: Option<f64>,
    /// The rule, repeated next to the numbers.
    pub label: String,
    pub winners: Vec<String>,
    /// Why nobody won, when nobody did.
    pub why: Option<String>,
    /// Every pick, lowest first.
    pub picks: Vec<Chosen>,
}

impl Round {
    /// What a pick can be typed as.
    pub fn answers(&self) -> Answers {
        self.answers.unwrap_or(if self.rule.whole_only() {
            Answers::Whole
        } else {
            Answers::Decimal
        })
    }

    /// Whether every pick has to come to a whole number.
    pub fn whole(&self) -> bool {
        self.rule.whole_only() || self.answers() == Answers::Whole
    }

    /// Every pick with who made it: the players in the order they
    /// joined, then anybody taken out since, under the name they picked
    /// with.
    /// Whether `other` is this round played again: the same rule over the
    /// same range.
    pub fn same_as(&self, other: &Round) -> bool {
        self.rule == other.rule && self.low == other.low && self.high == other.high
    }

    fn named<'a>(&'a self, players: &'a [Player]) -> Vec<(&'a str, &'a Pick)> {
        let mut named: Vec<(&str, &Pick)> = players
            .iter()
            .filter_map(|p| self.picks.get(&p.key).map(|pick| (p.name.as_str(), pick)))
            .collect();
        let mut gone: Vec<&Pick> = self
            .picks
            .iter()
            .filter(|(key, _)| !players.iter().any(|p| &p.key == *key))
            .map(|(_, pick)| pick)
            .collect();
        gone.sort_by_key(|pick| pick.at);
        named.extend(gone.into_iter().map(|pick| (pick.name.as_str(), pick)));
        named
    }

    /// Who won, by the round's rule. Empty when nobody did, which
    /// several of the rules allow: if every number is a duplicate, there
    /// is no lowest unique one.
    fn winners<'a>(&self, named: &[(&'a str, f64)]) -> Vec<&'a str> {
        let values: Vec<f64> = named.iter().map(|(_, value)| *value).collect();
        match self.rule {
            Rule::OfTheMean { .. } | Rule::OfTheMedian { .. } => {
                let Some(target) = self.target(&values) else {
                    return Vec::new();
                };
                let closest = values
                    .iter()
                    .map(|value| (value - target).abs())
                    .fold(f64::INFINITY, f64::min);
                named
                    .iter()
                    .filter(|(_, value)| (value - target).abs() <= closest + SAME)
                    .map(|(name, _)| *name)
                    .collect()
            }
            Rule::UnderTheMean => {
                let average = mean(&values);
                let best = values
                    .iter()
                    .copied()
                    .filter(|value| *value < average - SAME)
                    .fold(f64::NEG_INFINITY, f64::max);
                if best == f64::NEG_INFINITY {
                    return Vec::new();
                }
                named
                    .iter()
                    .filter(|(_, value)| (value - best).abs() <= SAME)
                    .map(|(name, _)| *name)
                    .collect()
            }
            Rule::MostPopular => {
                let mut counts: HashMap<i64, usize> = HashMap::new();
                for value in &values {
                    *counts.entry(*value as i64).or_default() += 1;
                }
                let most = counts.values().copied().max().unwrap_or(0);
                if most < 2 {
                    return Vec::new(); // no two people agreed
                }
                let agreed: Vec<i64> = counts
                    .iter()
                    .filter(|(_, count)| **count == most)
                    .map(|(value, _)| *value)
                    .collect();
                named
                    .iter()
                    .filter(|(_, value)| agreed.contains(&(*value as i64)))
                    .map(|(name, _)| *name)
                    .collect()
            }
            Rule::Loneliest => {
                if values.len() < 2 {
                    return Vec::new(); // there is nobody else to be far from
                }
                // The distance from each pick to the nearest pick by
                // anybody else. Somebody who picked the same number as
                // another person is nought away from them, so a shared
                // number never wins.
                let loneliness: Vec<f64> = (0..values.len())
                    .map(|mine| {
                        values
                            .iter()
                            .enumerate()
                            .filter(|(other, _)| *other != mine)
                            .map(|(_, other)| (other - values[mine]).abs())
                            .fold(f64::INFINITY, f64::min)
                    })
                    .collect();
                let furthest = loneliness.iter().copied().fold(0.0_f64, f64::max);
                if furthest <= SAME {
                    return Vec::new();
                }
                named
                    .iter()
                    .zip(&loneliness)
                    .filter(|(_, distance)| (*distance - furthest).abs() <= SAME)
                    .map(|((name, _), _)| *name)
                    .collect()
            }
            Rule::UnderTheLimit { limit } => {
                if values.iter().sum::<f64>() > limit + SAME {
                    return Vec::new(); // the total went over the limit
                }
                let best = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                named
                    .iter()
                    .filter(|(_, value)| (value - best).abs() <= SAME)
                    .map(|(name, _)| *name)
                    .collect()
            }
            Rule::LowestUnique | Rule::HighestUnique => {
                let mut counts: HashMap<i64, usize> = HashMap::new();
                for value in &values {
                    *counts.entry(*value as i64).or_default() += 1;
                }
                let alone: Vec<i64> = counts
                    .iter()
                    .filter(|(_, count)| **count == 1)
                    .map(|(value, _)| *value)
                    .collect();
                let wanted = if self.rule == Rule::LowestUnique {
                    alone.iter().min()
                } else {
                    alone.iter().max()
                };
                let Some(wanted) = wanted else {
                    return Vec::new();
                };
                named
                    .iter()
                    .filter(|(_, value)| *value as i64 == *wanted)
                    .map(|(name, _)| *name)
                    .collect()
            }
        }
    }

    fn target(&self, values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }
        match self.rule {
            Rule::OfTheMean { num, den } => Some(mean(values) * num as f64 / den as f64),
            Rule::OfTheMedian { num, den } => Some(median(values) * num as f64 / den as f64),
            Rule::UnderTheMean => Some(mean(values)),
            // None of these have a number to aim at. What wins depends
            // on the distribution of all the picks.
            Rule::LowestUnique
            | Rule::HighestUnique
            | Rule::MostPopular
            | Rule::Loneliest
            | Rule::UnderTheLimit { .. } => None,
        }
    }

    /// Why a round with picks in it had no winner, in words.
    fn why_nobody(&self, values: &[f64]) -> String {
        let all_same = values.iter().all(|value| (value - values[0]).abs() <= SAME);
        match self.rule {
            Rule::UnderTheLimit { limit } => format!(
                "Together everybody's numbers came to {}, over the limit of {}.",
                tidy_number(values.iter().sum()),
                tidy_number(limit)
            ),
            Rule::MostPopular => "No two people picked the same number.".into(),
            Rule::Loneliest if values.len() < 2 => {
                "Only one person picked, so there was nobody to be far from.".into()
            }
            _ if all_same => "Everybody picked the same number.".into(),
            Rule::UnderTheMean => "Nobody's number was below the average.".into(),
            _ => "Every number was picked by more than one person.".into(),
        }
    }

    /// The round's numbers, or `None` while it is still being played.
    pub fn outcome(&self, players: &[Player]) -> Option<Outcome> {
        if !self.revealed {
            return None;
        }
        let picked = self.named(players);
        let named: Vec<(&str, f64)> = picked
            .iter()
            .map(|(name, pick)| (*name, pick.value))
            .collect();
        let values: Vec<f64> = named.iter().map(|(_, value)| *value).collect();
        let winners = self.winners(&named);
        let why = if values.is_empty() {
            Some("Nobody picked a number this round.".to_string())
        } else if winners.is_empty() {
            Some(self.why_nobody(&values))
        } else {
            None
        };
        let won: HashSet<&str> = winners.iter().copied().collect();
        let mut picks: Vec<Chosen> = picked
            .iter()
            .map(|(name, pick)| Chosen {
                name: (*name).to_string(),
                value: pick.value,
                text: pick.text.clone(),
                won: won.contains(name),
            })
            .collect();
        picks.sort_by(|a, b| a.value.total_cmp(&b.value));
        let target = self.target(&values);
        Some(Outcome {
            n: named.len(),
            mean: (!values.is_empty()).then(|| mean(&values)),
            median: (!values.is_empty()).then(|| median(&values)),
            target,
            target_label: target.map(|_| match self.rule {
                Rule::UnderTheMean => "The average, to stay under",
                _ => "The number to aim at",
            }),
            total: matches!(self.rule, Rule::UnderTheLimit { .. }).then(|| values.iter().sum()),
            label: self.rule.label(),
            winners: winners.iter().map(|name| (*name).to_string()).collect(),
            why,
            picks,
        })
    }
}

/// One player's results over the evening.
#[derive(Serialize)]
pub struct Score {
    pub name: String,
    pub played: usize,
    /// Every round they won, shared or not: a round won by several
    /// counts in full for each of them.
    pub wins: usize,
    /// The rounds they won on their own. Ranks players with as many
    /// wins as each other.
    pub solo: usize,
    /// How far their picks were from the number to aim at, as a share
    /// of the round's range, on average: 0.04 is 4% of the range off.
    /// Only rounds with a number to aim at count, so that rounds played
    /// over different ranges can be compared. `None` when they have
    /// played none of those.
    pub off: Option<f64>,
}

impl Game {
    /// The evening so far, over the revealed rounds only: most wins
    /// first, then most won alone, then closest to the target on
    /// average, so that the order among equal wins is not just the
    /// alphabet. A tally that included the round being played would give away
    /// who was winning it.
    ///
    /// Each round is worked out once, not once per player: every phone
    /// watching the room is sent this, several times a second.
    pub fn scores(&self) -> Vec<Score> {
        let revealed: Vec<(&Round, HashSet<String>, Option<f64>)> = self
            .rounds
            .iter()
            .filter_map(|round| {
                let outcome = round.outcome(self.players())?;
                let aim = outcome.target.filter(|_| round.rule.aims());
                Some((round, outcome.winners.into_iter().collect(), aim))
            })
            .collect();
        let mut scores: Vec<Score> = self
            .players()
            .iter()
            .map(|player| {
                let played = revealed
                    .iter()
                    .filter(|(round, _, _)| round.picks.contains_key(&player.key))
                    .count();
                let off: Vec<f64> = revealed
                    .iter()
                    .filter_map(|(round, _, aim)| {
                        let pick = round.picks.get(&player.key)?;
                        Some((pick.value - (*aim)?).abs() / (round.high - round.low))
                    })
                    .collect();
                let won: Vec<usize> = revealed
                    .iter()
                    .filter(|(_, winners, _)| winners.contains(&player.name))
                    .map(|(_, winners, _)| winners.len())
                    .collect();
                Score {
                    name: player.name.clone(),
                    played,
                    wins: won.len(),
                    solo: won.iter().filter(|&&n| n == 1).count(),
                    off: (!off.is_empty()).then(|| mean(&off)),
                }
            })
            .filter(|score| score.played > 0)
            .collect();
        scores.sort_by(|a, b| {
            b.wins
                .cmp(&a.wins)
                .then(b.solo.cmp(&a.solo))
                .then(closer(a.off, b.off))
                .then(a.name.cmp(&b.name))
        });
        scores
    }
}

/// Less off the target first, and nobody who had nothing to aim at
/// ahead of somebody who did.
fn closer(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.total_cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rooms::lobby::admit;

    const THIRD: Rule = Rule::OfTheMean { num: 1, den: 3 };

    fn room(rule: Rule, low: f64, high: f64, picks: &[f64]) -> Game {
        let mut game = Game::new(1000);
        for (i, value) in picks.iter().enumerate() {
            let name = ["Ada", "Bo", "Cy", "Di", "Eve", "Fay"][i];
            let key = admit(&mut game, name, None, 1000).unwrap();
            if game.rounds.is_empty() {
                game.start(rule, low, high, 1000).unwrap();
            }
            game.pick(&key, *value, 1001).unwrap();
        }
        game
    }

    fn outcome(game: &Game) -> Outcome {
        game.round()
            .unwrap()
            .outcome(game.players())
            .expect("numbers")
    }

    #[test]
    fn a_third_of_the_average_is_what_everyone_is_aiming_at() {
        let mut game = room(THIRD, 0.0, 10.0, &[9.0, 6.0, 3.0, 0.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.mean, Some(4.5));
        assert_eq!(out.target, Some(1.5));
        assert_eq!(out.winners, vec!["Cy", "Di"], "3 and 0 are both 1.5 off");
        assert_eq!(out.picks.len(), 4);
        assert_eq!(out.picks[0].value, 0.0, "lowest first");
        assert!(out.picks[0].won);
    }

    #[test]
    fn two_people_the_same_distance_away_both_win() {
        let mut game = room(THIRD, 0.0, 10.0, &[9.0, 0.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.mean, Some(4.5));
        assert_eq!(out.target, Some(1.5));
        assert_eq!(out.winners, vec!["Bo"], "0 is 1.5 off and 9 is 7.5 off");
    }

    #[test]
    fn a_tie_is_shared() {
        let mut game = room(Rule::OfTheMean { num: 1, den: 1 }, 0.0, 10.0, &[2.0, 4.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.target, Some(3.0));
        assert_eq!(out.winners, vec!["Ada", "Bo"], "both are one away");
    }

    #[test]
    fn the_middle_number_can_be_aimed_at_instead_of_the_average() {
        let rule = Rule::OfTheMedian { num: 2, den: 3 };
        let mut game = room(rule, 0.0, 100.0, &[0.0, 3.0, 99.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.median, Some(3.0), "the 99 does not drag the middle");
        assert_eq!(out.target, Some(2.0));
        assert_eq!(out.winners, vec!["Bo"]);
    }

    #[test]
    fn the_lowest_number_nobody_else_picked_wins() {
        let mut game = room(Rule::LowestUnique, 1.0, 10.0, &[1.0, 1.0, 2.0, 3.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.target, None, "there is no number to aim at");
        assert_eq!(out.winners, vec!["Cy"], "1 was picked twice, so 2 wins");
    }

    #[test]
    fn nobody_wins_when_every_number_is_a_duplicate() {
        let mut game = room(Rule::LowestUnique, 1.0, 10.0, &[4.0, 4.0, 7.0, 7.0]);
        game.reveal().unwrap();
        assert!(outcome(&game).winners.is_empty());
    }

    #[test]
    fn the_highest_unique_number_is_the_other_way_round() {
        let mut game = room(Rule::HighestUnique, 1.0, 10.0, &[9.0, 9.0, 8.0, 1.0]);
        game.reveal().unwrap();
        assert_eq!(outcome(&game).winners, vec!["Cy"], "9 was taken twice");
    }

    #[test]
    fn under_the_average_goes_to_the_highest_one_that_is_still_under() {
        let mut game = room(Rule::UnderTheMean, 0.0, 100.0, &[10.0, 20.0, 90.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.target, Some(40.0));
        assert_eq!(out.winners, vec!["Bo"], "20 is the highest below 40");
    }

    #[test]
    fn nobody_is_under_the_average_when_everybody_picks_the_same() {
        let mut game = room(Rule::UnderTheMean, 0.0, 100.0, &[5.0, 5.0, 5.0]);
        game.reveal().unwrap();
        assert!(outcome(&game).winners.is_empty());
    }

    #[test]
    fn the_number_the_most_people_picked_wins() {
        let mut game = room(Rule::MostPopular, 1.0, 10.0, &[7.0, 7.0, 3.0, 9.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.target, None);
        assert_eq!(out.winners, vec!["Ada", "Bo"], "two of them said 7");
    }

    #[test]
    fn nobody_wins_the_popular_one_when_no_two_people_agree() {
        let mut game = room(Rule::MostPopular, 1.0, 10.0, &[1.0, 2.0, 3.0]);
        game.reveal().unwrap();
        assert!(outcome(&game).winners.is_empty());
    }

    #[test]
    fn the_loneliest_number_is_the_one_furthest_from_the_rest() {
        let mut game = room(Rule::Loneliest, 0.0, 100.0, &[10.0, 12.0, 14.0, 90.0]);
        game.reveal().unwrap();
        assert_eq!(outcome(&game).winners, vec!["Di"], "76 from the nearest");
    }

    #[test]
    fn nobody_is_lonely_when_everybody_picked_the_same_number() {
        let mut game = room(Rule::Loneliest, 0.0, 100.0, &[5.0, 5.0, 5.0]);
        game.reveal().unwrap();
        assert!(outcome(&game).winners.is_empty());
        let mut game = room(Rule::Loneliest, 0.0, 100.0, &[5.0]);
        game.reveal().unwrap();
        assert!(outcome(&game).winners.is_empty(), "nobody to be far from");
    }

    #[test]
    fn the_highest_wins_up_to_the_limit_and_nobody_wins_over_it() {
        let rule = Rule::UnderTheLimit { limit: 100.0 };
        let mut game = room(rule, 0.0, 100.0, &[10.0, 20.0, 30.0]);
        game.reveal().unwrap();
        assert_eq!(outcome(&game).winners, vec!["Cy"], "60 is under 100");

        let mut game = room(rule, 0.0, 100.0, &[10.0, 20.0, 70.0]);
        game.reveal().unwrap();
        assert_eq!(
            outcome(&game).winners,
            vec!["Cy"],
            "exactly 100 is no more than 100"
        );

        let mut game = room(rule, 0.0, 100.0, &[10.0, 20.0, 80.0]);
        game.reveal().unwrap();
        assert!(
            outcome(&game).winners.is_empty(),
            "110 is over the limit, so the room gets nothing"
        );
    }

    #[test]
    fn a_limit_has_to_be_a_number_above_nought() {
        let mut game = Game::new(1000);
        assert!(
            game.start(Rule::UnderTheLimit { limit: 0.0 }, 0.0, 10.0, 1000)
                .is_err()
        );
        assert!(
            game.start(Rule::UnderTheLimit { limit: 50.0 }, 0.0, 10.0, 1000)
                .is_ok()
        );
    }

    #[test]
    fn nothing_comes_out_of_a_round_that_has_not_been_revealed() {
        let game = room(THIRD, 0.0, 10.0, &[1.0, 2.0]);
        assert!(game.round().unwrap().outcome(game.players()).is_none());
    }

    #[test]
    fn a_pick_outside_the_range_is_refused() {
        let mut game = room(THIRD, 0.0, 10.0, &[5.0]);
        let key = game.players()[0].key.clone();
        assert!(game.pick(&key, 11.0, 1002).is_err());
        assert!(game.pick(&key, -1.0, 1002).is_err());
        assert!(game.pick(&key, 10.0, 1002).is_ok(), "the ends are in");
    }

    #[test]
    fn the_unique_rules_are_played_with_whole_numbers() {
        let mut game = room(Rule::LowestUnique, 1.0, 10.0, &[3.0]);
        let key = game.players()[0].key.clone();
        assert!(game.pick(&key, 2.5, 1002).is_err());
        let mut game = Game::new(1000);
        assert!(game.start(Rule::LowestUnique, 0.5, 10.0, 1000).is_err());
    }

    #[test]
    fn a_round_that_has_been_revealed_takes_no_more_picks() {
        let mut game = room(THIRD, 0.0, 10.0, &[5.0]);
        game.reveal().unwrap();
        let key = game.players()[0].key.clone();
        assert!(game.pick(&key, 1.0, 1002).is_err());
        assert!(game.set_open(true).is_err(), "and cannot be reopened");
    }

    #[test]
    fn a_fraction_has_to_be_one_of_the_ones_on_offer() {
        let mut game = Game::new(1000);
        assert!(
            game.start(Rule::OfTheMean { num: 0, den: 3 }, 0.0, 10.0, 1000)
                .is_err()
        );
        assert!(
            game.start(Rule::OfTheMean { num: 4, den: 3 }, 0.0, 10.0, 1000)
                .is_err()
        );
        assert!(
            game.start(Rule::OfTheMean { num: 1, den: 0 }, 0.0, 10.0, 1000)
                .is_err()
        );
        assert!(game.start(THIRD, 0.0, 10.0, 1000).is_ok());
    }

    #[test]
    fn a_range_has_to_go_upwards() {
        let mut game = Game::new(1000);
        assert!(game.start(THIRD, 10.0, 0.0, 1000).is_err());
        assert!(game.start(THIRD, 5.0, 5.0, 1000).is_err());
    }

    #[test]
    fn how_far_off_is_a_share_of_each_range_and_only_where_there_is_an_aim() {
        let mut game = room(THIRD, 0.0, 10.0, &[3.0, 6.0]);
        let keys: Vec<String> = game.players().iter().map(|p| p.key.clone()).collect();
        game.reveal().unwrap(); // a third of 4.5 is 1.5
        game.start(THIRD, 0.0, 100.0, 2000).unwrap();
        game.pick(&keys[0], 10.0, 2001).unwrap();
        game.pick(&keys[1], 50.0, 2001).unwrap();
        game.reveal().unwrap(); // a third of 30 is 10
        game.start(Rule::UnderTheMean, 0.0, 100.0, 3000).unwrap();
        game.pick(&keys[0], 90.0, 3001).unwrap();
        game.reveal().unwrap();

        let scores = game.scores();
        let ada = scores.iter().find(|s| s.name == "Ada").unwrap();
        assert_eq!(ada.played, 3);
        // 1.5 of 10 off, then dead on: 7.5% on average. The round below
        // the average has nothing to aim at, so it does not count.
        assert!((ada.off.unwrap() - 0.075).abs() < 1e-9, "{:?}", ada.off);
        let bo = scores.iter().find(|s| s.name == "Bo").unwrap();
        assert!(
            (bo.off.unwrap() - (0.45 + 0.4) / 2.0).abs() < 1e-9,
            "{:?}",
            bo.off
        );

        let mut game = room(Rule::LowestUnique, 1.0, 20.0, &[3.0]);
        game.reveal().unwrap();
        assert_eq!(game.scores()[0].off, None, "no number to aim at");
    }

    #[test]
    fn the_scoreboard_counts_the_revealed_rounds_only() {
        let mut game = room(THIRD, 0.0, 10.0, &[9.0, 0.0]);
        let keys: Vec<String> = game.players().iter().map(|p| p.key.clone()).collect();
        game.reveal().unwrap();
        game.start(THIRD, 0.0, 10.0, 2000).unwrap();
        game.pick(&keys[0], 1.0, 2001).unwrap();
        game.pick(&keys[1], 8.0, 2001).unwrap();

        let scores = game.scores();
        assert_eq!(scores[0].name, "Bo", "Bo won the revealed round");
        assert_eq!(scores[0].wins, 1);
        assert_eq!(scores[0].played, 1, "the round still going is not counted");
        assert_eq!(
            scores[0].off,
            Some(0.15),
            "1.5 to aim at, 0 picked, out of 10"
        );
        assert_eq!(scores[1].off, Some(0.75), "and 9 picked");
        assert_eq!(scores[1].wins, 0);
    }

    /// Ada and Cy share the first round: 0 and 3 are both 1.5 off a
    /// third of 4.5. Then a second round with `picks` for Ada, Bo and Cy.
    fn shared_then(rule: Rule, low: f64, high: f64, picks: [f64; 3]) -> Game {
        let mut game = room(THIRD, 0.0, 10.0, &[0.0, 6.0, 3.0, 9.0]);
        let keys: Vec<String> = game.players().iter().map(|p| p.key.clone()).collect();
        game.reveal().unwrap();
        game.start(rule, low, high, 2000).unwrap();
        for (key, value) in keys.iter().zip(picks) {
            game.pick(key, value, 2001).unwrap();
        }
        game.reveal().unwrap();
        game
    }

    fn order(game: &Game) -> Vec<(String, usize, usize)> {
        game.scores()
            .into_iter()
            .map(|s| (s.name, s.wins, s.solo))
            .collect()
    }

    #[test]
    fn a_shared_round_is_a_whole_win_for_each_winner() {
        let mut game = room(THIRD, 0.0, 10.0, &[0.0, 6.0, 3.0, 9.0]);
        game.reveal().unwrap();
        assert_eq!(outcome(&game).winners, vec!["Ada", "Cy"]);
        let scores = game.scores();
        for name in ["Ada", "Cy"] {
            let score = scores.iter().find(|s| s.name == name).unwrap();
            assert_eq!((score.wins, score.solo), (1, 0), "{name}");
        }
    }

    #[test]
    fn among_equal_wins_the_ones_won_alone_come_first() {
        // Bo wins the lowest unique number alone. Ada is closer to the
        // target over the evening than Bo, and comes first alphabetically,
        // but won her round with Cy.
        let game = shared_then(Rule::LowestUnique, 1.0, 20.0, [2.0, 1.0, 2.0]);
        assert_eq!(
            order(&game),
            vec![
                ("Bo".into(), 1, 1),
                ("Ada".into(), 1, 0),
                ("Cy".into(), 1, 0),
                ("Di".into(), 0, 0),
            ]
        );
        let scores = game.scores();
        assert!(scores[1].off < scores[0].off, "Ada was the closer");
    }

    #[test]
    fn among_equal_wins_and_wins_alone_the_closest_comes_first() {
        // A third of 5 is 1.67: Bo wins with 0, Cy's 6 is closer than
        // Ada's 9, so Cy goes ahead of Ada though both won one round,
        // together.
        let game = shared_then(THIRD, 0.0, 10.0, [9.0, 0.0, 6.0]);
        assert_eq!(
            order(&game),
            vec![
                ("Bo".into(), 1, 1),
                ("Cy".into(), 1, 0),
                ("Ada".into(), 1, 0),
                ("Di".into(), 0, 0),
            ]
        );
    }

    #[test]
    fn every_rule_says_what_it_is() {
        for (rule, label) in [
            (THIRD, "closest to 1/3 of the average"),
            (Rule::OfTheMean { num: 1, den: 1 }, "closest to the average"),
            (
                Rule::OfTheMedian { num: 2, den: 3 },
                "closest to 2/3 of the middle number",
            ),
            (Rule::LowestUnique, "the lowest number nobody else picked"),
            (Rule::HighestUnique, "the highest number nobody else picked"),
            (Rule::UnderTheMean, "the highest number below the average"),
            (Rule::MostPopular, "the number the most people picked"),
            (Rule::Loneliest, "the number furthest from everybody else's"),
            (
                Rule::UnderTheLimit { limit: 100.0 },
                "the highest number, if everybody's numbers together come to no more than 100",
            ),
        ] {
            assert_eq!(rule.label(), label);
        }
    }

    #[test]
    fn a_shared_number_is_never_the_loneliest() {
        // The two 5s are nought from each other; 90 is 85 from them.
        let mut game = room(Rule::Loneliest, 0.0, 100.0, &[5.0, 5.0, 90.0]);
        game.reveal().unwrap();
        assert_eq!(outcome(&game).winners, vec!["Cy"]);

        let mut game = room(Rule::Loneliest, 0.0, 100.0, &[5.0, 5.0, 90.0, 90.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert!(out.winners.is_empty(), "every number is shared");
        assert_eq!(
            out.why.as_deref(),
            Some("Every number was picked by more than one person.")
        );
    }

    #[test]
    fn a_round_nobody_won_says_why() {
        for (rule, picks, why) in [
            (
                Rule::MostPopular,
                &[1.0, 2.0, 3.0][..],
                "No two people picked the same number.",
            ),
            (
                Rule::LowestUnique,
                &[4.0, 4.0, 7.0, 7.0][..],
                "Every number was picked by more than one person.",
            ),
            (
                Rule::UnderTheMean,
                &[5.0, 5.0][..],
                "Everybody picked the same number.",
            ),
            (
                Rule::UnderTheLimit { limit: 100.0 },
                &[10.0, 20.0, 80.0][..],
                "Together everybody's numbers came to 110, over the limit of 100.",
            ),
            (
                Rule::Loneliest,
                &[5.0][..],
                "Only one person picked, so there was nobody to be far from.",
            ),
        ] {
            let mut game = room(rule, 0.0, 100.0, picks);
            game.reveal().unwrap();
            let out = outcome(&game);
            assert!(out.winners.is_empty(), "{why}");
            assert_eq!(out.why.as_deref(), Some(why));
        }
    }

    #[test]
    fn a_round_with_a_winner_has_no_excuse() {
        let mut game = room(THIRD, 0.0, 10.0, &[9.0, 0.0]);
        game.reveal().unwrap();
        assert!(outcome(&game).why.is_none());
    }

    #[test]
    fn a_round_revealed_with_no_picks_still_comes_out() {
        let mut game = Game::new(1000);
        admit(&mut game, "Ada", None, 1000).unwrap();
        game.start(THIRD, 0.0, 10.0, 1000).unwrap();
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.n, 0);
        assert_eq!(out.mean, None);
        assert!(out.winners.is_empty());
        assert_eq!(
            out.why.as_deref(),
            Some("Nobody picked a number this round.")
        );
    }

    #[test]
    fn under_the_average_marks_the_average_as_the_line_to_stay_under() {
        let mut game = room(Rule::UnderTheMean, 0.0, 100.0, &[10.0, 20.0, 90.0]);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(out.target_label, Some("The average, to stay under"));
        let mut game = room(THIRD, 0.0, 10.0, &[3.0]);
        game.reveal().unwrap();
        assert_eq!(outcome(&game).target_label, Some("The number to aim at"));
    }

    #[test]
    fn the_limit_rule_shows_the_total() {
        let mut game = room(
            Rule::UnderTheLimit { limit: 100.0 },
            0.0,
            100.0,
            &[10.0, 20.0],
        );
        game.reveal().unwrap();
        assert_eq!(outcome(&game).total, Some(30.0));
        let mut game = room(THIRD, 0.0, 10.0, &[3.0]);
        game.reveal().unwrap();
        assert_eq!(outcome(&game).total, None);
    }

    #[test]
    fn the_host_chooses_what_a_pick_can_be_typed_as() {
        let mut game = Game::new(1000);
        let ada = admit(&mut game, "Ada", None, 1000).unwrap();
        game.start_with(THIRD, Some(Answers::Expression), 0.0, 1000.0, 1000)
            .unwrap();
        let value = game.pick_text(&ada, "1,001/3", 1001).unwrap();
        assert!((value - 1001.0 / 3.0).abs() < 1e-9);
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!(
            out.picks[0].text.as_deref(),
            Some("1,001/3"),
            "shown as typed"
        );

        game.start_with(THIRD, Some(Answers::Whole), 0.0, 10.0, 2000)
            .unwrap();
        assert!(game.pick_text(&ada, "2.5", 2001).is_err());
        assert!(game.pick_text(&ada, "1+1", 2001).is_err());
        assert_eq!(game.pick_text(&ada, "3", 2001), Ok(3.0));
        assert_eq!(game.round().unwrap().picks[&ada].text, None);

        game.start_with(THIRD, Some(Answers::Decimal), 0.0, 10.0, 3000)
            .unwrap();
        assert_eq!(game.pick_text(&ada, "2,5", 3001), Ok(2.5));
        assert!(game.pick_text(&ada, "20", 3001).is_err(), "out of range");
    }

    #[test]
    fn a_rule_that_counts_matches_cannot_take_decimals() {
        let mut game = Game::new(1000);
        let ada = admit(&mut game, "Ada", None, 1000).unwrap();
        assert!(
            game.start_with(Rule::LowestUnique, Some(Answers::Decimal), 1.0, 10.0, 1000)
                .is_err()
        );
        game.start_with(
            Rule::LowestUnique,
            Some(Answers::Expression),
            1.0,
            10.0,
            1000,
        )
        .unwrap();
        assert!(
            game.pick_text(&ada, "10/4", 1001).is_err(),
            "has to come to a whole number"
        );
        assert_eq!(game.pick_text(&ada, "10/5", 1001), Ok(2.0));
    }

    #[test]
    fn rounds_saved_before_answers_were_chosen_take_what_their_rule_did() {
        let mut game = room(THIRD, 0.0, 10.0, &[1.5]);
        game.rounds[0].answers = None;
        assert_eq!(game.round().unwrap().answers(), Answers::Decimal);
        game.start(Rule::MostPopular, 0.0, 10.0, 2000).unwrap();
        assert_eq!(game.round().unwrap().answers(), Answers::Whole);
    }

    /// A room as saved before the lobby, and as the first lobby saved it,
    /// flattened, with a wall key at the top.
    fn saved(wall: &str) -> serde_json::Value {
        serde_json::from_str(&format!(
            r#"{{"join":"J",{wall}"players":[{{"key":"K","name":"Ada"}}],
            "rounds":[{{"rule":{{"rule":"of-the-mean","num":1,"den":3}},"low":0.0,"high":10.0,
            "open":false,"revealed":true,"asked":1,"picks":{{"K":{{"value":3.0,"at":2}}}}}}],
            "last_seen":5}}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_room_saved_before_the_lobby_or_with_it_flattened_loads() {
        use rooms::Room;
        for (old, wall) in [(saved(""), None), (saved(r#""wall":"W","#), Some("W"))] {
            let mut old = old;
            Game::migrate(&mut old);
            let game: Game = serde_json::from_value(old).expect("an old room loads");
            assert_eq!(game.lobby.join, "J");
            assert_eq!(game.player("K").unwrap().name, "Ada");
            match wall {
                Some(wall) => assert_eq!(game.lobby.wall, wall, "keeping its wall"),
                None => assert!(!game.lobby.wall.is_empty(), "and is given a wall"),
            }
            assert_eq!(outcome(&game).winners, vec!["Ada"]);
            let mut again = serde_json::to_value(&game).unwrap();
            Game::migrate(&mut again);
            let again: Game = serde_json::from_value(again).unwrap();
            assert_eq!(again.lobby.wall, game.lobby.wall, "which it keeps");
            assert_eq!(again.player("K").unwrap().id, game.player("K").unwrap().id);
        }
    }
}
