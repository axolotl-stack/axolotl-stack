//! A mob's target on the wire (matches the BDS 1.26.51 recordings
//! `skeleton-bow-field`, `skeleton-bow-field-hard`, `stray-bow-field` and
//! `zombie-spear-field`):
//!
//! - The `TARGET` actor data carries the target's unique id while the mob
//!   has a target, and 0 once it has none. It goes out in the same
//!   `SetActorData` as the flags the goals changed that tick; a skeleton's
//!   target and its raised bow (`FACING_TARGET_TO_RANGE_ATTACK`) arrive
//!   together. The client draws the bow and raises the arms from it
//!   (`query.has_target`).
//! - The tick after a mob takes a target it did not have, it plays
//!   `mob.warning` at its head, unless it has lost that target again by
//!   then or is silent.

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;

use crate::ecs::resources::TickCounter;
use crate::entity::components::{PhysicsBody, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::constants::{MOB_HEAD_ATTACH_RATIO, TARGET_WARNING_SOUND};
use crate::entity::mob::net::network::actor_unique_id;
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::state::{AttackTarget, Dead};

/// The mob took `target` and warns on tick `due`. Inserted when the
/// `TARGET` actor data changes to a new target, removed when the warning
/// is due.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct TargetWarning {
    pub target: Entity,
    pub due: u64,
}

/// `AttackTarget` insert hook: the `TARGET` actor data names the new
/// target; a target the data did not name yet is warned about next tick.
pub(crate) fn target_inserted(mut world: DeferredWorld<'_>, context: HookContext) {
    let mob = context.entity;
    let Some(target) = world.get::<AttackTarget>(mob).map(|target| target.0) else {
        return;
    };
    let id = actor_unique_id(&world, target);
    let due = world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current + 1);
    let Some(mut metadata) = world.get_mut::<EntityMetadata>(mob) else {
        return;
    };
    if metadata.set_long(ids::TARGET, id) {
        world
            .commands()
            .entity(mob)
            .insert(TargetWarning { target, due });
    }
}

/// `AttackTarget` remove hook: the `TARGET` actor data goes back to 0.
pub(crate) fn target_removed(mut world: DeferredWorld<'_>, context: HookContext) {
    if let Some(mut metadata) = world.get_mut::<EntityMetadata>(context.entity) {
        metadata.set_long(ids::TARGET, 0);
    }
}

/// System (exclusive, `EntityLogicSet`, after the mob AI): play the due
/// target warnings.
#[allow(clippy::type_complexity)]
pub fn play_target_warnings(
    world: &mut World,
    mobs: &mut QueryState<(
        Entity,
        &TargetWarning,
        Option<&AttackTarget>,
        &Position,
        &PhysicsBody,
        &ActorFlags,
        Has<Dead>,
    )>,
    mut due: Local<Vec<(Entity, Option<DVec3>)>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(
        mobs.iter(world)
            .filter(|(_, warning, ..)| warning.due <= now)
            .map(|(mob, warning, target, position, body, flags, dead)| {
                let warns = target.is_some_and(|target| target.0 == warning.target)
                    && !dead
                    && !flags.get(actor_flags::SILENT).unwrap_or(false);
                (
                    mob,
                    warns.then(|| position.0 + DVec3::Y * body.height * MOB_HEAD_ATTACH_RATIO),
                )
            }),
    );
    for (mob, head) in due.drain(..) {
        world.entity_mut(mob).remove::<TargetWarning>();
        if let Some(head) = head {
            play_level_sound(world, Some(mob), head, TARGET_WARNING_SOUND, -1);
        }
    }
}
