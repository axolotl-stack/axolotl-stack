//! Mob AI: authored goals, navigation and the native move/look/jump
//! controls.
//!
//! [`tick_mob_ai`] runs every server tick in the physics set, after the
//! definition barrier and [`super::derive_mob_state`] and before
//! `tick_physics`. For every living mob it (re)builds the goal composition
//! when the definition changed, ticks the goal selector, follows the current
//! path, runs the move and look controls, and turns the result into body
//! yaw, head yaw and velocity (acceleration and jumps) that the physics step
//! then integrates. [`push_mobs`] then pushes nearby pushable mobs apart.

mod break_door;
mod control;
mod eat_block;
mod gaze;
mod goals;
mod melee;
mod nearby;
mod path;
mod pickup;
pub mod sensing;
mod targeting;

#[cfg(test)]
mod tests;

pub(crate) use self::break_door::stop_breaking_doors;
pub use self::path::PathBlockTable;
pub(crate) use self::targeting::combat_candidate;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;

use self::control::{Body, LookControl, MoveControl, PathFollower, travel};
use self::goals::{Effects, GoalContext, GoalStates, goal_specs};
use self::nearby::{ItemView, MobView, Nearby, PlayerView};
use self::path::WalkPolicy;
use super::MobKind;
use super::components::{BasicMovementConfig, JumpConfig, Pushable, WalkNavigationConfig};
use super::constants::{
    DEFAULT_JUMP_POWER, DEFAULT_MAX_TURN, ENTITY_PUSH_MIN_DISTANCE, ENTITY_PUSH_REACH,
    ENTITY_PUSH_STRENGTH, PLAYER_BOX_HEIGHT, PLAYER_BOX_WIDTH,
};
use super::state::{AttackTarget, Dead, InLove, MateRequest, Persistent};
use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, OnGround, PhysicsBody, Player, Position, Rotation, Velocity,
};
use crate::entity::definition::DefinitionState;
use crate::entity::definition::components::MovementConfig;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::goal_selector::{GoalSelector, GoalSpec};
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::systems::physics::{living_block_friction, living_liquid};
use crate::registry::collision::Aabb;

/// Per-mob AI state: the goal selector over the authored goals, their
/// runtime state, the current path and the move/look controls. Inserted at
/// spawn; removed once the mob dies.
#[derive(Component, Debug, Default)]
pub struct MobBrain {
    selector: GoalSelector,
    specs: Vec<GoalSpec>,
    states: GoalStates,
    nav: PathFollower,
    movement: MoveControl,
    look: LookControl,
}

impl MobBrain {
    /// Drop a target at an authoritative lifecycle transition, including
    /// anger expiry, before the selector can run a melee goal again.
    pub(crate) fn clear_combat_target(&mut self) {
        self.states.targets.target = None;
    }
    /// Whether the mob currently follows a path.
    pub fn is_navigating(&self) -> bool {
        !self.nav.is_done()
    }
}

/// What a mob's AI does this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// Tick the goals; rebuild the goal composition first when `true`.
    Think(bool),
    /// The mob died: stop every running goal once and drop the brain.
    Die,
}

/// Reused per-tick buffers of [`tick_mob_ai`].
#[derive(Default)]
pub struct AiScratch {
    /// Gaze events deferred beyond the AI's random-resource scope.
    gaze: Vec<(
        Entity,
        super::components::triggers::ActorTrigger,
        Option<Entity>,
    )>,
    mobs: Vec<(Entity, Turn)>,
    nearby: Nearby,
    /// Mobs whose eat timer ran out this tick; they eat once the AI's
    /// resource scopes end (eating fires events, which draw randomness).
    eaten: Vec<Entity>,
    /// Melee hits `(attacker, target)` of this tick; dealt once the AI's
    /// resource scopes end (hits draw randomness and hurt players).
    attacks: Vec<(Entity, Entity)>,
    /// Item pickups, equips and pickup events of this tick; made once the
    /// AI's resource scopes end (they spawn items and fire events).
    gear: Vec<(Entity, pickup::GearWork)>,
    /// New targets whose lifecycle events run after the AI's RNG scope ends.
    acquired: Vec<(Entity, Entity)>,
    /// Door collision boxes for [`break_door`].
    boxes: Vec<Aabb>,
}

