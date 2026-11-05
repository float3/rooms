//! Speed updating: everyone says where they stand, then talks to whoever
//! stands furthest away.
//!
//! The facilitator picks topics. Each person either gets a bingo card, a
//! grid of statements dealt to them, or answers as many statements from
//! the room's deck as they like. An answer is a probability, 0 to 100,
//! which is how a room answers by default, or agreement on a scale of 1
//! (strongly disagree) to 7 (strongly agree).
//!
//! In a round the server pairs everybody with the person they disagree
//! with most, on the statement they are furthest apart on, and a timer
//! runs. When it runs out everybody says where they stand on that
//! statement now. The difference is what the evening is for.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Deref;

use rooms::lobby::{HasLobby, Lobby, OnWall};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::statements::{TOPICS, topic};
use crate::warmup::{MAX_WARMUPS, Stage, Warmup};

pub const MAX_ROUNDS: usize = 100;
pub const MAX_STATEMENT: usize = 200;
pub const MAX_CUSTOM: usize = 60;
/// The sides a bingo card can have.
pub const SIDES: &[u8] = &[3, 4, 5];
/// Round lengths the facilitator can pick, in minutes.
pub const MINUTES: &[u32] = &[2, 3, 4, 5, 7, 10];
/// How many statements each person can pick in the pick mode.
pub const PICKS: usize = 3;
/// How much a round is extended by.
pub const EXTRA_MS: u64 = 60_000;
/// How far an answer has to move to count as an update, on the shared
/// scale of `Statement::gap`: one step of agreement, or ten points of
/// probability.
pub const UPDATE: i16 = 20;
/// What one "I learned something from you" is worth on the board.
pub const KUDOS_POINTS: i32 = 25;
/// The most one conversation's closing is worth to each partner: one
/// perfect guess. Answers can be changed between rounds, so without a cap
/// somebody could exaggerate before a round, answer honestly after it and
/// run away with the board without anybody changing their mind.
pub const CLOSE_CAP: i32 = 100;
/// How much more a statement is worth talking about, in the bingo mode,
/// for each square already marked in the line it is on...
const LINE_STEP: i32 = 25;
/// ...and for the square that completes a line.
const LINE_DONE: i32 = 90;
/// Where each group meets, so that two people can find each other in a
/// full room by shouting "fox!" rather than a name.
pub const SPOTS: &[&str] = &[
    "🦊 Fox",
    "🐙 Octopus",
    "🦉 Owl",
    "🐢 Turtle",
    "🦋 Butterfly",
    "🐝 Bee",
    "🐸 Frog",
    "🦀 Crab",
    "🐧 Penguin",
    "🦔 Hedgehog",
    "🐳 Whale",
    "🦜 Parrot",
    "🐌 Snail",
    "🦒 Giraffe",
    "🐞 Ladybird",
    "🦩 Flamingo",
    "🐿️ Squirrel",
    "🦘 Kangaroo",
    "🐼 Panda",
    "🦭 Seal",
];

/// The meeting spot of the group at this index in a round.
pub fn spot(index: usize) -> String {
    let name = SPOTS[index % SPOTS.len()];
    match index / SPOTS.len() {
        0 => name.to_string(),
        n => format!("{name} {}", n + 1),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Everybody gets a grid of statements and marks the ones they talked
    /// about, for a line.
    Bingo,
    /// Everybody answers whatever they like from the whole deck.
    Open,
    /// Everybody browses the whole deck on their own time. Each statement
    /// shows how many people have answered it, so the room converges on
    /// the ones people care about, and who answered what is shown only to
    /// people who have answered it themselves.
    Free,
    /// Before anybody answers, everybody picks up to `PICKS` statements
    /// they would like to talk about from the topics' whole pool. The room
    /// then plays the statements somebody picked, as in the open mode.
    Pick,
}

/// How the room answers its statements. Most can be put either way, so
/// the facilitator chooses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scale {
    /// Each as the bank has it: mostly agreement, some probabilities.
    Written,
    /// Everything as agreement from 1 to 7; probability questions become
    /// claims.
    Agree,
    /// Everything as the chance that it is true, which is how a room
    /// starts: a probability means the same to everybody, and asking how
    /// sure people are makes for better conversations than asking whether
    /// they agree.
    #[default]
    Chance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// In the pick mode, before anybody answers: everybody picks the
    /// statements they would like to talk about.
    Picking,
    /// Answering, and finding people by hand.
    Answering,
    /// A calibration warm-up question is on. See `crate::warmup`.
    Warmup,
    /// A round is on: everybody is talking to who they were paired with.
    Talking,
    /// The round is over; everybody says where they stand now.
    Updating,
    /// The evening is over.
    Finished,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Statement {
    pub id: u32,
    pub text: String,
    pub topic: String,
    /// Answered as a probability, 0 to 100, rather than as agreement from
    /// 1 to 7.
    #[serde(default)]
    pub chance: bool,
}

impl Statement {
    pub fn range(&self) -> (i16, i16) {
        if self.chance { (0, 100) } else { (1, 7) }
    }

    /// How far apart two answers are, on one scale for both kinds, 0 to
    /// 200: from one end of either scale to the other is 200.
    pub fn gap(&self, a: i16, b: i16) -> i16 {
        let d = (a - b).abs();
        if self.chance { d * 2 } else { d * 100 / 3 }
    }

    /// An answer on a common scale, -1 to 1.
    pub fn norm(&self, v: i16) -> f64 {
        if self.chance {
            v as f64 / 50.0 - 1.0
        } else {
            (v as f64 - 4.0) / 3.0
        }
    }
}

/// What the game keeps for one player. Who they are, their name and key,
/// is in the room's [`Lobby`]; this is kept beside it, by their key.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Sheet {
    /// A bingo card's statements, row by row. Empty in the open mode,
    /// where the whole deck is everybody's.
    #[serde(default)]
    pub card: Vec<u32>,
    /// Where they stand now, by statement.
    #[serde(default)]
    pub answers: BTreeMap<u32, i16>,
    /// Where they stood the first time they answered.
    #[serde(default)]
    pub first: BTreeMap<u32, i16>,
    /// Statements they have talked about, in a round or on their own.
    #[serde(default)]
    pub marked: BTreeSet<u32>,
    /// Statements they passed on in the quick-fire feed, which it does
    /// not show them again.
    #[serde(default)]
    pub skipped: BTreeSet<u32>,
    /// In the pick mode, the statements they would like to talk about.
    #[serde(default)]
    pub picks: BTreeSet<u32>,
    /// Sitting rounds out: at the bar, gone home, or just resting. Not
    /// paired, and not suggested to anybody as somebody to go and find,
    /// until they say they are back or answer something. Set by them or
    /// by the host, never guessed from their phone's connection: phones
    /// sleep in pockets during a conversation.
    #[serde(default)]
    pub away: bool,
}

/// The sheet of somebody the lobby has and the game has no sheet for,
/// which only a damaged save could make: nothing dealt, nothing answered.
static BLANK: Sheet = Sheet {
    card: Vec::new(),
    answers: BTreeMap::new(),
    first: BTreeMap::new(),
    marked: BTreeSet::new(),
    skipped: BTreeSet::new(),
    picks: BTreeSet::new(),
    away: false,
};

/// Somebody in the room as the game sees them: their key and name, from
/// the lobby, and their sheet.
#[derive(Clone, Copy)]
pub struct Player<'a> {
    /// What the host's screen knows them by.
    pub id: u32,
    pub key: &'a str,
    pub name: &'a str,
    pub sheet: &'a Sheet,
}

impl Deref for Player<'_> {
    type Target = Sheet;
    fn deref(&self) -> &Sheet {
        self.sheet
    }
}

/// Takes each player's sheet out of a room saved before the lobby, when
/// each sheet was kept inside the player's entry in `players`, into
/// `sheets`, by key. A room that has `sheets` already is left alone.
fn migrate_sheets(saved: &mut Value) {
    let Some(room) = saved.as_object_mut() else {
        return;
    };
    if room.contains_key("sheets") {
        return;
    }
    let mut sheets = serde_json::Map::new();
    for player in room
        .get("players")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(key) = player.get("key").and_then(Value::as_str) {
            sheets.insert(key.to_string(), player.clone());
        }
    }
    room.insert("sheets".into(), Value::Object(sheets));
}

/// Two people, sometimes three, and what they were given to talk about.
#[derive(Serialize, Deserialize)]
pub struct Group {
    pub members: Vec<String>,
    pub statement: u32,
    /// Where each stood when the round started.
    pub before: BTreeMap<String, i16>,
    /// Where each said they stood after it.
    pub after: BTreeMap<String, i16>,
    /// Each member's guess at where each partner stands after talking: by
    /// who guessed, then whom. A guess is final once given, and the
    /// partner's answer is only shown to somebody who has guessed it.
    #[serde(default)]
    pub guesses: BTreeMap<String, BTreeMap<String, i16>>,
    /// "I learned something from you": by who said it, to whom.
    #[serde(default)]
    pub kudos: BTreeMap<String, BTreeSet<String>>,
}

