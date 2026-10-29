//! Target goals: `minecraft:behavior.hurt_by_target` and
//! `minecraft:behavior.nearest_attackable_target`, run in the target phase
//! of the goal selector before the normal goals.
//!
//! Both share the native target-goal rules ([`can_attack`],
//! [`keep_target`]): a target must be another living actor, a player only
//! outside Peaceful and when not invulnerable (creative, spectator), and
//! it must match one `entity_types` entry (within the entry's `max_dist`,
//! its filter passing with the candidate as `other`, seen when `must_see`).
//! A kept target is dropped once it is dead, beyond the follow range,
//! unseen for the forget time, or invulnerable. The chosen target is
//! mirrored to [`crate::entity::mob::state::AttackTarget`] for the rest of
//! the server.

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;

use super::goals::GoalContext;
use super::sensing::can_see;
use crate::entity::components::{
    ArmourInventory, EffectType, Effects, GameMode, Health, PhysicsBody, Player, PlayerState,
    Position, RuntimeId,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::hostile::{TargetEntry, TargetGoalBase};
use crate::entity::mob::components::{HurtByTargetGoal, NearestAttackableTargetGoal};
use crate::entity::mob::constants::{
    DEFAULT_FOLLOW_RANGE, DISGUISED_PLAYER_RANGE_FACTOR, HURT_BY_TARGET_MEMORY_TICKS,
    INVISIBLE_MIN_ARMOR_COVER, MOB_EYE_HEIGHT_RATIO, MOB_HEAD_DISGUISES, PLAYER_BOX_HEIGHT,
    PLAYER_BOX_WIDTH, TARGET_REACH_CHECK_BASE, TARGET_REACH_CHECK_SPREAD, TARGET_REACH_DIST_SQ,
};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::entity::mob::state::{Dead, LastHurt};
use crate::registry::collision::Aabb;
use crate::world::rules::Difficulty;
use jolyne::valentine::actor_flags;

/// Which target goal holds the current target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSource {
    HurtBy,
    Nearest,
}

/// Runtime state of the target goals.
#[derive(Debug, Default)]
pub struct TargetStates {
    /// The mob's current target (mirrored to `AttackTarget`).
    pub target: Option<Entity>,
    /// The goal that set it.
    source: Option<TargetSource>,
    /// The target `can_start` found, adopted by `start`.
    staged: Option<Entity>,
    /// The entry of the nearest-attackable-target goal the target matched.
    entry: Option<usize>,
    /// Effective `must_see` and forget time for the current target.
    must_see: bool,
    forget_ticks: i32,
    /// Consecutive ticks the target was not seen.
    unseen: i32,
    /// Re-check the target against the entries every tick
    /// (`reevaluate_description`).
    reevaluate: bool,
    /// `persist_time`: the target no longer matches but is kept.
    persisting: bool,
    persist_ticks: i32,
    /// `must_reach` cache: 0 unknown, 1 reachable, 2 unreachable; and the
    /// ticks left before it is recomputed.
    reach: i32,
    reach_countdown: i32,
    /// Per-entry cooldown deadlines (`cooldown`).
    cooldowns: Vec<(usize, u64)>,
}

/// An actor as the target goals see it.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub entity: Entity,
    /// The native actor position: a player's eye, a mob's feet.
    pub pos: DVec3,
    pub eye: DVec3,
    pub aabb: Aabb,
    pub player: bool,
}

impl Candidate {
    /// The current view of a living player or mob.
    pub fn of(world: &World, entity: Entity) -> Option<Self> {
        let actor = world.get_entity(entity).ok()?;
        if actor.contains::<Dead>() || actor.get::<Health>().is_some_and(Health::is_dead) {
            return None;
        }
        if actor.contains::<Player>() {
            let eye = actor.get::<Position>()?.0;
            let feet = eye - DVec3::Y * PLAYER_EYE_HEIGHT;
            return Some(Self {
                entity,
                pos: eye,
                eye,
                aabb: Aabb::from_feet(feet, PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT),
                player: true,
            });
        }
        actor.get::<MobKind>()?;
        let feet = actor.get::<Position>()?.0;
        let body = actor.get::<PhysicsBody>()?;
        Some(Self {
            entity,
            pos: feet,
            eye: feet + DVec3::Y * body.height * MOB_EYE_HEIGHT_RATIO,
            aabb: Aabb::from_feet(feet, body.width, body.height),
            player: false,
        })
    }
}

