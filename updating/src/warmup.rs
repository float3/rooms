//! The calibration warm-up: a few questions with a right answer, before
//! the statements, which mostly have none.
//!
//! Each question is a puzzle whose odds are known exactly: a bag picked by
//! a coin, a test with a known false alarm rate, a coin that may be a
//! trick one. The answer really is drawn at random, and each phone is
//! dealt its own clue drawn from it: a ball out of the bag, a test result,
//! a few flips. Everybody first says how likely the claim is before their
//! clue, then after it. Then the answer comes out.
//!
//! Answers are scored in bits against a coin flip: log2(p / 50%), where p
//! is what you gave the answer that turned out true. Saying 50% scores
//! nothing, being sure and right scores almost a bit, and being sure and
//! wrong costs a lot. That is a proper score, so the best you can do is
//! say what you believe. Answers count as 1% to 99%, so one bad miss
//! costs at most about 5.6 bits.
//!
//! The Bayes bot is the par to beat: it starts from the true prior, gets a
//! clue like everybody else, and updates exactly. Its score for a question
//! is the average over the clues the room was dealt.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::game::{hash, mix};

/// As many questions as one room plays.
pub const MAX_WARMUPS: usize = 30;

/// A claim, how likely it was to start with, and what a clue looks like.
/// A clue is a few draws, each "yes" with one chance if the claim is true
/// and another if it is false.
#[derive(Clone, Serialize, Deserialize)]
pub struct Puzzle {
    /// The situation, as everybody reads it.
    pub setup: String,
    /// What to give a probability for.
    pub claim: String,
    /// How likely the claim is before any clue, which the setup says.
    pub prior: f64,
    pub truth: bool,
    /// The chance of each draw coming up "yes", if the claim is true...
    pub yes_if_true: f64,
    /// ...and if it is not.
    pub yes_if_false: f64,
    /// How many draws each clue is.
    pub draws: u8,
    /// A "yes" and a "no" draw, in words: "red", "blue".
    pub yes: String,
    pub no: String,
    /// Before the draws: "You drew".
    pub clue_says: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// Before anybody has a clue.
    Before,
    /// Everybody has their clue.
    Clue,
    /// The answer is out.
    Revealed,
}

#[derive(Serialize, Deserialize)]
pub struct Warmup {
    pub number: usize,
    pub puzzle: Puzzle,
    pub stage: Stage,
    seed: u64,
    /// What each said before their clue, in percent, by player key.
    pub before: BTreeMap<String, i16>,
    /// And after it.
    pub after: BTreeMap<String, i16>,
}

/// A number between 0 and 1 from a hash.
fn unit(r: u64) -> f64 {
    (r % 1_000_000) as f64 / 1_000_000.0
}

/// The odds the clue gives, as a likelihood ratio, from what was drawn.
fn ratio(p: &Puzzle, drawn: &[bool]) -> f64 {
    drawn
        .iter()
        .map(|&yes| {
            if yes {
                p.yes_if_true / p.yes_if_false
            } else {
                (1.0 - p.yes_if_true) / (1.0 - p.yes_if_false)
            }
        })
        .product()
}

/// Where somebody who starts at `prior` should be after a clue with this
/// likelihood ratio.
pub fn posterior(prior: f64, ratio: f64) -> f64 {
    let odds = prior / (1.0 - prior) * ratio;
    odds / (1.0 + odds)
}

/// An answer in percent, as the chance it gave the truth, kept to 1%–99%.
fn given(percent: i16, truth: bool) -> f64 {
    let p = (percent.clamp(1, 99)) as f64 / 100.0;
    if truth { p } else { 1.0 - p }
}

/// Bits against a coin flip for an answer in percent.
pub fn bits(percent: i16, truth: bool) -> f64 {
    (given(percent, truth) / 0.5).log2()
}

/// The same for a probability that is not rounded to a percent.
fn bits_exact(p: f64, truth: bool) -> f64 {
    let p = p.clamp(0.01, 0.99);
    let q = if truth { p } else { 1.0 - p };
    (q / 0.5).log2()
}

