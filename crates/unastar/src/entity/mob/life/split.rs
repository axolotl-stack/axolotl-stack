//! Splitting: a dead slime or magma cube (`NativeTraits::slime`) bigger
//! than the smallest size leaves smaller copies of itself when its body is
//! removed, as BDS 1.26.51 does (`sweep.py slime --items split`, replayed
//! by `mob::tests::slime`).
//!
//! The body is removed at the end of its death animation, wherever its
//! knockback slid it; the children appear in the same tick, all at the
//! centre of the block the body ended in, half a block above its floor,
//! facing the body's yaw. There are [`SPLIT_MIN_CHILDREN`] to
//! [`SPLIT_MAX_CHILDREN`] of them (uniform), each born (no
//! `minecraft:entity_spawned`, so no random size) with the component group
//! whose `minecraft:variant` is half the parent's, which gives it its size,
//! box, health and attack. Each plays `born` as it appears, at the eyes of
//! the base collision box it has before its size group applies.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds;
use rand::Rng;
use thiserror::Error;
use unastar_data::actor_components::{ActorComponent, body};

use crate::entity::EntityRandom;
use crate::entity::components::{Position, Rotation};
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::{
    CompositionError, GroupChange, apply_pending_definitions, queue_group_change,
};
use crate::entity::mob::components::variant::appearance_id;
use crate::entity::mob::constants::{SPLIT_CHILD_RISE, SPLIT_MAX_CHILDREN, SPLIT_MIN_CHILDREN};
use crate::entity::mob::derive::derive_entity;
use crate::entity::mob::{MobKind, MobSpawnError, SpawnCause, spawn_mob};

/// Why a split could not make its children.
#[derive(Debug, Error)]
pub enum SplitError {
    #[error(transparent)]
    Spawn(#[from] MobSpawnError),
    #[error("{identifier} has no component group with variant {size}")]
    NoSizeGroup { identifier: String, size: i32 },
    #[error("component group {group}: minecraft:variant does not decode: {message}")]
    BadVariant { group: String, message: String },
    #[error(transparent)]
    Composition(#[from] CompositionError),
}

/// The name of the component group of `source` whose `minecraft:variant`
/// is `size`; a group whose variant does not decode is an error.
fn size_group(
    source: &unastar_packs::actor::ActorDefinition,
    size: i32,
) -> Result<Option<&str>, SplitError> {
    let key = ActorComponent::Variant.name();
    for (name, group) in &source.component_groups {
        let Some(json) = group.get(key) else {
            continue;
        };
        let variant: body::Variant =
            serde_json::from_value(json.clone()).map_err(|error| SplitError::BadVariant {
                group: name.clone(),
                message: error.to_string(),
            })?;
        if variant.value == size {
            return Ok(Some(name.as_str()));
        }
    }
    Ok(None)
}

/// Split `mob`, whose body is about to be removed, if it is a splitting
/// class bigger than the smallest size; returns the children.
pub fn split(world: &mut World, mob: Entity) -> Result<Vec<Entity>, SplitError> {
    let Some(kind) = world.get::<MobKind>(mob).map(|kind| kind.0.clone()) else {
        return Ok(Vec::new());
    };
    if kind.native.slime.is_none() {
        return Ok(Vec::new());
    }
    let size = world
        .get::<EntityMetadata>(mob)
        .and_then(|metadata| appearance_id(metadata, actor_data_i_ds::VARIANT))
        .unwrap_or(0);
    let (Some(feet), Some(yaw)) = (
        world.get::<Position>(mob).map(|p| p.0),
        world.get::<Rotation>(mob).map(|r| r.yaw),
    ) else {
        return Ok(Vec::new());
    };
    if size <= 1 {
        return Ok(Vec::new());
    }
    let child_size = size / 2;
    let group = size_group(&kind.source, child_size)?
        .and_then(|name| kind.definition.group(name))
        .ok_or_else(|| SplitError::NoSizeGroup {
            identifier: kind.identifier.to_string(),
            size: child_size,
        })?;
    let at = feet.floor() + DVec3::new(0.5, SPLIT_CHILD_RISE, 0.5);
    let count = world
        .resource_mut::<EntityRandom>()
        .0
        .gen_range(SPLIT_MIN_CHILDREN..=SPLIT_MAX_CHILDREN);
    let mut children = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let child = spawn_mob(world, &kind.identifier, at, yaw, SpawnCause::Born)?;
        if let Some(eyes) = crate::entity::mob::life::anger::head(world, child) {
            crate::entity::mob::net::sound::play_level_sound(world, Some(child), eyes, "born", -1);
        }
        queue_group_change(world, child, GroupChange::Add(group.clone()))?;
        children.push(child);
    }
    apply_pending_definitions(world);
    for &child in &children {
        derive_entity(world, child);
        // What the child has with its size is what `UpdateAttributes`
        // reports as its defaults, as for a mob spawned at a size.
        if let Some(mut attributes) = world.get_mut::<ActorAttributes>(child) {
            attributes.reset_defaults();
            attributes.mark_all_changed();
        }
    }
    Ok(children)
}