/// The fraction of `entity`'s four armour slots that hold an item.
fn armour_cover(world: &World, entity: Entity) -> f32 {
    let worn = if let Some(armour) = world.get::<ArmourInventory>(entity) {
        armour
            .slots()
            .iter()
            .filter(|stack| !stack.is_empty())
            .count()
    } else if let Some(gear) = world.get::<MobEquipment>(entity) {
        [
            GearSlot::Head,
            GearSlot::Chest,
            GearSlot::Legs,
            GearSlot::Feet,
        ]
        .into_iter()
        .filter(|&slot| !gear.get(slot).is_empty())
        .count()
    } else {
        0
    };
    worn as f32 / 4.0
}

impl GoalContext<'_> {
    pub(super) fn self_eye(&self) -> DVec3 {
        self.body.feet + DVec3::Y * self.body.height * MOB_EYE_HEIGHT_RATIO
    }

    pub(super) fn sees(&self, target: &Candidate) -> bool {
        can_see(self.world, self.self_eye(), target.eye)
    }

    fn difficulty(&self) -> Difficulty {
        self.world
            .get_resource::<Difficulty>()
            .copied()
            .unwrap_or_default()
    }

    /// A player the native rules never target: invulnerable (creative,
    /// spectator) or the world is peaceful.
    fn untargetable_player(&self, player: Entity) -> bool {
        matches!(
            self.world.get::<GameMode>(player),
            Some(GameMode::Creative | GameMode::Spectator)
        ) || self.difficulty() == Difficulty::Peaceful
    }

    /// The follow range: the nearest-attackable-target goal's
    /// `within_radius`, else the native default.
    fn follow_range(&self) -> f64 {
        f64::from(
            self.config::<NearestAttackableTargetGoal>()
                .map_or(DEFAULT_FOLLOW_RANGE, |goal| goal.follow_range),
        )
    }

    /// Set (or clear) the target; mirrored to `AttackTarget` after the tick.
    pub(super) fn set_target(&mut self, target: Option<Entity>) {
        self.states.targets.target = target;
        self.effects.target = Some(target);
    }

    /// The entries `target` matches, best (largest `max_dist`) first, and
    /// whether it passes the goal's sight rule (the native type match).
    fn matches(&self, base: &TargetGoalBase, target: &Candidate, current: bool) -> Option<usize> {
        let distance_sq = self.body.feet.distance_squared(target.pos);
        // A current target unseen for less than the forget time needs no
        // fresh sighting.
        let remembered = current
            && self.states.targets.must_see
            && self.states.targets.unseen < self.states.targets.forget_ticks;
        let mut best: Option<usize> = None;
        for (index, entry) in base.entries.iter().enumerate() {
            let max = f64::from(entry.max_dist);
            if distance_sq >= max * max {
                continue;
            }
            if !remembered && entry.must_see == Some(true) && !self.sees(target) {
                continue;
            }
            let facts = MobFacts {
                world: self.world,
                entity: self.entity,
                other: Some(target.entity),
            };
            if !entry.filter.evaluate(&facts) {
                continue;
            }
            if best.is_none_or(|b| base.entries[b].max_dist < entry.max_dist) {
                best = Some(index);
            }
        }
        let entry = &base.entries[best?];
        if entry.must_see.is_none() && base.must_see && !remembered && !self.sees(target) {
            return None;
        }
        best
    }

    /// The native `_canAttack`: whether `target` may become (or stay) the
    /// target of a goal with `base`.
    fn can_attack(
        &mut self,
        base: &TargetGoalBase,
        target: &Candidate,
        current: bool,
    ) -> Option<usize> {
        if target.entity == self.entity {
            return None;
        }
        if target.player && self.untargetable_player(target.entity) {
            return None;
        }
        let entry = self.matches(base, target, current)?;
        if base.must_reach && !self.reachable(target) {
            return None;
        }
        Some(entry)
    }

    /// `must_reach`: a path ends within reach of the target, cached for a
    /// few ticks.
    fn reachable(&mut self, target: &Candidate) -> bool {
        let states = &mut self.states.targets;
        states.reach_countdown -= 1;
        if states.reach_countdown < 1 {
            states.reach = 0;
        }
        if states.reach == 0 {
            states.reach_countdown =
                TARGET_REACH_CHECK_BASE + self.rng.gen_range(0..TARGET_REACH_CHECK_SPREAD);
            let feet = target.aabb.min
                + DVec3::new(
                    (target.aabb.max.x - target.aabb.min.x) / 2.0,
                    0.0,
                    (target.aabb.max.z - target.aabb.min.z) / 2.0,
                );
            let reached = self.path_toward(feet)
                && self.states.staging.waypoints.last().is_some_and(|end| {
                    let dx = end.feet.x.floor() - target.pos.x.floor();
                    let dz = end.feet.z.floor() - target.pos.z.floor();
                    dx * dx + dz * dz < TARGET_REACH_DIST_SQ
                });
            self.states.targets.reach = if reached { 1 } else { 2 };
        }
        self.states.targets.reach != 2
    }

    /// Visibility for the nearest-attackable-target goal: within the
    /// entry's `max_dist` (and `max_height`), shortened for sneaking or
    /// invisible players.
    fn target_visible(
        &self,
        goal: &NearestAttackableTargetGoal,
        target: &Candidate,
        entry: &TargetEntry,
    ) -> bool {
        let mut multiplier = 1.0f32;
        if target.player
            && self
                .world
                .get::<PlayerState>(target.entity)
                .is_some_and(|state| state.sneaking)
        {
            multiplier = goal.target_sneak_visibility_multiplier;
        }
        if self
            .world
            .get::<Effects>(target.entity)
            .is_some_and(|effects| effects.has(EffectType::INVISIBILITY))
        {
            multiplier *= goal.target_invisible_multiplier
                * armour_cover(self.world, target.entity).max(INVISIBLE_MIN_ARMOR_COVER);
        }
        if target.player && self.disguised(target.entity) {
            multiplier *= DISGUISED_PLAYER_RANGE_FACTOR;
        }
        let delta = target.pos - self.body.feet;
        let max_dist = f64::from(entry.max_dist * multiplier);
        let max_height = f64::from(entry.max_height * multiplier);
        (entry.max_dist < 0.0 || delta.length_squared() <= max_dist * max_dist)
            && (entry.max_height < 0.0 || delta.y.abs() <= max_height)
    }

    /// `player` wears the head of this mob's kind.
    fn disguised(&self, player: Entity) -> bool {
        let Some(armour) = self.world.get::<ArmourInventory>(player) else {
            return false;
        };
        let helmet = armour.helmet().item_id();
        MOB_HEAD_DISGUISES
            .iter()
            .any(|&(mob, head)| mob == &*self.archetype.identifier && head == helmet)
    }

    /// The nearest actor the nearest-attackable-target goal may target.
    fn find_target(&mut self, goal: &NearestAttackableTargetGoal) -> Option<(Entity, usize)> {
        let range = goal
            .base
            .entries
            .iter()
            .map(|entry| entry.max_dist)
            .fold(0.0f32, f32::max);
        let height = if goal.target_search_height < 0.0 {
            range
        } else {
            goal.target_search_height
        };
        let feet = self.body.feet;
        let area = self.aabb().inflate(DVec3::new(
            f64::from(range),
            f64::from(height),
            f64::from(range),
        ));
        let mut candidates: Vec<Candidate> = self
            .nearby
            .players_within(feet, f64::from(range) + f64::from(height))
            .map(|p| p.entity)
            .chain(self.nearby.mobs_in(area).map(|m| m.entity))
            .filter_map(|entity| Candidate::of(self.world, entity))
            .filter(|c| c.aabb.intersects(&area))
            .collect();
        candidates.sort_by(|a, b| {
            a.pos
                .distance_squared(feet)
                .total_cmp(&b.pos.distance_squared(feet))
        });
        for candidate in candidates {
            let Some(index) = self.can_attack(&goal.base, &candidate, false) else {
                continue;
            };
            let entry = &goal.base.entries[index];
            if self
                .states
                .targets
                .cooldowns
                .iter()
                .any(|(i, until)| *i == index && self.tick < *until)
            {
                continue;
            }
            if !self.target_visible(goal, &candidate, entry) {
                continue;
            }
            if goal.set_persistent {
                self.effects.persistent = true;
            }
            return Some((candidate.entity, index));
        }
        None
    }

    /// The native `_selectTarget`: roll the acquisition probability, find
    /// the nearest valid target and start its entry's cooldown.
    fn select_target(&mut self, goal: &NearestAttackableTargetGoal) -> Option<Entity> {
        if goal.target_acquisition_probability < 1.0
            && self.rng.r#gen::<f32>() >= goal.target_acquisition_probability
        {
            return None;
        }
        let (target, index) = self.find_target(goal)?;
        let entry = &goal.base.entries[index];
        let until = self.tick + entry.cooldown_ticks.max(0) as u64;
        let cooldowns = &mut self.states.targets.cooldowns;
        match cooldowns.iter_mut().find(|(i, _)| *i == index) {
            Some(slot) => slot.1 = until,
            None => cooldowns.push((index, until)),
        }
        self.states.targets.entry = Some(index);
        Some(target)
    }

    /// Adopt the sight rule of `entry` (or of the goal) for a new target.
    fn adopt(&mut self, base: &TargetGoalBase, entry: Option<usize>) {
        let states = &mut self.states.targets;
        match entry.and_then(|i| base.entries.get(i)) {
            Some(entry) if entry.must_see.is_some() => {
                states.must_see = entry.must_see == Some(true);
                states.forget_ticks = entry.must_see_forget_ticks;
                states.reevaluate = entry.reevaluate_description;
            }
            Some(entry) => {
                states.must_see = base.must_see;
                states.forget_ticks = base.must_see_forget_ticks;
                states.reevaluate = entry.reevaluate_description;
            }
            None => {
                states.must_see = base.must_see;
                states.forget_ticks = base.must_see_forget_ticks;
                states.reevaluate = false;
            }
        }
    }

    /// Native target-goal start: fresh sight and reach bookkeeping.
    fn start_targeting(&mut self) {
        let states = &mut self.states.targets;
        states.reach = 0;
        states.reach_countdown = 0;
        states.unseen = 0;
        states.persisting = false;
        states.persist_ticks = 0;
    }

    /// The native `TargetGoal::canContinueToUse`; drops the target when it
    /// fails.
    fn keep_target(&mut self, base: &TargetGoalBase) -> bool {
        let keep = self.keep_target_inner(base);
        if !keep {
            self.set_target(None);
        }
        keep
    }

    fn keep_target_inner(&mut self, base: &TargetGoalBase) -> bool {
        let Some(target) = self
            .states
            .targets
            .target
            .and_then(|t| Candidate::of(self.world, t))
        else {
            return false;
        };
        if self
            .world
            .get::<ActorFlags>(target.entity)
            .is_some_and(|flags| flags.get(actor_flags::PLAYING_DEAD).unwrap_or(false))
        {
            return false;
        }
        if target.player && self.difficulty() == Difficulty::Peaceful {
            return false;
        }
        let range = self.follow_range();
        if self.body.feet.distance_squared(target.pos) > range * range {
            return false;
        }
        if self.states.targets.must_see {
            if self.sees(&target) {
                self.states.targets.unseen = 0;
            } else {
                let unseen = self.states.targets.unseen;
                self.states.targets.unseen += 1;
                if unseen >= self.states.targets.forget_ticks {
                    return false;
                }
            }
        }
        if target.player
            && matches!(
                self.world.get::<GameMode>(target.entity),
                Some(GameMode::Creative | GameMode::Spectator)
            )
        {
            return false;
        }
        if !self.states.targets.reevaluate {
            return true;
        }
        if self.matches(base, &target, true).is_some() {
            self.states.targets.persisting = false;
            self.states.targets.persist_ticks = 0;
            return true;
        }
        self.states.targets.persisting = true;
        self.states.targets.persist_ticks < base.persist_ticks
    }

    // ----- behavior.nearest_attackable_target -----

    pub(super) fn nearest_target_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<NearestAttackableTargetGoal>() else {
            return false;
        };
        if goal.attack_interval_ticks > 0 {
            if self.rng.gen_range(0..goal.attack_interval_ticks) != 0 {
                return false;
            }
        } else {
            let id = self
                .world
                .get::<RuntimeId>(self.entity)
                .map_or(0, |id| id.0 as u64);
            if !id
                .wrapping_add(self.tick)
                .is_multiple_of(goal.scan_interval as u64)
            {
                return false;
            }
        }
        match self.select_target(goal) {
            Some(target) => {
                self.states.targets.staged = Some(target);
                true
            }
            None => false,
        }
    }

    pub(super) fn nearest_target_start(&mut self) {
        let Some(goal) = self.config::<NearestAttackableTargetGoal>() else {
            return;
        };
        let target = self.states.targets.staged.take();
        self.adopt(&goal.base, self.states.targets.entry);
        self.start_targeting();
        self.set_target(target);
        self.states.targets.source = Some(TargetSource::Nearest);
    }

    pub(super) fn nearest_target_can_continue(&mut self) -> bool {
        let Some(goal) = self.config::<NearestAttackableTargetGoal>() else {
            return false;
        };
        if !self.keep_target(&goal.base) {
            return false;
        }
        let visible = self.states.targets.entry.and_then(|index| {
            let target = Candidate::of(self.world, self.states.targets.target?)?;
            Some(self.target_visible(goal, &target, &goal.base.entries[index]))
        });
        if visible == Some(false) {
            self.set_target(None);
            return false;
        }
        if !goal.reselect_targets {
            return true;
        }
        if let Some(target) = self.select_target(goal) {
            if Some(target) != self.states.targets.target {
                self.adopt(&goal.base, self.states.targets.entry);
                self.set_target(Some(target));
            }
            return true;
        }
        if self.states.targets.must_see
            && self.states.targets.unseen <= self.states.targets.forget_ticks
        {
            return true;
        }
        self.set_target(None);
        false
    }

    // ----- behavior.hurt_by_target -----

    /// The actor that hurt this mob within the native memory window.
    fn last_attacker(&self) -> Option<Entity> {
        let hurt = self.world.get::<LastHurt>(self.entity)?;
        (self.tick.saturating_sub(hurt.tick) < HURT_BY_TARGET_MEMORY_TICKS)
            .then_some(hurt.attacker)
            .flatten()
    }

    pub(super) fn hurt_by_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<HurtByTargetGoal>() else {
            return false;
        };
        let Some(attacker) = self
            .last_attacker()
            .and_then(|a| Candidate::of(self.world, a))
        else {
            return false;
        };
        let current = self.states.targets.target == Some(attacker.entity);
        match self.can_attack(&goal.base, &attacker, current) {
            Some(entry) => {
                self.states.targets.staged = Some(attacker.entity);
                self.states.targets.entry = Some(entry);
                true
            }
            None => false,
        }
    }

    pub(super) fn hurt_by_start(&mut self) {
        let Some(goal) = self.config::<HurtByTargetGoal>() else {
            return;
        };
        let target = self.states.targets.staged.take();
        self.adopt(&goal.base, self.states.targets.entry);
        self.start_targeting();
        self.set_target(target);
        self.states.targets.source = Some(TargetSource::HurtBy);
    }

    pub(super) fn hurt_by_can_continue(&mut self) -> bool {
        let Some(goal) = self.config::<HurtByTargetGoal>() else {
            return false;
        };
        self.keep_target(&goal.base)
    }

    /// Native target-goal tick: count the ticks a persisting target is
    /// kept.
    pub(super) fn target_tick(&mut self) {
        if self.states.targets.persisting {
            self.states.targets.persist_ticks += 1;
        }
    }

    /// Native target-goal stop leaves the target in place; the failing
    /// continuation check already cleared it.
    pub(super) fn target_stop(&mut self, source: TargetSource) {
        if self.states.targets.source == Some(source) {
            self.states.targets.source = None;
        }
    }
}

impl TargetStates {
    /// Forget everything when the goal composition is rebuilt.
    pub fn reset(&mut self) {
        let target = self.target;
        *self = Self {
            target,
            ..Self::default()
        };
    }
}
