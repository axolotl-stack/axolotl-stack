//! `minecraft:behavior.use_kinetic_weapon`: a mob holding an item with
//! `minecraft:kinetic_weapon` (a spear) charges its target with the weapon
//! raised.
//!
//! The goal starts and continues as the melee goals do
//! (`super::melee`), and only while the main hand holds a kinetic weapon.
//! It runs in four phases:
//!
//! - **Approach**: chase the target like the melee goal. Once the target is
//!   horizontally closer than `approach_distance`, raise the weapon (unless
//!   it is still raised) and charge.
//! - **Charge**: keep chasing; the raised weapon deals its kinetic damage
//!   (`mob::combat::kinetic`). A target horizontally closer than the middle
//!   of the weapon's reach sends the mob back to a retreat position.
//! - **Reposition**: walk to the retreat position, then charge again; while
//!   the target is still that close, walk to a random position nearby.
//! - **Cooldown**: entered once the weapon's kinetic effects ran out (or the
//!   target died): the weapon is lowered and the mob backs off for the melee
//!   `cooldown_time`, then approaches again.
//!
//! A retreat position lies `reposition_distance` (or `cooldown_distance`)
//! blocks from the target, on the mob's side, turned at random by up to half
//! of `melee_fov`. Stopping the goal lowers the weapon. The weapon's reach
//! is scaled by `weapon_reach_multiplier`, its minimum speeds by
//! `weapon_min_speed_multiplier`. Matches the BDS 1.26.51 recordings
//! `zombie-spear-field` and `zombie-door-reach-spear`.

use bevy_ecs::entity::Entity;
use glam::{DVec2, DVec3};
use jolyne::valentine::actor_flags;
use rand::Rng;
use unastar_data::actor_components::body::FloatRange;
use unastar_data::item_components::KineticWeapon;

use super::goals::GoalContext;
use super::melee::{MeleeKind, MeleeState};
use crate::entity::components::{Player, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::combat::kinetic::{DealKineticDamage, KineticSettings};
use crate::entity::mob::components::UseKineticWeaponGoal;
use crate::entity::mob::constants::{KINETIC_RETREAT_REPATH_CHANCE, KINETIC_RETREAT_SPREAD_SCALE};
use crate::entity::mob::gear;

/// What the goal asks of the mob's weapon after its tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KineticRequest {
    /// Raise the weapon: a fresh [`DealKineticDamage`] and the `USINGITEM`
    /// flag.
    Raise(KineticSettings),
    /// Lower the weapon: no [`DealKineticDamage`], no `USINGITEM`.
    Lower,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Approach,
    Charge,
    Reposition,
    Cooldown,
}

/// Runtime state of the kinetic weapon goal.
#[derive(Debug, Default)]
pub struct KineticState {
    /// The shared melee approach state (its path timer).
    pub(super) melee: MeleeState,
    phase: Phase,
    /// Where the mob backs off to; `None` until a retreat was chosen.
    retreat: Option<DVec3>,
    /// The last tick of the cooldown.
    cooldown_until: u64,
}

