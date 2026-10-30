//! `minecraft:angry` and `minecraft:on_target_acquired` (the components are
//! compiled in `mob::components::angry`).
//!
//! Native rules:
//! - Anger: the deadline is drawn when the component is added, and the mob
//!   shows the `ANGRY` actor flag from then on. Every melee attack attempt
//!   draws the deadline again ([`restart_anger`]). The anger ends once the
//!   deadline comes or as soon as the mob's target is gone, dead or cannot
//!   be attacked (a creative or spectator player); a mob without a target
//!   keeps counting down. When it ends the flag clears, the mob drops its
//!   target and `calm_event` fires; the mob then stays calm until the
//!   component is added again. A dead mob's anger does nothing.
//! - Target acquired: `event` fires (when the filter passes with the target
//!   as `other`) every time the mob's target becomes a different actor:
//!   from none, or from another target. Keeping the same target fires
//!   nothing.

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_flags;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Health, Player};
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::{set_attack_target, set_flag};
use crate::entity::mob::components::angry::{Angry, OnTargetAcquiredConfig};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::life::sensors::fire;
use crate::entity::mob::state::{AttackTarget, Dead};

/// The target an `on_target_acquired` mob last fired its event for; present
/// while the mob has that target.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct AcquiredTarget(pub Entity);

/// Draw `mob`'s anger deadline again, if it is angry.
pub fn restart_anger(world: &mut World, mob: Entity) {
    let Some(config) = world.get::<Angry>(mob).map(|angry| angry.config.clone()) else {
        return;
    };
    let now = world.resource::<TickCounter>().current;
    let until = config
        .draw(&mut world.get_resource_or_init::<EntityRandom>().0)
        .map(|ticks| now + ticks);
    if let Some(mut angry) = world.get_mut::<Angry>(mob) {
        angry.until = until;
    }
}

/// Whether an angry mob can stay angry at `target`.
fn attackable(world: &World, target: Entity) -> bool {
    let Ok(target) = world.get_entity(target) else {
        return false;
    };
    if target.contains::<Dead>() || target.get::<Health>().is_some_and(Health::is_dead) {
        return false;
    }
    !target.contains::<Player>()
        || target
            .get::<GameMode>()
            .copied()
            .unwrap_or_default()
            .allows_damage()
}

/// System (exclusive, `EntityLogicSet`): end the anger whose deadline came
/// or whose target can no longer be attacked.
#[allow(clippy::type_complexity)]
pub fn tick_anger(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &Angry, Option<&AttackTarget>), (With<MobKind>, Without<Dead>)>,
    mut due: Local<Vec<Entity>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(
        mobs.iter(world)
            .filter(|(_, angry, target)| {
                angry.until.is_some_and(|until| now >= until)
                    || target.is_some_and(|target| !attackable(world, target.0))
            })
            .map(|(mob, _, _)| mob),
    );
    for mob in due.drain(..) {
        let Some(angry) = world.entity_mut(mob).take::<Angry>() else {
            continue;
        };
        set_flag(&mut world.entity_mut(mob), actor_flags::ANGRY, false);
        set_attack_target(world, mob, None);
        if let Some(event) = &angry.config.calm_event {
            fire(world, mob, event, "angry");
        }
    }
}

/// System (exclusive, `EntityLogicSet`): fire `on_target_acquired` for the
/// mobs whose target changed.
#[allow(clippy::type_complexity)]
pub fn fire_target_acquired(
    world: &mut World,
    changed: &mut QueryState<
        (
            Entity,
            &AttackTarget,
            Option<&AcquiredTarget>,
            &OnTargetAcquiredConfig,
        ),
        (With<MobKind>, Without<Dead>, Changed<AttackTarget>),
    >,
    lost: &mut QueryState<Entity, (With<AcquiredTarget>, Without<AttackTarget>)>,
    mut acquired: Local<Vec<(Entity, Entity, OnTargetAcquiredConfig)>>,
    mut cleared: Local<Vec<Entity>>,
) {
    cleared.extend(lost.iter(world));
    for mob in cleared.drain(..) {
        world.entity_mut(mob).remove::<AcquiredTarget>();
    }
    acquired.extend(
        changed
            .iter(world)
            .filter(|(_, target, last, _)| last.is_none_or(|last| last.0 != target.0))
            .map(|(mob, target, _, config)| (mob, target.0, config.clone())),
    );
    for (mob, target, config) in acquired.drain(..) {
        world.entity_mut(mob).insert(AcquiredTarget(target));
        let passes = config.filter.evaluate(&MobFacts {
            world,
            entity: mob,
            other: Some(target),
        });
        if passes {
            fire(world, mob, &config.event, "on_target_acquired");
        }
    }
}
