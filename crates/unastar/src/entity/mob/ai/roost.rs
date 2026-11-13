//! The bat class's resting (`NativeTraits::bat`), measured on BDS 1.26.51
//! with `sweep.py bat` (`cave`, `wake`, `low`, `unroof`, `canopy`) and
//! replayed by `mob::tests::bat`.
//!
//! A bat is born resting (`RESTING` is set in its `AddActor`). A resting
//! bat whose block above its feet is not solid wakes on its next tick (a
//! summoned bat in the open woke before the first sampled tick; one whose
//! roof was removed woke on that tick). Under a solid block it stays, its
//! feet held at the top of its feet block less its height, so its head
//! touches the roof, and it does not move, until a player comes within
//! [`BatClass::wake_range`] of it or it is hurt. Flying, a bat under a
//! solid block starts resting with [`BatClass::rest_chance`] per tick; it
//! still moves on the tick it settles and is held from the next.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::control::Body;
use super::nearby::Nearby;
use crate::entity::components::GameMode;
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::native::BatClass;
use crate::entity::systems::physics::BlockSource;

/// Whether the block above `feet`'s block has a collision box (not when
/// its chunk is not loaded).
pub fn roofed(blocks: &impl BlockSource, feet: DVec3) -> bool {
    super::fly::solid(blocks, feet.floor().as_ivec3() + IVec3::Y).unwrap_or(false)
}

/// What a resting bat does this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rest {
    /// It stays: this velocity moves its feet to the roost height.
    Hold(DVec3),
    /// It wakes, and stays still this tick.
    Wake,
}

/// A resting bat's tick: wake when its roof is gone or a player is near,
/// else hold it at the roost height.
pub fn rest(world: &World, nearby: &Nearby, class: &BatClass, body: &Body) -> Rest {
    if !roofed(world, body.feet) || player_near(world, nearby, class, body.feet) {
        return Rest::Wake;
    }
    let roost = body.feet.y.floor() + 1.0 - body.height;
    Rest::Hold(DVec3::new(0.0, roost - body.feet.y, 0.0))
}

/// Whether a flying bat at `feet` settles this tick.
pub fn settles(world: &World, class: &BatClass, feet: DVec3, rng: &mut impl Rng) -> bool {
    rng.r#gen::<f32>() < class.rest_chance && roofed(world, feet)
}

/// Whether a player that is not a spectator has its eyes within the wake
/// range of the bat's feet (BDS 1.26.51: creative and survival players
/// alike woke it).
fn player_near(world: &World, nearby: &Nearby, class: &BatClass, feet: DVec3) -> bool {
    let range_sq = class.wake_range * class.wake_range;
    nearby
        .players_within(feet, class.wake_range + PLAYER_EYE_HEIGHT)
        .any(|player| {
            player.eye.distance_squared(feet) < range_sq
                && world.get::<GameMode>(player.entity) != Some(&GameMode::Spectator)
        })
}

/// A hurt bat wakes on the tick it is hurt (BDS 1.26.51, `sweep.py bat
/// --items low`: 23 of 24 hurts of a resting bat woke it that tick; one
/// did not, for a reason not measured).
pub fn wake_hurt(world: &mut World, entity: Entity) {
    let mut mob = world.entity_mut(entity);
    if mob
        .get::<crate::entity::mob::MobKind>()
        .is_some_and(|kind| kind.0.native.bat.is_some())
    {
        super::set_flag(&mut mob, jolyne::valentine::actor_flags::RESTING, false);
    }
}
