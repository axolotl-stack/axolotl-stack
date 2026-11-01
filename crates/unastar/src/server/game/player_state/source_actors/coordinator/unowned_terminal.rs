//! Actual native post-drop persistence precedes publication, without inventing module ownership.

use super::*;
use crate::plugin::manager::players::PlayerLineage;
use crate::storage::{InventoryEffectRecord, terminal::NativeTerminalRecord};
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;
#[derive(Default, Clone)]
struct Budget(Arc<AtomicUsize>);
struct Lease {
    budget: Budget,
    bytes: usize,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.budget.0.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
impl Budget {
    /// Reserve owned snapshots before cloning any dynamic native data.
    fn reserve(&self, bytes: usize) -> Result<Arc<Lease>, Error> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|sum| *sum <= MAX_PENDING_BYTES)
            })
            .map_err(|_| Error::LimitExceeded)?;
        Ok(Arc::new(Lease {
            budget: self.clone(),
            bytes,
        }))
    }
}
#[derive(Component)]
struct Pending {
    uuid: uuid::Uuid,
    xuid: Option<String>,
    death: uuid::Uuid,
}

/// The actual body and exact canonical terminal receipt stay distinguishable from history.
#[derive(Component, Clone)]
pub(crate) struct Witness {
    pub proof: NativeTerminalRecord,
    pub data: Arc<PlayerData>,
    pub lineage: PlayerLineage,
    pub world: bevy_ecs::world::WorldId,
    _bytes: Arc<Lease>,
}
#[derive(Resource)]
struct Runtime {
    writer: Arc<SerializedPlayerProvider>,
    budget: Budget,
    tx: mpsc::Sender<Completion>,
    rx: Mutex<mpsc::Receiver<Completion>>,
}
struct Ticket {
    player: Entity,
    world: bevy_ecs::world::WorldId,
    dimension: i32,
    lineage: PlayerLineage,
    proof: NativeTerminalRecord,
    death: Arc<TerminalDeath>,
    _bytes: Arc<Lease>,
}
struct Completion {
    ticket: Arc<Ticket>,
    hold: quarantine::Handle,
    result: Result<Arc<PlayerData>, Failure>,
}

