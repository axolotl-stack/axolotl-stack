//! Descriptive immutable engine facts never resolve or authorize a historical actor.

use super::wit;
use crate::entity::damage::owned::Attribution;
use crate::entity::player_life::occurrence::{NativeDeathOccurrence, RuntimeIdentity};

/// Copy genuine engine generations without constructing a player or world capability.
pub(super) fn runtime(value: &RuntimeIdentity) -> wit::RuntimeIdentity {
    wit::RuntimeIdentity {
        lineage: value.lineage.clone(),
        world_lifetime: value.world_lifetime.clone(),
    }
}

/// Copy bounded captured owner facts; departed sources are never looked up again.
pub(super) fn attribution(value: &Attribution) -> wit::DamageAttribution {
    wit::DamageAttribution {
        source_dimension: value.source_dimension,
        source_world_lifetime: value.source_world_lifetime.clone(),
        owner: value.owner.as_ref().map(|owner| wit::DamageOwner {
            is_player: owner.is_player,
            uuid: owner.uuid.clone(),
            authenticated: owner.authenticated.as_ref().map(|account| {
                wit::AuthenticatedDamageAccount {
                    uuid: account.uuid.clone(),
                    xuid: account.xuid,
                }
            }),
            connection_id: owner.connection_id,
            dimension: owner.dimension,
            world_lifetime: owner.world_lifetime.clone(),
            feet: owner.feet.map(|feet| wit::Vec3 {
                x: feet[0],
                y: feet[1],
                z: feet[2],
            }),
        }),
    }
}

/// Return the original event clock and actual writer receipt, including explicit unknowns.
pub(super) fn death(value: &NativeDeathOccurrence) -> wit::DeathOccurrence {
    wit::DeathOccurrence {
        id: value.id.clone(),
        unix_time_millis: value.unix_time_millis,
        runtime: value.runtime.as_ref().map(runtime),
        cause: super::super::lifecycle::damage_cause(value.cause.into()),
        attribution: attribution(&value.attribution),
        fall_distance: value.fall_distance,
        keep_inventory: value.keep_inventory,
        post_drop: value.post_drop.as_ref().map(|proof| wit::DeathReceipt {
            request_id: proof.id.clone(),
            receipt: wit::InventoryReceipt {
                revision: proof.receipt.revision,
                request_hash: proof.receipt.request_hash.to_vec(),
            },
        }),
    }
}
