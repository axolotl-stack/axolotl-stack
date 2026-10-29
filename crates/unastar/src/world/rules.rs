//! World-wide gameplay state shared by every gameplay system: difficulty,
//! game rules and the world clock.
//!
//! These are ECS resources inserted by `GameServer`. Systems read them
//! instead of scanning the StartGame template; the systems in
//! `world::clock` advance the clock and replicate changes to players.
//!
//! None of this is persisted: the server has no level-data store (only
//! chunk columns and player records are saved), so the clock, difficulty
//! and game rules start from the configuration on every launch.

use std::sync::Arc;

use bevy_ecs::prelude::Resource;
use jolyne::valentine::StartGamePacket;
use jolyne::valentine::types::{
    EnumsSharedTypesLegacyDifficulty, GameRule as WireGameRule, GameRuleRuleValue,
    GameRulesChangedPacketData,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Level difficulty, as Bedrock numbers it on the wire.
#[derive(
    Resource, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
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

    /// The StartGame level setting.
    pub const fn legacy(self) -> EnumsSharedTypesLegacyDifficulty {
        match self {
            Self::Peaceful => EnumsSharedTypesLegacyDifficulty::Peaceful,
            Self::Easy => EnumsSharedTypesLegacyDifficulty::Easy,
            Self::Normal => EnumsSharedTypesLegacyDifficulty::Normal,
            Self::Hard => EnumsSharedTypesLegacyDifficulty::Hard,
        }
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

/// One of the game rules [`GameRules`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRule {
    DoDaylightCycle,
    DoMobLoot,
    DoMobSpawning,
    DrowningDamage,
    FallDamage,
    FireDamage,
    KeepInventory,
    MobGriefing,
    NaturalRegeneration,
    DoImmediateRespawn,
    ShowDeathMessages,
}

impl GameRule {
    /// Every rule, in the order BDS lists them in `GameRulesChanged`.
    pub const ALL: [Self; 11] = [
        Self::DoDaylightCycle,
        Self::DoMobLoot,
        Self::DoMobSpawning,
        Self::DrowningDamage,
        Self::FallDamage,
        Self::FireDamage,
        Self::KeepInventory,
        Self::MobGriefing,
        Self::NaturalRegeneration,
        Self::DoImmediateRespawn,
        Self::ShowDeathMessages,
    ];

    /// The rule's name as BDS sends it (`GameRulesChanged` at join).
    pub const fn name(self) -> &'static str {
        match self {
            Self::DoDaylightCycle => "doDayLightCycle",
            Self::DoMobLoot => "doMobLoot",
            Self::DoMobSpawning => "doMobSpawning",
            Self::DrowningDamage => "drowningDamage",
            Self::FallDamage => "fallDamage",
            Self::FireDamage => "fireDamage",
            Self::KeepInventory => "keepInventory",
            Self::MobGriefing => "mobGriefing",
            Self::NaturalRegeneration => "naturalRegeneration",
            Self::DoImmediateRespawn => "doImmediateRespawn",
            Self::ShowDeathMessages => "showDeathMessages",
        }
    }

    /// The rule a command names, ignoring case (`keepinventory`).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|rule| rule.name().eq_ignore_ascii_case(name))
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

impl GameRules {
    fn slot(&mut self, rule: GameRule) -> &mut bool {
        match rule {
            GameRule::DoDaylightCycle => &mut self.do_daylight_cycle,
            GameRule::DoMobLoot => &mut self.do_mob_loot,
            GameRule::DoMobSpawning => &mut self.do_mob_spawning,
            GameRule::DrowningDamage => &mut self.drowning_damage,
            GameRule::FallDamage => &mut self.fall_damage,
            GameRule::FireDamage => &mut self.fire_damage,
            GameRule::KeepInventory => &mut self.keep_inventory,
            GameRule::MobGriefing => &mut self.mob_griefing,
            GameRule::NaturalRegeneration => &mut self.natural_regeneration,
            GameRule::DoImmediateRespawn => &mut self.do_immediate_respawn,
            GameRule::ShowDeathMessages => &mut self.show_death_messages,
        }
    }

    pub const fn get(&self, rule: GameRule) -> bool {
        match rule {
            GameRule::DoDaylightCycle => self.do_daylight_cycle,
            GameRule::DoMobLoot => self.do_mob_loot,
            GameRule::DoMobSpawning => self.do_mob_spawning,
            GameRule::DrowningDamage => self.drowning_damage,
            GameRule::FallDamage => self.fall_damage,
            GameRule::FireDamage => self.fire_damage,
            GameRule::KeepInventory => self.keep_inventory,
            GameRule::MobGriefing => self.mob_griefing,
            GameRule::NaturalRegeneration => self.natural_regeneration,
            GameRule::DoImmediateRespawn => self.do_immediate_respawn,
            GameRule::ShowDeathMessages => self.show_death_messages,
        }
    }

    pub fn set(&mut self, rule: GameRule, value: bool) {
        *self.slot(rule) = value;
    }

    /// One rule as `GameRulesChanged` carries it.
    pub fn wire(&self, rule: GameRule) -> WireGameRule {
        WireGameRule {
            rule_name: rule.name().into(),
            rule_can_be_modified: true,
            rule_value: GameRuleRuleValue::Bool(self.get(rule)),
        }
    }

    /// Every rule, for StartGame and the join `GameRulesChanged`.
    pub fn packet_data(&self) -> GameRulesChangedPacketData {
        GameRulesChangedPacketData {
            rules_list: GameRule::ALL.map(|rule| self.wire(rule)).into(),
        }
    }
}

/// The world clock in ticks since the world was created. Time of day is
/// `ticks % 24000`, 0 being sunrise.
///
/// BDS keeps the clock in an `i32`: it never exceeds [`WorldTime::MAX`].
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorldTime {
    pub ticks: u64,
}

impl WorldTime {
    pub const DAY_TICKS: u64 = 24_000;
    /// The largest clock value; the next tick restarts the clock at 1.
    pub const MAX: u64 = i32::MAX as u64;

    pub const fn time_of_day(self) -> u64 {
        self.ticks % Self::DAY_TICKS
    }

    /// One tick of the daylight cycle (BDS wraps the clock at `i32::MAX`
    /// to 0 before counting the tick).
    pub fn advance(&mut self) {
        if self.ticks >= Self::MAX {
            self.ticks = 0;
        }
        self.ticks += 1;
    }

    /// The clock as the wire carries it.
    pub fn wire(self) -> i32 {
        i32::try_from(self.ticks).expect("the world clock never exceeds i32::MAX")
    }
}

/// What StartGame announces about the world, shared with the connection
/// tasks that build StartGame off the game thread. The clock systems
/// publish the live resources into it every tick.
#[derive(Resource, Debug, Clone, Default)]
pub struct StartGameWorldState(Arc<Mutex<StartGameSnapshot>>);

#[derive(Debug, Clone, Default)]
struct StartGameSnapshot {
    time: WorldTime,
    difficulty: Difficulty,
    rules: GameRules,
}

impl StartGameWorldState {
    pub fn publish(&self, time: WorldTime, difficulty: Difficulty, rules: &GameRules) {
        let mut snapshot = self.0.lock();
        snapshot.time = time;
        snapshot.difficulty = difficulty;
        if snapshot.rules != *rules {
            snapshot.rules = rules.clone();
        }
    }

    /// Write the clock, difficulty and game rules into a player's StartGame.
    pub fn apply(&self, start: &mut StartGamePacket) {
        let snapshot = self.0.lock();
        start.level_current_time = snapshot.time.ticks;
        start.settings.game_difficulty = snapshot.difficulty.legacy();
        start.settings.rule_data = snapshot.rules.packet_data();
    }
}
