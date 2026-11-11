//! What each screen is sent. Nobody is sent a player key but their own.

use rooms::lobby::{self, Present};
use serde::Serialize;

use crate::order::Order;

#[derive(Serialize)]
pub struct Place<'a> {
    place: &'a str,
    menu: &'a str,
    note: &'a str,
    open: bool,
}

fn place(order: &Order) -> Place<'_> {
    Place {
        place: &order.place,
        menu: &order.menu,
        note: &order.note,
        open: order.open,
    }
}

#[derive(Serialize)]
pub struct Line<'a> {
    id: u32,
    what: &'a str,
    /// In cents.
    price: Option<u32>,
}

/// One person's part of the order.
#[derive(Serialize)]
pub struct Share<'a> {
    /// Their lobby id, which the host's Paid button sends.
    id: u32,
    name: &'a str,
    items: Vec<Line<'a>>,
    /// What their priced items come to, in cents.
    total: u32,
    /// Whether any of their items has no price, so the total is short.
    unpriced: bool,
    paid: bool,
}

/// The same thing wanted by several people, for reading the order out.
#[derive(Serialize)]
pub struct Tally<'a> {
    what: &'a str,
    count: usize,
    names: Vec<&'a str>,
}

#[derive(Serialize)]
pub struct HostView<'a> {
    join: &'a str,
    wall: &'a str,
    people: Vec<Present<'a>>,
    away: Vec<u32>,
    #[serde(flatten)]
    place: Place<'a>,
    /// Everybody who has ordered, in the order they joined.
    shares: Vec<Share<'a>>,
    /// Everybody who has not ordered anything yet.
    waiting: Vec<&'a str>,
    tally: Vec<Tally<'a>>,
    items: usize,
    total: u32,
    unpriced: usize,
}

#[derive(Serialize)]
pub struct PlayerView<'a> {
    name: &'a str,
    #[serde(flatten)]
    place: Place<'a>,
    mine: Vec<Line<'a>>,
    total: u32,
    paid: bool,
    /// Everybody else's choices, so "the same as Ada" is easy. No keys.
    others: Vec<Share<'a>>,
    people: usize,
    ordered: usize,
}

fn line(item: &crate::order::Item) -> Line<'_> {
    Line {
        id: item.id,
        what: &item.what,
        price: item.price,
    }
}

/// Everybody's part, in the order they joined, leaving out who has not
/// ordered and, when given, one person.
fn shares<'a>(order: &'a Order, except: Option<&str>) -> Vec<Share<'a>> {
    order
        .lobby
        .players
        .iter()
        .filter(|p| Some(p.key.as_str()) != except)
        .filter_map(|p| {
            let items: Vec<Line> = order.mine(&p.key).map(line).collect();
            if items.is_empty() {
                return None;
            }
            Some(Share {
                id: p.id,
                name: &p.name,
                total: items.iter().filter_map(|i| i.price).sum(),
                unpriced: items.iter().any(|i| i.price.is_none()),
                paid: order.paid.contains(&p.key),
                items,
            })
        })
        .collect()
}

/// The order as the place needs it: each different thing once, with how
/// many. Things that differ only in case or spacing are the same thing.
fn tally(order: &Order) -> Vec<Tally<'_>> {
    let mut out: Vec<(String, Tally)> = Vec::new();
    for item in &order.items {
        let name = order.lobby.player(&item.by).map_or("", |p| p.name.as_str());
        let same = item.what.to_lowercase();
        match out.iter_mut().find(|(k, _)| *k == same) {
            Some((_, t)) => {
                t.count += 1;
                t.names.push(name);
            }
            None => out.push((
                same,
                Tally {
                    what: &item.what,
                    count: 1,
                    names: vec![name],
                },
            )),
        }
    }
    out.into_iter().map(|(_, t)| t).collect()
}

pub fn host(order: &Order, now: u64) -> HostView<'_> {
    let shares = shares(order, None);
    HostView {
        join: &order.lobby.join,
        wall: &order.lobby.wall,
        people: order.lobby.people(now, true),
        away: lobby::away_ids(order, now),
        place: place(order),
        waiting: order
            .lobby
            .players
            .iter()
            .filter(|p| order.mine(&p.key).next().is_none())
            .map(|p| p.name.as_str())
            .collect(),
        tally: tally(order),
        items: order.items.len(),
        total: order.items.iter().filter_map(|i| i.price).sum(),
        unpriced: order.items.iter().filter(|i| i.price.is_none()).count(),
        shares,
    }
}

pub fn player<'a>(order: &'a Order, key: &'a str) -> Option<PlayerView<'a>> {
    let me = order.lobby.player(key)?;
    let mine: Vec<Line> = order.mine(key).map(line).collect();
    Some(PlayerView {
        name: &me.name,
        place: place(order),
        total: mine.iter().filter_map(|i| i.price).sum(),
        mine,
        paid: order.paid.iter().any(|k| k == key),
        others: shares(order, Some(key)),
        people: order.lobby.len(),
        ordered: order.ordered(),
    })
}
