//! What a player's movement changes beyond its position, as BDS 1.26.51 does
//! it for a moving client (`testdata/bds-1.26.51-movement.json`):
//!
//! - the sprint, swim and glide toggles its inputs ask for, accepted or
//!   refused with the rules BDS applies: a sprint starts only while moving
//!   forward without sneaking and a stop is refused while the player still
//!   moves forward; a swim starts only in water; a glide only in the air
//!   with an elytra in the chest slot and out of water. A swimmer that lets
//!   go stops swimming at once and sprinting a tick later. A refused toggle
//!   is answered with the unchanged flag words and key 139, which also
//!   travel with the swim's and the late sprint's stop. Accepted changes
//!   reach the player and its viewers at once ([`super::actor_data`]);
//! - the `MOVING` flag: on while the player's last input moved it more than
//!   √0.001 blocks (`|Δ|² > 0.001`), checked on every tick with an input.
//!   A knockback counts as the move of the next input: BDS moves its copy
//!   of the player with the push at once, a tick before the client reports
//!   it (`push` phase: `MOVING` on with the input after the hit, whose own
//!   move was nothing);
//! - the food exhaustion of each move: 0.01 per block in water (any
//!   direction, swimming or not), a jump's 0.05 (0.2 sprinting) instead of
//!   the distance on the tick it jumps, the sprint's 0.1 per horizontal
//!   block on the ground, nothing in the air or walking
//!   (`minecraft:exhaustion_values`).
//!
//! The movement system (`server::game::movement`) records each accepted
//! input in [`PlayerMoves`]; [`settle_moves`] applies them with the world at
//! hand, right after it.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};

use super::actor_data::{self, Keys};
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{ArmourInventory, GameMode, Hunger, PlayerState, Position};
use crate::entity::systems::BlockSource;
use crate::registry::block::LiquidKind;
use crate::server::game::hunger::ExhaustionValues;

/// The squared distance one input must move the player for `MOVING`.
pub const MOVING_DISTANCE_SQ: f64 = 0.001;
/// The item a glide needs in the chest slot.
const ELYTRA: &str = "minecraft:elytra";

/// One accepted input's move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    /// The position change (the eyes, as reported).
    pub delta: DVec3,
    /// The input started a jump (`StartJumping`).
    pub jumped: bool,
    /// The move ended against the ground (`VerticalCollision`).
    pub on_ground: bool,
}

/// The toggles one tick's inputs asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Toggles {
    pub start_sprint: bool,
    pub stop_sprint: bool,
    pub start_swim: bool,
    pub stop_swim: bool,
    pub start_glide: bool,
    pub stop_glide: bool,
}

/// A player's moves since the last tick, and the movement state BDS keeps
/// for it.
#[derive(Component, Debug, Clone, PartialEq, Default)]
pub struct PlayerMoves {
    pub moves: Vec<Move>,
    pub toggles: Toggles,
    /// The player's last input moved it ([`MOVING_DISTANCE_SQ`]).
    pub moving: bool,
    /// The move vector of the last input points forward.
    pub forward: bool,
    /// A swimmer let go: its sprint ends on the next tick.
    sprint_ends: bool,
    /// A knockback sent since the last input: the next input moves at
    /// least this much.
    push: Option<DVec3>,
}

impl PlayerMoves {
    /// The server pushed the player by `velocity` (a knockback).
    pub fn push(&mut self, velocity: DVec3) {
        self.push = Some(velocity);
    }

    /// Record one accepted input.
    pub fn record(&mut self, step: Move, forward: bool, toggles: Toggles) {
        self.moves.push(step);
        self.forward = forward;
        let t = &mut self.toggles;
        t.start_sprint |= toggles.start_sprint;
        t.stop_sprint |= toggles.stop_sprint;
        t.start_swim |= toggles.start_swim;
        t.stop_swim |= toggles.stop_swim;
        t.start_glide |= toggles.start_glide;
        t.stop_glide |= toggles.stop_glide;
    }
}

