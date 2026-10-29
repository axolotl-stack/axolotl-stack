//! Mob marker. Everything else a mob carries comes from its behaviour-pack
//! definition (see `crate::entity::mob`).

use bevy_ecs::prelude::*;

/// Marker for definition-driven mob entities (see `crate::entity::mob::spawn_mob`).
#[derive(Component, Debug)]
pub struct Mob;
