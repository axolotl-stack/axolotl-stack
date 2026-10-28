//! The behaviour-pack goals a vanilla passive mob runs.
//!
//! Each authored `minecraft:behavior.*` component present on the mob becomes
//! one [`GoalSpec`] with its authored priority and the control flags of its
//! goal kind; [`GoalContext`] implements the selector callbacks and owns the
//! per-goal runtime state through [`GoalStates`].

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;
use rand::rngs::StdRng;

use super::control::{Body, LookControl, MoveControl, PathFollower, wrap_degrees, yaw_toward};
use super::nearby::{MobView, Nearby, PlayerView};
use super::path::{PathBlockTable, WalkPolicy, find_path, ground_below, random_targets};
use crate::entity::components::PhysicsBody;
use crate::entity::goal_selector::{GoalCallbacks, GoalControls, GoalId, GoalKind, GoalSpec};
use crate::entity::mob::config::{
    BreedGoal, BreedableConfig, EatBlockGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal,
    PanicGoal, RandomLookAroundGoal, RandomStrollGoal, TemptGoal, WalkNavigationConfig,
};
use crate::entity::mob::constants::{
    BREED_CHECK_TICKS, BREED_HEAD_TURN, BREED_MAX_PARTNER_DIST, BREED_SEARCH, BREED_TOUCH_DIST,
    FOLLOW_PARENT_MAX_DIST, FOLLOW_PARENT_MIN_DIST, FOLLOW_PARENT_REPATH_TICKS,
    FOLLOW_PARENT_SEARCH, LOOK_AT_PLAYER_HEAD_TURN, LOOK_IDLE_HEAD_TURN, PANIC_RANGE_XZ,
    PANIC_RANGE_Y, PANIC_RECENT_HURT_TICKS, PANIC_REPATH_TICKS, PLAYER_BOX_WIDTH,
    TEMPT_COOLDOWN_TICKS, TEMPT_DEFAULT_RADIUS, TEMPT_DEFAULT_STOP_DISTANCE, TEMPT_HEAD_TURN,
    TEMPT_REPATH_TICKS, TEMPT_SCARE_DISTANCE, TEMPT_SCARE_MOVE, TEMPT_SCARE_TURN,
};
use crate::entity::mob::events::held_item;
use crate::entity::mob::state::{InLove, LastHurt, MateRequest};
use crate::entity::mob::{MobArchetype, MobKind};
use crate::entity::navigation::{NavigationScratch, WalkNavigation};
use crate::registry::block::LiquidKind;
use crate::registry::collision::Aabb;

/// The goal kinds, numbered as their selector ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    Float,
    Panic,
    Breed,
    Tempt,
    FollowParent,
    RandomStroll,
    LookAtPlayer,
    RandomLookAround,
    EatBlock,
}

impl Goal {
    const ALL: [Goal; 9] = [
        Goal::Float,
        Goal::Panic,
        Goal::Breed,
        Goal::Tempt,
        Goal::FollowParent,
        Goal::RandomStroll,
        Goal::LookAtPlayer,
        Goal::RandomLookAround,
        Goal::EatBlock,
    ];

    pub fn id(self) -> GoalId {
        GoalId(self as u16)
    }

    fn from_id(id: GoalId) -> Self {
        Self::ALL[usize::from(id.0)]
    }

    /// The goal kind's controls; `behavior.eat_block` authors its own.
    fn controls(self) -> GoalControls {
        match self {
            Goal::Float => GoalControls::JUMP,
            Goal::Panic | Goal::FollowParent | Goal::RandomStroll => GoalControls::MOVE,
            Goal::Breed | Goal::Tempt | Goal::RandomLookAround => {
                GoalControls::MOVE | GoalControls::LOOK
            }
            Goal::LookAtPlayer => GoalControls::LOOK,
            Goal::EatBlock => crate::entity::mob::constants::EAT_BLOCK_DEFAULT_CONTROLS,
        }
    }
}

