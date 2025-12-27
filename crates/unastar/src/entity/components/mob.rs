//! Mob-specific components.

use bevy_ecs::prelude::*;

/// Marker for mob entities.
#[derive(Component, Debug)]
pub struct Mob;

/// Mob type identifier.
#[derive(Component, Debug, Clone)]
pub struct MobType {
    pub identifier: String,
    pub variant: i32,
}

impl MobType {
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            variant: 0,
        }
    }

    pub fn with_variant(identifier: impl Into<String>, variant: i32) -> Self {
        Self {
            identifier: identifier.into(),
            variant,
        }
    }
}

/// AI state for mobs.
#[derive(Component, Debug, Default)]
pub struct AiState {
    pub target: Option<Entity>,
    pub path: Vec<glam::IVec3>,
    pub current_goal: Option<String>,
}

/// Whether the mob is hostile.
#[derive(Component, Debug, Clone, Copy)]
pub struct Hostile(pub bool);

impl Default for Hostile {
    fn default() -> Self {
        Self(false)
    }
}

/// Whether the mob can be tamed.
#[derive(Component, Debug, Default)]
pub struct Tameable {
    pub owner: Option<Entity>,
}

/// Mob age (for babies).
#[derive(Component, Debug, Clone, Copy)]
pub struct MobAge {
    pub ticks: i32, // Negative = baby
}

impl Default for MobAge {
    fn default() -> Self {
        Self { ticks: 0 }
    }
}

impl MobAge {
    pub fn is_baby(&self) -> bool {
        self.ticks < 0
    }

    pub fn tick(&mut self) {
        if self.ticks < 0 {
            self.ticks += 1;
        }
    }
}