impl KineticState {
    /// The goal was (re)built.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl<'a> GoalContext<'a> {
    fn kinetic_config(&self) -> Option<&'a UseKineticWeaponGoal> {
        self.config::<UseKineticWeaponGoal>()
    }

    /// The `minecraft:kinetic_weapon` of the item in the mob's main hand.
    fn held_kinetic_weapon(&self) -> Option<&'a KineticWeapon> {
        gear::mainhand(self.world, self.entity)?
            .item()?
            .components()
            .kinetic_weapon
            .as_ref()
    }

    /// The held weapon as `goal` uses it.
    fn kinetic_settings(&self, goal: &UseKineticWeaponGoal) -> Option<KineticSettings> {
        Some(KineticSettings::new(
            self.held_kinetic_weapon()?,
            goal.weapon_reach_multiplier,
            goal.weapon_min_speed_multiplier,
        ))
    }

    pub(super) fn kinetic_can_start(&mut self) -> bool {
        self.kinetic_config().is_some()
            && self.held_kinetic_weapon().is_some()
            && self.melee_base_can_start()
    }

    pub(super) fn kinetic_start(&mut self) {
        if let Some(goal) = self.kinetic_config() {
            self.melee_base_start(&goal.melee, MeleeKind::Kinetic);
            let state = &mut self.states.kinetic;
            state.phase = Phase::Approach;
            state.retreat = None;
        }
    }

    pub(super) fn kinetic_can_continue(&mut self) -> bool {
        let Some(goal) = self.kinetic_config() else {
            return false;
        };
        self.held_kinetic_weapon().is_some() && self.melee_base_can_continue(&goal.melee)
    }

    pub(super) fn kinetic_stop(&mut self) {
        self.effects.kinetic = Some(KineticRequest::Lower);
        self.states.kinetic.phase = Phase::Approach;
        self.melee_stop();
    }

    pub(super) fn kinetic_tick(&mut self) {
        let (Some(goal), Some(target)) = (self.kinetic_config(), self.states.targets.target) else {
            return;
        };
        let Some(settings) = self.kinetic_settings(goal) else {
            return;
        };
        let mid = settings.mid_reach();
        let alive = self.melee_target();
        let raised = self.world.get::<DealKineticDamage>(self.entity).is_some();
        let distance_sq = alive.as_ref().map_or(f64::INFINITY, |target| {
            let (dx, dz) = (
                target.pos.x - self.body.feet.x,
                target.pos.z - self.body.feet.z,
            );
            dx * dx + dz * dz
        });
        match self.states.kinetic.phase {
            Phase::Approach => {
                let Some(alive) = alive else {
                    return;
                };
                if distance_sq < goal.approach_distance * goal.approach_distance {
                    self.states.kinetic.retreat = None;
                    self.states.kinetic.phase = Phase::Charge;
                    if !(raised && self.using_item()) {
                        self.effects.kinetic = Some(KineticRequest::Raise(settings));
                    }
                } else {
                    self.move_toward_target(
                        &goal.melee,
                        MeleeKind::Kinetic,
                        &alive,
                        false,
                        distance_sq,
                    );
                }
            }
            Phase::Charge => {
                let Some(alive) = alive.filter(|_| raised) else {
                    self.start_kinetic_cooldown(goal, target);
                    return;
                };
                if distance_sq < mid * mid {
                    self.states.kinetic.phase = Phase::Reposition;
                    self.choose_retreat(goal, target, goal.reposition_distance);
                    self.states.kinetic.melee.path_timer -= 1;
                    if !self.move_to_retreat(goal) {
                        self.states.kinetic.melee.path_timer += goal.melee.path_fail_ticks;
                    }
                } else {
                    self.move_toward_target(
                        &goal.melee,
                        MeleeKind::Kinetic,
                        &alive,
                        false,
                        distance_sq,
                    );
                }
            }
            Phase::Reposition => {
                if alive.is_none() || !raised {
                    self.start_kinetic_cooldown(goal, target);
                    return;
                }
                self.keep_retreating(goal);
                if !self.nav.is_done() {
                    return;
                }
                if distance_sq < mid * mid {
                    self.wander_near(goal, goal.reposition_distance, mid);
                } else {
                    self.states.kinetic.phase = Phase::Approach;
                }
            }
            Phase::Cooldown => {
                self.keep_retreating(goal);
                let cooling = self.tick <= self.states.kinetic.cooldown_until;
                if !self.nav.is_done() {
                    return;
                }
                if cooling {
                    self.wander_near(goal, goal.cooldown_distance, mid);
                } else {
                    self.states.kinetic.phase = Phase::Approach;
                }
            }
        }
    }

    fn using_item(&self) -> bool {
        self.world
            .get::<ActorFlags>(self.entity)
            .is_some_and(|flags| flags.get(actor_flags::USINGITEM).unwrap_or(false))
    }

    /// Lower the weapon and back off from `target` for the cooldown.
    fn start_kinetic_cooldown(&mut self, goal: &UseKineticWeaponGoal, target: Entity) {
        let state = &mut self.states.kinetic;
        state.phase = Phase::Cooldown;
        state.cooldown_until = self.tick + u64::try_from(goal.melee.cooldown_ticks).unwrap_or(0);
        self.effects.kinetic = Some(KineticRequest::Lower);
        self.choose_retreat(goal, target, goal.cooldown_distance);
        self.move_to_retreat(goal);
    }

    /// Choose a retreat position `range` blocks from `target` on the mob's
    /// side.
    fn choose_retreat(&mut self, goal: &UseKineticWeaponGoal, target: Entity, range: FloatRange) {
        let Some(from) = self.target_feet(target) else {
            return;
        };
        let away = DVec2::new(self.body.feet.x - from.x, self.body.feet.z - from.z);
        let away = away.try_normalize().unwrap_or(DVec2::Y);
        let spread = goal.melee.melee_fov * KINETIC_RETREAT_SPREAD_SCALE;
        let turn = f64::from((self.rng.r#gen::<f32>() * 2.0 - 1.0) * spread).to_radians();
        let direction = DVec2::from_angle(turn).rotate(away);
        let distance = f64::from(range.min + (range.max - range.min) * self.rng.r#gen::<f32>());
        let point = from + DVec3::new(direction.x, 0.0, direction.y) * distance;
        self.states.kinetic.retreat = Some(point.floor());
    }

    /// The feet of `target` (a player's position is its eye).
    fn target_feet(&self, target: Entity) -> Option<DVec3> {
        let position = self.world.get::<Position>(target)?.0;
        Some(if self.world.get::<Player>(target).is_some() {
            position - DVec3::Y * PLAYER_EYE_HEIGHT
        } else {
            position
        })
    }

    /// Path to the retreat position at the reposition speed. A mob already
    /// in the retreat position's column has no path to follow there.
    fn move_to_retreat(&mut self, goal: &UseKineticWeaponGoal) -> bool {
        let Some(retreat) = self.states.kinetic.retreat else {
            return false;
        };
        let feet = self.body.feet.floor();
        if feet.x == retreat.x && feet.z == retreat.z {
            return false;
        }
        if !self.path_toward(retreat) {
            return false;
        }
        self.follow_staged(goal.reposition_speed_multiplier);
        true
    }

    /// Once the path timer ran out, path to the retreat position again,
    /// unless the mob still follows a path (then only with a small chance).
    fn keep_retreating(&mut self, goal: &UseKineticWeaponGoal) {
        let state = &mut self.states.kinetic;
        let previous = state.melee.path_timer;
        state.melee.path_timer -= 1;
        if previous >= 2 {
            return;
        }
        if state.retreat.is_some()
            && !self.nav.is_done()
            && self.rng.r#gen::<f32>() >= KINETIC_RETREAT_REPATH_CHANCE
        {
            return;
        }
        if !self.move_to_retreat(goal) {
            self.states.kinetic.melee.path_timer += goal.melee.path_fail_ticks;
        }
    }

    /// Walk to a random position around the mob, `range` past the middle of
    /// the weapon's reach.
    fn wander_near(&mut self, goal: &UseKineticWeaponGoal, range: FloatRange, mid: f64) {
        let xz = (f64::from(range.min) + mid) as i32;
        let y = (f64::from(range.max) + mid) as i32;
        if self.prepare_random_path(xz, y, None).is_some() {
            self.follow_staged(goal.reposition_speed_multiplier);
        }
    }
}
