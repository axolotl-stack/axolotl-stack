//! `minecraft:looked_at` (compiled in `mob::components::looked_at`).
//!
//! Native rules, every tick for every living owner:
//! - While the cooldown runs the owner does nothing. With `set_target`
//!   `once_and_stop_scanning` it does not scan while it has a target.
//! - It looks for a looker: with `find_players_only` the nearest player
//!   within `search_radius` that passes, else the first player or mob in
//!   its box grown by `search_radius` that passes. A looker passes when it
//!   is not a creative or spectator player, and for some
//!   `look_at_locations` point (the owner's head, middle or feet raised by
//!   `vertical_offset`) the looker's view vector `v` and the unit vector
//!   `u` from its eye to the point satisfy
//!   `v · u > 1 − (1 − cos(fov / 2)) / d` (`d` the distance, or 1 without
//!   `scale_fov_by_distance`), its eye sees the point, and the owner's
//!   `filters` pass with the looker as `other`.
//! - Not found: `not_looked_at_event` fires and the run of detections
//!   resets. Found: the run grows; once it spans `min_looked_at_duration`
//!   the owner targets the looker (unless `set_target` is `never`, it
//!   already has a target, or it cannot attack the looker),
//!   `looked_at_event` fires, the [`STARE_SOUND`] plays at its head, the
//!   cooldown (a uniform draw in `looked_at_cooldown`) starts and the run
//!   resets.

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;
use unastar_data::actor_components::body::LookedAtSetTarget;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Health, PhysicsBody, Player, Position, Rotation};
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::sensing::can_see;
use crate::entity::mob::ai::set_attack_target;
use crate::entity::mob::components::SelfEvent;
use crate::entity::mob::components::looked_at::{AttachPoint, LookedAtConfig, LookedAtState};
use crate::entity::mob::constants::{MOB_EYE_HEIGHT_RATIO, STARE_SOUND, TICKS_PER_SECOND};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::life::sensors::fire;
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::projectile::launch::view_direction;
use crate::registry::collision::Aabb;

/// What one owner's scan decided this tick.
enum Outcome {
    /// Cooling down or not scanning.
    Idle,
    NotLooked(Option<SelfEvent>),
    /// A looker found, the run not yet long enough.
    Counting(u64),
    Looked {
        looker: Entity,
        set_target: bool,
        event: Option<SelfEvent>,
        cooldown: (f32, f32),
        head: DVec3,
    },
}

/// A possible looker: its eye and view vector.
struct Looker {
    entity: Entity,
    eye: DVec3,
    view: DVec3,
}

/// The eye and view of `actor` when it may look at an owner: a player
/// not in creative or spectator, or a living mob.
fn looker(world: &World, actor: Entity) -> Option<Looker> {
    let actor_ref = world.get_entity(actor).ok()?;
    let rotation = actor_ref.get::<Rotation>()?;
    let view = view_direction(rotation.head_yaw, rotation.pitch);
    let position = actor_ref.get::<Position>()?.0;
    if actor_ref.contains::<Player>() {
        if matches!(
            actor_ref.get::<GameMode>(),
            Some(GameMode::Creative | GameMode::Spectator)
        ) || actor_ref.get::<Health>().is_some_and(Health::is_dead)
        {
            return None;
        }
        // A player's position is its eye.
        return Some(Looker {
            entity: actor,
            eye: position,
            view,
        });
    }
    if !actor_ref.contains::<MobKind>() || actor_ref.contains::<Dead>() {
        return None;
    }
    let height = actor_ref.get::<PhysicsBody>()?.height;
    Some(Looker {
        entity: actor,
        eye: position + DVec3::Y * height * MOB_EYE_HEIGHT_RATIO,
        view,
    })
}

/// The owner's point for `point`.
fn attach(feet: DVec3, height: f64, point: AttachPoint) -> DVec3 {
    match point {
        AttachPoint::Head => feet + DVec3::Y * height * MOB_EYE_HEIGHT_RATIO,
        AttachPoint::Body => feet + DVec3::Y * height * 0.5,
        AttachPoint::Feet => feet,
    }
}

/// Whether `looker` looks at `owner` (see the module rules).
fn looks_at(
    world: &World,
    owner: Entity,
    config: &LookedAtConfig,
    feet: DVec3,
    height: f64,
    looker: &Looker,
) -> bool {
    let sees_a_point = config.points.iter().any(|point| {
        let target = attach(feet, height, point.point) + DVec3::Y * point.vertical_offset;
        let offset = target - looker.eye;
        let distance = offset.length();
        if distance <= 0.0 || !distance.is_finite() {
            return false;
        }
        let scale = if config.scale_fov_by_distance {
            1.0 / distance
        } else {
            1.0
        };
        looker.view.dot(offset / distance) > 1.0 - config.view_slack * scale
            && can_see(world, looker.eye, target)
    });
    sees_a_point
        && config.filter.evaluate(&MobFacts {
            world,
            entity: owner,
            other: Some(looker.entity),
        })
}