/// The `n`th question a room is asked. They go round four kinds, each
/// with its numbers drawn from the room's seed, so that the evening's
/// questions differ from the last.
pub fn puzzle(n: usize, seed: u64) -> Puzzle {
    let r = mix(seed ^ (n as u64).wrapping_mul(0x5851_F42D_4C95_7F2D));
    let pick =
        |options: &[u32], salt: u64| options[(mix(r ^ salt) % options.len() as u64) as usize];
    let mut p = match n % 4 {
        0 => {
            let red = pick(&[60, 70, 75, 80], 1);
            Puzzle {
                setup: format!(
                    "A coin was flipped to pick one of two bags. Bag A has {red} red balls and {} blue. Bag B has {} red and {red} blue. Each of you drew one ball from the picked bag, and put it back.",
                    100 - red,
                    100 - red
                ),
                claim: "The coin picked bag A.".into(),
                prior: 0.5,
                truth: false,
                yes_if_true: red as f64 / 100.0,
                yes_if_false: 1.0 - red as f64 / 100.0,
                draws: 1,
                yes: "red".into(),
                no: "blue".into(),
                clue_says: "You drew".into(),
            }
        }
        1 => {
            let base = pick(&[10, 20, 30], 2);
            Puzzle {
                setup: format!(
                    "{base}% of the patients at a clinic have a condition. A test finds it 90% of the time, and wrongly says yes to 20% of the people who do not have it. Each of you tested the same patient once."
                ),
                claim: "The patient has the condition.".into(),
                prior: base as f64 / 100.0,
                truth: false,
                yes_if_true: 0.9,
                yes_if_false: 0.2,
                draws: 1,
                yes: "positive".into(),
                no: "negative".into(),
                clue_says: "Your test came back".into(),
            }
        }
        2 => Puzzle {
            setup: "One of two coins was picked at random: a fair one, or a trick one that lands heads 75% of the time. Each of you flipped it three times.".into(),
            claim: "It is the trick coin.".into(),
            prior: 0.5,
            truth: false,
            yes_if_true: 0.75,
            yes_if_false: 0.5,
            draws: 3,
            yes: "heads".into(),
            no: "tails".into(),
            clue_says: "You flipped".into(),
        },
        _ => Puzzle {
            setup: "A die was rolled. On a six, bag A was picked, and otherwise bag B. Bag A has 80 red balls and 20 blue; bag B has 20 red and 80 blue. Each of you drew two balls from the picked bag, putting each back.".into(),
            claim: "The die picked bag A.".into(),
            prior: 1.0 / 6.0,
            truth: false,
            yes_if_true: 0.8,
            yes_if_false: 0.2,
            draws: 2,
            yes: "red".into(),
            no: "blue".into(),
            clue_says: "You drew".into(),
        },
    };
    p.truth = unit(mix(r ^ 0xA11CE)) < p.prior;
    p
}

impl Warmup {
    pub fn new(number: usize, seed: u64) -> Warmup {
        let seed = mix(seed ^ 0xCA11_B8A7);
        Warmup {
            number,
            puzzle: puzzle(number - 1, seed),
            stage: Stage::Before,
            seed,
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        }
    }

    /// What this player drew, true for each "yes". Drawn from the truth,
    /// so the clues really are evidence, and different for each player.
    pub fn drawn(&self, key: &str) -> Vec<bool> {
        let p = &self.puzzle;
        let chance = if p.truth {
            p.yes_if_true
        } else {
            p.yes_if_false
        };
        let mut r = mix(self.seed ^ hash(key));
        (0..p.draws)
            .map(|_| {
                r = mix(r);
                unit(r) < chance
            })
            .collect()
    }

    /// The clue in words: "You drew red, then blue."
    pub fn clue(&self, key: &str) -> String {
        let p = &self.puzzle;
        let words: Vec<&str> = self
            .drawn(key)
            .iter()
            .map(|&yes| if yes { p.yes.as_str() } else { p.no.as_str() })
            .collect();
        format!("{} {}.", p.clue_says, words.join(", then "))
    }

