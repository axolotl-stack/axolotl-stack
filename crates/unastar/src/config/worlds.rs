//! Declarative startup worlds retain distinct storage and immutable terrain settings.

use super::{SpawnRule, UnastarConfig};
use crate::world::{WorldBounds, WorldConfig, WorldGenerator};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

/// Primary-world storage, retaining the historical default directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldStorageConfig {
    pub name: String,
    pub data_dir: PathBuf,
    /// Record operator-verified settings for legacy data without a manifest.
    pub adopt_existing: bool,
}
impl Default for WorldStorageConfig {
    fn default() -> Self {
        Self {
            name: "default".into(),
            data_dir: "worlds/default".into(),
            adopt_existing: false,
        }
    }
}

/// One additional protocol dimension, not an arbitrary multi-world cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecondaryWorldConfig {
    pub name: String,
    pub data_dir: PathBuf,
    #[serde(default)]
    pub adopt_existing: bool,
    pub world: WorldConfig,
    pub spawn_rules: Vec<SpawnRule>,
}
impl SecondaryWorldConfig {
    /// Use identical storage policy for primary and additional worlds.
    pub fn storage(&self) -> WorldStorageConfig {
        WorldStorageConfig {
            name: self.name.clone(),
            data_dir: self.data_dir.clone(),
            adopt_existing: self.adopt_existing,
        }
    }
}

impl UnastarConfig {
    /// Validate every configured dimension before opening its provider.
    pub(crate) fn validate_worlds(&self) -> Result<(), String> {
        if self.secondary_worlds.len() > 2 {
            return Err(
                "at most two secondary worlds are supported; one world per protocol dimension"
                    .into(),
            );
        }
        if !self.secondary_worlds.is_empty() && !self.players.leveldb_enabled {
            return Err("configured secondary worlds require `players.leveldb_enabled` for shared account storage".into());
        }
        let mut dimensions = BTreeSet::new();
        let mut names = BTreeSet::new();
        for (index, (storage, world, spawn)) in std::iter::once((
            self.world_storage.clone(),
            self.world,
            self.spawn_rules.as_slice(),
        ))
        .chain(
            self.secondary_worlds
                .iter()
                .map(|entry| (entry.storage(), entry.world, entry.spawn_rules.as_slice())),
        )
        .enumerate()
        {
            if !(0..=2).contains(&world.dimension) || !dimensions.insert(world.dimension) {
                return Err(
                    "configured worlds require unique protocol dimensions 0, 1 or 2".into(),
                );
            }
            if storage.name.trim() != storage.name
                || storage.name.is_empty()
                || storage.name.len() > 64
                || storage.name.chars().any(char::is_control)
                || !names.insert(storage.name.to_lowercase())
                || storage.data_dir.as_os_str().is_empty()
            {
                return Err("world names must be unique, nonempty and bounded; storage directories cannot be empty".into());
            }
            validate_world(world)?;
            if spawn.is_empty() {
                return Err("each world requires at least one spawn rule".into());
            }
            let bounds = crate::world::dimension_block_bounds(world.dimension);
            for location in spawn.iter().filter_map(|rule| rule.location) {
                if ![
                    location.x,
                    location.y,
                    location.z,
                    location.yaw,
                    location.pitch,
                ]
                .into_iter()
                .all(f32::is_finite)
                    || location.y < bounds.start as f32
                    || location.y >= bounds.end as f32
                    || [location.x, location.z].into_iter().any(|coordinate| {
                        f64::from(coordinate) < f64::from(i32::MIN)
                            || f64::from(coordinate) >= f64::from(i32::MAX)
                    })
                {
                    return Err(
                        "spawn coordinates must be finite and within the world's block bounds"
                            .into(),
                    );
                }
            }
            if index != 0
                && !spawn
                    .iter()
                    .any(|rule| rule.always_at_location && rule.location.is_some())
            {
                return Err(
                    "secondary worlds require an explicit fixed fallback spawn rule".into(),
                );
            }
        }
        Ok(())
    }

    /// Select per-world spawn rules without changing the primary configuration.
    pub(crate) fn spawn_rules_for(&self, dimension: i32) -> Option<&[SpawnRule]> {
        if dimension == self.world.dimension {
            return Some(&self.spawn_rules);
        }
        self.secondary_worlds
            .iter()
            .find(|entry| entry.world.dimension == dimension)
            .map(|entry| entry.spawn_rules.as_slice())
    }
}

/// Generator requirements apply equally to primary and additional worlds.
fn validate_world(world: WorldConfig) -> Result<(), String> {
    if let WorldBounds::Rect {
        min_x,
        max_x,
        min_z,
        max_z,
    } = world.bounds
        && (min_x > max_x || min_z > max_z)
    {
        return Err("world rectangle bounds must be ordered".into());
    }
    match world.generator {
        WorldGenerator::JavaTerrain { preset, .. } if preset.dimension_id() != world.dimension => Err("`world.generator.preset` must match `world.dimension`".into()),
        WorldGenerator::Java { .. } if !unastar_noise::structures::template_data::TEMPLATES_EMBEDDED => Err("the Java-style generator needs structure templates: build with `--features java-worldgen` and UNASTAR_JAVA_SERVER_JAR=<path to server.jar>".into()),
        #[cfg(not(feature = "bedrock-worldgen"))]
        WorldGenerator::Bedrock { .. } => Err(crate::world::BEDROCK_BUILD_REQUIREMENT.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