/// The looker `owner` finds this tick, if any.
fn find_looker(
    world: &World,
    owner: Entity,
    config: &LookedAtConfig,
    feet: DVec3,
    body: &PhysicsBody,
    players: &mut QueryState<Entity, With<Player>>,
    mobs: &mut QueryState<Entity, (With<MobKind>, Without<Dead>)>,
) -> Option<Entity> {
    let radius = config.search_radius;
    if config.find_players_only {
        return players
            .iter(world)
            .filter_map(|player| {
                let looker = looker(world, player)?;
                let distance_sq = looker.eye.distance_squared(feet);
                (distance_sq <= radius * radius).then_some((distance_sq, looker))
            })
            .filter(|(_, looker)| looks_at(world, owner, config, feet, body.height, looker))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, looker)| looker.entity);
    }
    let area = Aabb::from_feet(feet, body.width, body.height);
    let area = Aabb::new(
        area.min - DVec3::splat(radius),
        area.max + DVec3::splat(radius),
    );
    let inside = |position: DVec3| position.cmpge(area.min).all() && position.cmple(area.max).all();
    players
        .iter(world)
        .chain(mobs.iter(world).filter(|&mob| mob != owner))
        .filter_map(|actor| looker(world, actor))
        .find(|looker| {
            world
                .get::<Position>(looker.entity)
                .is_some_and(|position| inside(position.0))
                && looks_at(world, owner, config, feet, body.height, looker)
        })
        .map(|looker| looker.entity)
}

/// Whether `owner` may make `looker` its target: a player that can be
/// hurt, or a living mob.
fn can_attack(world: &World, looker: Entity) -> bool {
    let Ok(looker) = world.get_entity(looker) else {
        return false;
    };
    if looker.contains::<Dead>() || looker.get::<Health>().is_some_and(Health::is_dead) {
        return false;
    }
    !looker.contains::<Player>()
        || looker
            .get::<GameMode>()
            .copied()
            .unwrap_or_default()
            .allows_damage()
}

fn scan(
    world: &World,
    owner: Entity,
    now: u64,
    players: &mut QueryState<Entity, With<Player>>,
    mobs: &mut QueryState<Entity, (With<MobKind>, Without<Dead>)>,
) -> Outcome {
    let (Some(config), Some(state), Some(feet), Some(body)) = (
        world.get::<LookedAtConfig>(owner),
        world.get::<LookedAtState>(owner),
        world.get::<Position>(owner).map(|p| p.0),
        world.get::<PhysicsBody>(owner),
    ) else {
        return Outcome::Idle;
    };
    let has_target = world.get::<AttackTarget>(owner).is_some();
    if now < state.cooldown_until
        || (config.set_target == LookedAtSetTarget::OnceAndStopScanning && has_target)
    {
        return Outcome::Idle;
    }
    let Some(looker) = find_looker(world, owner, config, feet, body, players, mobs) else {
        return Outcome::NotLooked(config.not_looked_at_event.clone());
    };
    let since = state.looked_since.unwrap_or(now);
    if now - since + 1 < u64::from(config.min_looked_ticks) {
        return Outcome::Counting(since);
    }
    Outcome::Looked {
        looker,
        set_target: config.set_target != LookedAtSetTarget::Never
            && !has_target
            && can_attack(world, looker),
        event: config.looked_at_event.clone(),
        cooldown: config.cooldown,
        head: attach(feet, body.height, AttachPoint::Head),
    }
}

/// System (exclusive, `EntityLogicSet`): run every owner's look scan.
#[allow(clippy::type_complexity)]
pub fn tick_looked_at(
    world: &mut World,
    owners: &mut QueryState<Entity, (With<LookedAtConfig>, With<MobKind>, Without<Dead>)>,
    players: &mut QueryState<Entity, With<Player>>,
    mobs: &mut QueryState<Entity, (With<MobKind>, Without<Dead>)>,
    mut due: Local<Vec<Entity>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(owners.iter(world));
    for owner in due.drain(..) {
        match scan(world, owner, now, players, mobs) {
            Outcome::Idle => {}
            Outcome::NotLooked(event) => {
                if let Some(mut state) = world.get_mut::<LookedAtState>(owner)
                    && state.looked_since.is_some()
                {
                    state.looked_since = None;
                }
                if let Some(event) = event {
                    fire(world, owner, &event, "looked_at");
                }
            }
            Outcome::Counting(since) => {
                if let Some(mut state) = world.get_mut::<LookedAtState>(owner)
                    && state.looked_since != Some(since)
                {
                    state.looked_since = Some(since);
                }
            }
            Outcome::Looked {
                looker,
                set_target,
                event,
                cooldown: (min, max),
                head,
            } => {
                if set_target {
                    set_attack_target(world, owner, Some(looker));
                }
                if let Some(event) = &event {
                    fire(world, owner, event, "looked_at");
                }
                play_level_sound(world, Some(owner), head, STARE_SOUND, -1);
                let seconds = if max > min {
                    world
                        .get_resource_or_init::<EntityRandom>()
                        .0
                        .gen_range(min..max)
                } else {
                    min
                };
                let ticks = (seconds * TICKS_PER_SECOND) as u64;
                if let Some(mut state) = world.get_mut::<LookedAtState>(owner) {
                    *state = LookedAtState {
                        cooldown_until: now + ticks + 1,
                        looked_since: None,
                    };
                }
            }
        }
    }
}
