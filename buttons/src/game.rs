//! Everybody presses red or blue.
//!
//! If more than a share the host sets press blue, everybody who pressed a
//! button lives. If not, only the ones who pressed red do. Red is safe
//! whatever the room does; blue is safe only if enough of the room
//! presses it too.

use std::collections::HashMap;

use rooms::lobby::{HasLobby, Lobby, OnWall};
use serde::{Deserialize, Serialize};

/// Somebody in the room. The lobby keeps who is in.
pub use rooms::lobby::Player;

pub const MAX_ROUNDS: usize = 200;

/// The two buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colour {
    Red,
    Blue,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Press {
    pub colour: Colour,
    pub at: u64,
    /// Who pressed it, as they were called then. A round that has come
    /// out keeps the press of somebody the host later took out, under
    /// this name, so taking somebody out does not change the count.
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct Round {
    /// The share of the presses blue has to beat, in percent.
    pub percent: u32,
    pub open: bool,
    /// Until this is true, nobody is sent anybody's press, including the
    /// host, whose screen is often the one on the wall.
    pub revealed: bool,
    pub asked: u64,
    /// By player key.
    pub presses: HashMap<String, Press>,
}

#[derive(Serialize, Deserialize)]
pub struct Game {
    /// Who is in, and the join and wall keys.
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
}

impl HasLobby for Game {
    fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    fn lobby_mut(&mut self) -> &mut Lobby {
        &mut self.lobby
    }

    /// Somebody taken out stops counting in the round still being
    /// played. Rounds that have come out keep their press, so the count
    /// already shown stays as it was.
    fn left(&mut self, player: &Player) {
        if let Some(round) = self.rounds.last_mut().filter(|round| !round.revealed) {
            round.presses.remove(&player.key);
        }
    }

    /// Pressed in the round being played or the one before. Somebody who
    /// has not, and whose phone is not connected, is offered to the host
    /// as away.
    fn recent(&self, player: &Player) -> bool {
        self.rounds
            .iter()
            .rev()
            .take(2)
            .any(|round| round.presses.contains_key(&player.key))
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

    /// Starts a round in which blue has to get more than `percent`% of
    /// the presses. The round before it stops taking presses.
    pub fn start(&mut self, percent: u32, now: u64) -> Result<(), &'static str> {
        // At 100, blue could never be more than everybody.
        if percent >= 100 {
            return Err("Blue has to be able to get more than the share: pick less than 100%.");
        }
        if self.rounds.len() >= MAX_ROUNDS {
            return Err("That is as many rounds as one room holds.");
        }
        if let Some(round) = self.rounds.last_mut() {
            round.open = false;
        }
        self.rounds.push(Round {
            percent,
            open: true,
            revealed: false,
            asked: now,
            presses: HashMap::new(),
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

    /// Shows everybody the split and who lived, and stops taking presses.
    pub fn reveal(&mut self) -> Result<(), &'static str> {
        let round = self.round_mut()?;
        round.open = false;
        round.revealed = true;
        Ok(())
    }

    /// Starts the round just revealed over again with the same share, so
    /// a room can see what it does once it knows how the last one went.
    pub fn again(&mut self, now: u64) -> Result<(), &'static str> {
        let round = self.round().ok_or("No round has been played yet.")?;
        if !round.revealed {
            return Err("Reveal this round first.");
        }
        self.start(round.percent, now)
    }

    /// Presses a button for a player. Pressing the other one afterwards
    /// replaces it, until the round closes.
    pub fn press(&mut self, player: &str, colour: Colour, now: u64) -> Result<(), &'static str> {
        let name = self
            .player(player)
            .ok_or("This link does not belong to anyone in the room.")?
            .name
            .clone();
        let round = self.round_mut()?;
        if !round.open {
            return Err("This round is closed.");
        }
        round.presses.insert(
            player.to_string(),
            Press {
                colour,
                at: now,
                name,
            },
        );
        Ok(())
    }
}

/// What one player pressed, with their name. Only built for a revealed
/// round.
#[derive(Serialize)]
pub struct Pressed {
    pub name: String,
    pub colour: Colour,
    pub lived: bool,
}

/// The result of a round.
#[derive(Serialize)]
pub struct Outcome {
    pub percent: u32,
    /// How many pressed a button. Nobody else is counted.
    pub n: usize,
    pub blue: usize,
    /// Whether blue got more than its share. `None` when nobody pressed.
    pub blue_won: Option<bool>,
    /// Every press: blue first, then in the order people joined.
    pub presses: Vec<Pressed>,
}

impl Round {
    /// How many pressed blue.
    pub fn blue(&self) -> usize {
        self.presses
            .values()
            .filter(|press| press.colour == Colour::Blue)
            .count()
    }

    /// Whether blue got more than its share, or `None` when nobody
    /// pressed. Counted in whole presses, so 50% of four is two exactly
    /// and three is more.
    pub fn blue_won(&self) -> Option<bool> {
        let n = self.presses.len();
        (n > 0).then(|| self.blue() * 100 > self.percent as usize * n)
    }

    /// Whether somebody who pressed `colour` lives. Red always does.
    pub fn lives(&self, colour: Colour) -> bool {
        colour == Colour::Red || self.blue_won() == Some(true)
    }

    /// Every press with who made it: the players in the order they
    /// joined, then anybody taken out since, under the name they pressed
    /// with.
    fn named<'a>(&'a self, players: &'a [Player]) -> Vec<(&'a str, &'a Press)> {
        let mut named: Vec<(&str, &Press)> = players
            .iter()
            .filter_map(|p| {
                self.presses
                    .get(&p.key)
                    .map(|press| (p.name.as_str(), press))
            })
            .collect();
        let mut gone: Vec<&Press> = self
            .presses
            .iter()
            .filter(|(key, _)| !players.iter().any(|p| &p.key == *key))
            .map(|(_, press)| press)
            .collect();
        gone.sort_by_key(|press| press.at);
        named.extend(gone.into_iter().map(|press| (press.name.as_str(), press)));
        named
    }

    /// The round's split, or `None` while it is still being played.
    pub fn outcome(&self, players: &[Player]) -> Option<Outcome> {
        if !self.revealed {
            return None;
        }
        let mut presses: Vec<Pressed> = self
            .named(players)
            .into_iter()
            .map(|(name, press)| Pressed {
                name: name.to_string(),
                colour: press.colour,
                lived: self.lives(press.colour),
            })
            .collect();
        presses.sort_by_key(|pressed| pressed.colour == Colour::Red);
        Some(Outcome {
            percent: self.percent,
            n: self.presses.len(),
            blue: self.blue(),
            blue_won: self.blue_won(),
            presses,
        })
    }
}