impl Group {
    /// Where somebody stands after the round: their new answer, or where
    /// they were if they gave none.
    pub fn landed(&self, key: &str) -> Option<i16> {
        self.after.get(key).or(self.before.get(key)).copied()
    }

    /// How far apart the group was, on the shared scale, before and after.
    pub fn gaps(&self, s: &Statement) -> (i16, i16) {
        let widest = |at: &dyn Fn(&str) -> Option<i16>| -> i16 {
            let mut widest = 0;
            for a in &self.members {
                for b in &self.members {
                    if let (Some(x), Some(y)) = (at(a), at(b)) {
                        widest = widest.max(s.gap(x, y));
                    }
                }
            }
            widest
        };
        (
            widest(&|k| self.before.get(k).copied()),
            widest(&|k| self.landed(k)),
        )
    }

    /// Whether `key` has said where they stand now and guessed where each
    /// partner does. From then on they see their partners' new answers,
    /// and their own is the round's answer as it was then: a change made
    /// after seeing their partners' would not be a change of mind from
    /// the conversation.
    pub fn done(&self, key: &str) -> bool {
        self.after.contains_key(key)
            && self
                .members
                .iter()
                .filter(|o| *o != key)
                .all(|o| self.guesses.get(key).is_some_and(|g| g.contains_key(o)))
    }

    /// Whether everybody in the group is [`done`](Self::done). Until then
    /// the wall does not show the group's new answers, because the
    /// guesses are made blind.
    pub fn settled(&self) -> bool {
        self.members.iter().all(|m| self.done(m))
    }
}

/// How a guess at a partner's answer scores: 100 for exactly right,
/// falling with the square of the miss, and nothing from fifty
/// percentage points (three steps of agreement) away. Squared error is a
/// proper score for a best guess, so the way to score most is to guess
/// what you really expect.
pub fn guess_points(s: &Statement, guess: i16, actual: i16) -> i32 {
    let miss = s.gap(guess, actual) as i32;
    (100 - miss * miss / 100).max(0)
}

/// What somebody's conversations came to, over every round.
#[derive(Clone, Copy, Default, Serialize)]
pub struct Score {
    /// How far apart they and their partners were before, less after, up
    /// to `CLOSE_CAP` a conversation: the same for everybody in the group,
    /// however it closed.
    pub together: i32,
    /// Points for guessing where their partners would land.
    pub listening: i32,
    /// How many times a partner said they learned something from them.
    pub kudos: u32,
    /// How far partners moved towards them. Shown as an award, not added
    /// to the total, so that winning an argument is not the game.
    pub persuaded: i32,
    /// How far they moved towards their partners.
    pub opened: i32,
    /// The widest gap they closed in one conversation, as it was, not
    /// capped: an award, not in the total.
    pub best_closed: i32,
    /// What the board is sorted by: together, listening and kudos.
    pub total: i32,
}

#[derive(Serialize, Deserialize)]
pub struct Round {
    pub started: u64,
    pub groups: Vec<Group>,
    /// Who could not be paired: nobody left, or nothing answered in common.
    pub waiting: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Game {
    /// What the game keeps for each player, by key.
    #[serde(default)]
    sheets: BTreeMap<String, Sheet>,
    /// Who is in, and the join and wall keys.
    #[serde(default)]
    pub lobby: Lobby,
    pub mode: Mode,
    pub topics: Vec<String>,
    pub custom: Vec<String>,
    /// How statements are answered. Kept apart from the deck so the deck
    /// can be built again from it.
    #[serde(default)]
    pub scale: Scale,
    pub side: u8,
    pub minutes: u32,
    pub deck: Vec<Statement>,
    pub phase: Phase,
    /// When the round being talked ends.
    pub ends: u64,
    pub rounds: Vec<Round>,
    /// The calibration questions asked so far. The last is the one on
    /// while the phase is `Warmup`.
    #[serde(default)]
    pub warmups: Vec<Warmup>,
    pub last_seen: u64,
    /// Seeds the dealing, so two rooms deal differently.
    seed: u64,
}

impl rooms::Room for Game {
    fn seen(&mut self, now: u64) {
        self.last_seen = now;
    }

    /// Ends a round whose time is up.
    fn tick(&mut self, now: u64) -> bool {
        if self.phase == Phase::Talking && now >= self.ends {
            self.phase = Phase::Updating;
            return true;
        }
        false
    }

    fn expired(&self, now: u64) -> bool {
        self.last_seen + rooms::DAY_MS < now
    }

    fn keys(&self) -> Vec<&str> {
        self.lobby.keys().collect()
    }

    /// Rooms saved before the lobby kept each sheet in the player's entry
    /// and the lobby's fields at the top level; rooms saved while it was
    /// flattened kept `sheets` beside those. Both load as they are now.
    fn migrate(saved: &mut Value) {
        migrate_sheets(saved);
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

    /// Somebody new: a blank sheet, and in the bingo mode a card.
    fn arrived(&mut self, key: &str, now: u64) -> Result<Option<u32>, &'static str> {
        self.sheets.insert(key.to_string(), Sheet::default());
        self.deal(key);
        self.last_seen = now;
        Ok(None)
    }

    /// Taken out by the host. Their sheet goes, so they are not counted or
    /// paired again, and so does their place in a group of the round that
    /// is on and has not settled, so that their partners can still finish
    /// it. A settled group, and earlier rounds, keep them, because their
    /// partners' points come from those.
    fn left(&mut self, player: &rooms::lobby::Player) {
        let key = player.key.as_str();
        self.sheets.remove(key);
        if matches!(self.phase, Phase::Talking | Phase::Updating)
            && let Some(round) = self.rounds.last_mut()
        {
            round.waiting.retain(|k| k != key);
            for g in round.groups.iter_mut().filter(|g| !g.settled()) {
                g.members.retain(|k| k != key);
                g.before.remove(key);
                g.after.remove(key);
                g.guesses.remove(key);
                for guessed in g.guesses.values_mut() {
                    guessed.remove(key);
                }
                g.kudos.remove(key);
                for given in g.kudos.values_mut() {
                    given.remove(key);
                }
            }
            round.groups.retain(|g| !g.members.is_empty());
        }
    }

    /// What the projector shows: see [`crate::view::wall`].
    fn wall_game(&self, _now: u64) -> Value {
        serde_json::to_value(crate::view::wall(self)).unwrap_or_default()
    }

    /// Once the evening is under way, past answering before the first
    /// round, the game has the wall.
    fn on_wall(&self, _now: u64) -> OnWall {
        OnWall {
            busy: !(self.phase == Phase::Answering && self.rounds.is_empty()),
            ..OnWall::default()
        }
    }
}

/// Trims, collapses whitespace, and cuts to `max` characters.
pub fn tidy(text: &str, max: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// A small generator for dealing. Nothing here needs to be unpredictable,
/// only different from room to room and person to person.
pub(crate) fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

pub(crate) fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    })
}

/// "What is the chance that a fish feels pain?" as a claim to agree with:
/// "A fish feels pain." A question that is not in that form is left as it
/// is.
pub fn claim(question: &str) -> String {
    let Some(rest) = question
        .strip_prefix("What is the chance that ")
        .and_then(|r| r.strip_suffix('?'))
    else {
        return question.to_string();
    };
    let mut chars = rest.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => question.to_string(),
    }
}

/// One statement as the room answers it: as written, or all as agreement,
/// or all as a probability.
fn shaped(text: &str, chance: bool, scale: Scale) -> (String, bool) {
    match (scale, chance) {
        (Scale::Agree, true) => (claim(text), false),
        (Scale::Chance, _) => (text.to_string(), true),
        _ => (text.to_string(), chance),
    }
}

/// Builds a deck from the bank and the facilitator's own statements. A
/// custom line starting with "%" asks for a probability.
fn statements(topics: &[String], custom: &[String], scale: Scale) -> Vec<Statement> {
    let mut deck = Vec::new();
    for key in topics {
        let Some(t) = topic(key) else { continue };
        for text in t.statements {
            let (chance, text) = match text.strip_prefix("% ") {
                Some(rest) => (true, rest),
                None => (false, *text),
            };
            let (text, chance) = shaped(text, chance, scale);
            deck.push(Statement {
                id: deck.len() as u32,
                text,
                topic: t.key.to_string(),
                chance,
            });
        }
    }
    for line in custom {
        let (chance, text) = match line.strip_prefix('%') {
            Some(rest) => (true, rest),
            None => (false, line.as_str()),
        };
        let text = tidy(text, MAX_STATEMENT);
        if !text.is_empty() {
            let (text, chance) = shaped(&text, chance, scale);
            deck.push(Statement {
                id: deck.len() as u32,
                text,
                topic: "custom".into(),
                chance,
            });
        }
    }
    deck
}