/// The goal specs of the mob's current composition, in a stable order.
pub fn goal_specs(world: &World, entity: Entity, out: &mut Vec<GoalSpec>) {
    out.clear();
    let entity = world.entity(entity);
    let priorities = [
        (Goal::Float, entity.get::<FloatGoal>().map(|g| g.priority)),
        (
            Goal::Panic,
            entity.get::<PanicGoal>().map(|g| g.base.priority),
        ),
        (
            Goal::Breed,
            entity.get::<BreedGoal>().map(|g| g.base.priority),
        ),
        (
            Goal::Tempt,
            entity.get::<TemptGoal>().map(|g| g.base.priority),
        ),
        (
            Goal::FollowParent,
            entity.get::<FollowParentGoal>().map(|g| g.base.priority),
        ),
        (
            Goal::RandomStroll,
            entity.get::<RandomStrollGoal>().map(|g| g.base.priority),
        ),
        (
            Goal::LookAtPlayer,
            entity.get::<LookAtPlayerGoal>().map(|g| g.priority),
        ),
        (
            Goal::RandomLookAround,
            entity.get::<RandomLookAroundGoal>().map(|g| g.priority),
        ),
    ];
    out.extend(priorities.into_iter().filter_map(|(goal, priority)| {
        Some(GoalSpec {
            id: goal.id(),
            kind: GoalKind::Normal,
            priority: priority?,
            controls: goal.controls(),
        })
    }));
    if let Some(goal) = entity.get::<EatBlockGoal>() {
        out.push(GoalSpec {
            id: Goal::EatBlock.id(),
            kind: GoalKind::Normal,
            priority: goal.priority,
            controls: goal.controls,
        });
    }
}

/// World writes the goals request; applied after the mob's tick.
#[derive(Debug, Default)]
pub struct Effects {
    pub persistent: bool,
    pub tempted: Option<bool>,
    pub mate: Option<MateRequest>,
    /// The mob lowered its head to eat (viewers get `EatGrass`).
    pub eat_started: bool,
    /// The eat timer ran out: eat the block now.
    pub eat_finished: bool,
}

#[derive(Debug, Default)]
struct TemptState {
    player: Option<Entity>,
    cooldown: u32,
    repath: u32,
    /// Player feet, yaw and pitch last tick (`can_get_scared`).
    last_seen: Option<(DVec3, f32, f32)>,
}

#[derive(Debug, Default)]
struct BreedState {
    partner: Option<Entity>,
    timer: u32,
}

/// Runtime state of every goal kind plus reusable search buffers.
#[derive(Debug, Default)]
pub struct GoalStates {
    panic_target: DVec3,
    panic_repath: u32,
    tempt: TemptState,
    parent: Option<Entity>,
    parent_repath: u32,
    look_player: Option<Entity>,
    look_ticks: i32,
    look_point: DVec3,
    breed: BreedState,
    /// Ticks until `behavior.eat_block` eats.
    eat_ticks: u32,
    /// A path prepared by `can_start`, installed by `start`.
    staging: WalkNavigation,
    scratch: NavigationScratch,
    candidates: Vec<(f32, DVec3)>,
    boxes: Vec<Aabb>,
}

/// Everything a goal reads and writes during one mob's tick.
pub struct GoalContext<'a> {
    pub world: &'a World,
    pub table: &'a PathBlockTable,
    pub entity: Entity,
    pub archetype: &'a Arc<MobArchetype>,
    pub body: Body,
    pub physics: &'a PhysicsBody,
    pub policy: WalkPolicy,
    pub yaw: f32,
    pub baby: bool,
    pub tick: u64,
    pub rng: &'a mut StdRng,
    pub nearby: &'a Nearby,
    pub states: &'a mut GoalStates,
    pub nav: &'a mut PathFollower,
    pub movement: &'a mut MoveControl,
    pub look: &'a mut LookControl,
    pub jump: bool,
    pub effects: &'a mut Effects,
}

fn view_range(duration: (i32, i32), rng: &mut StdRng) -> i32 {
    if duration.1 > duration.0 {
        rng.gen_range(duration.0..duration.1)
    } else {
        duration.0
    }
}

