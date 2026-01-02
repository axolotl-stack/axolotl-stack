//! Extension methods for Player that access actual components.
//!
//! These are implemented in unastar (not unastar-api) to avoid circular dependencies.

use crate::entity::components::{PlayerName, PlayerUuid, Position};
use bevy_ecs::prelude::*;
use unastar_api::native::{Player, Vec3};

/// Extension trait for Player with component access methods.
pub trait PlayerExt {
    fn name(&self) -> Option<String>;
    fn position(&self) -> Option<Vec3>;
    fn uuid(&self) -> Option<String>;
}

impl<'a> PlayerExt for Player<'a> {
    fn name(&self) -> Option<String> {
        self.world
            .get::<PlayerName>(self.entity)
            .map(|n| n.0.clone())
    }

    fn position(&self) -> Option<Vec3> {
        self.world.get::<Position>(self.entity).map(|p| Vec3 {
            x: p.0.x,
            y: p.0.y,
            z: p.0.z,
        })
    }

    fn uuid(&self) -> Option<String> {
        self.world
            .get::<PlayerUuid>(self.entity)
            .map(|u| u.0.to_string())
    }
}