impl Game {
    pub fn new(now: u64) -> Game {
        let lobby = Lobby::new();
        let seed = hash(&lobby.join) ^ now;
        let topics: Vec<String> = TOPICS
            .iter()
            .filter(|t| t.default)
            .map(|t| t.key.to_string())
            .collect();
        let mut game = Game {
            sheets: BTreeMap::new(),
            lobby,
            mode: Mode::Bingo,
            deck: Vec::new(),
            topics,
            custom: Vec::new(),
            scale: Scale::Chance,
            side: 4,
            minutes: 5,
            phase: Phase::Answering,
            ends: 0,
            rounds: Vec::new(),
            warmups: Vec::new(),
            last_seen: now,
            seed,
        };
        game.build_deck();
        game
    }

    /// Everybody in the room, in the order they joined.
    pub fn players(&self) -> impl Iterator<Item = Player<'_>> {
        self.lobby.players.iter().map(|p| Player {
            id: p.id,
            key: &p.key,
            name: &p.name,
            sheet: self.sheets.get(&p.key).unwrap_or(&BLANK),
        })
    }

    pub fn player(&self, key: &str) -> Option<Player<'_>> {
        let p = self.lobby.player(key)?;
        Some(Player {
            id: p.id,
            key: &p.key,
            name: &p.name,
            sheet: self.sheets.get(key).unwrap_or(&BLANK),
        })
    }

    fn player_mut(&mut self, key: &str) -> Result<&mut Sheet, &'static str> {
        if self.lobby.player(key).is_none() {
            return Err("You are not in this room.");
        }
        Ok(self.sheets.entry(key.to_string()).or_default())
    }

    pub fn statement(&self, id: u32) -> Option<&Statement> {
        self.deck.get(id as usize)
    }

    /// The statements this player can answer: their card, or the deck.
    pub fn hand(&self, player: Player<'_>) -> Vec<u32> {
        match self.mode {
            Mode::Bingo => player.card.clone(),
            Mode::Open | Mode::Free | Mode::Pick => self.deck.iter().map(|s| s.id).collect(),
        }
    }

    /// Whether anybody has answered anything. After that the deck stays as
    /// it is, because the answers refer to it.
    pub fn locked(&self) -> bool {
        !self.rounds.is_empty() || self.sheets.values().any(|p| !p.first.is_empty())
    }

    /// The deck the room plays with. A bingo room takes a sample about
    /// twice the size of a card, so that cards overlap enough for people
    /// to have statements in common; the open mode takes everything.
    fn build_deck(&mut self) {
        let all = statements(&self.topics, &self.custom, self.scale);
        let customs: Vec<&Statement> = all.iter().filter(|s| s.topic == "custom").collect();
        let mut picked: Vec<Statement> = match self.mode {
            Mode::Open | Mode::Free | Mode::Pick => all.clone(),
            Mode::Bingo => {
                let want = (self.side as usize).pow(2) * 2;
                let mut bank: Vec<&Statement> =
                    all.iter().filter(|s| s.topic != "custom").collect();
                let mut r = self.seed;
                // Fisher-Yates, so the sample is fair across topics.
                for i in (1..bank.len()).rev() {
                    r = mix(r);
                    bank.swap(i, (r % (i as u64 + 1)) as usize);
                }
                let room = want.saturating_sub(customs.len());
                let mut chosen: Vec<Statement> = bank.into_iter().take(room).cloned().collect();
                chosen.extend(customs.into_iter().cloned());
                chosen
            }
        };
        for (i, s) in picked.iter_mut().enumerate() {
            s.id = i as u32;
        }
        self.deck = picked;
        let keys: Vec<String> = self.lobby.players.iter().map(|p| p.key.clone()).collect();
        for p in self.sheets.values_mut() {
            p.card.clear();
            p.picks.clear();
        }
        for key in keys {
            self.deal(&key);
        }
    }

    /// Deals a card, favouring the statements fewest other cards have, so
    /// that every statement is somebody's and cards overlap evenly.
    fn deal(&mut self, key: &str) {
        if self.mode != Mode::Bingo {
            return;
        }
        let size = (self.side as usize).pow(2).min(self.deck.len());
        let mut used = vec![0usize; self.deck.len()];
        for p in self.sheets.values() {
            for &id in &p.card {
                used[id as usize] += 1;
            }
        }
        let mut r = mix(self.seed ^ hash(key));
        let mut order: Vec<(usize, u64, u32)> = (0..self.deck.len())
            .map(|i| {
                r = mix(r);
                (used[i], r, i as u32)
            })
            .collect();
        order.sort();
        let mut card: Vec<u32> = order.into_iter().take(size).map(|(_, _, id)| id).collect();
        // Shuffled again so the grid does not put the rarest in a corner.
        for i in (1..card.len()).rev() {
            r = mix(r);
            card.swap(i, (r % (i as u64 + 1)) as usize);
        }
        if let Some(p) = self.sheets.get_mut(key) {
            p.card = card;
        }
    }

    /// Changes what the room plays with, until anybody has answered.
    pub fn setup(
        &mut self,
        mode: Mode,
        scale: Scale,
        topics: &[String],
        custom: &str,
        side: u8,
    ) -> Result<(), &'static str> {
        if self.locked() {
            return Err("People have started answering, so the statements stay as they are.");
        }
        if !SIDES.contains(&side) {
            return Err("A card is 3, 4 or 5 squares wide.");
        }
        let topics: Vec<String> = TOPICS
            .iter()
            .filter(|t| topics.iter().any(|k| k == t.key))
            .map(|t| t.key.to_string())
            .collect();
        let custom: Vec<String> = custom
            .lines()
            .map(|l| tidy(l, MAX_STATEMENT + 1))
            .filter(|l| !l.is_empty())
            .take(MAX_CUSTOM)
            .collect();
        let size = statements(&topics, &custom, scale).len();
        if size == 0 {
            return Err("Pick at least one topic, or write some statements.");
        }
        if mode == Mode::Bingo && size < (side as usize).pow(2) {
            return Err("There are not enough statements for a card that size.");
        }
        self.mode = mode;
        self.scale = scale;
        self.topics = topics;
        self.custom = custom;
        self.side = side;
        self.build_deck();
        if matches!(self.phase, Phase::Answering | Phase::Picking) {
            self.phase = if mode == Mode::Pick {
                Phase::Picking
            } else {
                Phase::Answering
            };
        }
        Ok(())
    }

    pub fn set_minutes(&mut self, minutes: u32) -> Result<(), &'static str> {
        if !MINUTES.contains(&minutes) {
            return Err("That is not one of the round lengths.");
        }
        self.minutes = minutes;
        Ok(())
    }

    /// Records where somebody stands, or clears it. During and after a
    /// round, an answer to the statement they were given counts as their
    /// answer after the conversation, until they are done with the round
    /// ([`Group::done`]); after that it changes only their answer.
    pub fn answer(&mut self, key: &str, id: u32, value: Option<i16>) -> Result<(), &'static str> {
        if self.phase == Phase::Finished {
            return Err("The evening is over.");
        }
        if self.phase == Phase::Picking {
            return Err("Answering starts once the host closes the picks.");
        }
        let Some(statement) = self.statement(id).cloned() else {
            return Err("That statement is not in this room.");
        };
        let mode = self.mode;
        let player = self.player_mut(key)?;
        if mode == Mode::Bingo && !player.card.contains(&id) {
            return Err("That statement is not on your card.");
        }
        match value {
            Some(v) => {
                let (low, high) = statement.range();
                if v < low || v > high {
                    return Err("That answer is off the scale.");
                }
                player.answers.insert(id, v);
                player.away = false;
                player.skipped.remove(&id);
                player.first.entry(id).or_insert(v);
            }
            None => {
                player.answers.remove(&id);
            }
        }
        if matches!(self.phase, Phase::Talking | Phase::Updating)
            && let Some(round) = self.rounds.last_mut()
            && let Some(group) = round
                .groups
                .iter_mut()
                .find(|g| g.statement == id && g.members.iter().any(|m| m == key))
            && !group.done(key)
        {
            match value {
                Some(v) => group.after.insert(key.to_string(), v),
                None => group.after.remove(key),
            };
        }
        Ok(())
    }

    /// Says that somebody is sitting the next rounds out, or is back.
    pub fn set_away(&mut self, key: &str, away: bool) -> Result<(), &'static str> {
        self.player_mut(key)?.away = away;
        Ok(())
    }

    /// The same, by the host, for the person with this id.
    pub fn set_away_by_id(&mut self, id: u32, away: bool) -> Result<(), &'static str> {
        let key = self
            .lobby
            .by_id(id)
            .ok_or("That person is not in the room any more.")?
            .key
            .clone();
        self.set_away(&key, away)
    }

    /// Passes on a statement in the quick-fire feed.
    pub fn skip(&mut self, key: &str, id: u32) -> Result<(), &'static str> {
        if self.statement(id).is_none() {
            return Err("That statement is not in this room.");
        }
        let player = self.player_mut(key)?;
        if !player.answers.contains_key(&id) {
            player.skipped.insert(id);
        }
        Ok(())
    }

    /// The statements to show this player next in the quick-fire feed,
    /// best first.
    ///
    /// A large deck only gets people talking if they answer the same
    /// statements, so the feed leans towards what others have answered,
    /// most of all where they disagree, and towards the topics this player
    /// keeps answering. A little noise, fixed until the player answers or
    /// skips, keeps two phones from showing the same order. Every fourth
    /// card is instead one that few people have answered, so that new
    /// statements get their first answers too.
    pub fn feed(&self, player: Player<'_>, n: usize) -> Vec<u32> {
        let acted = (player.answers.len() + player.skipped.len()) as u64;
        let mine: Vec<&Statement> = player
            .answers
            .keys()
            .filter_map(|&id| self.statement(id))
            .collect();
        let affinity = |topic: &str| -> f64 {
            if mine.is_empty() {
                return 0.0;
            }
            mine.iter().filter(|s| s.topic == topic).count() as f64 / mine.len() as f64
        };
        let mut scored: Vec<(f64, usize, u32)> = Vec::new();
        for s in &self.deck {
            if player.answers.contains_key(&s.id) || player.skipped.contains(&s.id) {
                continue;
            }
            // Others' answers on one scale, -1 to 1.
            let theirs: Vec<f64> = self
                .players()
                .filter(|p| p.key != player.key)
                .filter_map(|p| p.sheet.answers.get(&s.id))
                .map(|&v| s.norm(v))
                .collect();
            let count = theirs.len();
            let spread = if count >= 2 {
                let mean = theirs.iter().sum::<f64>() / count as f64;
                (theirs.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64).sqrt()
            } else {
                0.0
            };
            let noise =
                (mix(hash(player.key) ^ (s.id as u64) << 20 ^ acted) % 1000) as f64 / 1000.0;
            let score = (1.0 + count as f64).ln() + spread + 0.6 * affinity(&s.topic) + 0.5 * noise;
            scored.push((score, count, s.id));
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut order: Vec<u32> = scored.iter().map(|&(_, _, id)| id).collect();
        if acted % 4 == 3
            && let Some(&(_, _, rare)) = scored
                .iter()
                .min_by(|a, b| a.1.cmp(&b.1).then(b.0.total_cmp(&a.0)))
        {
            order.retain(|&id| id != rare);
            order.insert(0, rare);
        }
        order.truncate(n);
        order
    }

    /// Marks a statement as talked about, or unmarks it.
    pub fn mark(&mut self, key: &str, id: u32, marked: bool) -> Result<(), &'static str> {
        if self.statement(id).is_none() {
            return Err("That statement is not in this room.");
        }
        let player = self.player_mut(key)?;
        if marked {
            player.marked.insert(id);
        } else {
            player.marked.remove(&id);
        }
        Ok(())
    }

    /// Picks a statement to talk about, or takes the pick back.
    pub fn pick(&mut self, key: &str, id: u32, picked: bool) -> Result<(), &'static str> {
        if self.phase != Phase::Picking {
            return Err("The picks are closed.");
        }
        if self.statement(id).is_none() {
            return Err("That statement is not in this room.");
        }
        let player = self.player_mut(key)?;
        if !picked {
            player.picks.remove(&id);
            return Ok(());
        }
        if !player.picks.contains(&id) && player.picks.len() >= PICKS {
            return Err("That is three picks. Take one back to pick another.");
        }
        player.picks.insert(id);
        Ok(())
    }

    /// How many people picked each statement, by statement.
    pub fn pick_counts(&self) -> BTreeMap<u32, usize> {
        let mut counts = BTreeMap::new();
        for id in self.sheets.values().flat_map(|p| &p.picks) {
            *counts.entry(*id).or_insert(0) += 1;
        }
        counts
    }

    /// Ends the picking: the deck becomes every statement somebody picked,
    /// once each however many picked it, the most picked first and the
    /// rest in the order the pool has them. Everybody answers from it,
    /// whether they picked anything or not.
    pub fn close_picks(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::Picking {
            return Err("Nobody is picking.");
        }
        let counts = self.pick_counts();
        if counts.is_empty() {
            return Err("Nobody has picked a statement yet.");
        }
        let mut order: Vec<u32> = counts.keys().copied().collect();
        order.sort_by_key(|id| (std::cmp::Reverse(counts[id]), *id));
        let renumbered: BTreeMap<u32, u32> = order
            .iter()
            .enumerate()
            .map(|(new, &old)| (old, new as u32))
            .collect();
        self.deck = order
            .iter()
            .enumerate()
            .filter_map(|(new, &old)| {
                let mut s = self.statement(old)?.clone();
                s.id = new as u32;
                Some(s)
            })
            .collect();
        for p in self.sheets.values_mut() {
            p.picks = p
                .picks
                .iter()
                .filter_map(|id| renumbered.get(id))
                .copied()
                .collect();
        }
        self.phase = Phase::Answering;
        Ok(())
    }

    /// Pairs everybody up and starts the clock.
    pub fn start_round(&mut self, now: u64) -> Result<(), &'static str> {
        if self.phase == Phase::Talking {
            return Err("A round is already on.");
        }
        if self.phase == Phase::Picking {
            return Err("Close the picks first, so everybody answers the same statements.");
        }
        if self.rounds.len() >= MAX_ROUNDS {
            return Err("That is as many rounds as one room holds.");
        }
        let met: HashSet<(String, String)> = self
            .rounds
            .iter()
            .flat_map(|r| r.groups.iter())
            .flat_map(|g| {
                g.members.iter().flat_map(move |a| {
                    g.members
                        .iter()
                        .filter(move |b| *b != a)
                        .map(move |b| (a.clone(), b.clone()))
                })
            })
            .collect();
        if self.phase == Phase::Warmup {
            return Err("Finish the warm-up first.");
        }
        let (groups, waiting) = pair(self, &met);
        if groups.is_empty() {
            return Err(
                "Nobody can be paired yet: people need to answer some of the same statements.",
            );
        }
        for g in &groups {
            for m in &g.members {
                if let Some(p) = self.sheets.get_mut(m) {
                    p.marked.insert(g.statement);
                }
            }
        }
        self.rounds.push(Round {
            started: now,
            groups,
            waiting,
        });
        self.phase = Phase::Talking;
        self.ends = now + self.minutes as u64 * 60_000;
        Ok(())
    }

    /// Ends the round now rather than when the clock runs out.
    pub fn stop(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::Talking {
            return Err("No round is on.");
        }
        self.phase = Phase::Updating;
        Ok(())
    }

    pub fn extend(&mut self, now: u64) -> Result<(), &'static str> {
        match self.phase {
            Phase::Talking => self.ends += EXTRA_MS,
            // A round that has just run out can be given its minute back.
            Phase::Updating if !self.rounds.is_empty() => {
                self.phase = Phase::Talking;
                self.ends = now + EXTRA_MS;
            }
            _ => return Err("No round is on."),
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), &'static str> {
        self.phase = Phase::Finished;
        Ok(())
    }

    /// Back from the end, for an evening that was not over after all.
    pub fn reopen(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::Finished {
            return Err("The evening is still on.");
        }
        self.phase = Phase::Answering;
        Ok(())
    }

    /// Asks the next calibration question. Only between rounds, because
    /// a round's screens would be lost under it.
    pub fn start_warmup(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::Answering {
            return Err("A warm-up question is asked between rounds.");
        }
        if self.warmups.len() >= MAX_WARMUPS {
            return Err("That is as many warm-up questions as one room holds.");
        }
        self.warmups
            .push(Warmup::new(self.warmups.len() + 1, self.seed));
        self.phase = Phase::Warmup;
        Ok(())
    }

    /// The question being asked, to change.
    fn warmup_mut(&mut self) -> Result<&mut Warmup, &'static str> {
        if self.phase != Phase::Warmup {
            return Err("No warm-up question is on.");
        }
        self.warmups.last_mut().ok_or("No warm-up question is on.")
    }

    /// The question being asked.
    pub fn warmup(&self) -> Option<&Warmup> {
        (self.phase == Phase::Warmup)
            .then(|| self.warmups.last())
            .flatten()
    }

    /// Deals the clues, then reveals the answer.
    pub fn warmup_next(&mut self) -> Result<(), &'static str> {
        let w = self.warmup_mut()?;
        w.stage = match w.stage {
            Stage::Before => Stage::Clue,
            Stage::Clue => Stage::Revealed,
            Stage::Revealed => return Err("The answer is already out."),
        };
        Ok(())
    }

    /// Back to the statements.
    pub fn end_warmup(&mut self) -> Result<(), &'static str> {
        self.warmup_mut()?;
        self.phase = Phase::Answering;
        Ok(())
    }

    /// Says how likely the warm-up claim is: before the clue, or after it.
    pub fn warm_answer(&mut self, key: &str, percent: i16) -> Result<(), &'static str> {
        if self.player(key).is_none() {
            return Err("You are not in this room.");
        }
        if !(0..=100).contains(&percent) {
            return Err("That answer is off the scale.");
        }
        let w = self.warmup_mut()?;
        match w.stage {
            Stage::Before => w.before.insert(key.to_string(), percent),
            Stage::Clue => w.after.insert(key.to_string(), percent),
            Stage::Revealed => return Err("The answer is already out."),
        };
        Ok(())
    }

    /// The group this player is in this round, if a round has been run.
    pub fn group_of(&self, key: &str) -> Option<(usize, &Group)> {
        if !matches!(self.phase, Phase::Talking | Phase::Updating) {
            return None;
        }
        self.rounds
            .last()?
            .groups
            .iter()
            .enumerate()
            .find(|(_, g)| g.members.iter().any(|m| m == key))
    }

    /// The statement two people disagree on most, and by how much,
    /// preferring one neither has talked about yet, and in the bingo mode
    /// one that brings either of them closer to a line. Disagreement still
    /// leads: a line is worth at most `LINE_DONE` to each of them, against
    /// up to 200 for the widest disagreement.
    pub fn best_between(&self, a: Player<'_>, b: Player<'_>) -> Option<(u32, i16)> {
        let mut best: Option<(i32, u32, i16)> = None;
        for (&id, &x) in &a.answers {
            let Some(&y) = b.answers.get(&id) else {
                continue;
            };
            let Some(s) = self.statement(id) else {
                continue;
            };
            let gap = s.gap(x, y);
            let fresh = !a.marked.contains(&id) && !b.marked.contains(&id);
            let score = gap as i32
                + if fresh { 1000 } else { 0 }
                + self.towards_a_line(a, id)
                + self.towards_a_line(b, id);
            if best.is_none_or(|(s, _, _)| score > s) {
                best = Some((score, id, gap));
            }
        }
        best.map(|(_, id, gap)| (id, gap))
    }

    /// The group this player is in during the round being played, to
    /// change.
    fn my_group(&mut self, key: &str) -> Result<&mut Group, &'static str> {
        if !matches!(self.phase, Phase::Talking | Phase::Updating) {
            return Err("No round is on.");
        }
        self.rounds
            .last_mut()
            .and_then(|r| {
                r.groups
                    .iter_mut()
                    .find(|g| g.members.iter().any(|m| m == key))
            })
            .ok_or("You are not in a group this round.")
    }

    /// The key of somebody in this player's group, by name.
    fn partner_key(&self, key: &str, name: &str) -> Result<String, &'static str> {
        let (_, group) = self
            .group_of(key)
            .ok_or("You are not in a group this round.")?;
        group
            .members
            .iter()
            .find(|m| *m != key && self.player(m).is_some_and(|p| p.name == name))
            .cloned()
            .ok_or("That is not somebody you talked to this round.")
    }

    /// Guesses where a partner stands after talking. Final once given.
    pub fn guess(&mut self, key: &str, name: &str, value: i16) -> Result<(), &'static str> {
        let partner = self.partner_key(key, name)?;
        let statement = self.my_group(key)?.statement;
        let (low, high) = self
            .statement(statement)
            .ok_or("That statement is not in this room.")?
            .range();
        if value < low || value > high {
            return Err("That guess is off the scale.");
        }
        let mine = self
            .my_group(key)?
            .guesses
            .entry(key.to_string())
            .or_default();
        if mine.contains_key(&partner) {
            return Err("Your guess is in, and it stays as it is.");
        }
        mine.insert(partner, value);
        Ok(())
    }

    /// Says, or takes back, that somebody learned something from a
    /// partner.
    pub fn kudos(&mut self, key: &str, name: &str, given: bool) -> Result<(), &'static str> {
        let partner = self.partner_key(key, name)?;
        let mine = self
            .my_group(key)?
            .kudos
            .entry(key.to_string())
            .or_default();
        if given {
            mine.insert(partner);
        } else {
            mine.remove(&partner);
        }
        Ok(())
    }

    /// What everybody's conversations came to, over every round, by player
    /// key.
    ///
    /// The total rewards closing a gap together, guessing well where your
    /// partner lands, and partners saying they learned something from you.
    /// It does not reward moving somebody towards you on its own: that
    /// made the evening a debate, and it paid to start from an extreme.
    /// It is still counted, as one award among several.
    pub fn scores(&self) -> BTreeMap<String, Score> {
        self.scores_of(|_, _| true)
    }

    /// The same, from only the groups whose result is out: settled, or in
    /// a round that is over. The wall's board is this, because a total
    /// that moves as somebody guesses would show the room the guess, and
    /// the answer it was a guess at, before the group has settled.
    pub fn revealed_scores(&self) -> BTreeMap<String, Score> {
        let on = matches!(self.phase, Phase::Talking | Phase::Updating);
        let last = self.rounds.len().saturating_sub(1);
        self.scores_of(|round, g| !on || round != last || g.settled())
    }

    fn scores_of(&self, counts: impl Fn(usize, &Group) -> bool) -> BTreeMap<String, Score> {
        let mut out: BTreeMap<String, Score> = BTreeMap::new();
        for (r, round) in self.rounds.iter().enumerate() {
            for g in &round.groups {
                if !counts(r, g) {
                    continue;
                }
                let Some(s) = self.statement(g.statement) else {
                    continue;
                };
                let answered = g.members.iter().any(|m| g.after.contains_key(m));
                let (before, after) = g.gaps(s);
                let closed = if answered {
                    (before - after).max(0) as i32
                } else {
                    0
                };
                for m in &g.members {
                    let score = out.entry(m.clone()).or_default();
                    score.together += closed.min(CLOSE_CAP);
                    score.best_closed = score.best_closed.max(closed);
                }
                for mover in &g.members {
                    let (Some(&from), Some(&to)) = (g.before.get(mover), g.after.get(mover)) else {
                        continue;
                    };
                    for other in g.members.iter().filter(|o| *o != mover) {
                        let Some(&there) = g.before.get(other) else {
                            continue;
                        };
                        let closer = (s.gap(from, there) - s.gap(to, there)).max(0) as i32;
                        out.entry(other.clone()).or_default().persuaded += closer;
                        out.entry(mover.clone()).or_default().opened += closer;
                    }
                }
                for (by, guessed) in &g.guesses {
                    for (of, &guess) in guessed {
                        if let Some(&actual) = g.after.get(of) {
                            out.entry(by.clone()).or_default().listening +=
                                guess_points(s, guess, actual);
                        }
                    }
                }
                for to in g.kudos.values().flatten() {
                    out.entry(to.clone()).or_default().kudos += 1;
                }
            }
        }
        for score in out.values_mut() {
            score.total = score.together + score.listening + KUDOS_POINTS * score.kudos as i32;
        }
        out
    }

    /// How much talking about this statement would bring a player closer
    /// to a bingo line: more for a line that is nearly full, most for the
    /// square that completes one. Nothing outside the bingo mode.
    fn towards_a_line(&self, player: Player<'_>, id: u32) -> i32 {
        if self.mode != Mode::Bingo || player.marked.contains(&id) {
            return 0;
        }
        let side = self.side as usize;
        if player.card.len() != side * side {
            return 0;
        }
        let Some(at) = player.card.iter().position(|&c| c == id) else {
            return 0;
        };
        let (row, col) = (at / side, at % side);
        let on = |r: usize, c: usize| player.marked.contains(&player.card[r * side + c]);
        let mut lines: Vec<usize> = vec![
            (0..side).filter(|&c| on(row, c)).count(),
            (0..side).filter(|&r| on(r, col)).count(),
        ];
        if row == col {
            lines.push((0..side).filter(|&i| on(i, i)).count());
        }
        if row + col == side - 1 {
            lines.push((0..side).filter(|&i| on(i, side - 1 - i)).count());
        }
        let best = lines.into_iter().max().unwrap_or(0);
        if best + 1 == side {
            LINE_DONE
        } else {
            best as i32 * LINE_STEP
        }
    }

    /// Whether this player's marked squares make a line, and how many.
    pub fn lines(&self, player: Player<'_>) -> usize {
        if self.mode != Mode::Bingo {
            return 0;
        }
        let side = self.side as usize;
        if player.card.len() != side * side {
            return 0;
        }
        let on = |r: usize, c: usize| player.marked.contains(&player.card[r * side + c]);
        let rows = (0..side).filter(|&r| (0..side).all(|c| on(r, c))).count();
        let cols = (0..side).filter(|&c| (0..side).all(|r| on(r, c))).count();
        let down = (0..side).all(|i| on(i, i)) as usize;
        let up = (0..side).all(|i| on(i, side - 1 - i)) as usize;
        rows + cols + down + up
    }
}