impl<'a> GoalContext<'a> {
    fn config<T: Component>(&self) -> Option<&'a T> {
        self.world.get::<T>(self.entity)
    }

    fn eye(&self) -> DVec3 {
        self.body.feet + DVec3::Y * self.body.height
    }

    /// Search a path to `goal` into the staging buffer.
    fn path_to(&mut self, goal: DVec3) -> bool {
        find_path(
            self.world,
            self.table,
            self.physics,
            self.policy,
            self.body.liquid == Some(LiquidKind::Water),
            self.body.feet,
            goal,
            &mut self.states.scratch,
            &mut self.states.staging,
        )
    }

    /// Search a path onto the ground under `point` into the staging buffer.
    fn path_toward(&mut self, point: DVec3) -> bool {
        let Some(goal) = ground_below(self.world, point, &mut self.states.boxes) else {
            return false;
        };
        self.path_to(goal)
    }

    fn follow_staged(&mut self, speed_multiplier: f32) {
        self.nav
            .install(&mut self.states.staging, speed_multiplier, self.body.feet);
    }

    /// Re-path toward `point`; an unreachable target keeps the current path.
    fn repath_toward(&mut self, point: DVec3, speed_multiplier: f32) {
        if self.path_toward(point) {
            self.follow_staged(speed_multiplier);
        }
    }

    /// Prepare a path to the best reachable random position.
    fn prepare_random_path(&mut self, xz: i32, y: i32, away: Option<DVec3>) -> Option<DVec3> {
        let mut candidates = std::mem::take(&mut self.states.candidates);
        random_targets(
            self.world,
            self.table,
            self.rng,
            self.body.feet,
            xz,
            y,
            away,
            self.policy.avoid_water,
            &mut self.states.boxes,
            &mut candidates,
        );
        let found = candidates
            .iter()
            .map(|(_, target)| *target)
            .find(|target| self.path_to(*target));
        self.states.candidates = candidates;
        found
    }

    fn player(&self, entity: Option<Entity>) -> Option<PlayerView> {
        PlayerView::of(self.world, entity?)
    }

    fn mob(&self, entity: Option<Entity>) -> Option<MobView> {
        MobView::of(self.world, entity?, self.tick).filter(|m| !m.dead)
    }

    /// Whether `mob` has the same archetype as this mob.
    fn same_kind(&self, mob: Entity) -> bool {
        self.world
            .get::<MobKind>(mob)
            .is_some_and(|kind| Arc::ptr_eq(&kind.0, self.archetype))
    }

    fn holds_tempt_item(&self, goal: &TemptGoal, player: Entity) -> bool {
        held_item(self.world, player).is_some_and(|stack| {
            goal.items
                .iter()
                .any(|item| item.matches(&stack.item_id, stack.damage))
        })
    }

    fn tempt_radius(goal: &TemptGoal) -> f64 {
        f64::from(if goal.within_radius > 0.0 {
            goal.within_radius
        } else {
            TEMPT_DEFAULT_RADIUS
        })
    }

    fn in_view(&self, goal: &LookAtPlayerGoal, player: &PlayerView) -> bool {
        let delta = player.eye - self.eye();
        if goal.half_angle_horizontal < 180.0 {
            let off = wrap_degrees(yaw_toward(delta) - self.yaw).abs();
            if off > goal.half_angle_horizontal {
                return false;
            }
        }
        if goal.half_angle_vertical < 180.0 {
            let horizontal = (delta.x * delta.x + delta.z * delta.z).sqrt();
            let pitch = delta.y.atan2(horizontal).to_degrees().abs() as f32;
            if pitch > goal.half_angle_vertical {
                return false;
            }
        }
        true
    }

    fn breed_partner(&self) -> Option<Entity> {
        let breedable = self.config::<BreedableConfig>();
        let search = self.aabb().inflate(DVec3::splat(BREED_SEARCH));
        self.nearby
            .mobs_in(search)
            .filter(|m| {
                m.entity != self.entity
                    && m.in_love
                    && !m.dead
                    && !m.baby
                    && self
                        .world
                        .get::<MobKind>(m.entity)
                        .is_some_and(|kind| compatible(self.archetype, breedable, &kind.0))
            })
            .min_by(|a, b| {
                a.feet
                    .distance_squared(self.body.feet)
                    .total_cmp(&b.feet.distance_squared(self.body.feet))
            })
            .map(|m| m.entity)
    }

    fn aabb(&self) -> Aabb {
        Aabb::from_feet(self.body.feet, self.body.width, self.body.height)
    }

    fn self_in_love(&self) -> bool {
        self.world
            .get::<InLove>(self.entity)
            .is_some_and(|love| self.tick < love.until_tick)
    }
}

