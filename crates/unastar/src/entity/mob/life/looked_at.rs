//! `minecraft:looked_at` (compiled in `mob::components::looked_at`): the
//! enderman's stare, measured on BDS 1.26.51 (`sweep.py enderman`,
//! `docs/research/enderman-bds-1.26.51.md`).
//!
//! Native rules:
//! - Every tick, a mob that may scan (with `once_and_stop_scanning`, one
//!   without a target) picks the nearest player within the search radius
//!   (its eye to the look-at point) that a hostile mob could target (not creative or spectator, and not on
//!   Peaceful)
//!   and that passes the filter (`other` is the player).
//! - That player looks at the mob when the angle between its view and the
//!   line from its eye to the mob's look-at point (2.55 up the enderman,
//!   [`LOOKED_AT_POINT_RATIO`] of its height) is inside a cone that narrows with
//!   the distance (see [`in_view`]) and the line passes no block collision
//!   box.
//! - A player that keeps looking for the minimum duration becomes the
//!   target, as if a target goal chose it: `TARGET`, `on_target_acquired`
//!   and `mob.warning`; with it the mob plays `stare` at its eye, one tick
//!   later, with `mob.warning`. A break in the look starts the count again.

use bevy_ecs::prelude::*;
use glam::DVec3;
use unastar_data::actor_components::body::LookedAtSetTarget;

use crate::ecs::resources::TickCounter;
use crate::entity::components::{PhysicsBody, Player, Position, Rotation};
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::sensing::can_see;
use crate::entity::mob::components::LookedAtConfig;
use crate::entity::mob::constants::LOOKED_AT_POINT_RATIO;
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::life::anger::head;
use crate::entity::mob::net::sound::queue_synced_sound;
use crate::entity::mob::state::{AttackTarget, Dead};

/// The player a mob sees looking at it, and since which tick.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct LookedAtBy {
    pub player: Entity,
    pub since: u64,
}

/// Whether a view along `view` (a unit vector) from `eye` takes in `point`:
/// the angle between them stays inside a cone whose `1 - cos` is
/// `slack / distance`, `slack` being the `1 - cos` of half the field of view
/// (BDS 1.26.51, 26 degrees: from 4 to 32 blocks every look fits
/// 0.02528 < slack < 0.02607 aiming at 2.55 up the enderman, and
/// `1 - cos(13°)` is 0.02563).
pub fn in_view(eye: DVec3, view: DVec3, point: DVec3, slack: f64) -> bool {
    let to = point - eye;
    let distance = to.length();
    if distance <= f64::EPSILON {
        return true;
    }
    1.0 - view.dot(to / distance) < slack / distance
}

/// The unit view vector of a rotation (degrees; Bedrock yaw 0 looks +z).
pub fn view_vector(rotation: &Rotation) -> DVec3 {
    let yaw = f64::from(rotation.yaw).to_radians();
    let pitch = f64::from(rotation.pitch).to_radians();
    DVec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

/// The mobs a looked-at scan visits: living ones.
type LivingMob = (With<MobKind>, Without<Dead>);

/// System (exclusive, `EntityLogicSet`): mobs notice the players looking
/// at them.
pub fn tick_looked_at(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &LookedAtConfig), LivingMob>,
    players: &mut QueryState<(Entity, &Position, &Rotation), With<Player>>,
    mut scanning: Local<Vec<(Entity, LookedAtConfig)>>,
    mut provoked: Local<Vec<(Entity, Entity)>>,
) {
    let now = world.resource::<TickCounter>().current;
    scanning.extend(
        mobs.iter(world)
            .map(|(entity, config)| (entity, config.clone())),
    );
    for (mob, config) in scanning.drain(..) {
        let stopped = config.set_target == LookedAtSetTarget::OnceAndStopScanning
            && world.get::<AttackTarget>(mob).is_some();
        let watcher = if stopped {
            None
        } else {
            looking_player(world, mob, &config, players)
        };
        let previous = world.get::<LookedAtBy>(mob).copied();
        let Some(player) = watcher else {
            if previous.is_some() {
                world.entity_mut(mob).remove::<LookedAtBy>();
            }
            continue;
        };
        let since = match previous {
            Some(seen) if seen.player == player => seen.since,
            _ => {
                world
                    .entity_mut(mob)
                    .insert(LookedAtBy { player, since: now });
                now
            }
        };
        if now - since >= u64::from(config.min_ticks)
            && config.set_target != LookedAtSetTarget::Never
        {
            provoked.push((mob, player));
        }
    }
    for (mob, player) in provoked.drain(..) {
        world.entity_mut(mob).remove::<LookedAtBy>();
        if crate::entity::mob::ai::provoke(world, mob, player)
            && let Some(eye) = head(world, mob)
        {
            queue_synced_sound(world, Some(mob), eye, "stare", -1);
        }
    }
}

/// The nearest player `mob` may scan, if it is looking at the mob.
fn looking_player(
    world: &World,
    mob: Entity,
    config: &LookedAtConfig,
    players: &mut QueryState<(Entity, &Position, &Rotation), With<Player>>,
) -> Option<Entity> {
    let feet = world.get::<Position>(mob)?.0;
    let height = world.get::<PhysicsBody>(mob)?.height;
    let eye = feet + DVec3::Y * height * LOOKED_AT_POINT_RATIO;
    let (player, player_eye, rotation) = players
        .iter(world)
        .filter(|(player, _, _)| !crate::entity::mob::ai::untargetable_player(world, *player))
        .map(|(player, position, rotation)| (player, position.0, *rotation))
        .filter(|(_, player_eye, _)| player_eye.distance(eye) <= config.search_radius)
        .filter(|(player, _, _)| {
            config.filter.as_ref().is_none_or(|filter| {
                filter.evaluate(&MobFacts {
                    world,
                    entity: mob,
                    other: Some(*player),
                })
            })
        })
        .min_by(|a, b| {
            a.1.distance_squared(eye)
                .total_cmp(&b.1.distance_squared(eye))
        })?;
    (in_view(player_eye, view_vector(&rotation), eye, config.cone_slack)
        && can_see(world, player_eye, eye))
    .then_some(player)
}
