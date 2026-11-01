//! Canonical source generations and native-only publication ownership.

use super::*;
use crate::storage::source::{SourceActorRecord, SourcePhase, SourceProof};
use std::sync::atomic::{AtomicU64, Ordering};

mod coordinator;
mod residency;
pub(crate) use coordinator::{rebind, unowned_terminal};
/// Accepted persistence work is distinct from a mechanical source hold.
pub(crate) fn pending(world: &World, player: Entity) -> bool {
    coordinator::pending(world, player)
        || rebind::pending(world, player)
        || unowned_terminal::pending(world, player)
}
/// All account admission paths serialize against the same real pending actors.
pub(crate) fn pending_account(world: &World, uuid: uuid::Uuid, xuid: Option<&str>) -> bool {
    coordinator::pending_account(world, uuid, xuid)
        || rebind::pending_account(world, uuid, xuid)
        || unowned_terminal::pending_account(world, uuid, xuid)
}

pub(crate) use coordinator::{
    Action, Error, Intent, LiveSource, Operation, Phase, Physical, Prepared, Reference, Status,
    defer_death, defer_death_after_inventory, defer_death_after_inventory_rejection,
    defer_death_with_rules, install, prepare, snapshot_bytes, submit,
};

/// The same actual source hold stays owned by its native actor after guest resources disappear.
#[derive(Component, Clone)]
pub(crate) struct SourceBody(pub(crate) quarantine::Handle);
#[derive(Component)]
struct TerminalPending;

/// A genuine native actor carries one acknowledged source generation, never an old connection.
#[derive(Component)]
pub(crate) struct ManagedSource {
    pub(crate) record: SourceActorRecord,
    samples: AtomicU64,
    retention: Option<std::sync::Arc<dyn Send + Sync>>,
    reserve: Option<coordinator::Reserve>,
}
impl ManagedSource {
    /// A new physical generation starts after a definitive provider acknowledgement.
    pub(crate) fn acknowledged(record: SourceActorRecord) -> Self {
        let sample = record.sample;
        Self {
            record,
            samples: AtomicU64::new(sample),
            retention: None,
            reserve: None,
        }
    }

    /// Accepted body ownership outlives component resource drops and unloads.
    pub(crate) fn retaining(mut self, retention: std::sync::Arc<Operation>) -> Self {
        self.reserve = Some(retention.reserve.clone());
        self.retention = Some(retention);
        self
    }

    /// Exact proof is descriptive storage ownership, not a player/session capability.
    pub(crate) fn proof(&self) -> SourceProof {
        (&self.record).into()
    }
}

/// A normal account join cannot publish unresolved source values from its stored row.
#[derive(Component)]
pub(crate) struct SourceAdmissionBlocked;

/// The native barrier cannot be cleared by a component's ordinary admission acknowledgement.
pub(crate) fn admission_blocked(world: &World, player: Entity) -> bool {
    world.get::<SourceAdmissionBlocked>(player).is_some()
}

/// Full canonical state acknowledgement may release only this additional native barrier.
pub(crate) fn release_admission(world: &mut World, player: Entity) {
    if let Ok(mut actor) = world.get_entity_mut(player) {
        actor.remove::<SourceAdmissionBlocked>();
    }
}

/// Raw captures advance one monotonic native sample; stale queued saves cannot rewind an actor.
pub(crate) fn active_record(world: &World, player: Entity) -> Option<(uuid::Uuid, PlayerData)> {
    let managed = world.get::<ManagedSource>(player)?;
    if managed.record.phase != SourcePhase::Active {
        return None;
    }
    let snapshot = capture(world, player).ok()?;
    let sample = managed
        .samples
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |sample| {
            sample.checked_add(1)
        })
        .ok()?
        .checked_add(1)?;
    let mut record = managed.record.clone();
    record.sample = sample;
    record.supplement = Some(snapshot.supplement());
    let mut data = snapshot.data;
    data.game_mode = record
        .supplement
        .as_ref()?
        .base_mode
        .unwrap_or(data.game_mode);
    data.source_actor = Some(record);
    Some((uuid::Uuid::parse_str(&data.uuid).ok()?, data))
}

/// Hydrate acknowledged raw values silently, then retain the native source-ownership barrier.
pub(crate) fn resume(
    world: &mut World,
    player: Entity,
    data: &PlayerData,
) -> Result<(), SnapshotError> {
    let Some(record) = data.source_actor.as_ref() else {
        return Ok(());
    };
    record.validate(data).map_err(invalid)?;
    if record.phase.unresolved()
        && world
            .get_resource::<crate::world::WorldIdentity>()
            .is_none_or(|identity| {
                identity.id.to_string() != record.world || identity.dimension != record.dimension
            })
    {
        return Err(SnapshotError::WrongDimension);
    }
    if !record.phase.unresolved() && record.supplement.is_none() {
        return Ok(());
    }
    if !super::super::player_events::defer_initial_state(world, player) {
        return Err(SnapshotError::MissingPlayer);
    }
    if record.requires_handoff() {
        world.entity_mut(player).insert(SourceAdmissionBlocked);
    }
    if let Some(supplement) = &record.supplement {
        let raw = PlayerSnapshot::from_supplement(data.clone(), supplement)?;
        restore(world, player, &raw)?;
    }
    if !record.requires_handoff()
        && !super::super::player_events::release_initial_state(world, player)
    {
        return Err(SnapshotError::MissingPlayer);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