/// Pairs the room so that people talk to whoever disagrees with them most.
///
/// Every possible pair is scored by the statement they are furthest apart
/// on, preferring statements neither has talked about, and pairs who have
/// already talked come last. The pairs are then taken greedily, biggest
/// disagreement first. With an odd number, the one left over joins the
/// group whose statement they disagree with most. Greedy is not the best
/// possible matching, but it gives the most opposed pairs the evening's
/// best conversations, which is the point.
fn pair(game: &Game, met: &HashSet<(String, String)>) -> (Vec<Group>, Vec<String>) {
    let people: Vec<Player> = game
        .players()
        .filter(|p| !p.answers.is_empty() && !p.away)
        .collect();
    let mut candidates: Vec<(i32, usize, usize, u32)> = Vec::new();
    for i in 0..people.len() {
        for j in i + 1..people.len() {
            let (a, b) = (people[i], people[j]);
            let Some((id, gap)) = game.best_between(a, b) else {
                continue;
            };
            let fresh = !a.marked.contains(&id) && !b.marked.contains(&id);
            let again = met.contains(&(a.key.to_string(), b.key.to_string()));
            let score = gap as i32 + if fresh { 300 } else { 0 } - if again { 10_000 } else { 0 };
            candidates.push((score, i, j, id));
        }
    }
    // Ties broken by the order people joined, so a round is reproducible.
    candidates.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));

    let mut partner: Vec<Option<usize>> = vec![None; people.len()];
    let mut chosen: Vec<(usize, usize, u32)> = Vec::new();
    for &(_, i, j, id) in &candidates {
        if partner[i].is_some() || partner[j].is_some() {
            continue;
        }
        partner[i] = Some(j);
        partner[j] = Some(i);
        chosen.push((i, j, id));
    }
    // Greedy can strand people a different pairing would seat: A and B
    // take each other, while A could have had C and B had D. Swap
    // partners wherever that pairs two more people, until nothing does.
    let best: std::collections::HashMap<(usize, usize), u32> = candidates
        .iter()
        .map(|&(_, i, j, id)| ((i, j), id))
        .collect();
    let edge = |x: usize, y: usize| best.get(&(x.min(y), x.max(y))).copied();
    loop {
        let mut swapped = false;
        'search: for u in 0..people.len() {
            if partner[u].is_some() {
                continue;
            }
            for k in 0..chosen.len() {
                let (a, b, _) = chosen[k];
                for (x, y) in [(a, b), (b, a)] {
                    let Some(ux) = edge(u, x) else { continue };
                    for v in 0..people.len() {
                        if v == u || partner[v].is_some() {
                            continue;
                        }
                        let Some(vy) = edge(v, y) else { continue };
                        chosen[k] = (u.min(x), u.max(x), ux);
                        chosen.push((v.min(y), v.max(y), vy));
                        partner[u] = Some(x);
                        partner[x] = Some(u);
                        partner[v] = Some(y);
                        partner[y] = Some(v);
                        swapped = true;
                        break 'search;
                    }
                }
            }
        }
        if !swapped {
            break;
        }
    }

    let taken: Vec<bool> = partner.iter().map(|p| p.is_some()).collect();
    let mut groups: Vec<Group> = Vec::new();
    for (i, j, id) in chosen {
        let (a, b) = (people[i], people[j]);
        groups.push(Group {
            members: vec![a.key.to_string(), b.key.to_string()],
            statement: id,
            before: BTreeMap::from([
                (a.key.to_string(), a.answers[&id]),
                (b.key.to_string(), b.answers[&id]),
            ]),
            after: BTreeMap::new(),
            guesses: BTreeMap::new(),
            kudos: BTreeMap::new(),
        });
    }

    let mut waiting = Vec::new();
    for (i, p) in people.iter().enumerate() {
        if taken[i] {
            continue;
        }
        // Into the pair whose statement they are furthest from, where
        // they have answered it; three people still make a conversation.
        let best = groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.members.len() == 2)
            .filter_map(|(gi, g)| {
                let mine = *p.answers.get(&g.statement)?;
                let s = game.statement(g.statement)?;
                let far = g.before.values().map(|&v| s.gap(mine, v)).max()?;
                Some((far, gi))
            })
            .max();
        match best {
            Some((_, gi)) => {
                let g = &mut groups[gi];
                g.before.insert(p.key.to_string(), p.answers[&g.statement]);
                g.members.push(p.key.to_string());
            }
            None => waiting.push(p.key.to_string()),
        }
    }
    // People who answered nothing wait too, and are told why.
    waiting.extend(
        game.players()
            .filter(|p| p.answers.is_empty())
            .map(|p| p.key.to_string()),
    );
    (groups, waiting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rooms::tool::Joinable;

    fn room(mode: Mode) -> Game {
        let mut g = Game::new(1);
        g.setup(mode, Scale::Written, &["ea".into(), "mind".into()], "", 3)
            .unwrap();
        g
    }

    #[test]
    fn a_bingo_deck_is_about_twice_a_card_and_cards_cover_it_evenly() {
        let mut g = room(Mode::Bingo);
        assert_eq!(g.deck.len(), 18);
        for n in 0..4 {
            g.join(&format!("P{n}"), 1).unwrap();
        }
        let mut used = vec![0; g.deck.len()];
        for p in g.players() {
            assert_eq!(p.card.len(), 9);
            let distinct: BTreeSet<_> = p.card.iter().collect();
            assert_eq!(distinct.len(), 9, "no square twice");
            for &id in &p.card {
                used[id as usize] += 1;
            }
        }
        assert!(used.iter().all(|&n| n == 2), "{used:?}");
    }

    #[test]
    fn the_open_mode_takes_every_statement() {
        let g = room(Mode::Open);
        let all = topic("ea").unwrap().statements.len() + topic("mind").unwrap().statements.len();
        assert_eq!(g.deck.len(), all);
        assert!(
            g.deck.iter().any(|s| s.chance),
            "probability questions are in"
        );
    }

    #[test]
    fn custom_lines_join_the_deck_and_a_percent_sign_asks_for_a_chance() {
        let mut g = Game::new(1);
        g.setup(
            Mode::Open,
            Scale::Written,
            &[],
            "Cats are better than dogs.\n%Will it rain tomorrow?\n\n",
            4,
        )
        .unwrap();
        assert_eq!(g.deck.len(), 2);
        assert!(!g.deck[0].chance);
        assert!(g.deck[1].chance);
        assert_eq!(g.deck[1].text, "Will it rain tomorrow?");
        assert!(
            g.setup(Mode::Bingo, Scale::Written, &[], "only one", 3)
                .is_err(),
            "too few for a card"
        );
    }

    #[test]
    fn answers_stay_on_the_scale_and_the_first_one_is_kept() {
        let mut g = room(Mode::Open);
        let k = g.join("Ada", 1).unwrap();
        let chance = g.deck.iter().find(|s| s.chance).unwrap().id;
        let agree = g.deck.iter().find(|s| !s.chance).unwrap().id;
        assert!(g.answer(&k, chance, Some(-5)).is_err());
        assert!(g.answer(&k, chance, Some(101)).is_err());
        assert!(g.answer(&k, agree, Some(0)).is_err());
        assert!(g.answer(&k, agree, Some(8)).is_err());
        g.answer(&k, agree, Some(2)).unwrap();
        g.answer(&k, agree, Some(5)).unwrap();
        let p = g.player(&k).unwrap();
        assert_eq!(p.first[&agree], 2);
        assert_eq!(p.answers[&agree], 5);
        assert!(
            g.setup(Mode::Bingo, Scale::Written, &["ea".into()], "", 3)
                .is_err(),
            "locked once answered"
        );
    }

    #[test]
    fn a_round_pairs_the_most_opposed_and_the_odd_one_makes_three() {
        let mut g = room(Mode::Open);
        let s = g.deck.iter().find(|s| !s.chance).unwrap().id;
        let t = g.deck.iter().filter(|s| !s.chance).nth(1).unwrap().id;
        let keys: Vec<String> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|n| g.join(n, 1).unwrap())
            .collect();
        // A and D are opposite on s; B and C on t; E agrees with A on s.
        for (k, (vs, vt)) in keys.iter().zip([(7, 4), (4, 7), (4, 1), (1, 4), (7, 4)]) {
            g.answer(k, s, Some(vs)).unwrap();
            g.answer(k, t, Some(vt)).unwrap();
        }
        g.start_round(1000).unwrap();
        let r = g.rounds.last().unwrap();
        assert_eq!(r.groups.len(), 2);
        let ad = r
            .groups
            .iter()
            .find(|g| g.members.contains(&keys[0]))
            .unwrap();
        assert!(ad.members.contains(&keys[3]), "A talks to D");
        assert_eq!(ad.statement, s);
        let bc = r
            .groups
            .iter()
            .find(|g| g.members.contains(&keys[1]))
            .unwrap();
        assert!(bc.members.contains(&keys[2]), "B talks to C");
        assert_eq!(bc.statement, t);
        assert_eq!(ad.members.len() + bc.members.len(), 5, "E joins a group");
        assert!(r.waiting.is_empty());
        assert_eq!(g.phase, Phase::Talking);
        assert_eq!(g.ends, 1000 + 5 * 60_000);
        assert!(g.player(&keys[0]).unwrap().marked.contains(&s));
    }

    #[test]
    fn the_clock_ends_a_round_and_the_new_answer_is_recorded() {
        use rooms::Room;
        let mut g = room(Mode::Open);
        let s = g.deck.iter().find(|s| !s.chance).unwrap().id;
        let a = g.join("A", 1).unwrap();
        let b = g.join("B", 1).unwrap();
        g.answer(&a, s, Some(7)).unwrap();
        g.answer(&b, s, Some(1)).unwrap();
        g.start_round(0).unwrap();
        assert!(!g.tick(1000));
        assert!(g.tick(g.ends));
        assert_eq!(g.phase, Phase::Updating);
        g.answer(&a, s, Some(5)).unwrap();
        let group = &g.rounds[0].groups[0];
        assert_eq!(group.before[&a], 7);
        assert_eq!(group.after[&a], 5);
    }

    #[test]
    fn people_who_have_talked_are_not_paired_again_while_others_are_free() {
        let mut g = room(Mode::Open);
        let s = g.deck.iter().find(|s| !s.chance).unwrap().id;
        let keys: Vec<String> = ["A", "B", "C", "D"]
            .iter()
            .map(|n| g.join(n, 1).unwrap())
            .collect();
        for (k, v) in keys.iter().zip([7, 1, 6, 2]) {
            g.answer(k, s, Some(v)).unwrap();
        }
        g.start_round(0).unwrap();
        let first: Vec<Vec<String>> = g.rounds[0]
            .groups
            .iter()
            .map(|g| g.members.clone())
            .collect();
        g.stop().unwrap();
        g.start_round(1).unwrap();
        for group in &g.rounds[1].groups {
            assert!(
                !first.contains(&group.members),
                "{:?} met twice",
                group.members
            );
        }
    }

    #[test]
    fn nobody_is_stranded_when_a_different_pairing_seats_everyone() {
        let mut g = room(Mode::Open);
        let ids: Vec<u32> = g
            .deck
            .iter()
            .filter(|s| !s.chance)
            .take(3)
            .map(|s| s.id)
            .collect();
        let keys: Vec<String> = ["A", "B", "C", "D"]
            .iter()
            .map(|n| g.join(n, 1).unwrap())
            .collect();
        // A and B are furthest apart, on s0; C shares s1 only with A, and
        // D shares s2 only with B. Greedy alone would leave C and D out.
        g.answer(&keys[0], ids[0], Some(7)).unwrap();
        g.answer(&keys[1], ids[0], Some(1)).unwrap();
        g.answer(&keys[0], ids[1], Some(4)).unwrap();
        g.answer(&keys[2], ids[1], Some(5)).unwrap();
        g.answer(&keys[1], ids[2], Some(4)).unwrap();
        g.answer(&keys[3], ids[2], Some(5)).unwrap();
        g.start_round(0).unwrap();
        let r = &g.rounds[0];
        assert!(r.waiting.is_empty(), "{:?}", r.waiting);
        assert_eq!(r.groups.len(), 2);
    }

    /// A and B on one probability statement, A at 80% and B at 20%, after
    /// a round.
    fn talked() -> (Game, String, String, u32) {
        let mut g = Game::new(1);
        g.setup(Mode::Open, Scale::Chance, &["mind".into()], "", 4)
            .unwrap();
        let s = g.deck[0].id;
        let a = g.join("A", 1).unwrap();
        let b = g.join("B", 1).unwrap();
        g.answer(&a, s, Some(80)).unwrap();
        g.answer(&b, s, Some(20)).unwrap();
        g.start_round(0).unwrap();
        g.stop().unwrap();
        (g, a, b, s)
    }

    #[test]
    fn closing_a_gap_scores_the_same_for_both_whoever_moved() {
        let (mut g, a, b, s) = talked();
        // 60 points apart, 120 on the shared scale. B comes to 50: 30
        // apart, 60 on the scale, so 60 closed, for both of them.
        g.answer(&b, s, Some(50)).unwrap();
        let p = g.scores();
        assert_eq!(p[&a].together, 60);
        assert_eq!(p[&b].together, 60);
        assert_eq!(p[&a].persuaded, 60, "still counted, as an award");
        assert_eq!(p[&b].opened, 60);
        assert_eq!(p[&a].total, 60, "persuading is not in the total");
    }

    #[test]
    fn closing_more_than_the_cap_scores_the_cap() {
        let (mut g, a, b, s) = talked();
        // 120 apart on the shared scale. Both come to 50: 120 closed, but
        // one conversation is worth at most one perfect guess.
        g.answer(&a, s, Some(50)).unwrap();
        g.answer(&b, s, Some(50)).unwrap();
        let p = g.scores();
        assert_eq!(p[&a].together, CLOSE_CAP);
        assert_eq!(p[&b].together, CLOSE_CAP);
        assert_eq!(p[&a].total, CLOSE_CAP);
        assert_eq!(p[&a].best_closed, 120, "the award says what was closed");
    }

    #[test]
    fn moving_apart_closes_nothing() {
        let (mut g, a, b, s) = talked();
        g.answer(&a, s, Some(90)).unwrap();
        g.answer(&b, s, Some(10)).unwrap();
        let p = g.scores();
        assert_eq!(p[&a].together, 0);
        assert_eq!(p[&b].persuaded, 0);
    }

    #[test]
    fn a_good_guess_at_your_partner_scores_and_is_final() {
        let (mut g, a, b, s) = talked();
        g.guess(&a, "B", 40).unwrap();
        assert!(g.guess(&a, "B", 50).is_err(), "no second guess");
        assert!(g.guess(&a, "A", 50).is_err(), "not at yourself");
        assert!(g.guess(&a, "B", 101).is_err(), "on the scale");
        g.answer(&b, s, Some(50)).unwrap();
        // 10 percentage points off is 20 on the shared scale: 100 - 4.
        assert_eq!(g.scores()[&a].listening, 96);
        assert_eq!(guess_points(g.statement(s).unwrap(), 0, 100), 0);
    }

    #[test]
    fn a_guess_at_somebody_who_never_answered_scores_nothing() {
        let (mut g, a, _, _) = talked();
        g.guess(&a, "B", 20).unwrap();
        assert_eq!(g.scores()[&a].listening, 0);
    }

    #[test]
    fn learning_something_from_a_partner_is_worth_points_to_them() {
        let (mut g, a, b, _) = talked();
        g.kudos(&a, "B", true).unwrap();
        g.kudos(&a, "B", true).unwrap();
        assert_eq!(g.scores()[&b].kudos, 1, "once per partner per round");
        assert_eq!(g.scores()[&b].total, KUDOS_POINTS);
        g.kudos(&a, "B", false).unwrap();
        assert_eq!(g.scores().get(&b).map_or(0, |s| s.kudos), 0);
    }

    #[test]
    fn a_group_is_settled_once_everybody_answered_and_guessed() {
        let (mut g, a, b, s) = talked();
        let settled = |g: &Game| g.rounds[0].groups[0].settled();
        g.answer(&a, s, Some(70)).unwrap();
        g.answer(&b, s, Some(30)).unwrap();
        assert!(!settled(&g));
        g.guess(&a, "B", 25).unwrap();
        g.guess(&b, "A", 75).unwrap();
        assert!(settled(&g));
    }

    #[test]
    fn nothing_can_be_guessed_outside_a_round() {
        let (mut g, a, _, _) = talked();
        g.finish().unwrap();
        assert!(g.guess(&a, "B", 50).is_err());
        assert!(g.kudos(&a, "B", true).is_err());
    }

    #[test]
    fn in_bingo_a_square_that_completes_a_line_is_preferred() {
        let mut g = room(Mode::Bingo);
        let a = g.join("A", 1).unwrap();
        let card = g.player(&a).unwrap().card.clone();
        // Two of the top row marked: the third completes it.
        g.mark(&a, card[0], true).unwrap();
        g.mark(&a, card[1], true).unwrap();
        let me = g.player(&a).unwrap();
        assert_eq!(g.towards_a_line(me, card[2]), LINE_DONE);
        assert_eq!(g.towards_a_line(me, card[0]), 0, "already marked");
        // The middle of the bottom row is in a line with one mark (the
        // middle column has card[1]).
        assert_eq!(g.towards_a_line(me, card[7]), LINE_STEP);
    }

    #[test]
    fn every_group_has_somewhere_to_meet() {
        assert_eq!(spot(0), "🦊 Fox");
        assert_eq!(spot(SPOTS.len()), "🦊 Fox 2");
        let all: BTreeSet<String> = (0..100).map(spot).collect();
        assert_eq!(all.len(), 100, "no two groups share a spot");
    }

    #[test]
    fn a_full_row_of_marks_is_bingo() {
        let mut g = room(Mode::Bingo);
        let k = g.join("A", 1).unwrap();
        let card = g.player(&k).unwrap().card.clone();
        for &id in &card[..2] {
            g.mark(&k, id, true).unwrap();
        }
        assert_eq!(g.lines(g.player(&k).unwrap()), 0);
        g.mark(&k, card[2], true).unwrap();
        assert_eq!(g.lines(g.player(&k).unwrap()), 1);
        g.mark(&k, card[4], true).unwrap();
        g.mark(&k, card[8], true).unwrap();
        assert_eq!(g.lines(g.player(&k).unwrap()), 2, "and the diagonal");
    }

    #[test]
    fn the_room_can_answer_everything_one_way() {
        let mut g = Game::new(1);
        g.setup(
            Mode::Open,
            Scale::Agree,
            &["mind".into()],
            "%What is the chance that it snows?",
            4,
        )
        .unwrap();
        assert!(g.deck.iter().all(|s| !s.chance));
        assert!(
            g.deck
                .iter()
                .any(|s| s.text == "Today's large language models are conscious.")
        );
        assert!(g.deck.iter().any(|s| s.text == "It snows."));
        g.setup(Mode::Open, Scale::Chance, &["mind".into()], "", 4)
            .unwrap();
        assert!(g.deck.iter().all(|s| s.chance));
        assert!(g.deck.iter().any(|s| s.text == "Free will is an illusion."));
    }

    #[test]
    fn every_probability_question_in_the_bank_becomes_a_claim() {
        for t in TOPICS {
            for q in t.statements.iter().filter_map(|s| s.strip_prefix("% ")) {
                let c = claim(q);
                assert!(!c.starts_with("What") && c.ends_with('.'), "{q} -> {c}");
            }
        }
    }

    #[test]
    fn the_free_mode_takes_every_statement_like_the_open_one() {
        let g = room(Mode::Free);
        assert_eq!(g.deck.len(), room(Mode::Open).deck.len());
        let mut g = g;
        let k = g.join("A", 1).unwrap();
        assert_eq!(g.hand(g.player(&k).unwrap()).len(), g.deck.len());
        assert_eq!(g.lines(g.player(&k).unwrap()), 0);
    }

    #[test]
    fn the_feed_leads_with_what_others_answered_and_skips_what_you_have() {
        let mut g = room(Mode::Free);
        let ids: Vec<u32> = g.deck.iter().filter(|s| !s.chance).map(|s| s.id).collect();
        let me = g.join("Me", 1).unwrap();
        let others: Vec<String> = (0..4)
            .map(|i| g.join(&format!("O{i}"), 1).unwrap())
            .collect();
        // Everybody answered ids[5]; two answered ids[7], far apart.
        for k in &others {
            g.answer(k, ids[5], Some(5)).unwrap();
        }
        g.answer(&others[0], ids[7], Some(7)).unwrap();
        g.answer(&others[1], ids[7], Some(1)).unwrap();
        // The split one leads: two people at opposite ends make a better
        // conversation than four who agree.
        let feed = g.feed(g.player(&me).unwrap(), 5);
        assert_eq!(feed[..2], [ids[7], ids[5]]);

        g.answer(&me, ids[5], Some(4)).unwrap();
        g.skip(&me, ids[7]).unwrap();
        let feed = g.feed(g.player(&me).unwrap(), 50);
        assert!(!feed.contains(&ids[5]) && !feed.contains(&ids[7]));
        assert_eq!(
            g.feed(g.player(&me).unwrap(), 5),
            g.feed(g.player(&me).unwrap(), 5),
            "stable"
        );
    }

    #[test]
    fn every_fourth_card_is_one_few_have_answered() {
        let mut g = room(Mode::Free);
        let ids: Vec<u32> = g.deck.iter().filter(|s| !s.chance).map(|s| s.id).collect();
        let me = g.join("Me", 1).unwrap();
        let other = g.join("O", 1).unwrap();
        for &id in &ids[3..] {
            g.answer(&other, id, Some(6)).unwrap();
        }
        for &id in &ids[..3] {
            g.skip(&me, id).unwrap();
        }
        // Three acted on, so this is the fourth card: one nobody answered.
        let first = g.feed(g.player(&me).unwrap(), 1)[0];
        assert!(
            !g.player(&other).unwrap().answers.contains_key(&first),
            "{first}"
        );
    }

    #[test]
    fn both_scales_span_the_same_gap() {
        let s = Statement {
            id: 0,
            text: String::new(),
            topic: String::new(),
            chance: true,
        };
        assert_eq!(s.gap(10, 60), 100);
        assert_eq!(s.gap(0, 100), 200);
        let agree = Statement { chance: false, ..s };
        assert_eq!(agree.gap(1, 7), 200);
        assert_eq!(agree.gap(4, 5), 33);
        assert_eq!(agree.norm(4), 0.0);
    }

    #[test]
    fn in_the_pick_mode_everybody_picks_up_to_three_before_answering() {
        let mut g = room(Mode::Pick);
        assert_eq!(g.phase, Phase::Picking);
        assert_eq!(g.deck.len(), room(Mode::Open).deck.len(), "the whole pool");
        let a = g.join("A", 1).unwrap();
        assert!(
            g.answer(&a, 0, Some(4)).is_err(),
            "no answers while picking"
        );
        for id in 0..3 {
            g.pick(&a, id, true).unwrap();
        }
        g.pick(&a, 2, true).unwrap();
        assert!(g.pick(&a, 3, true).is_err(), "three at most");
        g.pick(&a, 0, false).unwrap();
        g.pick(&a, 3, true).unwrap();
        assert_eq!(g.player(&a).unwrap().picks, BTreeSet::from([1, 2, 3]));
        assert!(g.start_round(0).is_err(), "no round before the picks close");
        assert!(g.start_warmup().is_err());
    }

    #[test]
    fn closing_the_picks_keeps_each_picked_statement_once_most_picked_first() {
        let mut g = room(Mode::Pick);
        let pool: Vec<String> = g.deck.iter().map(|s| s.text.clone()).collect();
        let a = g.join("A", 1).unwrap();
        let b = g.join("B", 1).unwrap();
        let c = g.join("C", 1).unwrap();
        let d = g.join("D", 1).unwrap();
        for id in [9, 4, 7] {
            g.pick(&a, id, true).unwrap();
        }
        for id in [7, 4, 1] {
            g.pick(&b, id, true).unwrap();
        }
        g.pick(&c, 7, true).unwrap();
        assert!(g.close_picks().is_ok());
        assert_eq!(g.phase, Phase::Answering);
        let texts: Vec<&str> = g.deck.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            [&pool[7], &pool[4], &pool[1], &pool[9]],
            "by how many picked it, then as the pool has them"
        );
        assert!(g.deck.iter().enumerate().all(|(i, s)| s.id == i as u32));
        assert_eq!(g.player(&a).unwrap().picks, BTreeSet::from([0, 1, 3]));
        assert_eq!(
            g.hand(g.player(&d).unwrap()).len(),
            4,
            "somebody who picked nothing answers the room's picks"
        );
        g.answer(&d, 0, Some(5)).unwrap();
        assert!(g.pick(&d, 1, true).is_err(), "the picks are closed");
        assert!(g.close_picks().is_err());
    }

    #[test]
    fn the_picks_cannot_close_with_nothing_picked_and_a_new_setup_starts_over() {
        let mut g = room(Mode::Pick);
        let a = g.join("A", 1).unwrap();
        assert!(g.close_picks().is_err(), "nothing to play");
        g.pick(&a, 5, true).unwrap();
        g.setup(Mode::Pick, Scale::Chance, &["mind".into()], "", 3)
            .unwrap();
        assert_eq!(g.phase, Phase::Picking);
        assert!(
            g.player(&a).unwrap().picks.is_empty(),
            "picks of another pool"
        );
        g.setup(Mode::Open, Scale::Chance, &["mind".into()], "", 3)
            .unwrap();
        assert_eq!(g.phase, Phase::Answering, "another mode answers at once");
    }
}
