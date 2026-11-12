//! `minecraft:navigation.climb`: a wall climber shows its climbing state.
//!
//! The physics step climbs (see [`WallClimbing`]); the mob's paths are
//! walk paths. Viewers see the climbing state as the `WALLCLIMBING` actor
//! flag, set in the tick the climber's move is first blocked horizontally
//! and cleared in the first tick its move is free again, as a BDS 1.26.51
//! spider's flag changes.

use bevy_ecs::prelude::*;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};

use crate::entity::components::WallClimbing;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;

/// Mobs whose climbing state changed, with the flags it is mirrored into.
type ClimbingMobs<'w, 's> = Query<
    'w,
    's,
    (
        &'static WallClimbing,
        &'static mut ActorFlags,
        &'static mut EntityMetadata,
    ),
    (With<MobKind>, Changed<WallClimbing>),
>;

/// System: mirror each mob's climbing state into its `WALLCLIMBING` flag
/// after the physics step.
pub fn sync_wall_climbing(mut mobs: ClimbingMobs) {
    for (climbing, mut flags, mut metadata) in &mut mobs {
        if flags.get(actor_flags::WALLCLIMBING) == Ok(climbing.0) {
            continue;
        }
        flags
            .update(actor_flags::WALLCLIMBING, climbing.0)
            .expect("catalogue flag index");
        let words = *flags.snapshot().words();
        metadata.set_long(ids::RESERVED_0, words[0] as i64);
        metadata.set_long(ids::RESERVED_092, words[1] as i64);
    }
}
