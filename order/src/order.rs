//! The order itself: where it is from, the menu's link, and what
//! everybody wants.

use rooms::lobby::{self, HasLobby, Lobby, Player};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The longest place name, in characters.
pub const MAX_PLACE: usize = 80;
/// The longest menu link, in characters.
pub const MAX_MENU: usize = 1000;
/// The longest note from the host, in characters.
pub const MAX_NOTE: usize = 280;
/// The longest item, in characters: "Margherita, large, no basil".
pub const MAX_WHAT: usize = 120;
/// The most items one person can add. A guard, not a diet.
pub const MAX_ITEMS_EACH: usize = 20;
/// The most an item can cost, in cents.
pub const MAX_PRICE: u32 = 100_000;

/// One thing somebody wants.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    /// A number for it, so it can be taken off again.
    pub id: u32,
    /// Who wants it, by their player key.
    pub by: String,
    pub what: String,
    /// What it costs, in cents, if they said.
    #[serde(default)]
    pub price: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct Order {
    /// The join key, the wall key and the people.
    #[serde(default)]
    pub lobby: Lobby,
    /// Where the food is from: "Pizzeria Da Mario".
    #[serde(default)]
    pub place: String,
    /// A link to the menu, `https://...`, or nothing.
    #[serde(default)]
    pub menu: String,
    /// Anything else the host wants everybody to know: "ordering at
    /// 19:30, pay Ada in cash".
    #[serde(default)]
    pub note: String,
    /// Whether people can still add and take off items.
    #[serde(default = "yes")]
    pub open: bool,
    /// Everything everybody wants, in the order it was added.
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    next: u32,
    /// Who has paid the host, by player key.
    #[serde(default)]
    pub paid: Vec<String>,
    pub seen: u64,
}

fn yes() -> bool {
    true
}

impl rooms::Room for Order {
    fn seen(&mut self, now: u64) {
        self.seen = now;
    }

    fn expired(&self, now: u64) -> bool {
        self.seen + rooms::DAY_MS < now
    }

    fn keys(&self) -> Vec<&str> {
        self.lobby.keys().collect()
    }

    fn migrate(saved: &mut Value) {
        lobby::migrate(saved);
    }
}

impl HasLobby for Order {
    fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    fn lobby_mut(&mut self) -> &mut Lobby {
        &mut self.lobby
    }

    /// Somebody taken out of the room is out of the order too.
    fn left(&mut self, player: &Player) {
        self.items.retain(|item| item.by != player.key);
        self.paid.retain(|key| key != &player.key);
    }

    /// Somebody who has ordered is still in it, phone in a pocket or not.
    fn recent(&self, player: &Player) -> bool {
        self.items.iter().any(|item| item.by == player.key)
    }

    /// The projector shows where from, the menu, and how far along the
    /// order is. Not who wants what: that is between them and the host.
    fn wall_game(&self, _now: u64) -> Value {
        serde_json::json!({
            "place": self.place,
            "menu": self.menu,
            "note": self.note,
            "open": self.open,
            "ordered": self.ordered(),
            "items": self.items.len(),
        })
    }
}

/// A menu link as typed, made into one that can be opened: `https://` is
/// added when it was left off, and anything that is not a web link is
/// refused, so that the page never links to `javascript:` or a file.
pub fn menu_link(typed: &str) -> Result<String, &'static str> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Ok(String::new());
    }
    if typed.chars().count() > MAX_MENU {
        return Err("That link is too long.");
    }
    if typed.chars().any(char::is_whitespace) {
        return Err("A link has no spaces in it. Paste the menu's address.");
    }
    let lower = typed.to_ascii_lowercase();
    let link = if lower.starts_with("https://") || lower.starts_with("http://") {
        typed.to_string()
    } else if lower.contains("://")
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
    {
        return Err("The menu has to be a web link, starting https://.");
    } else {
        format!("https://{typed}")
    };
    let host = link
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if host.is_empty() || (!host.contains('.') && !host.starts_with("localhost")) {
        return Err("That does not look like a link. Paste the menu's address.");
    }
    Ok(link)
}

/// A price in euros, as a phone sends it, in cents.
pub fn cents(euros: Option<f64>) -> Result<Option<u32>, &'static str> {
    match euros {
        None => Ok(None),
        Some(e) if !e.is_finite() || e < 0.0 => Err("A price is a number of euros, like 9.50."),
        Some(e) => {
            let c = (e * 100.0).round();
            if c > MAX_PRICE as f64 {
                Err("That is a lot for one item. Check the price.")
            } else {
                Ok(Some(c as u32))
            }
        }
    }
}

impl Order {
    pub fn new(now: u64) -> Order {
        Order {
            lobby: Lobby::new(),
            place: String::new(),
            menu: String::new(),
            note: String::new(),
            open: true,
            items: Vec::new(),
            next: 1,
            paid: Vec::new(),
            seen: now,
        }
    }