/// One player's results over the evening.
#[derive(Serialize)]
pub struct Score {
    pub name: String,
    /// The rounds they pressed a button in.
    pub played: usize,
    /// Of those, the ones they lived through.
    pub lived: usize,
    /// How many times they pressed blue.
    pub blue: usize,
}

impl Game {
    /// The evening so far, over the revealed rounds only: lived through
    /// the most rounds first. A tally that included the round being
    /// played would give away how it is going.
    pub fn scores(&self) -> Vec<Score> {
        let revealed: Vec<&Round> = self.rounds.iter().filter(|round| round.revealed).collect();
        let mut scores: Vec<Score> = self
            .players()
            .iter()
            .map(|player| {
                let pressed: Vec<(&Round, Colour)> = revealed
                    .iter()
                    .filter_map(|round| Some((*round, round.presses.get(&player.key)?.colour)))
                    .collect();
                Score {
                    name: player.name.clone(),
                    played: pressed.len(),
                    lived: pressed
                        .iter()
                        .filter(|(round, colour)| round.lives(*colour))
                        .count(),
                    blue: pressed
                        .iter()
                        .filter(|(_, colour)| *colour == Colour::Blue)
                        .count(),
                }
            })
            .filter(|score| score.played > 0)
            .collect();
        scores.sort_by(|a, b| b.lived.cmp(&a.lived).then(a.name.cmp(&b.name)));
        scores
    }
}

#[cfg(test)]
mod tests {
    use super::Colour::{Blue, Red};
    use super::*;
    use rooms::lobby::admit;

    /// A room in which Ada, Bo, Cy and so on press `presses` in turn.
    fn room(percent: u32, presses: &[Colour]) -> Game {
        let mut game = Game::new(1000);
        game.start(percent, 1000).unwrap();
        for (i, colour) in presses.iter().enumerate() {
            let name = ["Ada", "Bo", "Cy", "Di", "Eve", "Fay"][i];
            let key = admit(&mut game, name, None, 1000).unwrap();
            game.press(&key, *colour, 1001).unwrap();
        }
        game
    }

    fn outcome(game: &Game) -> Outcome {
        game.round()
            .unwrap()
            .outcome(game.players())
            .expect("the round came out")
    }

