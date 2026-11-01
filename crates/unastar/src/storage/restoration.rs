//! A nonrecursive raw-state handoff beside the canonical player record and receipt.

use super::{PlayerData, StorageError, StorageResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One actual remaining status effect, independent of ECS and absolute world clocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestorationEffect {
    pub id: u8,
    pub level: u8,
    pub duration_ticks: u32,
    pub ambient: bool,
    pub show_particles: bool,
    pub infinite: bool,
}

/// Raw fields not otherwise represented by the canonical player row.
/// Inventory and receipt history remain in `PlayerData`, never duplicated here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestorationSupplement {
    pub version: u16,
    pub xuid: Option<String>,
    pub effective_mode: u8,
    pub base_mode: Option<u8>,
    pub flight_override: Option<bool>,
    pub velocity: [f64; 3],
    pub head_yaw: f32,
    pub absorption: f32,
    pub air_current: u32,
    pub air_max: u32,
    pub on_fire_ticks: u32,
    pub burning_remaining: u64,
    pub burning_next: u64,
    pub effects: Vec<RestorationEffect>,
    pub dead: bool,
}

impl RestorationSupplement {
    /// Validate immutable extra state before persistence or reconstruction.
    pub fn validate(&self, data: &PlayerData) -> StorageResult<()> {
        let valid_mode = |mode| matches!(mode, 0 | 1 | 2 | 6);
        if self.version != 1
            || !valid_mode(self.effective_mode)
            || self.base_mode.is_some_and(|mode| !valid_mode(mode))
            || self.base_mode.unwrap_or(self.effective_mode) != data.game_mode
            || self.dead != (data.health == 0.0)
            || self
                .velocity
                .iter()
                .any(|value| !value.is_finite() || !(*value as f32).is_finite())
            || !self.head_yaw.is_finite()
            || !self.absorption.is_finite()
            || !(0.0..=crate::server::game::attributes::MAX_ABSORPTION).contains(&self.absorption)
            || self.air_max == 0
            || self.air_max > u32::MAX - 4
            || self.air_current > self.air_max
            || (self.burning_remaining == 0 && self.burning_next != 0)
            || self.effects.len() > 36
            || self
                .effects
                .iter()
                .any(|effect| !(1..=36).contains(&effect.id))
            || self
                .effects
                .iter()
                .map(|effect| effect.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.effects.len()
            || self.xuid.as_ref().is_some_and(|xuid| {
                xuid.is_empty() || xuid.len() > 128 || xuid.chars().any(char::is_control)
            })
        {
            return Err(StorageError::Deserialization(
                "invalid restoration supplement".into(),
            ));
        }
        Ok(())
    }
}

/// Persisted only with its exact new receipt, before any restored state is published.
/// A later ordinary save retires this one-shot handoff rather than replaying old extras.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestorationCheckpoint {
    pub id: String,
    pub revision: u64,
    pub request_hash: [u8; 32],
    pub supplement: RestorationSupplement,
}

impl RestorationCheckpoint {
    /// A checkpoint cannot be attached to an unrelated or advanced canonical row.
    pub fn validate(&self, data: &PlayerData) -> StorageResult<()> {
        let receipt = data.inventory_effects.get(&self.id);
        if self.revision != data.inventory_revision
            || receipt.is_none_or(|receipt| {
                receipt.revision != self.revision || receipt.request_hash != self.request_hash
            })
        {
            return Err(StorageError::Deserialization(
                "restoration checkpoint receipt mismatch".into(),
            ));
        }
        self.supplement.validate(data)
    }
}