/// Whether a mob may mate with a `partner`: an identifier listed in
/// `breeds_with`, or the same archetype when nothing is listed.
fn compatible(
    own: &Arc<MobArchetype>,
    breedable: Option<&BreedableConfig>,
    partner: &Arc<MobArchetype>,
) -> bool {
    match breedable {
        Some(config) if !config.breeds_with.is_empty() => config
            .breeds_with
            .iter()
            .any(|id| *id == partner.identifier),
        _ => Arc::ptr_eq(own, partner),
    }
}

/// Squared distance between two boxes (zero when they overlap).
fn box_gap_sq(a: &Aabb, b: &Aabb) -> f64 {
    let gap = (a.min - b.max).max(b.min - a.max).max(DVec3::ZERO);
    gap.length_squared()
}

impl GoalCallbacks for GoalContext<'_> {
    fn can_start(&mut self, id: GoalId) -> bool {
        match Goal::from_id(id) {
            Goal::Float => self.config::<FloatGoal>().is_some() && self.body.liquid.is_some(),
            Goal::Panic => {
                let Some(goal) = self.config::<PanicGoal>() else {
                    return false;
                };
                let hurt = self.world.get::<LastHurt>(self.entity).copied();
                let attacker = hurt
                    .and_then(|h| h.attacker)
                    .and_then(|a| self.world.get::<crate::entity::components::Position>(a))
                    .map(|p| p.0);
                let triggered = goal.force
                    || hurt.is_some_and(|h| {
                        let recent = self.tick.saturating_sub(h.tick) <= PANIC_RECENT_HURT_TICKS;
                        let by_mob = attacker.is_some() && !goal.ignore_mob_damage;
                        let by_cause = goal
                            .damage_sources
                            .as_ref()
                            .is_none_or(|causes| causes.contains(&h.cause));
                        recent && (by_mob || by_cause)
                    });
                if !triggered {
                    return false;
                }
                match self.prepare_random_path(PANIC_RANGE_XZ, PANIC_RANGE_Y, attacker) {
                    Some(target) => {
                        self.states.panic_target = target;
                        true
                    }
                    None => false,
                }
            }
            Goal::RandomStroll => {
                let Some(goal) = self.config::<RandomStrollGoal>() else {
                    return false;
                };
                let (interval, xz, y) = (goal.interval, goal.xz_dist, goal.y_dist);
                if interval > 0 && self.rng.gen_range(0..interval) != 0 {
                    return false;
                }
                self.prepare_random_path(xz, y, None).is_some()
            }
            Goal::Tempt => {
                let Some(goal) = self.config::<TemptGoal>() else {
                    return false;
                };
                if self.states.tempt.cooldown > 0 {
                    self.states.tempt.cooldown -= 1;
                    return false;
                }
                let radius = Self::tempt_radius(goal);
                let feet = self.body.feet;
                let nearest = self
                    .nearby
                    .players_within(feet, radius)
                    .filter(|p| self.holds_tempt_item(goal, p.entity))
                    .min_by(|a, b| {
                        a.feet
                            .distance_squared(feet)
                            .total_cmp(&b.feet.distance_squared(feet))
                    })
                    .copied();
                match nearest {
                    Some(player) => {
                        self.states.tempt.player = Some(player.entity);
                        self.states.tempt.last_seen = Some((player.feet, player.yaw, player.pitch));
                        true
                    }
                    None => false,
                }
            }
            Goal::FollowParent => {
                if self.config::<FollowParentGoal>().is_none() || !self.baby {
                    return false;
                }
                let (sx, sy, sz) = FOLLOW_PARENT_SEARCH;
                let feet = self.body.feet;
                let search = self.aabb().inflate(DVec3::new(sx, sy, sz));
                let parent = self
                    .nearby
                    .mobs_in(search)
                    .filter(|m| {
                        m.entity != self.entity && !m.baby && !m.dead && self.same_kind(m.entity)
                    })
                    .min_by(|a, b| {
                        a.feet
                            .distance_squared(feet)
                            .total_cmp(&b.feet.distance_squared(feet))
                    });
                match parent {
                    Some(parent)
                        if parent.feet.distance_squared(feet)
                            >= FOLLOW_PARENT_MIN_DIST * FOLLOW_PARENT_MIN_DIST =>
                    {
                        self.states.parent = Some(parent.entity);
                        true
                    }
                    _ => false,
                }
            }
            Goal::LookAtPlayer => {
                let Some(goal) = self.config::<LookAtPlayerGoal>() else {
                    return false;
                };
                if self.rng.r#gen::<f32>() >= goal.probability {
                    return false;
                }
                let range = f64::from(goal.look_distance);
                let feet = self.body.feet;
                let target = self
                    .nearby
                    .players_within(feet, range)
                    .filter(|p| self.in_view(goal, p))
                    .min_by(|a, b| {
                        a.feet
                            .distance_squared(feet)
                            .total_cmp(&b.feet.distance_squared(feet))
                    })
                    .map(|p| p.entity);
                self.states.look_player = target;
                target.is_some()
            }
            Goal::RandomLookAround => {
                let Some(goal) = self.config::<RandomLookAroundGoal>() else {
                    return false;
                };
                self.rng.r#gen::<f32>() < goal.probability
            }
            Goal::Breed => {
                if self.config::<BreedGoal>().is_none() || self.baby || !self.self_in_love() {
                    return false;
                }
                let partner = self.breed_partner();
                self.states.breed.partner = partner;
                partner.is_some()
            }
            Goal::EatBlock => {
                let Some(goal) = self.config::<EatBlockGoal>() else {
                    return false;
                };
                if self.config::<WalkNavigationConfig>().is_none() {
                    return false;
                }
                let roll = self.rng.r#gen::<f32>();
                let chance = super::eat_block::success_chance(goal, self.baby, self.rng);
                if roll >= chance {
                    return false;
                }
                let feet = self.body.feet.floor().as_ivec3();
                super::eat_block::edible(self.world, goal, feet).is_some()
                    || super::eat_block::edible(self.world, goal, feet - glam::IVec3::Y).is_some()
            }
        }
    }

    fn can_continue(&mut self, id: GoalId) -> bool {
        match Goal::from_id(id) {
            Goal::Float => self.body.liquid.is_some(),
            Goal::Panic | Goal::RandomStroll => !self.nav.is_done(),
            Goal::Tempt => {
                let Some(goal) = self.config::<TemptGoal>() else {
                    return false;
                };
                let Some(player) = self.player(self.states.tempt.player) else {
                    return false;
                };
                if goal.can_get_scared
                    && let Some((feet, yaw, pitch)) = self.states.tempt.last_seen
                    && player.feet.distance_squared(self.body.feet)
                        < TEMPT_SCARE_DISTANCE * TEMPT_SCARE_DISTANCE
                    && (player.feet.distance_squared(feet) > TEMPT_SCARE_MOVE * TEMPT_SCARE_MOVE
                        || wrap_degrees(player.yaw - yaw).abs() > TEMPT_SCARE_TURN
                        || (player.pitch - pitch).abs() > TEMPT_SCARE_TURN)
                {
                    return false;
                }
                self.states.tempt.last_seen = Some((player.feet, player.yaw, player.pitch));
                let radius = Self::tempt_radius(goal);
                player.feet.distance_squared(self.body.feet) <= radius * radius
                    && self.holds_tempt_item(goal, player.entity)
            }
            Goal::FollowParent => {
                let Some(parent) = self.mob(self.states.parent) else {
                    return false;
                };
                let distance_sq = parent.feet.distance_squared(self.body.feet);
                (FOLLOW_PARENT_MIN_DIST * FOLLOW_PARENT_MIN_DIST
                    ..=FOLLOW_PARENT_MAX_DIST * FOLLOW_PARENT_MAX_DIST)
                    .contains(&distance_sq)
            }
            Goal::LookAtPlayer => {
                let Some(goal) = self.config::<LookAtPlayerGoal>() else {
                    return false;
                };
                let Some(player) = self.player(self.states.look_player) else {
                    return false;
                };
                let range = f64::from(goal.look_distance);
                self.states.look_ticks > 0
                    && player.feet.distance_squared(self.body.feet) <= range * range
                    && self.in_view(goal, &player)
            }
            Goal::RandomLookAround => self.states.look_ticks > 0,
            Goal::EatBlock => self.states.eat_ticks > 0,
            Goal::Breed => {
                let Some(partner) = self.mob(self.states.breed.partner) else {
                    return false;
                };
                if !partner.in_love || !self.self_in_love() {
                    return false;
                }
                self.states.breed.timer < BREED_CHECK_TICKS
                    || partner.feet.distance_squared(self.body.feet)
                        <= BREED_MAX_PARTNER_DIST * BREED_MAX_PARTNER_DIST
            }
        }
    }

    fn interruptible(&mut self, _id: GoalId) -> bool {
        true
    }

    fn start(&mut self, id: GoalId) {
        match Goal::from_id(id) {
            Goal::Float => {}
            Goal::Panic => {
                let speed = self
                    .config::<PanicGoal>()
                    .map_or(1.0, |g| g.base.speed_multiplier);
                self.follow_staged(speed);
                self.states.panic_repath = PANIC_REPATH_TICKS;
            }
            Goal::RandomStroll => {
                let speed = self
                    .config::<RandomStrollGoal>()
                    .map_or(1.0, |g| g.base.speed_multiplier);
                self.follow_staged(speed);
            }
            Goal::Tempt => {
                self.effects.persistent = true;
                self.effects.tempted = Some(true);
                self.states.tempt.repath = 0;
            }
            Goal::FollowParent => self.states.parent_repath = 0,
            Goal::LookAtPlayer => {
                let ticks = self
                    .config::<LookAtPlayerGoal>()
                    .map_or((0, 0), |g| g.look_ticks);
                self.states.look_ticks = view_range(ticks, self.rng);
            }
            Goal::RandomLookAround => {
                let Some(goal) = self.config::<RandomLookAroundGoal>() else {
                    return;
                };
                let (min, max) = goal.angle_range;
                let ticks = goal.look_ticks;
                let offset = if max > min {
                    self.rng.gen_range(min..=max)
                } else {
                    min
                };
                let yaw = f64::from(self.yaw + offset).to_radians();
                self.states.look_point = self.eye() + DVec3::new(-yaw.sin(), 0.0, yaw.cos());
                self.states.look_ticks = view_range(ticks, self.rng);
                // The goal also owns movement: the mob stands still.
                self.nav.stop(self.movement);
            }
            Goal::Breed => self.states.breed.timer = 0,
            Goal::EatBlock => {
                self.states.eat_ticks = self.config::<EatBlockGoal>().map_or(0, |g| g.eat_ticks);
                self.effects.eat_started = true;
                self.nav.stop(self.movement);
            }
        }
    }

    fn stop(&mut self, id: GoalId) {
        match Goal::from_id(id) {
            Goal::Float => {}
            Goal::Panic | Goal::RandomStroll => self.nav.stop(self.movement),
            Goal::FollowParent => {
                self.nav.stop(self.movement);
                self.states.parent = None;
            }
            Goal::Breed => {
                self.nav.stop(self.movement);
                self.states.breed = BreedState::default();
            }
            Goal::Tempt => {
                self.nav.stop(self.movement);
                self.states.tempt.player = None;
                self.states.tempt.last_seen = None;
                self.states.tempt.cooldown = TEMPT_COOLDOWN_TICKS;
                self.effects.tempted = Some(false);
            }
            Goal::LookAtPlayer => self.states.look_player = None,
            Goal::RandomLookAround => self.states.look_ticks = 0,
            Goal::EatBlock => self.states.eat_ticks = 0,
        }
    }

    fn tick(&mut self, id: GoalId) {
        match Goal::from_id(id) {
            Goal::Float => {
                let chance = self
                    .config::<FloatGoal>()
                    .map_or(0.0, |g| g.chance_per_tick);
                if self.rng.r#gen::<f32>() < chance {
                    self.jump = true;
                }
            }
            Goal::Panic => {
                self.states.panic_repath = self.states.panic_repath.saturating_sub(1);
                if self.states.panic_repath == 0 {
                    self.states.panic_repath = PANIC_REPATH_TICKS;
                    let speed = self
                        .config::<PanicGoal>()
                        .map_or(1.0, |g| g.base.speed_multiplier);
                    let target = self.states.panic_target;
                    if self.path_to(target) {
                        self.follow_staged(speed);
                    }
                }
            }
            Goal::RandomStroll => {}
            Goal::EatBlock => {
                self.states.eat_ticks = self.states.eat_ticks.max(1) - 1;
                if self.states.eat_ticks == 0 {
                    self.effects.eat_finished = true;
                }
            }
            Goal::Tempt => {
                let Some(goal) = self.config::<TemptGoal>() else {
                    return;
                };
                let Some(player) = self.player(self.states.tempt.player) else {
                    return;
                };
                let (speed, vertical) = (goal.base.speed_multiplier, goal.can_tempt_vertically);
                let stop = if goal.stop_distance < 0.0 {
                    TEMPT_DEFAULT_STOP_DISTANCE
                } else {
                    goal.stop_distance
                };
                self.look.look_at(player.eye, TEMPT_HEAD_TURN);
                let mut delta = player.feet - self.body.feet;
                if !vertical {
                    delta.y = 0.0;
                }
                let close = (PLAYER_BOX_WIDTH + self.body.width) * 0.5 + f64::from(stop);
                if delta.length_squared() < close * close {
                    self.nav.stop(self.movement);
                    return;
                }
                if self.states.tempt.repath == 0 || self.nav.is_done() {
                    self.states.tempt.repath = TEMPT_REPATH_TICKS + self.rng.gen_range(0..=3);
                    self.repath_toward(player.feet, speed);
                } else {
                    self.states.tempt.repath -= 1;
                }
            }
            Goal::FollowParent => {
                let Some(parent) = self.mob(self.states.parent).map(|m| m.feet) else {
                    return;
                };
                if self.states.parent_repath > 0 {
                    self.states.parent_repath -= 1;
                    return;
                }
                self.states.parent_repath = FOLLOW_PARENT_REPATH_TICKS;
                let speed = self
                    .config::<FollowParentGoal>()
                    .map_or(1.0, |g| g.base.speed_multiplier);
                self.repath_toward(parent, speed);
            }
            Goal::LookAtPlayer => {
                if let Some(player) = self.player(self.states.look_player) {
                    self.look.look_at(player.eye, LOOK_AT_PLAYER_HEAD_TURN);
                }
                self.states.look_ticks -= 1;
            }
            Goal::RandomLookAround => {
                self.look
                    .look_at(self.states.look_point, LOOK_IDLE_HEAD_TURN);
                self.states.look_ticks -= 1;
            }
            Goal::Breed => {
                let Some(partner) = self.mob(self.states.breed.partner) else {
                    return;
                };
                self.look
                    .look_at(partner.feet + DVec3::Y * partner.height, BREED_HEAD_TURN);
                let speed = self
                    .config::<BreedGoal>()
                    .map_or(1.0, |g| g.base.speed_multiplier);
                self.repath_toward(partner.feet, speed);
                self.states.breed.timer += 1;
                if self.states.breed.timer >= BREED_CHECK_TICKS {
                    if box_gap_sq(&self.aabb(), &partner.aabb())
                        <= BREED_TOUCH_DIST * BREED_TOUCH_DIST
                    {
                        self.effects.mate = Some(MateRequest {
                            a: self.entity,
                            b: partner.entity,
                        });
                    }
                    if partner.feet.distance_squared(self.body.feet)
                        <= BREED_MAX_PARTNER_DIST * BREED_MAX_PARTNER_DIST
                    {
                        self.states.breed.timer = 0;
                    }
                }
            }
        }
    }
}