/// Whether water is in any block `player`'s box (0.6 × 1.8, from its feet)
/// touches.
fn in_water(world: &World, feet: DVec3) -> bool {
    const INSET: f64 = 1.0e-3;
    let min = (feet + DVec3::new(-0.3 + INSET, INSET, -0.3 + INSET))
        .floor()
        .as_ivec3();
    let max = (feet + DVec3::new(0.3 - INSET, 1.8 - INSET, 0.3 - INSET))
        .floor()
        .as_ivec3();
    let registry = world.registry();
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                if world
                    .block_at(IVec3::new(x, y, z))
                    .and_then(|id| registry.liquid_state(id))
                    .is_some_and(|liquid| liquid.kind == LiquidKind::Water)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether `player` wears an elytra.
fn wears_elytra(world: &World, player: Entity) -> bool {
    world
        .get::<ArmourInventory>(player)
        .is_some_and(|armour| armour.chestplate().item_id() == ELYTRA)
}

/// The exhaustion of one move.
fn exhaustion(step: &Move, sprinting: bool, in_water: bool, values: &ExhaustionValues) -> f32 {
    if in_water {
        return step.delta.length() as f32 * values.swim;
    }
    if step.jumped {
        return if sprinting {
            values.sprint_jump
        } else {
            values.jump
        };
    }
    if !step.on_ground {
        return 0.0;
    }
    let horizontal = step.delta.x.hypot(step.delta.z) as f32;
    horizontal
        * if sprinting {
            values.sprint
        } else {
            values.walk
        }
}

/// The outcome of one tick's toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Outcome {
    /// The movement state changed: send it at once.
    changed: bool,
    /// A toggle was refused: resend the flag words with key 139.
    refused: bool,
}

/// Apply one tick's toggles to `state` (see the module documentation).
fn toggle(
    state: &mut PlayerState,
    moves: &mut PlayerMoves,
    in_water: bool,
    on_ground: bool,
    elytra: bool,
) -> Outcome {
    let mut out = Outcome::default();
    let t = std::mem::take(&mut moves.toggles);
    if std::mem::take(&mut moves.sprint_ends) && state.sprinting {
        state.sprinting = false;
        out.changed = true;
        out.refused = true;
    }
    let can_sprint = moves.forward && !state.sneaking;
    if t.start_sprint && !state.sprinting {
        if can_sprint {
            state.sprinting = true;
            out.changed = true;
        } else {
            out.refused = true;
        }
    }
    if t.stop_swim && state.swimming {
        state.swimming = false;
        out.changed = true;
        if t.stop_sprint && state.sprinting {
            // The swim ends now and the sprint on the next tick.
            moves.sprint_ends = true;
            out.refused = true;
        }
    } else if t.stop_sprint && state.sprinting {
        if can_sprint {
            out.refused = true;
        } else {
            state.sprinting = false;
            out.changed = true;
        }
    }
    if t.start_swim && !state.swimming {
        if in_water && state.sprinting {
            state.swimming = true;
            out.changed = true;
        } else {
            out.refused = true;
        }
    }
    if t.stop_glide && state.gliding {
        state.gliding = false;
        out.changed = true;
    } else if t.start_glide && !state.gliding {
        if !on_ground && !in_water && elytra {
            state.gliding = true;
            out.changed = true;
        } else {
            out.refused = true;
        }
    }
    out
}

