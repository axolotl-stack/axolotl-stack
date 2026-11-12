//! The hopping mobs' goals and their move control (slimes and magma cubes,
//! `minecraft:movement.jump`), measured on BDS 1.26.51 with
//! `sweep.py slime` and replayed by `mob::tests::slime` against
//! `mob/ai/testdata/bds-1.26.51-slime.json`.
//!
//! A hopping mob never paths. Its goals only say where to face and whether
//! to move:
//!
//! - `behavior.slime_random_direction` runs while the mob has no target and
//!   stands on the ground or in a liquid. Every running tick counts a timer
//!   down; when it runs out the goal draws a new yaw, `0..turn_range`
//!   degrees, and a new timer, `min + 0..add` ticks (40 to 99 ground ticks
//!   between draws for the vanilla values, 46 draws).
//! - `behavior.slime_attack` faces the target while it has one, for at most
//!   `grow_tired_cooldown_time`.
//! - `behavior.slime_keep_on_jumping` asks to move every tick at its speed;
//!   `behavior.slime_float` does in water, where it also jumps with
//!   `jump_chance_percentage` per tick (in lava it never runs: a magma cube
//!   stays on the floor of a lava pool).
//!
//! The control turns the body (and head) toward the wanted yaw by at most
//! `max_turn` on the ticks a facing goal runs, so a calm slime turns only on
//! the ground. Asked to move, a mob on the ground counts its hop delay
//! down; when it is out it jumps at its movement speed times the goal's
//! multiplier and draws the next delay from `jump_delay`, cut to whole
//! ticks. In the air it keeps that speed; on the ground between hops it
//! stands still. The travel step is the ordinary one: the jump impulse,
//! ground acceleration on the jump tick and air acceleration after it.

use bevy_ecs::prelude::*;
use rand::Rng;

use super::control::{rotate_towards, wrap_degrees, yaw_toward};
use super::goals::GoalContext;
use crate::entity::components::OnGround;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::{
    HopAttackGoal, HopFloatGoal, HopKeepJumpingGoal, HopMovementConfig, HopRandomDirectionGoal,
};
use crate::entity::mob::constants::{SLIME_CLIENT_EVENT_LANDED, SLIME_CLIENT_EVENT_LEFT_GROUND};
use crate::entity::mob::state::{AttackTarget, Dead};

/// What the hop control and its goals keep between ticks.
#[derive(Debug, Default)]
pub struct HopState {
    /// Ticks the mob still waits on the ground before its next hop; drawn
    /// from `jump_delay` the first time the control runs.
    delay: Option<u32>,
    /// Running ticks until `slime_random_direction` draws again.
    direction_timer: u32,
    /// The yaw `slime_random_direction` drew last.
    direction: f32,
    /// Running ticks `slime_attack` has left before it tires.
    tired: u32,
    /// The yaw a facing goal wants this tick.
    face: Option<f32>,
    /// The speed multiplier a moving goal asks for this tick.
    speed: Option<f32>,
    /// A floating goal's jump this tick.
    jump: bool,
}

/// The hop control's result for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hop {
    pub yaw: f32,
    /// The forward speed of the travel step.
    pub speed: f32,
    pub jump: bool,
}

