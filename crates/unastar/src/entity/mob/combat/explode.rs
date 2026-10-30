//! Fuses (`minecraft:explode`, compiled in `mob::components::explode`) and
//! the swelling of the classes that swell (`NativeTraits::swells`).
//!
//! Native rules, run every tick, swelling first:
//! - **Swell.** A living mob of a class that swells keeps a swell counter
//!   that moves one step toward [`SWELL_MAX`] while its `IGNITED` flag is
//!   set and one step toward 0 otherwise. The first tick it is ignited with
//!   an empty counter it hisses ([`FUSE_SOUND`]) at its head, unless it is
//!   silent. Only the client draws the swelling: the counter is not sent.
//! - **Fuse.** While a fuse burns the `IGNITED` flag is set and `FUSE_TIME`
//!   counts down to 0, one per tick, from `ticks − 1` on the tick the fuse
//!   was lit. The tick after it reached 0 the mob explodes at its feet if it
//!   is alive (a dying mob keeps waiting) and is removed: no death, so no
//!   loot and no experience. Its explosion breaks blocks when
//!   `breaks_blocks` and, with `destroy_affected_by_griefing`, the
//!   `mobGriefing` game rule allow it; fire likewise with `causes_fire` and
//!   `fire_affected_by_griefing`.
//! - Removing `minecraft:explode` clears `IGNITED`; `FUSE_TIME` keeps its
//!   last value. `minecraft:is_charged` sets the `POWERED` flag.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;

use crate::ecs::resources::TickCounter;
use crate::entity::components::{PhysicsBody, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::set_flag;
use crate::entity::mob::components::explode::{Explode, Fuse};
use crate::entity::mob::constants::{FUSE_SOUND, MOB_HEAD_ATTACH_RATIO, SWELL_MAX};
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::state::Dead;
use crate::world::explosion::{Explosion, explode};
use crate::world::rules::GameRules;

/// The swell counter of a mob whose class swells; present while above 0.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Swell(pub f32);

/// Clear the `IGNITED` flag (the fuse component was removed).
pub(crate) fn extinguish(entity: &mut EntityWorldMut<'_>) {
    set_flag(entity, actor_flags::IGNITED, false);
}

/// Set or clear the `POWERED` flag (`minecraft:is_charged`).
pub(crate) fn charged_flag(entity: &mut EntityWorldMut<'_>, on: bool) {
    set_flag(entity, actor_flags::POWERED, on);
}

/// System (exclusive, `EntityLogicSet`, before [`tick_fuses`]): move the
/// swell counters and hiss when a fuse lights.
#[allow(clippy::type_complexity)]
pub fn tick_swell(
    world: &mut World,
    mobs: &mut QueryState<
        (
            Entity,
            &MobKind,
            &ActorFlags,
            &Position,
            &PhysicsBody,
            Option<&Swell>,
        ),
        Without<Dead>,
    >,
    mut changed: Local<Vec<(Entity, f32, Option<DVec3>)>>,
) {
    changed.extend(
        mobs.iter(world)
            .filter(|(_, kind, ..)| kind.0.native.swells)
            .filter_map(|(entity, _, flags, position, body, swell)| {
                let ignited = flags.get(actor_flags::IGNITED).unwrap_or(false);
                let old = swell.map_or(0.0, |swell| swell.0);
                let step = if ignited { 1.0 } else { -1.0 };
                let new = (old + step).clamp(0.0, SWELL_MAX);
                let silent = flags.get(actor_flags::SILENT).unwrap_or(false);
                let hiss = (ignited && old == 0.0 && !silent)
                    .then(|| position.0 + DVec3::Y * body.height * MOB_HEAD_ATTACH_RATIO);
                (new != old || hiss.is_some()).then_some((entity, new, hiss))
            }),
    );
    for (entity, swell, hiss) in changed.drain(..) {
        let mut mob = world.entity_mut(entity);
        if swell > 0.0 {
            mob.insert(Swell(swell));
        } else {
            mob.remove::<Swell>();
        }
        if let Some(head) = hiss {
            play_level_sound(world, Some(entity), head, FUSE_SOUND, -1);
        }
    }
}

/// System (exclusive, `EntityLogicSet`): burn the lit fuses down and
/// explode the mobs whose fuse ran out.
pub fn tick_fuses(
    world: &mut World,
    fuses: &mut QueryState<(Entity, &Fuse, Has<Dead>), With<MobKind>>,
    mut burning: Local<Vec<(Entity, Fuse, bool)>>,
) {
    let now = world.resource::<TickCounter>().current;
    burning.extend(
        fuses
            .iter(world)
            .map(|(entity, fuse, dead)| (entity, *fuse, dead)),
    );
    for (entity, fuse, dead) in burning.drain(..) {
        let Ok(mut mob) = world.get_entity_mut(entity) else {
            continue;
        };
        set_flag(&mut mob, actor_flags::IGNITED, true);
        let remaining = fuse.explode_at.saturating_sub(now + 1);
        if let Some(mut metadata) = mob.get_mut::<EntityMetadata>() {
            metadata.set_int(ids::FUSE_TIME, remaining as i32);
        }
        if now >= fuse.explode_at && !dead {
            explode_mob(world, entity);
        }
    }
}

/// Explode `mob` at its feet with its `minecraft:explode` and remove it.
fn explode_mob(world: &mut World, mob: Entity) {
    let (Some(config), Some(feet)) = (
        world
            .get::<Explode>(mob)
            .map(|explode| explode.config.clone()),
        world.get::<Position>(mob).map(|position| position.0),
    ) else {
        return;
    };
    let griefing = world.resource::<GameRules>().mob_griefing;
    let explosion = Explosion {
        centre: feet,
        power: config.power,
        source: Some(mob),
        breaks_blocks: config.breaks_blocks && (!config.destroy_affected_by_griefing || griefing),
        causes_fire: config.causes_fire && (!config.fire_affected_by_griefing || griefing),
        allow_underwater: config.allow_underwater,
        max_resistance: config.max_resistance,
        damage_scaling: config.damage_scaling,
        knockback_scaling: config.knockback_scaling,
        sound: config.sound.clone(),
    };
    explode(world, &explosion);
    world.despawn(mob);
}
