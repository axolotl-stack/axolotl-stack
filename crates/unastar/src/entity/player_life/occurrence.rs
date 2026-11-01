//! Immutable occurrence facts captured at the actual native death boundary.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::entity::damage::owned::{Attribution, Cause, valid_uuid};
use crate::entity::damage::{DamageAttribution, DamageWorldLifetime};
use crate::plugin::manager::players::PlayerLineage;

/// Descriptive runtime generations; neither value grants access to an actor or world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeIdentity {
    pub lineage: String,
    pub world_lifetime: String,
}
impl RuntimeIdentity {
    /// Every generation is a genuine nonzero engine UUID, never an account or session surrogate.
    pub(crate) fn valid(&self) -> bool {
        valid_uuid(&self.lineage) && valid_uuid(&self.world_lifetime)
    }
}

/// Read installed engine metadata even while inventory and gameplay fields are withheld.
pub(crate) fn runtime_identity(world: &World, player: Entity) -> Option<RuntimeIdentity> {
    let lineage = world.get::<PlayerLineage>(player)?;
    if !lineage.matches(world, player) {
        return None;
    }
    Some(RuntimeIdentity {
        lineage: lineage.key().to_string(),
        world_lifetime: world.get_resource::<DamageWorldLifetime>()?.0.to_string(),
    })
}

/// Bounded persisted event metadata, independent of snapshot phase and restoration authority.
/// Missing time remains unknown. Loading this value never creates a native death token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDeathOccurrence {
    pub(crate) id: String,
    pub(crate) unix_time_millis: Option<u64>,
    pub(crate) runtime: Option<RuntimeIdentity>,
    pub(crate) cause: Cause,
    pub(crate) attribution: Attribution,
    pub(crate) fall_distance: Option<f64>,
    pub(crate) keep_inventory: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) post_drop: Option<RecordedDeathReceipt>,
}

/// An actual canonical receipt recorded by the terminal writer, never a pending request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordedDeathReceipt {
    pub id: String,
    pub receipt: crate::storage::InventoryEffectRecord,
}
impl NativeDeathOccurrence {
    /// Copy original cause, credit and rules before persistence or any deferred continuation.
    pub(crate) fn capture(
        world: &World,
        player: Entity,
        id: uuid::Uuid,
        cause: super::death::DeathCause,
        attribution: &DamageAttribution,
        keep_inventory: bool,
    ) -> Self {
        Self {
            id: id.to_string(),
            unix_time_millis: now_millis(world),
            runtime: runtime_identity(world, player),
            cause: cause.cause.into(),
            attribution: Attribution::from(attribution),
            fall_distance: cause.fall_distance,
            keep_inventory,
            post_drop: None,
        }
    }

    /// Fixed schema and canonical identities bound serialized data without truncating observations.
    pub(crate) fn valid(&self) -> bool {
        valid_uuid(&self.id)
            && self.runtime.as_ref().is_none_or(RuntimeIdentity::valid)
            && self.attribution.valid()
            && self
                .fall_distance
                .is_none_or(|distance| distance.is_finite() && distance >= 0.0)
            && self.post_drop.as_ref().is_none_or(|proof| {
                crate::storage::valid_inventory_effect_id(&proof.id) && proof.receipt.revision > 0
            })
    }

    /// Historical observations remain verifiable after later canonical state changes.
    pub(crate) fn matches_history(&self, data: &crate::storage::PlayerData) -> bool {
        self.post_drop
            .as_ref()
            .is_none_or(|proof| data.inventory_effects.get(&proof.id) == Some(&proof.receipt))
    }
}

/// Clock failures are explicit absence, never epoch zero or a later discovery timestamp.
fn now_millis(_world: &World) -> Option<u64> {
    #[cfg(test)]
    if let Some(clock) = _world.get_resource::<TestOccurrenceTime>() {
        return clock.0;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
}

/// Native tests inject the boundary clock while still executing the genuine death pipeline.
#[cfg(test)]
#[derive(Resource)]
pub(crate) struct TestOccurrenceTime(pub(crate) Option<u64>);
