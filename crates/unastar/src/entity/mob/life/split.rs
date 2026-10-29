//! Splitting: a dead mob of a splitting class (`NativeTraits::split`, the
//! slime) leaves smaller copies of itself when its body is removed.
//!
//! Only a mob whose size (its `VARIANT` actor data) is above 1 and whose
//! health ran out splits. It leaves `SPLIT_BASE_COUNT +
//! nextInt(SPLIT_EXTRA_COUNTS)` children (2–4) of its own type, all at the
//! centre of its block, [`SPLIT_CHILD_OFFSET`] above the block floor, facing
//! its yaw. A child is created as born (no `minecraft:entity_spawned`; the
//! slime defines no `minecraft:entity_born`) and gets the size group of half
//! the parent's size, which sets its size, box, health and attack. A
//! persistent parent's children are persistent and a named parent's children
//! carry its name. This matches BDS 1.26.51.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds;
use jolyne::valentine::types::DataItemEntryPayload;
use rand::Rng;
use thiserror::Error;

use crate::entity::EntityRandom;
use crate::entity::components::{Health, Position, Rotation};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::{
    CompositionError, GroupChange, apply_pending_definitions, queue_group_change,
};
use crate::entity::mob::constants::{SPLIT_BASE_COUNT, SPLIT_CHILD_OFFSET, SPLIT_EXTRA_COUNTS};
use crate::entity::mob::derive::derive_entity;
use crate::entity::mob::state::Persistent;
use crate::entity::mob::{MobKind, MobSpawnError, SpawnCause, spawn_mob};

/// Why a split could not create its children.
#[derive(Debug, Error)]
pub enum SplitError {
    #[error(transparent)]
    Spawn(#[from] MobSpawnError),
    #[error("{identifier} has no component group {group}")]
    UnknownGroup {
        identifier: String,
        group: &'static str,
    },
    #[error(transparent)]
    Composition(#[from] CompositionError),
}

/// Split `mob` if it is a dead mob of a splitting class (module docs), just
/// before its body is removed; returns the children.
pub fn split(world: &mut World, mob: Entity) -> Result<Vec<Entity>, SplitError> {
    let Some(kind) = world.get::<MobKind>(mob).map(|kind| kind.0.clone()) else {
        return Ok(Vec::new());
    };
    let Some(traits) = kind.native.split else {
        return Ok(Vec::new());
    };
    let size = world
        .get::<EntityMetadata>(mob)
        .and_then(|metadata| metadata.get_int(actor_data_i_ds::VARIANT))
        .unwrap_or(0);
    let dead = world.get::<Health>(mob).is_none_or(Health::is_dead);
    let (Some(feet), Some(rotation)) = (
        world.get::<Position>(mob).map(|p| p.0),
        world.get::<Rotation>(mob).copied(),
    ) else {
        return Ok(Vec::new());
    };
    if size <= 1 || !dead {
        return Ok(Vec::new());
    }
    let group_name = traits.size_group(size / 2);
    let group = kind
        .definition
        .group(group_name)
        .ok_or_else(|| SplitError::UnknownGroup {
            identifier: kind.identifier.to_string(),
            group: group_name,
        })?;
    let persistent = world.get::<Persistent>(mob).is_some();
    let name = world.get::<EntityMetadata>(mob).and_then(|metadata| {
        match metadata.get(actor_data_i_ds::NAME) {
            Some(DataItemEntryPayload::DataItemStringPayload(name)) if !name.value.is_empty() => {
                Some(name.value.clone())
            }
            _ => None,
        }
    });
    let at = feet.floor() + DVec3::new(0.5, SPLIT_CHILD_OFFSET, 0.5);
    let count = SPLIT_BASE_COUNT
        + world
            .resource_mut::<EntityRandom>()
            .0
            .gen_range(0..SPLIT_EXTRA_COUNTS);
    let mut children = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let child = spawn_mob(world, &kind.identifier, at, rotation.yaw, SpawnCause::Born)?;
        children.push(child);
        queue_group_change(world, child, GroupChange::Add(group.clone()))?;
        let mut entity = world.entity_mut(child);
        if persistent {
            entity.insert(Persistent);
        }
        if let Some(name) = &name
            && let Some(mut metadata) = entity.get_mut::<EntityMetadata>()
        {
            metadata.set_string(actor_data_i_ds::NAME, name.clone());
        }
    }
    apply_pending_definitions(world);
    for &child in &children {
        derive_entity(world, child);
    }
    Ok(children)
}