    /// The host says where the food is from.
    pub fn describe(&mut self, place: &str, menu: &str, note: &str) -> Result<(), &'static str> {
        let menu = menu_link(menu)?;
        self.place = lobby::tidy(place, MAX_PLACE);
        self.menu = menu;
        // Line breaks are kept in the note: "Pay Ada.\nCash only."
        self.note = note.trim().chars().take(MAX_NOTE).collect();
        Ok(())
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// Somebody adds something they want.
    pub fn add(&mut self, by: &str, what: &str, price: Option<f64>) -> Result<(), &'static str> {
        if !self.open {
            return Err("The order has gone in. Ask the host to open it again.");
        }
        let what = lobby::tidy(what, MAX_WHAT);
        if what.is_empty() {
            return Err("Say what you would like first.");
        }
        let price = cents(price)?;
        if self.items.iter().filter(|item| item.by == by).count() >= MAX_ITEMS_EACH {
            return Err("That is as many items as one person can add.");
        }
        let id = self.next.max(1);
        self.next = id + 1;
        self.items.push(Item {
            id,
            by: by.to_string(),
            what,
            price,
        });
        Ok(())
    }

    /// Somebody takes one of their own items off again.
    pub fn take_off(&mut self, by: &str, id: u32) -> Result<(), &'static str> {
        if !self.open {
            return Err("The order has gone in. Ask the host to open it again.");
        }
        let at = self
            .items
            .iter()
            .position(|item| item.id == id && item.by == by)
            .ok_or("That is not on your order any more.")?;
        self.items.remove(at);
        Ok(())
    }

    /// The host takes an item off anybody's order, open or not: the place
    /// is out of it, or it went on twice.
    pub fn strike(&mut self, id: u32) -> Result<(), &'static str> {
        let at = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or("That item is not on the order any more.")?;
        self.items.remove(at);
        Ok(())
    }

    /// The host marks somebody, by their lobby id, as having paid.
    pub fn set_paid(&mut self, id: u32, paid: bool) -> Result<(), &'static str> {
        let key = self
            .lobby
            .by_id(id)
            .map(|p| p.key.clone())
            .ok_or("That person is not in the room any more.")?;
        self.paid.retain(|k| k != &key);
        if paid {
            self.paid.push(key);
        }
        Ok(())
    }

    /// Everything `key` wants.
    pub fn mine<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Item> + 'a {
        self.items.iter().filter(move |item| item.by == key)
    }

    /// How many people have at least one item in.
    pub fn ordered(&self) -> usize {
        self.lobby
            .players
            .iter()
            .filter(|p| self.items.iter().any(|item| item.by == p.key))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> (Order, String, String) {
        let mut order = Order::new(0);
        let ada = lobby::admit(&mut order, "Ada", None, 0).unwrap();
        let bo = lobby::admit(&mut order, "Bo", None, 0).unwrap();
        (order, ada, bo)
    }

    #[test]
    fn a_menu_link_without_its_scheme_is_given_one() {
        assert_eq!(
            menu_link("pizza.example/menu").unwrap(),
            "https://pizza.example/menu"
        );
        assert_eq!(
            menu_link(" http://x.de/a?b=1 ").unwrap(),
            "http://x.de/a?b=1"
        );
        assert_eq!(menu_link("").unwrap(), "");
    }

    #[test]
    fn a_menu_link_that_is_not_a_web_link_is_refused() {
        for bad in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,hi",
            "file:///etc/passwd",
            "ftp://x.de",
            "two words",
            "nodot",
            "https://",
        ] {
            assert!(menu_link(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn prices_are_kept_in_cents() {
        assert_eq!(cents(Some(9.5)), Ok(Some(950)));
        assert_eq!(cents(Some(0.1 + 0.2)), Ok(Some(30)));
        assert_eq!(cents(None), Ok(None));
        assert!(cents(Some(-1.0)).is_err());
        assert!(cents(Some(f64::NAN)).is_err());
        assert!(cents(Some(5000.0)).is_err());
    }

    #[test]
    fn people_add_and_take_off_their_own_items_only() {
        let (mut order, ada, bo) = room();
        order.add(&ada, "  Margherita   large ", Some(9.5)).unwrap();
        order.add(&bo, "Diavola", None).unwrap();
        assert_eq!(order.items[0].what, "Margherita large");
        let adas = order.items[0].id;
        assert!(order.take_off(&bo, adas).is_err(), "not Bo's to take off");
        order.take_off(&ada, adas).unwrap();
        assert_eq!(order.items.len(), 1);
        assert!(order.add(&ada, "   ", None).is_err());
    }

    #[test]
    fn a_closed_order_takes_no_changes_but_the_host_can_still_strike() {
        let (mut order, ada, _) = room();
        order.add(&ada, "Calzone", None).unwrap();
        let id = order.items[0].id;
        order.set_open(false);
        assert!(order.add(&ada, "Tiramisu", None).is_err());
        assert!(order.take_off(&ada, id).is_err());
        order.strike(id).unwrap();
        assert!(order.items.is_empty());
    }

    #[test]
    fn somebody_taken_out_leaves_the_order_too() {
        let (mut order, ada, bo) = room();
        order.add(&ada, "Calzone", None).unwrap();
        order.add(&bo, "Funghi", None).unwrap();
        let id = order.lobby.player(&ada).unwrap().id;
        order.set_paid(id, true).unwrap();
        lobby::remove_key(&mut order, &ada);
        assert_eq!(order.items.len(), 1);
        assert!(order.paid.is_empty());
        assert_eq!(order.ordered(), 1);
    }

    #[test]
    fn the_host_describes_the_place_and_a_bad_link_changes_nothing() {
        let mut order = Order::new(0);
        order
            .describe("Da Mario", "damario.example/menu", "At 19:30")
            .unwrap();
        assert!(order.describe("Elsewhere", "javascript:x", "").is_err());
        assert_eq!(order.place, "Da Mario");
        assert_eq!(order.menu, "https://damario.example/menu");
    }

    #[test]
    fn an_order_saved_before_it_had_some_fields_still_loads() {
        let saved = serde_json::json!({
            "lobby": serde_json::to_value(Lobby::new()).unwrap(),
            "seen": 5,
        });
        let order: Order = serde_json::from_value(saved).unwrap();
        assert!(order.open);
        assert!(order.items.is_empty());
    }
}
