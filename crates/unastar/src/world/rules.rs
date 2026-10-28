//! World-wide gameplay state shared by every gameplay system: difficulty,
//! game rules and the world clock.
//!
//! These are ECS resources inserted by `GameServer`. Systems read them
//! instead of scanning the StartGame template; the systems in
//! `world::clock` advance the clock and replicate changes to players.

use bevy_ecs::prelude::Resource;

/// Level difficulty, as Bedrock numbers it on the wire.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Difficulty {
    Peaceful = 0,
    #[default]
    Easy = 1,
    Normal = 2,
    Hard = 3,
}

impl Difficulty {
    pub const fn wire(self) -> u32 {
        self as u32
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "peaceful" | "p" | "0" => Self::Peaceful,
            "easy" | "e" | "1" => Self::Easy,
            "normal" | "n" | "2" => Self::Normal,
            "hard" | "h" | "3" => Self::Hard,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Peaceful => "peaceful",
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }
}

/// The game rules gameplay depends on, with Bedrock's defaults.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct GameRules {
    pub keep_inventory: bool,
    pub do_immediate_respawn: bool,
    pub show_death_messages: bool,
    pub natural_regeneration: bool,
    pub do_mob_spawning: bool,
    pub mob_griefing: bool,
    pub do_daylight_cycle: bool,
    pub do_mob_loot: bool,
    pub fall_damage: bool,
    pub fire_damage: bool,
    pub drowning_damage: bool,
}

impl Default for GameRules {
    fn default() -> Self {
        Self {
            keep_inventory: false,
            do_immediate_respawn: false,
            show_death_messages: true,
            natural_regeneration: true,
            do_mob_spawning: true,
            mob_griefing: true,
            do_daylight_cycle: true,
            do_mob_loot: true,
            fall_damage: true,
            fire_damage: true,
            drowning_damage: true,
        }
    }
}

/// The world clock in ticks since the world was created. Time of day is
/// `ticks % 24000`, 0 being sunrise.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorldTime {
    pub ticks: u64,
}

impl WorldTime {
    pub const DAY_TICKS: u64 = 24_000;

    pub const fn time_of_day(self) -> u64 {
        self.ticks % Self::DAY_TICKS
    }
}
