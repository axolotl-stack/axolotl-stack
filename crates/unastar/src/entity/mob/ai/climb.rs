//! `minecraft:navigation.climb`: the navigation of a wall climber.
//!
//! A climber's paths are ordinary walk paths; the search has no wall-face
//! nodes. What makes it climb:
//!
//! - Every path request records a destination block (the target's block for
//!   an actor, the goal's block for a position), whether or not a path is
//!   found (`GoalContext::request_path`).
//! - Each tick the mob shows the `WALLCLIMBING` actor flag (and
//!   [`Climbing::wall_climbing`]) while it has a destination and its last
//!   move was blocked horizontally; the physics step then climbs it at
//!   [`CLIMB_SPEED`].
//! - With no path left to follow and a destination still set, it heads
//!   straight for the destination. It arrives, dropping the destination,
//!   within [`CLIMB_ARRIVAL_WIDTH_SQ_FACTOR`] × width² (squared distance) of
//!   the destination block's centre, or, once above the block, of the centre
//!   of the destination column at its own level. It drops the destination
//!   when the direct sweep ([`climber_walks_directly`]) is blocked, and
//!   otherwise walks toward the bottom centre of the block.
//!
//! Stopping the path keeps the destination. This matches the BDS 1.26.51
//! capture of a spider climbing to a player on a wall.
//!
//! [`Climbing::wall_climbing`]: crate::entity::components::Climbing::wall_climbing
//! [`CLIMB_SPEED`]: crate::entity::mob::constants::CLIMB_SPEED

use glam::DVec3;

use super::control::{Body, MoveControl, PathFollower};
use super::path::{PathBlockTable, climber_walks_directly};
use crate::entity::mob::constants::{CLIMB_ARRIVAL_WIDTH_SQ_FACTOR, CLIMB_SWEEP_HEIGHT_FACTOR};
use crate::entity::systems::physics::BlockSource;
use crate::registry::block::LiquidKind;

/// One navigation tick of a wall climber whose last move was blocked
/// horizontally when `collided`: follow the path, or approach the
/// destination directly when no path is left. Returns whether the mob is
/// wall climbing this tick.
pub(super) fn navigate(
    blocks: &impl BlockSource,
    table: &PathBlockTable,
    body: &Body,
    collided: bool,
    nav: &mut PathFollower,
    control: &mut MoveControl,
    door_hold: bool,
) -> bool {
    let wall_climbing = collided && nav.destination().is_some();
    match nav.destination() {
        Some(destination) if nav.is_done() => {
            let block = destination.as_dvec3();
            let centre = block + DVec3::splat(0.5);
            let arrive = body.width * body.width * CLIMB_ARRIVAL_WIDTH_SQ_FACTOR;
            let feet = body.feet;
            let above = feet.y > block.y
                && feet.distance_squared(DVec3::new(centre.x, feet.y.floor() + 0.5, centre.z))
                    < arrive;
            if above || feet.distance_squared(centre) < arrive {
                nav.set_destination(None);
            } else if climber_walks_directly(
                blocks,
                table,
                feet,
                block,
                body.width.ceil() as i32,
                body.height.ceil() as i32 * CLIMB_SWEEP_HEIGHT_FACTOR,
                body.liquid == Some(LiquidKind::Water),
            ) {
                control.set_wanted(
                    DVec3::new(centre.x, block.y, centre.z),
                    nav.speed_multiplier(),
                    true,
                );
            } else {
                nav.set_destination(None);
            }
        }
        _ => nav.tick(body, control, door_hold),
    }
    wall_climbing
}