    /// Where Bayes says this player should be after their clue: from the
    /// true prior, and from wherever they said they were before it.
    pub fn ideal(&self, key: &str) -> (f64, Option<f64>) {
        let lr = ratio(&self.puzzle, &self.drawn(key));
        let theirs = self
            .before
            .get(key)
            .map(|&b| posterior((b.clamp(1, 99)) as f64 / 100.0, lr));
        (posterior(self.puzzle.prior, lr), theirs)
    }

    /// The bot's bits on this question: a perfect updater from the true
    /// prior, averaged over the clues of everybody who answered.
    pub fn bot_bits(&self) -> Option<f64> {
        if self.after.is_empty() {
            return None;
        }
        let total: f64 = self
            .after
            .keys()
            .map(|key| bits_exact(self.ideal(key).0, self.puzzle.truth))
            .sum();
        Some(total / self.after.len() as f64)
    }
}

/// Everybody's bits over the revealed questions, for their answers after
/// the clue, by player key. And the bot's.
pub fn board(warmups: &[Warmup]) -> (BTreeMap<String, f64>, f64) {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    let mut bot = 0.0;
    for w in warmups.iter().filter(|w| w.stage == Stage::Revealed) {
        for (key, &answer) in &w.after {
            *out.entry(key.clone()).or_default() += bits(answer, w.puzzle.truth);
        }
        bot += w.bot_bits().unwrap_or(0.0);
    }
    (out, bot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn being_sure_and_right_scores_almost_a_bit_and_wrong_costs_more() {
        assert_eq!(bits(50, true), 0.0);
        assert!((bits(99, true) - 0.9855).abs() < 1e-3);
        assert!((bits(99, false) - -5.643).abs() < 1e-3);
        assert_eq!(bits(100, false), bits(99, false), "kept to 99%");
    }

    #[test]
    fn bayes_is_the_prior_odds_times_the_likelihood_ratio() {
        // A 70/30 bag and a red ball: 70%.
        assert!((posterior(0.5, 0.7 / 0.3) - 0.7).abs() < 1e-9);
        // 10% base rate, positive on a 90%/20% test: 9 / (9 + 18) = 1/3.
        assert!((posterior(0.1, 0.9 / 0.2) - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn every_kind_of_question_says_its_prior_and_deals_its_clues() {
        for n in 0..8 {
            let w = Warmup::new(n + 1, 42);
            let p = &w.puzzle;
            assert!(p.prior > 0.0 && p.prior < 1.0);
            assert_eq!(w.drawn("ada").len(), p.draws as usize);
            assert!(w.clue("ada").starts_with(&p.clue_says), "{}", w.clue("ada"));
        }
    }

    #[test]
    fn clues_are_drawn_from_the_truth() {
        // Over many players, a 75% bag gives mostly the colour of the
        // picked bag.
        let w = Warmup::new(1, 7);
        let p = &w.puzzle;
        let yes = (0..2000).filter(|i| w.drawn(&format!("p{i}"))[0]).count() as f64 / 2000.0;
        let expect = if p.truth {
            p.yes_if_true
        } else {
            p.yes_if_false
        };
        assert!((yes - expect).abs() < 0.05, "{yes} against {expect}");
    }

    #[test]
    fn the_same_player_always_gets_the_same_clue() {
        let w = Warmup::new(3, 9);
        assert_eq!(w.clue("ada"), w.clue("ada"));
    }

    #[test]
    fn the_answer_is_drawn_with_the_prior() {
        // The die question is true about one time in six.
        let trick = (0..600)
            .filter(|&seed| Warmup::new(4, seed).puzzle.truth)
            .count();
        assert!((60..140).contains(&trick), "{trick} of 600");
    }

    #[test]
    fn only_revealed_questions_count_and_the_bot_has_a_score() {
        let mut w = Warmup::new(1, 5);
        w.after
            .insert("ada".into(), if w.puzzle.truth { 90 } else { 10 });
        let (board_now, _) = board(std::slice::from_ref(&w));
        assert!(board_now.is_empty());
        w.stage = Stage::Revealed;
        let (board_now, bot) = board(std::slice::from_ref(&w));
        assert!(board_now["ada"] > 0.8);
        assert!(bot != 0.0);
    }
}