    fn played(percent: u32, presses: &[Colour]) -> Outcome {
        let mut game = room(percent, presses);
        game.reveal().unwrap();
        outcome(&game)
    }

    fn lived(outcome: &Outcome) -> Vec<&str> {
        let mut names: Vec<&str> = outcome
            .presses
            .iter()
            .filter(|pressed| pressed.lived)
            .map(|pressed| pressed.name.as_str())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn more_blue_than_the_share_and_everybody_lives() {
        let out = played(50, &[Blue, Blue, Red]);
        assert_eq!(lived(&out), ["Ada", "Bo", "Cy"]);
        assert_eq!((out.n, out.blue, out.blue_won), (3, 2, Some(true)));
    }

    #[test]
    fn not_more_blue_than_the_share_and_only_red_lives() {
        let out = played(50, &[Blue, Blue, Red, Red]);
        assert_eq!(lived(&out), ["Cy", "Di"], "half is not more than half");
        assert_eq!(out.blue_won, Some(false));
        assert_eq!(lived(&played(50, &[Blue, Red, Red])), ["Bo", "Cy"]);
    }

    #[test]
    fn the_host_sets_the_share_blue_has_to_beat() {
        let three_of_four = [Blue, Blue, Blue, Red];
        assert_eq!(lived(&played(70, &three_of_four)).len(), 4, "75% beats 70%");
        assert_eq!(
            lived(&played(75, &three_of_four)),
            ["Di"],
            "75% does not beat 75%"
        );
        assert_eq!(
            lived(&played(0, &[Blue, Red])).len(),
            2,
            "one blue beats nought"
        );
        assert_eq!(
            lived(&played(0, &[Red, Red])).len(),
            2,
            "red lives either way"
        );
        let mut game = Game::new(1000);
        assert!(game.start(100, 1000).is_err(), "nothing is more than all");
        assert!(game.start(99, 1000).is_ok());
    }

    #[test]
    fn whoever_pressed_nothing_is_not_counted() {
        let mut game = room(50, &[Blue, Red, Blue]);
        admit(&mut game, "Di", None, 1000).unwrap();
        game.reveal().unwrap();
        let out = outcome(&game);
        assert_eq!((out.n, out.blue), (3, 2), "2 of 3, not 2 of 4");
        assert_eq!(lived(&out), ["Ada", "Bo", "Cy"], "and Di is not among them");
        assert!(game.scores().iter().all(|score| score.name != "Di"));
    }

    #[test]
    fn a_press_can_be_changed_until_the_round_closes() {
        let mut game = room(50, &[Blue]);
        let ada = game.players()[0].key.clone();
        game.press(&ada, Red, 1002).unwrap();
        assert_eq!(game.round().unwrap().blue(), 0);
        game.set_open(false).unwrap();
        assert!(game.press(&ada, Blue, 1003).is_err());
        assert!(game.press("nobody", Blue, 1003).is_err());
    }

    #[test]
    fn a_round_nobody_pressed_in_has_no_side_that_won() {
        let out = played(50, &[]);
        assert_eq!((out.n, out.blue, out.blue_won), (0, 0, None));
        assert!(out.presses.is_empty());
    }

    #[test]
    fn the_same_share_again_only_after_the_reveal() {
        let mut game = room(40, &[Blue]);
        assert!(game.again(2000).is_err(), "not over unseen presses");
        game.reveal().unwrap();
        game.again(2000).unwrap();
        let round = game.round().unwrap();
        assert_eq!(round.percent, 40);
        assert!(round.open && round.presses.is_empty());
    }

    #[test]
    fn the_evening_counts_the_rounds_each_person_lived_through() {
        let mut game = room(50, &[Blue, Red, Red]);
        game.reveal().unwrap();
        let scores = game.scores();
        let ada = scores.iter().find(|score| score.name == "Ada").unwrap();
        assert_eq!((ada.played, ada.lived, ada.blue), (1, 0, 1));
        assert_eq!(scores[0].name, "Bo", "the living first, then by name");
        assert_eq!((scores[0].lived, scores[0].blue), (1, 0));
    }

    #[test]
    fn somebody_taken_out_keeps_their_press_in_a_round_that_came_out() {
        let mut game = room(50, &[Blue, Blue, Red]);
        game.reveal().unwrap();
        let id = game.players()[0].id;
        rooms::lobby::remove(&mut game, id).unwrap();
        let out = outcome(&game);
        assert_eq!((out.n, out.blue_won), (3, Some(true)));
        assert!(out.presses.iter().any(|pressed| pressed.name == "Ada"));
    }
}
