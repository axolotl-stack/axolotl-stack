//! Durable ownership acknowledgement precedes an explicit, atomic live publication.
use super::*;
use crate::plugin::manager::players::PlayerLineage;
use crate::server::game::player_events::ConnectionIdentity;
use crate::storage::InventoryEffectRecord;
use std::sync::atomic::{AtomicBool, Ordering};

type Check = Arc<dyn Fn(&World) -> bool + Send + Sync>;
/// Only actual native hold bookkeeping supplies these internal checks.
#[derive(Clone)]
pub(crate) struct Authority {
    pub valid: Check,
    pub ready: Check,
    pub allow: Check,
    pub reject: Check,
}
#[derive(Clone)]
pub(crate) enum Status {
    Staged,
    Queued,
    Acknowledged(InventoryEffectRecord),
    Rebound(InventoryEffectRecord),
    Terminal(InventoryEffectRecord, bool),
    Retired(InventoryEffectRecord, bool),
    Failed(InventoryEffectRecord, Error),
    Unowned,
    Rejected(Error),
    Uncertain,
    Cancelled,
}
struct Progress {
    status: Status,
    ack: Option<Arc<Ack>>,
}
struct Ack {
    observed: Arc<Observation>,
    receipt: InventoryEffectRecord,
}
/// Accepted native ownership survives guest ticket drops without protecting a living source.
pub(crate) struct Operation {
    pub id: uuid::Uuid,
    pub correlation: u64,
    pub source: Arc<super::Operation>,
    pub source_player: Entity,
    pub target: Entity,
    pub target_lineage: PlayerLineage,
    source_lineage: PlayerLineage,
    target_connection: ConnectionIdentity,
    authority: Authority,
    expected: Arc<PlayerSnapshot>,
    hold: Mutex<Option<quarantine::Handle>>,
    progress: Mutex<Progress>,
    accepted: AtomicBool,
    terminal: AtomicBool,
    retiring: AtomicBool,
    written: Mutex<Option<Result<Arc<Ack>, Failure>>>,
    ready: tokio::sync::Notify,
    notice: Arc<dyn Fn(Status) + Send + Sync>,
    reserve: Reserve,
    _retention: Arc<dyn Send + Sync>,
}
impl Operation {
    /// Owned completion remains readable without a live player context.
    pub(crate) fn status(&self) -> Status {
        self.progress.lock().status.clone()
    }
    /// A failed callback cannot start provider work or consume another native actor.
    pub(crate) fn cancel(&self) {
        self.set(Status::Cancelled);
    }
    fn set(&self, status: Status) {
        self.progress.lock().status = status.clone();
        (self.notice)(status);
    }
    /// Resolve only the actual published replacement connection.
    pub(crate) fn player(&self, world: &World) -> Option<Entity> {
        (matches!(self.status(), Status::Rebound(_))
            && world.id() == self.source.world
            && self.target_lineage.matches(world, self.target)
            && self.target_connection.matches(world, self.target))
        .then_some(self.target)
    }
    fn original(&self, world: &World) -> bool {
        world.id() == self.source.world
            && self.source_lineage.matches(world, self.source_player)
            && world.get::<PlayerSession>(self.source_player).is_none()
            && world
                .get::<ManagedSource>(self.source_player)
                .is_some_and(|source| {
                    self.source.observation().is_some_and(|observed| {
                        source.record.matches(&SourceProof::from(&observed.source))
                    })
                })
    }
    fn replacement(&self, world: &World) -> bool {
        world.id() == self.source.world
            && self.target_lineage.matches(world, self.target)
            && self.target_connection.matches(world, self.target)
            && crate::server::game::player_events::initial_state_pending(world, self.target)
            && (self.authority.valid)(world)
    }
}
#[derive(Component)]
struct Pending(Arc<Operation>);
#[derive(Resource)]
struct Runtime {
    writer: Arc<SerializedPlayerProvider>,
    tx: mpsc::Sender<Completion>,
    rx: Mutex<mpsc::Receiver<Completion>>,
    waiting: Mutex<Vec<Arc<Operation>>>,
}
enum Completion {
    Ownership {
        operation: Arc<Operation>,
        result: Result<Arc<Ack>, Failure>,
    },
    Terminal {
        operation: Arc<Operation>,
        hold: quarantine::Handle,
        death: Arc<TerminalDeath>,
        result: Result<Arc<Observation>, Failure>,
    },
}
/// Same-account initializers and durable mutations wait on accepted ownership work.
pub(crate) fn pending(world: &World, player: Entity) -> bool {
    world.get::<Pending>(player).is_some()
}
/// Account matching never selects or creates a physical source.
pub(crate) fn pending_account(world: &World, uuid: uuid::Uuid, xuid: Option<&str>) -> bool {
    world.try_query::<&Pending>().is_some_and(|mut query| {
        query.iter(world).any(|pending| {
            pending.0.source.reference.uuid == uuid
                || xuid
                    .filter(|x| !x.is_empty())
                    .is_some_and(|x| pending.0.source.reference.xuid.as_deref() == Some(x))
        })
    })
}
/// Install on the same serialized player provider as source checkpoints and state installs.
pub(super) fn install(
    server: &mut super::super::super::super::GameServer,
) -> Result<(), super::super::super::super::inventory_effects::EffectError> {
    let world = server.ecs.world_mut();
    if world.contains_resource::<Runtime>() {
        return Ok(());
    }
    let writer = world.resource::<Coordinator>().writer.clone();
    let (tx, rx) = mpsc::channel();
    world.insert_resource(Runtime {
        writer,
        tx,
        rx: Mutex::new(rx),
        waiting: Mutex::new(Vec::new()),
    });
    server
        .ecs
        .schedule_mut()
        .add_systems(poll.before(crate::ecs::PacketApplySet));
    Ok(())
}
/// Stage owned real-world evidence; actual acceptance still waits for callback success.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    world: &World,
    source: Arc<super::Operation>,
    target: Entity,
    target_lineage: PlayerLineage,
    expected: Arc<PlayerSnapshot>,
    authority: Authority,
    correlation: u64,
    reserve: Reserve,
    retention: Arc<dyn Send + Sync>,
    notice: Arc<dyn Fn(Status) + Send + Sync>,
) -> Result<Arc<Operation>, Error> {
    if !world.contains_resource::<Runtime>() {
        return Err(Error::NotInstalled);
    }
    if world.id() != source.world {
        return Err(Error::WrongWorld);
    }
    let observed = source.observation().ok_or(Error::NotOwned)?;
    if observed.source.phase != SourcePhase::Active {
        return Err(Error::NotOwned);
    }
    let source_player = source.player(world).ok_or(Error::SourceGone)?;
    if source_player == target
        || crate::server::game::player_state::source_actors::pending(world, source_player)
        || crate::server::game::inventory_effects::is_quarantined(world, source_player)
        || world.get::<Dead>(source_player).is_some()
    {
        return Err(Error::Busy);
    }
    if world
        .get::<PlayerUuid>(target)
        .is_none_or(|uuid| uuid.0 != source.reference.uuid)
        || account_xuid(world, target) != source.reference.xuid.as_deref()
    {
        return Err(Error::NotOwned);
    }
    let source_lineage = world
        .get::<PlayerLineage>(source_player)
        .ok_or(Error::SourceGone)?
        .clone();
    let target_connection = ConnectionIdentity::capture(world, target).ok_or(Error::SourceGone)?;
    let operation = Arc::new(Operation {
        id: uuid::Uuid::new_v4(),
        correlation,
        source,
        source_player,
        target,
        target_lineage,
        source_lineage,
        target_connection,
        authority,
        expected,
        hold: Mutex::new(None),
        progress: Mutex::new(Progress {
            status: Status::Staged,
            ack: None,
        }),
        accepted: AtomicBool::new(false),
        terminal: AtomicBool::new(false),
        retiring: AtomicBool::new(false),
        written: Mutex::new(None),
        ready: tokio::sync::Notify::new(),
        notice,
        reserve,
        _retention: retention,
    });
    if !operation.replacement(world) {
        return Err(Error::SourceGone);
    }
    Ok(operation)
}
/// The request key belongs to this exact native source generation and generated request.
fn key(operation: &Operation) -> Result<String, Error> {
    let observed = operation.source.observation().ok_or(Error::NotOwned)?;
    Ok(format!(
        "source-rebind:{}:{}:{}",
        operation.source.reference.nonce, observed.source.generation, operation.id
    ))
}
/// Hold only the replacement. The original actor remains physically complete and attackable.
pub(crate) fn submit(world: &mut World, operation: Arc<Operation>) {
    let result = (|| {
        if !operation.original(world) || !operation.replacement(world) {
            return Err(Error::SourceGone);
        }
        if crate::server::game::inventory_effects::is_quarantined(world, operation.source_player)
            || world.get::<Dead>(operation.source_player).is_some()
        {
            return Err(Error::Busy);
        }
        if crate::server::game::player_state::source_actors::pending(world, operation.source_player)
        {
            return Err(Error::Busy);
        }
        let hold = quarantine::hold(
            world,
            operation.target,
            operation.expected.clone(),
            quarantine::Role::Target,
            operation.clone(),
        )
        .map_err(|_| Error::Conflict)?;
        *operation.hold.lock() = Some(hold);
        world
            .entity_mut(operation.source_player)
            .insert(Pending(operation.clone()));
        world
            .entity_mut(operation.target)
            .insert(SourceAdmissionBlocked);
        let cloned = operation.clone();
        if !crate::server::worlds::queue_router_request(world, move |router, _| {
            start(router, cloned)
        }) {
            return Err(Error::Busy);
        }
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        reject(world, &operation);
        operation.set(Status::Rejected(error));
    }
}
fn reject(world: &mut World, operation: &Arc<Operation>) {
    if let Some(hold) = operation.hold.lock().take() {
        quarantine::reject(world, &hold);
    }
    if world
        .get::<Pending>(operation.source_player)
        .is_some_and(|pending| Arc::ptr_eq(&pending.0, operation))
    {
        world
            .entity_mut(operation.source_player)
            .remove::<Pending>();
    }
}
fn duplicate_request(
    router: &mut crate::server::worlds::DimensionRouter,
    operation: &Operation,
) -> bool {
    for (_, server) in router.iter_mut() {
        let world = server.ecs.world_mut();
        let mut query = world.query_filtered::<(Entity, &PlayerUuid), With<Player>>();
        if query.iter(world).any(|(actor, uuid)| {
            let same = uuid.0 == operation.source.reference.uuid
                || operation
                    .source
                    .reference
                    .xuid
                    .as_deref()
                    .is_some_and(|xuid| account_xuid(world, actor) == Some(xuid));
            same && !(world.id() == operation.source.world
                && (actor == operation.source_player || actor == operation.target))
        }) {
            return true;
        }
    }
    false
}
/// Interruption is never reported as a definite rejected ownership write.
struct WorkerGuard {
    operation: Arc<Operation>,
    writer: Arc<SerializedPlayerProvider>,
    initial: bool,
    done: bool,
}
impl WorkerGuard {
    fn complete(mut self) {
        self.done = true;
    }
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        self.writer
            .require_recovery(self.operation.source.reference.uuid);
        if self.initial {
            let mut written = self.operation.written.lock();
            if written.is_none() {
                *written = Some(Err(Failure {
                    error: Error::RecoveryRequired,
                    uncertain: true,
                }));
            }
            self.operation.ready.notify_waiters();
        }
        self.operation.set(Status::Uncertain);
    }
}
fn start(router: &mut crate::server::worlds::DimensionRouter, operation: Arc<Operation>) {
    let result = (|| {
        if duplicate_request(router, &operation) {
            return Err(Error::Conflict);
        }
        let server = router
            .world_mut(operation.source.reference.dimension)
            .ok_or(Error::WrongWorld)?;
        let world = server.ecs.world_mut();
        if !operation.original(world) || !operation.replacement(world) {
            return Err(Error::SourceGone);
        }
        let size = crate::server::game::player_state::death_capture::estimate(
            world,
            operation.source_player,
        )
        .map_err(|_| Error::LimitExceeded)?;
        let lease = (operation.reserve)(size.checked_mul(6).ok_or(Error::LimitExceeded)?)?;
        let snapshot =
            Arc::new(capture(world, operation.source_player).map_err(|_| Error::InvalidState)?);
        let runtime = world.get_resource::<Runtime>().ok_or(Error::NotInstalled)?;
        let writer = runtime.writer.clone();
        let tx = runtime.tx.clone();
        let handle = tokio::runtime::Handle::try_current().map_err(|_| Error::NotInstalled)?;
        operation.set(Status::Queued);
        let operation = operation.clone();
        handle.spawn(async move {
            let guard = WorkerGuard {
                operation: operation.clone(),
                writer: writer.clone(),
                initial: true,
                done: false,
            };
            let result = persist(&writer, &operation, snapshot, lease).await;
            *operation.written.lock() = Some(result.clone());
            operation.ready.notify_waiters();
            if tx
                .send(Completion::Ownership {
                    operation: operation.clone(),
                    result,
                })
                .is_err()
            {
                writer.require_recovery(operation.source.reference.uuid);
                operation.set(Status::Uncertain);
            }
            guard.complete();
        });
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        if let Some(server) = router.world_mut(operation.source.reference.dimension)
            && server.ecs.world().id() == operation.source.world
        {
            reject(server.ecs.world_mut(), &operation);
        }
        *operation.written.lock() = Some(Err(Failure {
            error,
            uncertain: false,
        }));
        operation.ready.notify_waiters();
        operation.set(Status::Rejected(error));
    }
}
async fn persist(
    writer: &SerializedPlayerProvider,
    operation: &Operation,
    snapshot: Arc<PlayerSnapshot>,
    lease: Arc<dyn Send + Sync>,
) -> Result<Arc<Ack>, Failure> {
    let fail = |error| Failure {
        error,
        uncertain: false,
    };
    let observed = operation
        .source
        .observation()
        .ok_or_else(|| fail(Error::NotOwned))?;
    let mut data = snapshot.data.clone();
    data.game_mode = snapshot.base_game_mode.unwrap_or(data.game_mode);
    data.source_actor = None;
    use sha2::Digest;
    let hash = sha2::Sha256::digest(
        bounded_encode(&(
            *operation.id.as_bytes(),
            operation.correlation,
            *operation.target_lineage.key().as_bytes(),
            &data,
            &snapshot.supplement(),
        ))
        .map_err(fail)?,
    )
    .into();
    let key = key(operation).map_err(fail)?;
    let effect = writer
        .commit_state_effect(
            operation.source.reference.uuid,
            &key,
            hash,
            data,
            snapshot.supplement(),
            crate::storage::player_writer::StateOrigin {
                source: Some(SourceProof::from(&observed.source)),
                baseline: None,
            },
        )
        .await
        .map_err(write_error)?;
    let data = writer
        .load(operation.source.reference.uuid)
        .await
        .map_err(|_| Failure {
            error: Error::RecoveryRequired,
            uncertain: true,
        })?
        .ok_or_else(|| fail(Error::RecoveryRequired))?;
    let source = validate_reference(&data, &operation.source.reference).map_err(fail)?;
    if data.inventory_effects.get(&key) != Some(&effect.record)
        || source.phase != SourcePhase::Transferred
        || source
            .handoff
            .as_ref()
            .is_none_or(|handoff| handoff.id != key || handoff.request_hash != hash)
    {
        return Err(Failure {
            error: Error::RecoveryRequired,
            uncertain: true,
        });
    }
    Ok(Arc::new(Ack {
        receipt: effect.record,
        observed: Arc::new(Observation {
            data: Arc::new(data),
            source,
            _bytes: lease,
        }),
    }))
}
/// Callback acceptance is separate from native initial publication and backpressure.
pub(crate) fn publish(world: &mut World, operation: &Arc<Operation>) -> Result<(), Error> {
    if operation.accepted.load(Ordering::Acquire) {
        return Ok(());
    }
    if !matches!(operation.status(), Status::Acknowledged(_)) {
        return Err(Error::Conflict);
    }
    if !operation.replacement(world) || !(operation.authority.allow)(world) {
        let acknowledged = operation.progress.lock().ack.clone();
        if let Some(ack) = acknowledged {
            operation.set(Status::Failed(ack.receipt.clone(), Error::SourceGone));
        }
        (operation.authority.reject)(world);
        return Err(Error::SourceGone);
    }
    operation.accepted.store(true, Ordering::Release);
    world
        .resource::<Runtime>()
        .waiting
        .lock()
        .push(operation.clone());
    Ok(())
}
fn poll(world: &mut World) {
    let (completed, waiting) = world.get_resource::<Runtime>().map_or_else(
        || (Vec::new(), Vec::new()),
        |runtime| {
            (
                runtime.rx.lock().try_iter().collect::<Vec<_>>(),
                std::mem::take(&mut *runtime.waiting.lock()),
            )
        },
    );
    for completion in completed {
        let operation = match &completion {
            Completion::Ownership { operation, .. } | Completion::Terminal { operation, .. } => {
                operation.clone()
            }
        };
        if !crate::server::worlds::queue_router_request(world, move |router, _| {
            complete(router, completion)
        }) {
            operation.set(Status::Uncertain);
        }
    }
    for operation in waiting {
        let retry = operation.clone();
        if !crate::server::worlds::queue_router_request(world, move |router, _| {
            apply(router, retry)
        }) {
            operation.set(Status::Uncertain);
        }
    }
}
fn complete(router: &mut crate::server::worlds::DimensionRouter, completion: Completion) {
    let Completion::Ownership { operation, result } = completion else {
        let Completion::Terminal {
            operation,
            hold,
            death,
            result,
        } = completion
        else {
            unreachable!()
        };
        if let Ok(observed) = &result {
            let key = format!(
                "source:{}:{}:terminal",
                operation.source.reference.nonce, observed.source.generation
            );
            let receipt = observed.data.inventory_effects.get(&key).cloned();
            super::apply(
                router,
                super::Completion {
                    operation: operation.source.clone(),
                    action: Apply::Terminal {
                        player: operation.source_player,
                        hold,
                        death,
                    },
                    result,
                    remaining: 200,
                },
            );
            if let Some(receipt) = receipt {
                let applied = matches!(
                    operation.source.status(),
                    super::Status::Committed {
                        phase: Phase::Terminal,
                        live_applied: true,
                        ..
                    }
                );
                if let Some(server) = router.world_mut(operation.source.reference.dimension)
                    && server.ecs.world().id() == operation.source.world
                {
                    reject(server.ecs.world_mut(), &operation);
                    (operation.authority.reject)(server.ecs.world());
                }
                operation.set(Status::Terminal(receipt, applied));
            } else {
                operation.set(Status::Uncertain);
            }
        } else {
            operation.set(Status::Uncertain);
        }
        return;
    };
    match result {
        Ok(ack) => {
            operation.progress.lock().ack = Some(ack.clone());
            if !operation.terminal.load(Ordering::Acquire) {
                operation.set(Status::Acknowledged(ack.receipt.clone()));
            }
        }
        Err(failure) => {
            if !failure.uncertain
                && let Some(server) = router.world_mut(operation.source.reference.dimension)
                && server.ecs.world().id() == operation.source.world
            {
                reject(server.ecs.world_mut(), &operation);
            }
            operation.set(if failure.uncertain {
                Status::Uncertain
            } else {
                Status::Rejected(failure.error)
            });
        }
    }
}
fn apply(router: &mut crate::server::worlds::DimensionRouter, operation: Arc<Operation>) {
    if operation.terminal.load(Ordering::Acquire) || operation.retiring.load(Ordering::Acquire) {
        return;
    }
    let Some(server) = router.world_mut(operation.source.reference.dimension) else {
        return;
    };
    let world = server.ecs.world_mut();
    if !operation.original(world) || !operation.replacement(world) {
        let acknowledged = operation.progress.lock().ack.clone();
        if let Some(ack) = acknowledged {
            operation.set(Status::Failed(ack.receipt.clone(), Error::SourceGone));
        }
        (operation.authority.reject)(world);
        return;
    }
    if !(operation.authority.ready)(world) {
        world.resource::<Runtime>().waiting.lock().push(operation);
        return;
    }
    if world
        .get::<PlayerSession>(operation.target)
        .is_none_or(|session| session.outbound_tx.capacity() == 0)
    {
        world.resource::<Runtime>().waiting.lock().push(operation);
        return;
    }
    let Some(ack) = operation.progress.lock().ack.clone() else {
        return;
    };
    let Some(hold) = operation.hold.lock().clone() else {
        return;
    };
    let Ok(moved) = quarantine::move_live(world, operation.source_player, &hold) else {
        return;
    };
    release_admission(world, operation.target);
    let Some(mut batch) = server.initial_packet_batch(operation.target) else {
        moved.rollback(server.ecs.world_mut());
        server
            .ecs
            .world_mut()
            .entity_mut(operation.target)
            .insert(SourceAdmissionBlocked);
        server
            .ecs
            .world()
            .resource::<Runtime>()
            .waiting
            .lock()
            .push(operation);
        return;
    };
    let Ok(raw_packets) =
        super::super::super::live_initial_packets(server.ecs.world(), operation.target)
    else {
        moved.rollback(server.ecs.world_mut());
        server
            .ecs
            .world_mut()
            .entity_mut(operation.target)
            .insert(SourceAdmissionBlocked);
        server
            .ecs
            .world()
            .resource::<Runtime>()
            .waiting
            .lock()
            .push(operation);
        return;
    };
    batch.packets.extend(raw_packets);
    let sender = server
        .ecs
        .world()
        .get::<PlayerSession>(operation.target)
        .expect("validated target session")
        .outbound_tx
        .clone();
    let Ok(permits) = sender.try_reserve_many(batch.packets.len()) else {
        moved.rollback(server.ecs.world_mut());
        server
            .ecs
            .world_mut()
            .entity_mut(operation.target)
            .insert(SourceAdmissionBlocked);
        server
            .ecs
            .world()
            .resource::<Runtime>()
            .waiting
            .lock()
            .push(operation);
        return;
    };
    let world = server.ecs.world_mut();
    if !crate::server::game::player_events::release_initial_state(world, operation.target)
        || !crate::server::game::player_events::take_ready_initial_state(world, operation.target)
    {
        moved.rollback(world);
        world
            .entity_mut(operation.target)
            .insert(SourceAdmissionBlocked);
        crate::server::game::player_events::defer_initial_state(world, operation.target);
        world.resource::<Runtime>().waiting.lock().push(operation);
        return;
    }
    crate::server::broadcast::player_views::forget_world_player(world, operation.source_player);
    world.despawn(operation.source_player);
    moved.commit();
    world.entity_mut(operation.target).insert(
        crate::server::game::inventory_effects::InventoryPersistence::from_record(Some(
            &ack.observed.data,
        )),
    );
    for (permit, packet) in permits.zip(batch.packets) {
        permit.send(packet);
    }
    crate::plugin::manager::commands::note_initial_packet(world, operation.target, batch.stamp);
    crate::server::game::player_events::mark_initial_state_published(world, operation.target);
    super::super::super::note_live_initial_sent(world, operation.target);
    if let (Some(position), Some(runtime)) = (
        world.get::<Position>(operation.target),
        world.get::<RuntimeEntityId>(operation.target),
    ) {
        world.write_message(crate::world::ecs::PlayerSpawnedEvent {
            entity: operation.target,
            position: position.0,
            runtime_id: runtime.0,
        });
    }
    crate::plugin::manager::sessions::published(world, operation.target);
    operation.set(Status::Rebound(ack.receipt.clone()));
}

/// The live source is held only after genuine death; its exact terminal row
/// follows the accepted ownership receipt before any native drop is published.
pub(super) fn defer_death(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
) -> bool {
    let Some(operation) = world
        .get::<Pending>(player)
        .map(|pending| pending.0.clone())
    else {
        return false;
    };
    if operation.terminal.swap(true, Ordering::AcqRel) {
        return true;
    }
    let result = (|| {
        if !operation.original(world) || world.get::<Dead>(player).is_none() {
            return Err(Error::SourceGone);
        }
        let size = crate::server::game::player_state::death_capture::estimate(world, player)
            .map_err(|_| Error::LimitExceeded)?;
        let lease = (operation.reserve)(size.checked_mul(3).ok_or(Error::LimitExceeded)?)?;
        let pre = Arc::new(capture(world, player).map_err(|_| Error::InvalidState)?);
        let mut post = post_drop(world, player, &pre)?;
        let hold = quarantine::hold(
            world,
            player,
            pre,
            quarantine::Role::Source,
            Arc::new((operation.clone(), lease)),
        )
        .map_err(|_| Error::Conflict)?;
        world
            .entity_mut(player)
            .insert((SourceBody(hold.clone()), TerminalPending));
        let death = Arc::new(TerminalDeath {
            cause,
            attribution,
            rules: world.resource::<crate::world::rules::GameRules>().clone(),
        });
        let runtime = world.resource::<Runtime>();
        let writer = runtime.writer.clone();
        let tx = runtime.tx.clone();
        let handle = tokio::runtime::Handle::try_current().map_err(|_| Error::NotInstalled)?;
        let operation = operation.clone();
        handle.spawn(async move {
            let guard = WorkerGuard {
                operation: operation.clone(),
                writer: writer.clone(),
                initial: false,
                done: false,
            };
            let initial = loop {
                let notified = operation.ready.notified();
                if let Some(result) = operation.written.lock().clone() {
                    break result;
                }
                notified.await;
            };
            let result = match initial {
                Ok(ack) => {
                    post.data.inventory_revision = ack.observed.data.inventory_revision;
                    post.data.inventory_effects = ack.observed.data.inventory_effects.clone();
                    if let Some(handoff) = ack.observed.source.handoff.clone() {
                        Work::TerminalAfterHandoff {
                            snapshot: Arc::new(post),
                            proof: SourceProof::from(&ack.observed.source),
                            handoff,
                        }
                        .execute(
                            &writer,
                            &operation.source.reference,
                            operation.reserve.clone(),
                        )
                        .await
                    } else {
                        Err(Failure {
                            error: Error::RecoveryRequired,
                            uncertain: true,
                        })
                    }
                }
                Err(_) => Err(Failure {
                    error: Error::RecoveryRequired,
                    uncertain: true,
                }),
            };
            if tx
                .send(Completion::Terminal {
                    operation: operation.clone(),
                    hold,
                    death,
                    result,
                })
                .is_err()
            {
                writer.require_recovery(operation.source.reference.uuid);
                operation.set(Status::Uncertain);
            }
            guard.complete();
        });
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        world
            .resource::<Runtime>()
            .writer
            .require_recovery(operation.source.reference.uuid);
        operation.set(Status::Uncertain);
        tracing::error!(
            ?error,
            "Native handoff terminal processing remains withheld"
        );
    }
    true
}

/// Reload observations contain no historical actor, network connection or fabricated hold.
pub(crate) enum Lookup {
    Live(Arc<Operation>),
    Readback(Arc<Readback>),
}

pub(crate) struct Readback {
    progress: Mutex<Status>,
    pub id: uuid::Uuid,
}
impl Readback {
    /// A read proves only canonical acknowledgement, never live application.
    pub(crate) fn status(&self) -> Status {
        self.progress.lock().clone()
    }
}
/// Only a genuine immutable source witness can query the generated request identifier.
pub(crate) fn inspect(
    world: &mut World,
    reference: Reference,
    generation: u64,
    id: uuid::Uuid,
    reserve: Reserve,
) -> Result<Lookup, Error> {
    let anchored = world.try_query::<&Pending>().and_then(|mut query| {
        query.iter(world).find_map(|pending| {
            let operation = &pending.0;
            (operation.id == id
                && operation.source.world == world.id()
                && operation.source.reference.module == reference.module
                && operation.source.reference.uuid == reference.uuid
                && operation.source.reference.xuid == reference.xuid
                && operation.source.reference.nonce == reference.nonce
                && operation.source.reference.world == reference.world
                && operation.source.reference.dimension == reference.dimension
                && operation
                    .source
                    .observation()
                    .is_some_and(|observed| observed.source.generation == generation)
                && operation
                    .source_lineage
                    .matches(world, operation.source_player))
            .then(|| operation.clone())
        })
    });
    if let Some(operation) = anchored {
        return Ok(Lookup::Live(operation));
    }
    let runtime = world.get_resource::<Runtime>().ok_or(Error::NotInstalled)?;
    let writer = runtime.writer.clone();
    let result = Arc::new(Readback {
        progress: Mutex::new(Status::Queued),
        id,
    });
    let observed = result.clone();
    let handle = tokio::runtime::Handle::try_current().map_err(|_| Error::NotInstalled)?;
    handle.spawn(async move {
        let state = async {
            let Some(data) = writer
                .load(reference.uuid)
                .await
                .map_err(|_| Error::RecoveryRequired)?
            else {
                return Ok(Status::Unowned);
            };
            let _lease = reserve(bytes(&data)?)?;
            let key = format!("source-rebind:{}:{}:{}", reference.nonce, generation, id);
            let Some(receipt) = data.inventory_effects.get(&key).cloned() else {
                return Ok(Status::Unowned);
            };
            if let Ok(source) = validate_reference(&data, &reference) {
                let suffix = match source.phase {
                    SourcePhase::Terminal => Some("terminal"),
                    SourcePhase::Retired => Some("retire"),
                    _ => None,
                };
                if let Some(suffix) = suffix {
                    let key = format!("source:{}:{}:{suffix}", reference.nonce, source.generation);
                    let receipt = data
                        .inventory_effects
                        .get(&key)
                        .cloned()
                        .ok_or(Error::RecoveryRequired)?;
                    return Ok(if source.phase == SourcePhase::Terminal {
                        Status::Terminal(receipt, false)
                    } else {
                        Status::Retired(receipt, false)
                    });
                }
            }
            Ok::<_, Error>(Status::Acknowledged(receipt))
        }
        .await;
        *observed.progress.lock() = match state {
            Ok(status) => status,
            Err(Error::RecoveryRequired) => Status::Uncertain,
            Err(error) => Status::Rejected(error),
        };
    });
    Ok(Lookup::Readback(result))
}

/// Only an actual native pending anchor and its acknowledged ownership can retire this body.
pub(super) fn prepare_retirement(
    world: &mut World,
    operation: &Arc<Operation>,
) -> Result<(Work, Apply), Error> {
    if operation.retiring.load(Ordering::Acquire) {
        return Err(Error::Busy);
    }
    if !matches!(
        operation.status(),
        Status::Acknowledged(_) | Status::Failed(_, _)
    ) || operation.terminal.load(Ordering::Acquire)
        || !operation.original(world)
    {
        return Err(Error::SourceGone);
    }
    if !world
        .get::<Pending>(operation.source_player)
        .is_some_and(|pending| Arc::ptr_eq(&pending.0, operation))
    {
        return Err(Error::NotOwned);
    }
    let ack = operation.progress.lock().ack.clone().ok_or(Error::Busy)?;
    let size =
        crate::server::game::player_state::death_capture::estimate(world, operation.source_player)
            .map_err(|_| Error::LimitExceeded)?;
    let lease = (operation.reserve)(size.checked_mul(3).ok_or(Error::LimitExceeded)?)?;
    let snapshot =
        Arc::new(capture(world, operation.source_player).map_err(|_| Error::InvalidState)?);
    let hold = quarantine::hold(
        world,
        operation.source_player,
        snapshot.clone(),
        quarantine::Role::Source,
        Arc::new((operation.clone(), lease)),
    )
    .map_err(|_| Error::Conflict)?;
    operation.retiring.store(true, Ordering::Release);
    world
        .entity_mut(operation.source_player)
        .insert(SourceBody(hold.clone()));
    Ok((
        Work::Transition {
            observed: ack.observed.clone(),
            phase: SourcePhase::Retired,
            snapshot: Some(snapshot),
        },
        Apply::RetireRebind {
            player: operation.source_player,
            hold,
            operation: operation.clone(),
        },
    ))
}
/// Retired raw values stay canonical until a separate real full-state handoff replaces them.
pub(super) fn retired(world: &mut World, operation: &Arc<Operation>, observed: &Arc<Observation>) {
    reject(world, operation);
    (operation.authority.reject)(world);
    let key = format!(
        "source:{}:{}:retire",
        operation.source.reference.nonce, observed.source.generation
    );
    if let Some(receipt) = observed.data.inventory_effects.get(&key) {
        operation.set(Status::Retired(receipt.clone(), true));
    } else {
        operation.set(Status::Uncertain);
    }
}