/// Accepted terminal work blocks competing ordinary account writes and admission.
pub(crate) fn pending(world: &World, player: Entity) -> bool {
    world.get::<Pending>(player).is_some() || world.get::<Adopting>(player).is_some()
}
/// Account equality gates admission only; it never selects a source actor.
pub(crate) fn pending_account(world: &World, uuid: uuid::Uuid, xuid: Option<&str>) -> bool {
    let same = |other: uuid::Uuid, other_xuid: Option<&str>| {
        other == uuid
            || xuid
                .filter(|id| !id.is_empty())
                .is_some_and(|id| other_xuid == Some(id))
    };
    world.try_query::<&Pending>().is_some_and(|mut q| {
        q.iter(world)
            .any(|marker| same(marker.uuid, marker.xuid.as_deref()))
    }) || world.try_query::<&Adopting>().is_some_and(|mut q| {
        q.iter(world)
            .any(|marker| same(marker.uuid, marker.xuid.as_deref()))
    })
}
/// This uses the same serialized canonical player provider as every other state effect.
pub(super) fn install(server: &mut super::super::super::super::GameServer) {
    if server.ecs.world().contains_resource::<Runtime>() {
        return;
    }
    let writer = server.ecs.world().resource::<Coordinator>().writer.clone();
    let (tx, rx) = mpsc::channel();
    server.ecs.world_mut().insert_resource(Runtime {
        writer,
        budget: Budget::default(),
        tx,
        rx: Mutex::new(rx),
    });
    server
        .ecs
        .schedule_mut()
        .add_systems(poll.before(crate::ecs::PacketApplySet));
}
/// Ordinary native retention never implies module/source ownership.
pub(super) fn retained(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
) -> bool {
    if world
        .get::<super::super::super::super::player_events::DetachedPlayer>(player)
        .is_none()
        || world.get::<PlayerSession>(player).is_some()
    {
        return false;
    }
    begin(world, player, cause, attribution, rules)
}
/// A native inventory continuation may also hold an actual online corpse after its exact ACK.
#[allow(clippy::too_many_arguments)]
pub(crate) fn after_inventory(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
    lineage: &PlayerLineage,
    id: &str,
    receipt: &InventoryEffectRecord,
) -> bool {
    if !lineage.matches(world, player)
        || !world
            .get::<crate::server::game::inventory_effects::InventoryPersistence>(player)
            .is_some_and(|state| state.receipts().get(id) == Some(receipt))
    {
        return false;
    }
    begin(world, player, cause, attribution, rules)
}
pub(super) fn begin(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
) -> bool {
    if !world.contains_resource::<Runtime>()
        || world.get::<Dead>(player).is_none()
        || world.get::<Player>(player).is_none()
    {
        return false;
    }
    if pending(world, player) {
        return true;
    }
    let Some(uuid) = world.get::<PlayerUuid>(player).map(|uuid| uuid.0) else {
        return false;
    };
    let Some(death) = world
        .get::<crate::entity::player_life::death::NativeDeathId>(player)
        .map(|id| id.0)
    else {
        return false;
    };
    let xuid = account_xuid(world, player).map(str::to_owned);
    world
        .entity_mut(player)
        .insert(Pending { uuid, xuid, death });
    let result = (|| {
        let lineage = world
            .get::<PlayerLineage>(player)
            .ok_or(Error::SourceGone)?
            .clone();
        if !lineage.matches(world, player) {
            return Err(Error::SourceGone);
        }
        let identity = world
            .get_resource::<crate::world::WorldIdentity>()
            .ok_or(Error::WrongWorld)?
            .clone();
        let lifetime = world
            .get_resource::<crate::entity::damage::DamageWorldLifetime>()
            .ok_or(Error::WrongWorld)?
            .0;
        let size = crate::server::game::player_state::death_capture::estimate(world, player)
            .map_err(|_| Error::LimitExceeded)?;
        let bytes = world
            .resource::<Runtime>()
            .budget
            .reserve(size.checked_mul(6).ok_or(Error::LimitExceeded)?)?;
        let pre = Arc::new(capture(world, player).map_err(|_| Error::InvalidState)?);
        let post = post_drop(world, player, &pre, rules.keep_inventory)?;
        let proof = NativeTerminalRecord {
            version: 1,
            death: death.to_string(),
            world: identity.id.to_string(),
            lifetime: lifetime.to_string(),
            dimension: identity.dimension,
            lineage: lineage.key().to_string(),
            name: world
                .get::<crate::entity::components::PlayerName>(player)
                .ok_or(Error::SourceGone)?
                .0
                .clone(),
            xuid: pre.xuid.clone(),
            id: format!("native-terminal:{lifetime}:{death}"),
            receipt: InventoryEffectRecord {
                revision: 0,
                request_hash: [0; 32],
            },
        };
        let ticket = Arc::new(Ticket {
            player,
            world: world.id(),
            dimension: identity.dimension,
            lineage,
            proof,
            death: Arc::new(TerminalDeath {
                cause,
                attribution,
                rules,
            }),
            _bytes: bytes,
        });
        let hold = quarantine::hold(world, player, pre, quarantine::Role::Source, ticket.clone())
            .map_err(|_| Error::Conflict)?;
        let runtime = world.resource::<Runtime>();
        let writer = runtime.writer.clone();
        let tx = runtime.tx.clone();
        let handle = tokio::runtime::Handle::try_current().map_err(|_| Error::NotInstalled)?;
        handle.spawn(async move {
            let guard = WorkerGuard {
                writer: writer.clone(),
                uuid,
                done: false,
            };
            let result = persist(&writer, &ticket, post).await;
            if tx
                .send(Completion {
                    ticket,
                    hold,
                    result,
                })
                .is_err()
            {
                writer.require_recovery(uuid);
            }
            guard.complete();
        });
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        world.resource::<Runtime>().writer.require_recovery(uuid);
        tracing::error!(?error, "Native terminal publication remains withheld");
    }
    true
}
struct WorkerGuard {
    writer: Arc<SerializedPlayerProvider>,
    uuid: uuid::Uuid,
    done: bool,
}
impl WorkerGuard {
    fn complete(mut self) {
        self.done = true;
    }
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.done {
            self.writer.require_recovery(self.uuid);
        }
    }
}
async fn persist(
    writer: &SerializedPlayerProvider,
    ticket: &Ticket,
    post: PlayerSnapshot,
) -> Result<Arc<PlayerData>, Failure> {
    let uuid = uuid::Uuid::parse_str(&post.data.uuid).map_err(|_| Failure {
        error: Error::InvalidState,
        uncertain: false,
    })?;
    let mut data = post.data.clone();
    data.game_mode = post.base_game_mode.unwrap_or(data.game_mode);
    let written = writer
        .commit_native_terminal(uuid, data, post.supplement(), ticket.proof.clone())
        .await
        .map_err(write_error)?;
    let proof = written.data.native_terminal.as_ref().ok_or(Failure {
        error: Error::RecoveryRequired,
        uncertain: true,
    })?;
    if proof.death != ticket.proof.death
        || proof.receipt != written.effect.record
        || proof.validate(&written.data).is_err()
    {
        writer.require_recovery(uuid);
        return Err(Failure {
            error: Error::RecoveryRequired,
            uncertain: true,
        });
    }
    Ok(Arc::new(written.data))
}
fn poll(world: &mut World) {
    let completed = world
        .get_resource::<Runtime>()
        .map_or_else(Vec::new, |runtime| {
            runtime.rx.lock().try_iter().collect::<Vec<_>>()
        });
    for completion in completed {
        if !crate::server::worlds::queue_router_request(world, move |router, _| {
            apply(router, completion)
        }) {
            tracing::error!("Native terminal result requires account recovery");
        }
    }
}
fn apply(router: &mut crate::server::worlds::DimensionRouter, completion: Completion) {
    let ticket = completion.ticket;
    let Some(server) = router.world_mut(ticket.dimension) else {
        return;
    };
    let world = server.ecs.world_mut();
    if world.id() != ticket.world
        || !ticket.lineage.matches(world, ticket.player)
        || world
            .get::<Pending>(ticket.player)
            .is_none_or(|pending| pending.death.to_string() != ticket.proof.death)
        || !quarantine::matches(world, &completion.hold)
    {
        return;
    }
    let Ok(data) = completion.result else {
        return;
    };
    if !quarantine::acknowledge_source_receipts(world, &completion.hold, &data)
        || !quarantine::release_source(world, &completion.hold)
    {
        return;
    }
    let proof = data
        .native_terminal
        .as_ref()
        .expect("validated terminal record")
        .clone();
    world.entity_mut(ticket.player).insert((
        Witness {
            proof,
            data: data.clone(),
            lineage: ticket.lineage.clone(),
            world: ticket.world,
            _bytes: ticket._bytes.clone(),
        },
        crate::server::game::inventory_effects::InventoryPersistence::from_record(Some(&data)),
    ));
    world.entity_mut(ticket.player).remove::<Pending>();
    crate::entity::player_life::death::finish_death_with_rules(
        world,
        ticket.player,
        ticket.death.cause,
        ticket.death.attribution,
        ticket.death.rules.clone(),
    );
}

