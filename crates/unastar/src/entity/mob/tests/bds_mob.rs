//! The structured mob fixtures `tools/bds-sweep/mob_fixture.py` distils from
//! `sweep.py spider` (and `sweep.py enderman`) runs, as the replays read
//! them. Fields the replays do not read yet are skipped.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct Fixture {
    pub items: Vec<Item>,
}

/// One sweep item: what BDS showed.
#[derive(Debug, Deserialize)]
pub(super) struct Item {
    /// `<label>/<item>`, the label naming the run (its difficulty).
    pub key: String,
    /// Where the mob spawned: x, y, z and yaw.
    pub at: [f64; 4],
    /// `(t, x, y, z, on_ground)` on every tick the mob moved.
    pub moves: Vec<(u32, f64, f64, f64, bool)>,
    pub events: Vec<Event>,
}

/// An observed packet about the mob or the player, on the tick of the
/// sample whose mark came before it.
#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(super) enum Event {
    /// The mob's `TARGET` actor data was set or cleared.
    Target { t: u32, set: bool },
    /// One of the mob's flag bits changed.
    Flag { t: u32, bit: u32, set: bool },
    /// A `MobEffect` about the player.
    Effect {
        effect: i32,
        op: EffectOp,
        duration: i32,
    },
    /// Every other observation (sounds, actor events, blocks, items, ...).
    #[serde(other)]
    Other,
}

/// What a `MobEffect` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) enum EffectOp {
    Add,
    Update,
    Remove,
}

impl Fixture {
    pub fn parse(text: &str) -> Self {
        serde_json::from_str(text).expect("mob fixture decodes")
    }

    pub fn item(&self, key: &str) -> &Item {
        self.items
            .iter()
            .find(|item| item.key == key)
            .unwrap_or_else(|| panic!("no fixture item {key}"))
    }

    /// The items whose key starts with `prefix`.
    pub fn items<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a Item> + 'a {
        self.items
            .iter()
            .filter(move |item| item.key.starts_with(prefix))
    }
}

impl Item {
    /// The mob's feet at sample `t` (the last move at or before it).
    pub fn feet(&self, t: u32) -> [f64; 3] {
        let (_, x, y, z, _) = *self
            .moves
            .iter()
            .take_while(|m| m.0 <= t)
            .last()
            .expect("a move at t = 0");
        [x, y, z]
    }

    /// Ticks at which flag `bit` was set (`true`) or cleared.
    pub fn flag_changes(&self, bit: u32) -> Vec<(u32, bool)> {
        self.events
            .iter()
            .filter_map(|e| match *e {
                Event::Flag { t, bit: b, set } if b == bit => Some((t, set)),
                _ => None,
            })
            .collect()
    }

    /// Ticks at which the target was set or cleared (after the spawn's own
    /// cleared target).
    pub fn target_changes(&self) -> Vec<(u32, bool)> {
        self.events
            .iter()
            .filter_map(|e| match *e {
                Event::Target { t, set } if t > 0 || set => Some((t, set)),
                _ => None,
            })
            .collect()
    }
}