/// System (exclusive, right after the movement system): settle every
/// player's moves of this tick.
pub fn settle_moves(
    world: &mut World,
    players: &mut QueryState<(Entity, &PlayerMoves)>,
    mut due: Local<Vec<Entity>>,
) {
    due.extend(
        players
            .iter(world)
            .filter(|(_, moves)| !moves.moves.is_empty() || moves.sprint_ends)
            .map(|(player, _)| player),
    );
    if due.is_empty() {
        return;
    }
    let values = *world.get_resource_or_init::<ExhaustionValues>();
    for player in due.drain(..) {
        let Some(eyes) = world.get::<Position>(player).map(|p| p.0) else {
            continue;
        };
        let feet = eyes - DVec3::Y * PLAYER_EYE_HEIGHT;
        let water = in_water(world, feet);
        let elytra = wears_elytra(world, player);
        let Some(mut moves) = world.get_mut::<PlayerMoves>(player).map(|m| m.clone()) else {
            continue;
        };
        let steps = std::mem::take(&mut moves.moves);
        let on_ground = steps.last().is_some_and(|step| step.on_ground);
        if let Some(last) = steps.last() {
            let pushed = moves
                .push
                .take()
                .is_some_and(|push| push.length_squared() > MOVING_DISTANCE_SQ);
            moves.moving = pushed || last.delta.length_squared() > MOVING_DISTANCE_SQ;
        }
        let Some(mut state) = world.get::<PlayerState>(player).copied() else {
            continue;
        };
        let was_sprinting = state.sprinting;
        let outcome = toggle(&mut state, &mut moves, water, on_ground, elytra);
        let sprinting = state.sprinting;
        if sprinting != was_sprinting
            && let Some(mut events) =
                world.get_resource_mut::<crate::server::game::MovementEvents>()
        {
            events
                .0
                .push(crate::ecs::events::ServerEvent::PlayerToggleSprint {
                    entity: player,
                    is_sprinting: sprinting,
                });
        }
        *world.get_mut::<PlayerState>(player).expect("read above") = state;
        *world.get_mut::<PlayerMoves>(player).expect("read above") = moves;
        let survival = world
            .get::<GameMode>(player)
            .is_some_and(|mode| !matches!(mode, GameMode::Creative | GameMode::Spectator));
        if survival {
            let amount: f32 = steps
                .iter()
                .map(|step| exhaustion(step, sprinting, water, &values))
                .sum();
            if amount > 0.0
                && let Some(mut hunger) = world.get_mut::<Hunger>(player)
            {
                hunger.exhaust(amount);
            }
        }
        if outcome.refused {
            actor_data::resync(world, player);
        }
        if outcome.changed {
            actor_data::send_changes(world, player, Keys::All);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PlayerState {
        PlayerState::default()
    }

    fn moving_forward() -> PlayerMoves {
        PlayerMoves {
            forward: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_sprint_starts_only_moving_forward_and_stops_only_after() {
        let mut s = state();
        let mut still = PlayerMoves::default();
        still.toggles.start_sprint = true;
        let out = toggle(&mut s, &mut still, false, true, false);
        assert!(out.refused && !s.sprinting);

        let mut forward = moving_forward();
        forward.toggles.start_sprint = true;
        let out = toggle(&mut s, &mut forward, false, true, false);
        assert!(out.changed && s.sprinting);

        // Letting go of the sprint key while still running: refused.
        forward.toggles.stop_sprint = true;
        let out = toggle(&mut s, &mut forward, false, true, false);
        assert!(out.refused && s.sprinting);

        forward.forward = false;
        forward.toggles.stop_sprint = true;
        let out = toggle(&mut s, &mut forward, false, true, false);
        assert!(out.changed && !out.refused && !s.sprinting);
    }

    #[test]
    fn a_swimmer_letting_go_stops_swimming_then_sprinting() {
        let mut s = PlayerState {
            sprinting: true,
            swimming: true,
            ..state()
        };
        let mut moves = PlayerMoves::default();
        moves.toggles.stop_sprint = true;
        moves.toggles.stop_swim = true;
        let out = toggle(&mut s, &mut moves, true, false, false);
        assert!(out.changed && out.refused && !s.swimming && s.sprinting);
        let out = toggle(&mut s, &mut moves, true, false, false);
        assert!(out.changed && out.refused && !s.sprinting);
        let out = toggle(&mut s, &mut moves, true, false, false);
        assert_eq!(out, Outcome::default());
    }

    #[test]
    fn swims_and_glides_need_water_and_air() {
        let mut s = state();
        let mut moves = moving_forward();
        moves.toggles.start_swim = true;
        assert!(toggle(&mut s, &mut moves, false, true, false).refused);
        moves.toggles.start_sprint = true;
        moves.toggles.start_swim = true;
        let out = toggle(&mut s, &mut moves, true, false, false);
        assert!(out.changed && s.swimming && s.sprinting);

        let mut s = state();
        moves.toggles.start_glide = true;
        assert!(toggle(&mut s, &mut moves, false, true, true).refused);
        moves.toggles.start_glide = true;
        assert!(toggle(&mut s, &mut moves, false, false, false).refused);
        moves.toggles.start_glide = true;
        assert!(toggle(&mut s, &mut moves, false, false, true).changed && s.gliding);
        moves.toggles.stop_glide = true;
        assert!(toggle(&mut s, &mut moves, false, true, true).changed && !s.gliding);
    }

    #[test]
    fn exhaustion_follows_the_move() {
        let values = ExhaustionValues::vanilla();
        let ground = Move {
            delta: DVec3::new(0.3, 0.0, 0.4),
            jumped: false,
            on_ground: true,
        };
        assert_eq!(exhaustion(&ground, false, false, &values), 0.0);
        assert!((exhaustion(&ground, true, false, &values) - 0.05).abs() < 1e-6);
        let air = Move {
            on_ground: false,
            ..ground
        };
        assert_eq!(exhaustion(&air, true, false, &values), 0.0);
        let jump = Move {
            jumped: true,
            on_ground: false,
            ..ground
        };
        assert_eq!(exhaustion(&jump, false, false, &values), values.jump);
        assert_eq!(exhaustion(&jump, true, false, &values), values.sprint_jump);
        let sink = Move {
            delta: DVec3::new(0.0, -0.025, 0.0),
            jumped: false,
            on_ground: false,
        };
        assert!((exhaustion(&sink, false, true, &values) - 0.00025).abs() < 1e-8);
    }
}