impl HopState {
    /// Run the control once the goals have ticked. `movement` is the mob's
    /// movement speed; `rng` draws the next delay.
    pub fn control(
        &mut self,
        config: &HopMovementConfig,
        yaw: f32,
        on_ground: bool,
        movement: f32,
        rng: &mut impl Rng,
    ) -> Hop {
        let yaw = match self.face.take() {
            Some(face) => rotate_towards(yaw, face, config.max_turn),
            None => yaw,
        };
        let float_jump = std::mem::take(&mut self.jump);
        let Some(multiplier) = self.speed.take() else {
            return Hop {
                yaw,
                speed: 0.0,
                jump: float_jump,
            };
        };
        let speed = movement * multiplier;
        if !on_ground {
            return Hop {
                yaw,
                speed,
                jump: float_jump,
            };
        }
        let delay = self
            .delay
            .get_or_insert_with(|| config.delay_ticks(rng.r#gen::<f32>()));
        if *delay > 0 {
            *delay -= 1;
            return Hop {
                yaw,
                speed: 0.0,
                jump: float_jump,
            };
        }
        // BDS's AI runs after the physics of the tick the mob lands, so the
        // landing tick already counts; unastar's runs before it, so the
        // drawn delay is one tick shorter here (BDS 1.26.51's calm slimes
        // waited 10 to 29 ground ticks for `[0.5, 1.5]` seconds).
        self.delay = Some(config.delay_ticks(rng.r#gen::<f32>()).saturating_sub(1));
        Hop {
            yaw,
            speed,
            jump: true,
        }
    }
}

impl GoalContext<'_> {
    fn hop(&mut self) -> &mut HopState {
        &mut self.states.hop
    }

    pub(super) fn hop_direction_can_start(&self) -> bool {
        self.config::<HopRandomDirectionGoal>().is_some()
            && self.world.get::<AttackTarget>(self.entity).is_none()
            && (self.body.on_ground || self.body.liquid.is_some())
    }

    pub(super) fn hop_direction_tick(&mut self) {
        let Some(goal) = self.config::<HopRandomDirectionGoal>() else {
            return;
        };
        let state = &mut self.states.hop;
        if state.direction_timer <= 1 {
            state.direction_timer = goal.min_ticks
                + if goal.add_ticks > 0 {
                    self.rng.gen_range(0..goal.add_ticks)
                } else {
                    0
                };
            let range = goal.turn_range.max(1);
            state.direction = wrap_degrees(self.rng.gen_range(0..range) as f32);
        } else {
            state.direction_timer -= 1;
        }
        state.face = Some(state.direction);
    }

    fn hop_target_centre(&self) -> Option<glam::DVec3> {
        let target = self.world.get::<AttackTarget>(self.entity)?.0;
        super::target_centre(self.world, target)
    }

    pub(super) fn hop_attack_can_start(&self) -> bool {
        self.config::<HopAttackGoal>().is_some() && self.hop_target_centre().is_some()
    }

    pub(super) fn hop_attack_start(&mut self) {
        if let Some(goal) = self.config::<HopAttackGoal>() {
            self.states.hop.tired = goal.grow_tired_ticks;
        }
    }

    pub(super) fn hop_attack_can_continue(&mut self) -> bool {
        if self.states.hop.tired == 0 {
            return false;
        }
        self.states.hop.tired -= 1;
        self.hop_attack_can_start()
    }

    pub(super) fn hop_attack_tick(&mut self) {
        let Some(goal) = self.config::<HopAttackGoal>() else {
            return;
        };
        let Some(centre) = self.hop_target_centre() else {
            return;
        };
        if goal.set_persistent {
            self.effects.persistent = true;
        }
        let face = yaw_toward(centre - self.body.feet);
        self.hop().face = Some(face);
    }

    pub(super) fn hop_keep_jumping_tick(&mut self) {
        if let Some(goal) = self.config::<HopKeepJumpingGoal>() {
            self.hop().speed = Some(goal.speed_multiplier);
        }
    }

    /// In water only: BDS 1.26.51's magma cubes summoned into a lava pool
    /// sat on its floor for 200 ticks without a hop.
    pub(super) fn hop_float_can_start(&self) -> bool {
        self.config::<HopFloatGoal>().is_some()
            && self.body.liquid == Some(crate::registry::block::LiquidKind::Water)
    }

    pub(super) fn hop_float_tick(&mut self) {
        let Some(goal) = self.config::<HopFloatGoal>() else {
            return;
        };
        if self.rng.r#gen::<f32>() < goal.jump_chance {
            self.hop().jump = true;
        }
        self.hop().speed = Some(goal.speed_multiplier);
    }
}

/// The ground contact a slime class reported last (see
/// [`report_ground_changes`]).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedGround(bool);

/// System: a slime class reports leaving the ground and landing in its
/// `CLIENT_EVENT` actor data on the tick it happens (BDS 1.26.51: 2 on the
/// tick of every hop, 1 on the tick it lands; the client squishes it).
#[allow(clippy::type_complexity)]
pub fn report_ground_changes(
    mut commands: Commands,
    mut mobs: Query<
        (
            Entity,
            &MobKind,
            &OnGround,
            Option<&mut ReportedGround>,
            &mut EntityMetadata,
        ),
        Without<Dead>,
    >,
) {
    for (entity, kind, on_ground, reported, mut metadata) in &mut mobs {
        if kind.0.native.slime.is_none() {
            continue;
        }
        let Some(mut reported) = reported else {
            commands.entity(entity).insert(ReportedGround(on_ground.0));
            continue;
        };
        if reported.0 == on_ground.0 {
            continue;
        }
        reported.0 = on_ground.0;
        let event = if on_ground.0 {
            SLIME_CLIENT_EVENT_LANDED
        } else {
            SLIME_CLIENT_EVENT_LEFT_GROUND
        };
        metadata.set_byte(jolyne::valentine::actor_data_i_ds::CLIENT_EVENT, event);
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    fn config(min: f32, max: f32) -> HopMovementConfig {
        HopMovementConfig {
            delay_seconds: (min, max),
            max_turn: 30.0,
        }
    }

    #[test]
    fn a_grounded_hopper_waits_out_its_delay_then_jumps_and_draws_again() {
        let config = config(0.5, 1.5);
        let mut state = HopState {
            delay: Some(2),
            ..HopState::default()
        };
        let mut rng = StdRng::seed_from_u64(1);
        let mut jumps = Vec::new();
        for _ in 0..4 {
            state.speed = Some(1.0);
            jumps.push(state.control(&config, 0.0, true, 0.3, &mut rng).jump);
        }
        assert_eq!(jumps, [false, false, true, false]);
        let delay = state.delay.expect("drawn");
        assert!(
            (8..=27).contains(&delay),
            "a 10..=29 tick delay less the landing tick, one counted"
        );
    }

    #[test]
    fn the_body_turns_only_toward_a_wanted_yaw_and_by_max_turn() {
        let config = config(0.5, 1.5);
        let mut rng = StdRng::seed_from_u64(1);
        let mut state = HopState::default();
        assert_eq!(state.control(&config, 10.0, true, 0.3, &mut rng).yaw, 10.0);
        state.face = Some(100.0);
        assert_eq!(state.control(&config, 10.0, true, 0.3, &mut rng).yaw, 40.0);
    }

    #[test]
    fn delays_are_cut_to_whole_ticks() {
        let config = config(0.16, 0.5);
        assert_eq!(config.delay_ticks(0.0), 3);
        assert_eq!(config.delay_ticks(0.999), 9);
    }
}