#[derive(Component)]
struct Adopting {
    uuid: uuid::Uuid,
    xuid: Option<String>,
    nonce: String,
}
/// Read only genuine ACK evidence attached to this exact still-dead native body.
pub(crate) fn witness(world: &World, player: Entity) -> Result<Witness, Error> {
    if let Some(pending) = world.get::<Pending>(player) {
        return Err(
            if world
                .get_resource::<Runtime>()
                .is_some_and(|runtime| runtime.writer.recovery_required(pending.uuid))
            {
                Error::RecoveryRequired
            } else {
                Error::Busy
            },
        );
    }
    let witness = world.get::<Witness>(player).ok_or(Error::NotOwned)?;
    if witness.world != world.id()
        || !witness.lineage.matches(world, player)
        || world.get::<Dead>(player).is_none()
        || world.get::<ManagedSource>(player).is_some()
        || witness.proof.validate(&witness.data).is_err()
        || world
            .get::<PlayerUuid>(player)
            .is_none_or(|uuid| uuid.0.to_string() != witness.data.uuid)
        || account_xuid(world, player) != witness.proof.xuid.as_deref()
    {
        return Err(Error::SourceGone);
    }
    Ok(witness.clone())
}
/// Accepted adoption serializes this exact body without claiming a historical archive.
pub(super) fn accept_adoption(
    world: &mut World,
    operation: &Arc<Operation>,
    player: Entity,
    expected: &Witness,
) -> Result<(), Error> {
    let current = witness(world, player)?;
    if current.proof != expected.proof
        || current.lineage.key() != expected.lineage.key()
        || pending(world, player)
        || world.get::<Adopting>(player).is_some()
        || current.proof.name != operation.reference.name
        || current.proof.world != operation.reference.world.to_string()
    {
        return Err(Error::Conflict);
    }
    world.entity_mut(player).insert(Adopting {
        uuid: operation.reference.uuid,
        xuid: operation.reference.xuid.clone(),
        nonce: operation.reference.nonce.clone(),
    });
    Ok(())
}
/// Only this exact accepted adoption may be retried or cleared.
pub(super) fn adoption_matches(world: &World, player: Entity, operation: &Operation) -> bool {
    world
        .get::<Adopting>(player)
        .is_some_and(|marker| marker.nonce == operation.reference.nonce)
}
/// Definite rejection leaves the actual native terminal witness available, not consumed.
pub(super) fn reject_adoption(world: &mut World, player: Entity, operation: &Operation) {
    if adoption_matches(world, player, operation) {
        world.entity_mut(player).remove::<Adopting>();
    }
}

/// The native marker has no externally supplied identity or actor selection.
pub(super) fn is_adopting(world: &World, player: Entity) -> bool {
    world.get::<Adopting>(player).is_some()
}