type BrainQuery = (Entity, Ref<'static, DefinitionState>, Has<Dead>);
type ViewQuery = (
    Entity,
    &'static Position,
    &'static PhysicsBody,
    &'static ActorFlags,
    Option<&'static InLove>,
    Has<Dead>,
);

/// System: run every mob's AI for this tick.
#[allow(clippy::type_complexity)]
pub fn tick_mob_ai(
    world: &mut World,
    brains: &mut QueryState<BrainQuery, (With<MobKind>, With<MobBrain>)>,
    views: &mut QueryState<ViewQuery, With<MobKind>>,
    players: &mut QueryState<(Entity, &Position, &Rotation), With<Player>>,
    items: &mut QueryState<(Entity, &Position), With<DroppedItem>>,
    mut scratch: Local<AiScratch>,
) {
    let scratch = &mut *scratch;
    scratch.mobs.clear();
    scratch
        .mobs
        .extend(brains.iter(world).map(|(entity, definition, dead)| {
            let turn = if dead {
                Turn::Die
            } else {
                Turn::Think(definition.is_changed())
            };
            (entity, turn)
        }));
    if scratch.mobs.is_empty() {
        return;
    }
    let tick = world.resource::<TickCounter>().current;
    scratch.nearby.rebuild(
        players
            .iter(world)
            .map(|(entity, position, rotation)| PlayerView::new(entity, position.0, rotation)),
        views
            .iter(world)
            .map(|(entity, position, body, flags, love, dead)| MobView {
                entity,
                feet: position.0,
                width: body.width,
                height: body.height,
                baby: flags.get(actor_flags::BABY).unwrap_or(false),
                in_love: love.is_some_and(|love| tick < love.until_tick),
                dead,
            }),
    );
    scratch
        .nearby
        .rebuild_items(items.iter(world).map(|(entity, position)| ItemView {
            entity,
            feet: position.0,
        }));
    world.resource_scope(|world, table: Mut<PathBlockTable>| {
        world.resource_scope(|world, mut rng: Mut<EntityRandom>| {
            for &(entity, turn) in &scratch.mobs {
                let mut brain = std::mem::take(
                    &mut *world
                        .get_mut::<MobBrain>(entity)
                        .expect("queried with a brain"),
                );
                let output = think(
                    world,
                    entity,
                    turn,
                    &mut brain,
                    &table,
                    &mut rng,
                    tick,
                    &scratch.nearby,
                );
                if turn == Turn::Die {
                    world.entity_mut(entity).remove::<MobBrain>();
                } else {
                    *world
                        .get_mut::<MobBrain>(entity)
                        .expect("queried with a brain") = brain;
                }
                if let Some(mut output) = output {
                    scratch.gaze.extend(
                        output
                            .effects
                            .gaze_events
                            .drain(..)
                            .map(|(event, other)| (entity, event, other)),
                    );
                    apply(
                        world,
                        entity,
                        output,
                        &mut scratch.eaten,
                        &mut scratch.attacks,
                        &mut scratch.gear,
                        &mut scratch.acquired,
                    );
                }
            }
        });
    });
    for (entity, target) in scratch.acquired.drain(..) {
        super::life::triggers::target_acquired(world, entity, target);
    }
    for (entity, event, other) in scratch.gaze.drain(..) {
        super::life::triggers::fire(world, entity, &event, other);
    }
    for entity in scratch.eaten.drain(..) {
        eat_block::finish_eating(world, entity);
    }
    for (attacker, target) in scratch.attacks.drain(..) {
        super::combat::melee::melee_attack(world, attacker, target);
    }
    for (entity, work) in scratch.gear.drain(..) {
        pickup::finish(world, entity, work);
    }
    break_door::tick_door_breakers(
        world,
        scratch
            .mobs
            .iter()
            .filter(|(_, turn)| *turn != Turn::Die)
            .map(|&(entity, _)| entity),
        tick,
        &mut scratch.boxes,
    );
}

/// The result of one mob's tick.
struct Output {
    yaw: f32,
    head_yaw: f32,
    velocity: DVec3,
    effects: Effects,
}

#[allow(clippy::too_many_arguments)]
fn think(
    world: &World,
    entity: Entity,
    turn: Turn,
    brain: &mut MobBrain,
    table: &PathBlockTable,
    rng: &mut EntityRandom,
    tick: u64,
    nearby: &Nearby,
) -> Option<Output> {
    let mob = world.entity(entity);
    let kind = mob.get::<MobKind>()?;
    let feet = mob.get::<Position>()?.0;
    let rotation = *mob.get::<Rotation>()?;
    let velocity = mob.get::<Velocity>()?.0;
    let on_ground = mob.get::<OnGround>().is_some_and(|g| g.0);
    let physics = mob.get::<PhysicsBody>()?;
    let baby = mob
        .get::<ActorFlags>()
        .is_some_and(|f| f.get(actor_flags::BABY).unwrap_or(false));
    let jump_power = mob
        .get::<JumpConfig>()
        .map_or(DEFAULT_JUMP_POWER, |j| j.jump_power);
    let max_turn = mob
        .get::<BasicMovementConfig>()
        .map_or(DEFAULT_MAX_TURN, |m| m.max_turn);
    let body = Body {
        feet,
        width: physics.width,
        height: physics.height,
        step_height: physics.step_height,
        on_ground,
        liquid: living_liquid(world, Aabb::from_feet(feet, physics.width, physics.height)),
        movement: mob.get::<MovementConfig>().map_or(0.0, |m| m.value),
    };
    let policy = WalkPolicy::new(
        mob.get::<WalkNavigationConfig>(),
        mob.get::<ActorFlags>()
            .is_some_and(|f| f.get(actor_flags::DOOR_BREAKER).unwrap_or(false)),
        jump_power,
        physics.gravity,
    );

    let MobBrain {
        selector,
        specs,
        states,
        nav,
        movement,
        look,
    } = brain;
    if turn == Turn::Think(true) {
        // A rebuilt composition rebuilds the native goals.
        states.melee.reset();
        states.targets.reset();
    }
    let mut effects = Effects::default();
    let mut ctx = GoalContext {
        world,
        table,
        entity,
        archetype: &kind.0,
        body,
        physics,
        policy,
        yaw: rotation.yaw,
        head_yaw: rotation.head_yaw,
        baby,
        tick,
        rng: &mut rng.0,
        nearby,
        states: &mut *states,
        nav: &mut *nav,
        movement: &mut *movement,
        look: &mut *look,
        jump: false,
        effects: &mut effects,
    };
    match turn {
        Turn::Die => {
            selector.stop_all(&mut ctx);
            ctx.effects.target = Some(None);
            return Some(Output {
                yaw: rotation.yaw,
                head_yaw: rotation.head_yaw,
                velocity,
                effects,
            });
        }
        Turn::Think(true) => {
            goal_specs(world, entity, specs);
            selector
                .update(specs, &mut ctx)
                .expect("goal ids are distinct per kind");
        }
        Turn::Think(false) => {}
    }
    selector.tick(&mut ctx);
    ctx.scan_gaze();
    let goal_jump = ctx.jump;

    let door_hold = nav.current().is_some_and(|(index, path)| {
        break_door::held_by_breakable_door(
            world,
            entity,
            table,
            path,
            index,
            Aabb::from_feet(feet, physics.width, physics.height),
        )
    });
    nav.tick(&body, movement, door_hold);
    let mut yaw = rotation.yaw;
    let move_jump = movement.tick(&body, &mut yaw, max_turn);
    let mut head_yaw = rotation.head_yaw;
    look.tick(feet + DVec3::Y * body.height, yaw, &mut head_yaw);
    let velocity = travel(
        &body,
        movement,
        goal_jump || move_jump,
        jump_power,
        yaw,
        living_block_friction(world, feet),
        velocity,
    );
    Some(Output {
        yaw,
        head_yaw,
        velocity,
        effects,
    })
}

fn apply(
    world: &mut World,
    entity: Entity,
    output: Output,
    eaten: &mut Vec<Entity>,
    attacks: &mut Vec<(Entity, Entity)>,
    gear: &mut Vec<(Entity, pickup::GearWork)>,
    acquired_events: &mut Vec<(Entity, Entity)>,
) {
    let Output {
        yaw,
        head_yaw,
        velocity,
        effects,
    } = output;
    let mut mob = world.entity_mut(entity);
    if let Some(mut rotation) = mob.get_mut::<Rotation>()
        && (rotation.yaw != yaw || rotation.head_yaw != head_yaw)
    {
        rotation.yaw = yaw;
        rotation.head_yaw = head_yaw;
    }
    if let Some(mut current) = mob.get_mut::<Velocity>()
        && current.0 != velocity
    {
        current.0 = velocity;
    }
    if effects.persistent && !mob.contains::<Persistent>() {
        mob.insert(Persistent);
    }
    let acquired = match effects.target {
        Some(Some(target)) => {
            let changed = mob
                .get::<AttackTarget>()
                .is_none_or(|before| before.0 != target);
            mob.insert(AttackTarget(target));
            changed.then_some(target)
        }
        Some(None) => {
            mob.remove::<AttackTarget>();
            None
        }
        None => None,
    };
    if let Some(tempted) = effects.tempted {
        set_flag(&mut mob, actor_flags::TEMPTED, tempted);
    }
    if let Some(target) = acquired {
        acquired_events.push((entity, target));
    }
    if let Some(request) = effects.mate
        && let Some(mut messages) = world.get_resource_mut::<Messages<MateRequest>>()
    {
        messages.write(request);
    }
    if effects.eat_started {
        super::interaction::interact::broadcast_actor_event(
            world,
            entity,
            jolyne::valentine::types::EnumsActorEvent::EatGrass,
            0,
        );
    }
    if effects.eat_finished {
        eaten.push(entity);
    }
    if let Some(target) = effects.attack {
        attacks.push((entity, target));
    }
    if !effects.gear.is_empty() {
        gear.push((entity, effects.gear));
    }
}

/// Set one actor flag and mirror the flag words into the metadata the
/// viewers receive.
fn set_flag(mob: &mut EntityWorldMut<'_>, flag: u32, on: bool) {
    use jolyne::valentine::actor_data_i_ds as ids;
    let Some(mut flags) = mob.get_mut::<ActorFlags>() else {
        return;
    };
    if !flags.update(flag, on).expect("catalogue flag index") {
        return;
    }
    let words = *flags.snapshot().words();
    if let Some(mut metadata) = mob.get_mut::<EntityMetadata>() {
        metadata.set_long(ids::RESERVED_0, words[0] as i64);
        metadata.set_long(ids::RESERVED_092, words[1] as i64);
    }
}

/// A pushing actor this tick ([`push_mobs`]).
#[derive(Debug, Clone, Copy)]
pub struct Pusher {
    /// The mob, or `None` for a player.
    entity: Option<Entity>,
    aabb: Aabb,
    centre: DVec3,
    /// Whether pushes move this actor (`pushable_by_entity`; players are
    /// pushable, but their own client moves them).
    pushable: bool,
}

/// System: actors near each other push each other apart, as BDS does once
/// per tick for every mob and player.
///
/// Each actor looks for every other actor whose box touches its own box
/// grown by [`ENTITY_PUSH_REACH`] horizontally. It pushes each pushable one
/// away from itself and, when both are pushable, is pushed back by the same
/// amount, so a pair of pushable mobs each receive two pushes per tick. A
/// push follows the vector between the two positions, scaled by
/// `1 / sqrt(max(|dx|, |dz|))` (capped at 1) and by
/// [`ENTITY_PUSH_STRENGTH`]; positions closer than
/// [`ENTITY_PUSH_MIN_DISTANCE`] on both axes do not push. Only mob
/// velocities change: players are moved by their own client.
#[allow(clippy::type_complexity)]
pub fn push_mobs(
    mut mobs: Query<
        (
            Entity,
            &Position,
            &PhysicsBody,
            Option<&Pushable>,
            &mut Velocity,
        ),
        (With<MobKind>, Without<Dead>),
    >,
    players: Query<&Position, With<Player>>,
    mut pushers: Local<Vec<Pusher>>,
    mut impulses: Local<Vec<DVec3>>,
) {
    pushers.clear();
    for (entity, position, body, pushable, _) in &mobs {
        pushers.push(Pusher {
            entity: Some(entity),
            aabb: Aabb::from_feet(position.0, body.width, body.height),
            centre: position.0,
            pushable: pushable.is_some_and(|p| p.by_entity),
        });
    }
    for position in &players {
        let feet = position.0 - DVec3::Y * PLAYER_EYE_HEIGHT;
        pushers.push(Pusher {
            entity: None,
            aabb: Aabb::from_feet(feet, PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT),
            centre: feet,
            pushable: true,
        });
    }
    if pushers.len() < 2 {
        return;
    }
    // Sweep along x so only actors within reach on that axis are compared.
    pushers.sort_by(|a, b| a.aabb.min.x.total_cmp(&b.aabb.min.x));
    impulses.clear();
    impulses.resize(pushers.len(), DVec3::ZERO);
    let reach = DVec3::new(ENTITY_PUSH_REACH, 0.0, ENTITY_PUSH_REACH);
    for i in 0..pushers.len() {
        let pusher = pushers[i];
        let area = pusher.aabb.inflate(reach);
        for (j, other) in pushers.iter().enumerate() {
            if other.aabb.min.x >= area.max.x {
                break;
            }
            if j == i || !other.pushable || !area.intersects(&other.aabb) {
                continue;
            }
            let Some(push) = push_impulse(other.centre, pusher.centre) else {
                continue;
            };
            impulses[j] += push;
            if pusher.pushable {
                impulses[i] -= push;
            }
        }
    }
    for (pusher, impulse) in pushers.iter().zip(impulses.iter()) {
        if let Some(entity) = pusher.entity
            && pusher.pushable
            && *impulse != DVec3::ZERO
            && let Ok((_, _, _, _, mut velocity)) = mobs.get_mut(entity)
        {
            velocity.0 += *impulse;
        }
    }
}

/// The push an actor at `pushed` receives from a pusher at `pusher`, if
/// they are far enough apart to push.
fn push_impulse(pushed: DVec3, pusher: DVec3) -> Option<DVec3> {
    let dx = pusher.x - pushed.x;
    let dz = pusher.z - pushed.z;
    let dominant = dx.abs().max(dz.abs());
    if dominant < ENTITY_PUSH_MIN_DISTANCE {
        return None;
    }
    let root = dominant.sqrt();
    let scale = (1.0 / root).min(1.0) / root * ENTITY_PUSH_STRENGTH;
    Some(DVec3::new(-dx * scale, 0.0, -dz * scale))
}
