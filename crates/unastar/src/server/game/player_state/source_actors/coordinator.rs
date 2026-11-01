//! Durable source transitions execute outside guest callbacks and borrowed worlds.

use super::*;
use crate::storage::player_writer::{
    EffectWriteFailure, SerializedPlayerProvider, WriteFailureKind,
};
use crate::storage::{PlayerProvider, source::SourceProof};
use parking_lot::Mutex;
use std::sync::{Arc, mpsc};

mod live;
pub(crate) mod rebind;
pub(crate) mod unowned_terminal;
pub(crate) use live::{LiveSource, pending, pending_account};

/// Definite rejection is distinct from a write whose durable result must be reconciled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    NotInstalled,
    Conflict,
    NotOwned,
    WrongWorld,
    SourceGone,
    Busy,
    InvalidState,
    LimitExceeded,
    RecoveryRequired,
}

/// Source disposition is an actual canonical observation, never inferred from an absent actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Frozen,
    Active,
    Retired,
    Transferred,
    Terminal,
}
impl From<SourcePhase> for Phase {
    /// Native state names remain independent of component wire numbering.
    fn from(phase: SourcePhase) -> Self {
        match phase {
            SourcePhase::Frozen => Self::Frozen,
            SourcePhase::Active => Self::Active,
            SourcePhase::Retired => Self::Retired,
            SourcePhase::Transferred => Self::Transferred,
            SourcePhase::Terminal => Self::Terminal,
        }
    }
}

/// Archive-derived correlation binds one real source without selecting an actor by account.
#[derive(Clone)]
pub(crate) struct Reference {
    pub module: String,
    pub uuid: uuid::Uuid,
    pub xuid: Option<String>,
    pub name: String,
    pub nonce: String,
    pub world: uuid::Uuid,
    pub dimension: i32,
}

/// An owned canonical row is charged while an operation, worker or actual body retains it.
pub(crate) struct Observation {
    pub data: Arc<PlayerData>,
    pub source: SourceActorRecord,
    _bytes: Arc<dyn Send + Sync>,
}
impl Observation {
    /// Account borrowed retained data before materializing detached component values.
    pub(crate) fn retained_bytes(&self) -> Result<usize, Error> {
        bytes(&self.data)
    }
    /// Current raw state uses current canonical slots and receipts, never an old archive payload.
    pub(crate) fn snapshot(&self) -> Result<PlayerSnapshot, SnapshotError> {
        let supplement = self
            .source
            .supplement
            .as_ref()
            .ok_or(SnapshotError::MissingResource)?;
        PlayerSnapshot::from_supplement((*self.data).clone(), supplement)
    }
}

#[derive(Clone)]
pub(crate) enum Status {
    Staged,
    Queued,
    Committed {
        phase: Phase,
        generation: u64,
        live_applied: bool,
        physical: Physical,
    },
    Rejected(Error),
    Uncertain,
    Cancelled,
}
struct Progress {
    status: Status,
    observation: Option<Arc<Observation>>,
    actor: Option<Entity>,
}

/// Actual absence in an observed world differs from unavailable live access.
#[derive(Clone, Copy)]
pub(crate) enum Physical {
    Unavailable,
    Absent,
    Held,
    Present,
}

/// A ticket owns immutable completion evidence through disconnect, unload and uncertain writes.
pub(crate) struct Operation {
    pub reference: Reference,
    pub world: bevy_ecs::world::WorldId,
    progress: Mutex<Progress>,
    _retention: Arc<dyn Send + Sync>,
    pub(super) reserve: Reserve,
    pub registry: Arc<crate::registry::ItemRegistry>,
    pub world_name: String,
    pub tick: u64,
    live: Option<Arc<live::State>>,
    initial_terminal: Option<Arc<PlayerSnapshot>>,
}
impl Operation {
    /// A departure that removes its body cannot commit a request requiring retention.
    pub(crate) fn live_for(&self, player: Entity) -> bool {
        self.live
            .as_ref()
            .is_some_and(|state| state.player == player)
    }
    /// Provisional history is not canonical source authority.
    pub(crate) fn provisional(&self) -> Option<Arc<PlayerSnapshot>> {
        (!matches!(self.status(), Status::Cancelled))
            .then(|| {
                self.live
                    .as_ref()
                    .map(|live| live.snapshot.clone())
                    .or_else(|| self.initial_terminal.clone())
            })
            .flatten()
    }
    /// Reading completion never requires a live or fabricated player context.
    pub(crate) fn status(&self) -> Status {
        self.progress.lock().status.clone()
    }
    /// Only a definitive canonical read or write supplies current owned state.
    pub(crate) fn observation(&self) -> Option<Arc<Observation>> {
        self.progress.lock().observation.clone()
    }
    /// Resolve only the genuinely created or observed actor of this exact world lifetime.
    pub(crate) fn player(&self, world: &World) -> Option<Entity> {
        if world.id() != self.world {
            return None;
        }
        let progress = self.progress.lock();
        let actor = progress.actor?;
        let observed = progress.observation.as_ref()?;
        let managed = world.get::<ManagedSource>(actor)?;
        (same_body(&observed.source, &managed.record)
            && world
                .get::<PlayerUuid>(actor)
                .is_some_and(|uuid| uuid.0 == self.reference.uuid)
            && world
                .get::<crate::entity::components::PlayerSession>(actor)
                .is_none()
            && world
                .get::<crate::server::game::player_events::DetachedPlayer>(actor)
                .is_some())
        .then_some(actor)
    }
    /// A trapped callback never starts queued physical or provider work.
    pub(crate) fn cancel(&self) {
        self.progress.lock().status = Status::Cancelled;
    }
    fn reject(&self, error: Error) {
        self.progress.lock().status = Status::Rejected(error);
    }
    /// Preserve definite durable evidence even when physical application is no longer possible.
    fn complete(
        &self,
        observation: Arc<Observation>,
        actor: Option<Entity>,
        live_applied: bool,
        physical: Physical,
    ) {
        let mut progress = self.progress.lock();
        progress.status = Status::Committed {
            phase: observation.source.phase.into(),
            generation: observation.source.generation,
            live_applied,
            physical,
        };
        progress.observation = Some(observation);
        progress.actor = actor;
    }
}

/// A proven handoff can still have its original withheld body awaiting physical cleanup.
fn same_body(observed: &SourceActorRecord, managed: &SourceActorRecord) -> bool {
    managed.matches(&SourceProof::from(observed))
        || (matches!(
            observed.phase,
            SourcePhase::Transferred | SourcePhase::Retired | SourcePhase::Terminal
        ) && (observed.phase != SourcePhase::Transferred || observed.handoff.is_some())
            && managed.module == observed.module
            && managed.nonce == observed.nonce
            && managed.generation.checked_add(1) == Some(observed.generation))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Recreate,
    Activate,
    Retire,
}
pub(crate) enum Intent {
    AdoptTerminal {
        player: Entity,
        snapshot: Arc<PlayerSnapshot>,
        witness: Arc<unowned_terminal::Witness>,
        reference: Reference,
    },
    RetireRebind {
        operation: Arc<rebind::Operation>,
    },
    Live {
        player: Entity,
        lineage: crate::plugin::manager::players::PlayerLineage,
        snapshot: Arc<PlayerSnapshot>,
        reference: Reference,
    },
    Seal {
        player: Entity,
        hold: quarantine::Handle,
        snapshot: Arc<PlayerSnapshot>,
        reference: Reference,
    },
    Inspect(Reference),
    Continue {
        observed: Arc<Operation>,
        action: Action,
    },
    Terminal {
        player: Entity,
        hold: quarantine::Handle,
        post: Arc<PlayerSnapshot>,
        proof: SourceProof,
        reference: Reference,
        death: Arc<TerminalDeath>,
    },
}
/// One deferred death preserves its actual native cause and original drop rules.
pub(crate) struct TerminalDeath {
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
}

pub(crate) struct Prepared {
    pub operation: Arc<Operation>,
    intent: Intent,
    reserve: Reserve,
}
pub(super) type Reserve = Arc<dyn Fn(usize) -> Result<Arc<dyn Send + Sync>, Error> + Send + Sync>;

/// Prepare only owned evidence; actual world/provider mutations wait for callback success.
pub(crate) fn prepare(
    world: &World,
    intent: Intent,
    retention: Arc<dyn Send + Sync>,
    reserve: Reserve,
) -> Result<Prepared, Error> {
    if !world.contains_resource::<Coordinator>() {
        return Err(Error::NotInstalled);
    }
    let reference = match &intent {
        Intent::RetireRebind { operation } => operation.source.reference.clone(),
        Intent::AdoptTerminal { reference, .. }
        | Intent::Live { reference, .. }
        | Intent::Seal { reference, .. }
        | Intent::Inspect(reference)
        | Intent::Terminal { reference, .. } => reference.clone(),
        Intent::Continue { observed, .. } => {
            if observed.world != world.id() || observed.observation().is_none() {
                return Err(Error::Conflict);
            }
            observed.reference.clone()
        }
    };
    let identity = world
        .get_resource::<crate::world::WorldIdentity>()
        .ok_or(Error::WrongWorld)?;
    if identity.id != reference.world || identity.dimension != reference.dimension {
        return Err(Error::WrongWorld);
    }
    let live = match &intent {
        Intent::Live {
            player,
            lineage,
            snapshot,
            ..
        } => Some(Arc::new(live::State::new(
            *player,
            lineage.clone(),
            snapshot.clone(),
        ))),
        _ => None,
    };
    let initial_terminal = match &intent {
        Intent::AdoptTerminal { snapshot, .. } => Some(snapshot.clone()),
        _ => None,
    };
    let operation = Arc::new(Operation {
        reference,
        live,
        initial_terminal,
        world: world.id(),
        progress: Mutex::new(Progress {
            status: Status::Staged,
            observation: None,
            actor: None,
        }),
        _retention: retention,
        reserve: reserve.clone(),
        registry: world
            .get_resource::<super::super::super::types::ItemRegistryResource>()
            .ok_or(Error::NotInstalled)?
            .0
            .clone(),
        world_name: identity.name.clone(),
        tick: world
            .get_resource::<TickCounter>()
            .ok_or(Error::NotInstalled)?
            .current,
    });
    Ok(Prepared {
        operation,
        intent,
        reserve,
    })
}

#[derive(Resource)]
struct Coordinator {
    writer: Arc<SerializedPlayerProvider>,
    tx: mpsc::Sender<Completion>,
    rx: Mutex<mpsc::Receiver<Completion>>,
    deferred: Mutex<Vec<Completion>>,
}
struct Completion {
    operation: Arc<Operation>,
    action: Apply,
    result: Result<Arc<Observation>, Failure>,
    remaining: u16,
}
#[derive(Clone)]
struct Failure {
    error: Error,
    uncertain: bool,
}
enum Apply {
    AdoptTerminal {
        player: Entity,
        hold: quarantine::Handle,
    },
    RetireRebind {
        player: Entity,
        hold: quarantine::Handle,
        operation: Arc<rebind::Operation>,
    },
    Live,
    Inspect,
    Seal {
        player: Entity,
        hold: quarantine::Handle,
    },
    Recreate,
    Activate {
        player: Entity,
        hold: quarantine::Handle,
    },
    Retire {
        player: Entity,
        hold: quarantine::Handle,
        provisional: bool,
    },
    RetireAbsent,
    Terminal {
        player: Entity,
        hold: quarantine::Handle,
        death: Arc<TerminalDeath>,
    },
}
impl Apply {
    fn read_only(&self) -> bool {
        matches!(self, Self::Inspect)
    }
}

/// Install over the same provider/serialization boundary as inventory and full-state writes.
pub(crate) fn install(
    server: &mut super::super::super::GameServer,
) -> Result<(), super::super::super::inventory_effects::EffectError> {
    let world = server.ecs.world_mut();
    if world.contains_resource::<Coordinator>() {
        rebind::install(server)?;
        unowned_terminal::install(server);
        return Ok(());
    }
    let writer = super::super::super::inventory_effects::prepare_shared_writer(world)?;
    let (tx, rx) = mpsc::channel();
    world.insert_resource(Coordinator {
        writer,
        tx,
        rx: Mutex::new(rx),
        deferred: Mutex::new(Vec::new()),
    });
    server
        .ecs
        .schedule_mut()
        .add_systems(poll.before(crate::ecs::PacketApplySet));
    server
        .ecs
        .schedule_mut()
        .add_systems(super::residency::update.before(crate::ecs::PhysicsSet));
    server
        .ecs
        .schedule_mut()
        .add_systems(super::residency::motion.in_set(crate::ecs::PhysicsSet));
    rebind::install(server)?;
    unowned_terminal::install(server);
    Ok(())
}

/// Requests enter the router only after every guest borrow and manager guard has been released.
pub(crate) fn submit(world: &mut World, prepared: Prepared) {
    let operation = prepared.operation.clone();
    let adoption_player = match &prepared.intent {
        Intent::AdoptTerminal { player, .. } => Some(*player),
        _ => None,
    };
    if let Intent::AdoptTerminal {
        player, witness, ..
    } = &prepared.intent
        && let Err(error) = unowned_terminal::accept_adoption(world, &operation, *player, witness)
    {
        operation.reject(error);
        return;
    }
    if operation.live.is_some()
        && let Err(error) = live::accept(world, &operation)
    {
        operation.reject(error);
        return;
    }
    if !crate::server::worlds::queue_router_request(world, move |router, _| run(router, prepared)) {
        live::reject_pending(world, &operation);
        if let Some(player) = adoption_player {
            unowned_terminal::reject_adoption(world, player, &operation);
        }
        operation.reject(Error::Busy);
    }
}

/// Compare all actual native actors; no directory/session-only approximation authorizes creation.
fn duplicate(
    router: &mut crate::server::worlds::DimensionRouter,
    reference: &Reference,
    allowed: Option<(bevy_ecs::world::WorldId, Entity)>,
    allow_blocked_admission: bool,
) -> bool {
    for (_, server) in router.iter_mut() {
        let world = server.ecs.world_mut();
        let mut query = world.query_filtered::<(Entity, &PlayerUuid), With<Player>>();
        for (actor, uuid) in query.iter(world) {
            let same = uuid.0 == reference.uuid
                || reference
                    .xuid
                    .as_deref()
                    .is_some_and(|xuid| account_xuid(world, actor) == Some(xuid));
            if same
                && allowed != Some((world.id(), actor))
                && !(allow_blocked_admission
                    && (admission_blocked(world, actor)
                        || (crate::server::game::player_events::initial_state_pending(
                            world, actor,
                        ) && crate::plugin::manager::sessions::blocks_publication(
                            world, actor,
                        ))))
            {
                return true;
            }
        }
    }
    false
}

/// Owns the worker's interruption/closed-world disposition even when all guest tickets disappear.
struct Reply {
    operation: Arc<Operation>,
    writer: Arc<SerializedPlayerProvider>,
    tx: mpsc::Sender<Completion>,
    action: Option<Apply>,
}
impl Reply {
    fn finish(mut self, result: Result<Arc<Observation>, Failure>) {
        let action = self.action.take().expect("source worker ownership");
        let changed = !action.read_only();
        if self
            .tx
            .send(Completion {
                operation: self.operation.clone(),
                action,
                result,
                remaining: 200,
            })
            .is_err()
        {
            if changed {
                self.writer.require_recovery(self.operation.reference.uuid);
                self.operation.progress.lock().status = Status::Uncertain;
            } else {
                self.operation.reject(Error::SourceGone);
            }
        }
    }
}
impl Drop for Reply {
    fn drop(&mut self) {
        if let Some(action) = self.action.take() {
            if matches!(action, Apply::Live)
                && let Some(live) = &self.operation.live
            {
                let mut result = live.result.lock();
                if result.is_none() {
                    *result = Some(Err(Failure {
                        error: Error::RecoveryRequired,
                        uncertain: true,
                    }));
                    live.ready.notify_waiters();
                }
            }
            if !action.read_only() {
                self.writer.require_recovery(self.operation.reference.uuid);
                self.operation.progress.lock().status = Status::Uncertain;
            } else {
                self.operation.reject(Error::SourceGone);
            }
        }
    }
}

/// The native body is sessionless and still owns exactly the observed canonical generation.
fn body(world: &World, observed: &Operation) -> Result<(Entity, Arc<Observation>), Error> {
    let observation = observed.observation().ok_or(Error::Conflict)?;
    let player = observed.player(world).ok_or(Error::SourceGone)?;
    if crate::server::game::player_state::source_actors::pending(world, player) {
        return Err(Error::Busy);
    }
    Ok((player, observation))
}

/// All synchronous validation precedes the first account worker or physical application.
fn run(router: &mut crate::server::worlds::DimensionRouter, prepared: Prepared) {
    let operation = prepared.operation.clone();
    if operation.live.is_some() {
        live::run(router, prepared);
        return;
    }
    let dimension = operation.reference.dimension;
    let allowed = match &prepared.intent {
        Intent::RetireRebind { operation } => {
            Some((operation.source.world, operation.source_player))
        }
        Intent::AdoptTerminal { player, .. }
        | Intent::Seal { player, .. }
        | Intent::Terminal { player, .. } => Some((operation.world, *player)),
        Intent::Continue { observed, action } if *action != Action::Recreate => router
            .world(dimension)
            .and_then(|server| observed.player(server.ecs.world()))
            .map(|actor| (operation.world, actor)),
        _ => None,
    };
    let closed_retirement = matches!(&prepared.intent, Intent::Continue { observed, action: Action::Retire } if observed.observation().is_some_and(|observation| !observation.source.phase.unresolved()));
    if !matches!(prepared.intent, Intent::Inspect(_))
        && !closed_retirement
        && duplicate(
            router,
            &operation.reference,
            allowed,
            matches!(
                prepared.intent,
                Intent::Terminal { .. } | Intent::Seal { .. } | Intent::RetireRebind { .. }
            ),
        )
    {
        operation.reject(Error::Conflict);
        return;
    }
    let Some(server) = router.world_mut(dimension) else {
        operation.reject(Error::WrongWorld);
        return;
    };
    let world = server.ecs.world_mut();
    if world.id() != operation.world {
        operation.reject(Error::WrongWorld);
        return;
    }
    let Some(coordinator) = world.get_resource::<Coordinator>() else {
        operation.reject(Error::NotInstalled);
        return;
    };
    let writer = coordinator.writer.clone();
    let tx = coordinator.tx.clone();
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        operation.reject(Error::NotInstalled);
        return;
    };
    let (work, apply) = match prepare_work(world, &prepared.intent) {
        Ok(work) => work,
        Err(error) => {
            operation.reject(error);
            return;
        }
    };
    operation.progress.lock().status = Status::Queued;
    let reply = Reply {
        operation: operation.clone(),
        writer: writer.clone(),
        tx,
        action: Some(apply),
    };
    runtime.spawn(async move {
        let result = work
            .execute(&writer, &operation.reference, prepared.reserve)
            .await;
        reply.finish(result);
    });
}

enum Work {
    AdoptTerminal {
        snapshot: Arc<PlayerSnapshot>,
        witness: crate::storage::terminal::NativeTerminalRecord,
    },
    Live(Arc<PlayerSnapshot>, Option<SourceProof>),
    Inspect,
    Seal(Arc<PlayerSnapshot>, Option<SourceProof>),
    Transition {
        observed: Arc<Observation>,
        phase: SourcePhase,
        snapshot: Option<Arc<PlayerSnapshot>>,
    },
    RetireConsumed(Arc<Observation>),
    Terminal {
        snapshot: Arc<PlayerSnapshot>,
        proof: SourceProof,
    },
    TerminalAfterHandoff {
        snapshot: Arc<PlayerSnapshot>,
        proof: SourceProof,
        handoff: crate::storage::source::SourceHandoff,
    },
}

/// Active retirement freezes the genuine current state before the durable write starts.
fn prepare_work(world: &mut World, intent: &Intent) -> Result<(Work, Apply), Error> {
    match intent {
        Intent::AdoptTerminal {
            player, witness, ..
        } => {
            let actual = unowned_terminal::witness(world, *player)?;
            if actual.proof != witness.proof
                || actual.lineage.key() != witness.lineage.key()
                || !unowned_terminal::is_adopting(world, *player)
            {
                return Err(Error::SourceGone);
            }
            let snapshot = Arc::new(capture(world, *player).map_err(|_| Error::InvalidState)?);
            let hold = quarantine::hold(
                world,
                *player,
                snapshot.clone(),
                quarantine::Role::Source,
                witness.clone(),
            )
            .map_err(|_| Error::Conflict)?;
            world.entity_mut(*player).insert(SourceBody(hold.clone()));
            Ok((
                Work::AdoptTerminal {
                    snapshot,
                    witness: actual.proof,
                },
                Apply::AdoptTerminal {
                    player: *player,
                    hold,
                },
            ))
        }
        Intent::RetireRebind { operation } => rebind::prepare_retirement(world, operation),
        Intent::Live { .. } => Err(Error::InvalidState),
        Intent::Terminal {
            player,
            hold,
            post,
            proof,
            death,
            ..
        } => {
            if !quarantine::matches(world, hold)
                || world
                    .get::<ManagedSource>(*player)
                    .is_none_or(|managed| !managed.record.matches(proof))
            {
                return Err(Error::SourceGone);
            }
            Ok((
                Work::Terminal {
                    snapshot: post.clone(),
                    proof: proof.clone(),
                },
                Apply::Terminal {
                    player: *player,
                    hold: hold.clone(),
                    death: death.clone(),
                },
            ))
        }
        Intent::Inspect(_) => Ok((Work::Inspect, Apply::Inspect)),
        Intent::Seal {
            player,
            hold,
            snapshot,
            ..
        } => {
            if world.get::<PlayerSession>(*player).is_some()
                || !quarantine::matches(world, hold)
                || hold.player != *player
            {
                return Err(Error::SourceGone);
            }
            Ok((
                Work::Seal(
                    snapshot.clone(),
                    world
                        .get::<ManagedSource>(*player)
                        .map(ManagedSource::proof),
                ),
                Apply::Seal {
                    player: *player,
                    hold: hold.clone(),
                },
            ))
        }
        Intent::Continue {
            observed,
            action: Action::Recreate,
        } => {
            let data = observed.observation().ok_or(Error::Conflict)?;
            if !data.source.phase.unresolved() {
                return Err(Error::NotOwned);
            }
            Ok((
                Work::Transition {
                    observed: data,
                    phase: SourcePhase::Frozen,
                    snapshot: None,
                },
                Apply::Recreate,
            ))
        }
        Intent::Continue {
            observed,
            action: Action::Activate,
        } => {
            let (player, data) = body(world, observed)?;
            if data.source.phase != SourcePhase::Frozen {
                return Err(Error::Conflict);
            }
            let hold = world
                .get::<SourceBody>(player)
                .ok_or(Error::SourceGone)?
                .0
                .clone();
            if !quarantine::matches(world, &hold) {
                return Err(Error::SourceGone);
            }
            Ok((
                Work::Transition {
                    observed: data,
                    phase: SourcePhase::Active,
                    snapshot: None,
                },
                Apply::Activate { player, hold },
            ))
        }
        Intent::Continue {
            observed,
            action: Action::Retire,
        } => {
            let data = observed.observation().ok_or(Error::Conflict)?;
            let Some(player) = observed.player(world) else {
                return Ok((
                    if data.source.phase.unresolved() {
                        Work::Transition {
                            observed: data,
                            phase: SourcePhase::Retired,
                            snapshot: None,
                        }
                    } else {
                        Work::RetireConsumed(data)
                    },
                    Apply::RetireAbsent,
                ));
            };
            if world.get::<TerminalPending>(player).is_some()
                || crate::server::game::player_state::source_actors::pending(world, player)
            {
                return Err(Error::Busy);
            }
            if !data.source.phase.unresolved() {
                if data.source.handoff.is_none()
                    && !matches!(
                        data.source.phase,
                        SourcePhase::Terminal | SourcePhase::Retired
                    )
                {
                    return Err(Error::NotOwned);
                }
                let hold = if let Some(held) = world.get::<SourceBody>(player) {
                    held.0.clone()
                } else {
                    let raw = Arc::new(capture(world, player).map_err(|_| Error::InvalidState)?);
                    let hold = quarantine::hold(
                        world,
                        player,
                        raw,
                        quarantine::Role::Source,
                        observed.clone(),
                    )
                    .map_err(|_| Error::Conflict)?;
                    world.entity_mut(player).insert(SourceBody(hold.clone()));
                    hold
                };
                if !quarantine::matches(world, &hold) {
                    return Err(Error::SourceGone);
                }
                return Ok((
                    Work::RetireConsumed(data),
                    Apply::Retire {
                        player,
                        hold,
                        provisional: false,
                    },
                ));
            }
            if let Some(held) = world.get::<SourceBody>(player) {
                if !quarantine::matches(world, &held.0) {
                    return Err(Error::SourceGone);
                }
                let snapshot =
                    quarantine::source_snapshot(world, &held.0).ok_or(Error::SourceGone)?;
                return Ok((
                    Work::Transition {
                        observed: data,
                        phase: SourcePhase::Retired,
                        snapshot: Some(snapshot),
                    },
                    Apply::Retire {
                        player,
                        hold: held.0.clone(),
                        provisional: false,
                    },
                ));
            }
            let snapshot = Arc::new(capture(world, player).map_err(|_| Error::InvalidState)?);
            let hold = quarantine::hold(
                world,
                player,
                snapshot.clone(),
                quarantine::Role::Source,
                observed.clone(),
            )
            .map_err(|_| Error::Conflict)?;
            world.entity_mut(player).insert(SourceBody(hold.clone()));
            Ok((
                Work::Transition {
                    observed: data,
                    phase: SourcePhase::Retired,
                    snapshot: Some(snapshot),
                },
                Apply::Retire {
                    player,
                    hold,
                    provisional: true,
                },
            ))
        }
    }
}

/// Charge provisional history before any additional retained serialization copies.
pub(crate) fn snapshot_bytes(snapshot: &PlayerSnapshot) -> Result<usize, Error> {
    bytes(&snapshot.data)
}

/// Borrowed sizing precedes retained copies of provider-owned state.
fn bytes(data: &PlayerData) -> Result<usize, Error> {
    let mut size = 32768usize;
    for item in data
        .inventory
        .iter()
        .chain(&data.armour)
        .chain(std::iter::once(&data.offhand))
        .chain(&data.ui)
        .chain(std::iter::once(&data.cursor))
        .chain(&data.ender_chest)
    {
        size = size
            .checked_add(item.item_id.len())
            .and_then(|size| size.checked_add(item.nbt.as_ref().map_or(0, Vec::len)))
            .and_then(|size| size.checked_add(128))
            .filter(|size| *size <= 16 * 1024 * 1024)
            .ok_or(Error::LimitExceeded)?;
    }
    for key in data.inventory_effects.keys() {
        size = size
            .checked_add(key.len())
            .and_then(|size| size.checked_add(256))
            .filter(|size| *size <= 16 * 1024 * 1024)
            .ok_or(Error::LimitExceeded)?;
    }
    Ok(size)
}

/// Encoding cannot grow an unbounded transient buffer from native item metadata.
fn bounded_encode(value: &impl serde::Serialize) -> Result<Vec<u8>, Error> {
    struct Output(Vec<u8>);
    impl std::io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > (16 * 1024 * 1024usize).saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("source record limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Output(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(|_| Error::LimitExceeded)?;
    Ok(output.0)
}

/// Canonical source reads require the exact archive/module/world correlation and account binding.
fn validate_reference(
    data: &PlayerData,
    reference: &Reference,
) -> Result<SourceActorRecord, Error> {
    let source = data.source_actor.as_ref().ok_or(Error::NotOwned)?;
    if source.module != reference.module
        || source.nonce != reference.nonce
        || source.world != reference.world.to_string()
        || source.dimension != reference.dimension
        || source.name != reference.name
    {
        return Err(Error::Conflict);
    }
    if source.phase.unresolved()
        && source
            .supplement
            .as_ref()
            .and_then(|supplement| supplement.xuid.as_deref())
            != reference.xuid.as_deref()
    {
        return Err(Error::Conflict);
    }
    Ok(source.clone())
}

impl Work {
    /// An exact canonical generation and receipt epoch fence every source transition.
    async fn execute(
        self,
        writer: &SerializedPlayerProvider,
        reference: &Reference,
        reserve: Reserve,
    ) -> Result<Arc<Observation>, Failure> {
        let fail = |error| Failure {
            error,
            uncertain: false,
        };
        let current = writer.load(reference.uuid).await.map_err(|_| Failure {
            error: Error::RecoveryRequired,
            uncertain: true,
        })?;
        if let Self::AdoptTerminal { snapshot, witness } = self {
            let actual = current.as_ref().ok_or_else(|| fail(Error::NotOwned))?;
            if actual.native_terminal.as_ref() != Some(&witness)
                || witness.validate(actual).is_err()
                || witness.world != reference.world.to_string()
                || witness.name != reference.name
                || witness.xuid != reference.xuid
            {
                return Err(fail(Error::Conflict));
            }
            let generation = actual
                .source_actor
                .as_ref()
                .map_or(Some(1), |source| source.generation.checked_add(1))
                .ok_or_else(|| fail(Error::Conflict))?;
            let lease = reserve(
                snapshot_bytes(&snapshot)
                    .map_err(fail)?
                    .checked_mul(6)
                    .ok_or_else(|| fail(Error::LimitExceeded))?,
            )
            .map_err(fail)?;
            let mut candidate = snapshot.data.clone();
            candidate.game_mode = snapshot.base_game_mode.unwrap_or(candidate.game_mode);
            candidate.native_terminal = None;
            let source = SourceActorRecord {
                version: 1,
                module: reference.module.clone(),
                nonce: reference.nonce.clone(),
                generation,
                sample: 0,
                name: reference.name.clone(),
                world: reference.world.to_string(),
                dimension: reference.dimension,
                phase: SourcePhase::Terminal,
                supplement: Some(snapshot.supplement()),
                handoff: None,
            };
            use sha2::Digest;
            let hash = sha2::Sha256::digest(
                bounded_encode(&(&candidate, &source, &witness)).map_err(fail)?,
            )
            .into();
            let id = format!("source:{}:{}:initial-terminal", reference.nonce, generation);
            let written = writer
                .commit_initial_terminal(
                    reference.uuid,
                    &id,
                    hash,
                    candidate,
                    witness,
                    source.clone(),
                )
                .await
                .map_err(write_error)?;
            if written.data.inventory_effects.get(&id) != Some(&written.effect.record)
                || written.data.source_actor.as_ref() != Some(&source)
            {
                return Err(Failure {
                    error: Error::RecoveryRequired,
                    uncertain: true,
                });
            }
            return Ok(Arc::new(Observation {
                data: Arc::new(written.data),
                source,
                _bytes: lease,
            }));
        }
        if matches!(self, Self::Inspect) {
            let data = current.ok_or_else(|| fail(Error::NotOwned))?;
            let source = validate_reference(&data, reference).map_err(fail)?;
            let lease = reserve(bytes(&data).map_err(fail)?).map_err(fail)?;
            return Ok(Arc::new(Observation {
                data: Arc::new(data),
                source,
                _bytes: lease,
            }));
        }
        if let Self::RetireConsumed(ref observed) = self {
            let data = current.ok_or_else(|| fail(Error::NotOwned))?;
            let source = validate_reference(&data, reference).map_err(fail)?;
            if !source.matches(&SourceProof::from(&observed.source)) || source.phase.unresolved() {
                return Err(fail(Error::Conflict));
            }
            let lease = reserve(bytes(&data).map_err(fail)?).map_err(fail)?;
            return Ok(Arc::new(Observation {
                data: Arc::new(data),
                source,
                _bytes: lease,
            }));
        }
        let generation = current
            .as_ref()
            .and_then(|data| data.source_actor.as_ref())
            .map_or(Some(1), |source| source.generation.checked_add(1))
            .ok_or_else(|| fail(Error::Conflict))?;
        let size = match &self {
            Self::Seal(snapshot, _) | Self::Live(snapshot, _) => bytes(&snapshot.data),
            Self::Terminal { snapshot, .. } | Self::TerminalAfterHandoff { snapshot, .. } => {
                bytes(&snapshot.data)
            }
            Self::Transition {
                snapshot: Some(snapshot),
                ..
            } => bytes(&snapshot.data),
            _ => current.as_ref().ok_or(Error::NotOwned).and_then(bytes),
        }
        .map_err(fail)?;
        let lease = reserve(
            size.checked_mul(6)
                .ok_or_else(|| fail(Error::LimitExceeded))?,
        )
        .map_err(fail)?;
        let terminal_handoff = match &self {
            Self::TerminalAfterHandoff { handoff, .. } => Some(handoff.clone()),
            _ => None,
        };
        let (mut candidate, mut source, expected, label) = match self {
            Self::Live(snapshot, proof) => {
                let mut data = snapshot.data.clone();
                data.game_mode = snapshot.base_game_mode.unwrap_or(data.game_mode);
                let source = if let Some(proof) = &proof {
                    let actual = current.as_ref().ok_or_else(|| fail(Error::NotOwned))?;
                    let mut previous = validate_reference(actual, reference).map_err(fail)?;
                    if !previous.matches(proof)
                        || previous.phase != SourcePhase::Active
                        || snapshot.data.inventory_revision != actual.inventory_revision
                    {
                        return Err(fail(Error::Conflict));
                    }
                    data.inventory_effects = actual.inventory_effects.clone();
                    previous.generation = generation;
                    previous.sample = 0;
                    previous.supplement = Some(snapshot.supplement());
                    previous
                } else {
                    if current
                        .as_ref()
                        .and_then(|data| data.source_actor.as_ref())
                        .is_some_and(SourceActorRecord::requires_handoff)
                    {
                        return Err(fail(Error::Busy));
                    }
                    SourceActorRecord {
                        version: 1,
                        module: reference.module.clone(),
                        nonce: reference.nonce.clone(),
                        generation,
                        sample: 0,
                        name: reference.name.clone(),
                        world: reference.world.to_string(),
                        dimension: reference.dimension,
                        phase: SourcePhase::Active,
                        supplement: Some(snapshot.supplement()),
                        handoff: None,
                    }
                };
                (data, source, proof, "checkpoint")
            }
            Self::Seal(snapshot, proof) => {
                if let Some(proof) = &proof {
                    let actual = current.as_ref().ok_or_else(|| fail(Error::NotOwned))?;
                    let previous = validate_reference(actual, reference).map_err(fail)?;
                    if !previous.matches(proof) || !previous.phase.unresolved() {
                        return Err(fail(Error::Conflict));
                    }
                    if previous.phase == SourcePhase::Frozen {
                        let physical = PlayerSnapshot::from_supplement(
                            actual.clone(),
                            previous
                                .supplement
                                .as_ref()
                                .ok_or_else(|| fail(Error::InvalidState))?,
                        )
                        .map_err(|_| fail(Error::InvalidState))?;
                        if !physical.same_physical_state(&snapshot) {
                            return Err(fail(Error::Conflict));
                        }
                        return Ok(Arc::new(Observation {
                            data: Arc::new(actual.clone()),
                            source: previous,
                            _bytes: lease,
                        }));
                    }
                    if snapshot.data.inventory_revision != actual.inventory_revision {
                        return Err(fail(Error::Conflict));
                    }
                    let mut data = snapshot.data.clone();
                    data.game_mode = snapshot.base_game_mode.unwrap_or(data.game_mode);
                    data.inventory_effects = actual.inventory_effects.clone();
                    let mut record = previous;
                    record.generation = generation;
                    record.sample = 0;
                    record.phase = SourcePhase::Frozen;
                    record.supplement = Some(snapshot.supplement());
                    (data, record, Some(proof.clone()), "freeze")
                } else {
                    if current
                        .as_ref()
                        .and_then(|data| data.source_actor.as_ref())
                        .is_some_and(|source| source.phase.unresolved())
                    {
                        return Err(fail(Error::Busy));
                    }
                    let mut data = snapshot.data.clone();
                    data.game_mode = snapshot.base_game_mode.unwrap_or(data.game_mode);
                    let record = SourceActorRecord {
                        version: 1,
                        module: reference.module.clone(),
                        nonce: reference.nonce.clone(),
                        generation,
                        sample: 0,
                        name: reference.name.clone(),
                        world: reference.world.to_string(),
                        dimension: reference.dimension,
                        phase: SourcePhase::Frozen,
                        supplement: Some(snapshot.supplement()),
                        handoff: None,
                    };
                    (data, record, None, "seal")
                }
            }
            Self::Transition {
                observed,
                phase,
                snapshot,
            } => {
                let actual = current.as_ref().ok_or_else(|| fail(Error::NotOwned))?;
                let actual_source = validate_reference(actual, reference).map_err(fail)?;
                if !actual_source.matches(&SourceProof::from(&observed.source)) {
                    return Err(fail(Error::Conflict));
                }
                let mut source = actual_source;
                source.generation = generation;
                source.sample = 0;
                source.phase = phase;
                if phase == SourcePhase::Retired {
                    // This new recovery sample needs its own future target handoff.
                    source.handoff = None;
                }
                let data = if let Some(snapshot) = snapshot {
                    source.supplement = Some(snapshot.supplement());
                    let mut data = snapshot.data.clone();
                    data.game_mode = snapshot.base_game_mode.unwrap_or(data.game_mode);
                    data.inventory_revision = actual.inventory_revision;
                    data.inventory_effects = actual.inventory_effects.clone();
                    data
                } else {
                    actual.clone()
                };
                let label = match phase {
                    SourcePhase::Frozen => "recreate",
                    SourcePhase::Active => "activate",
                    SourcePhase::Retired => "retire",
                    SourcePhase::Terminal => "terminal",
                    SourcePhase::Transferred => return Err(fail(Error::Conflict)),
                };
                (
                    data,
                    source,
                    Some(SourceProof::from(&observed.source)),
                    label,
                )
            }
            Self::Terminal { snapshot, proof }
            | Self::TerminalAfterHandoff {
                snapshot, proof, ..
            } => {
                let actual = current.as_ref().ok_or_else(|| fail(Error::NotOwned))?;
                let mut source = validate_reference(actual, reference).map_err(fail)?;
                if !source.matches(&proof)
                    || !(source.phase == SourcePhase::Active
                        || (source.phase == SourcePhase::Transferred
                            && terminal_handoff
                                .as_ref()
                                .is_some_and(|handoff| source.handoff.as_ref() == Some(handoff))))
                    || snapshot.data.inventory_revision != actual.inventory_revision
                {
                    return Err(fail(Error::Conflict));
                }
                source.generation = generation;
                source.sample = 0;
                source.phase = SourcePhase::Terminal;
                source.handoff = None;
                source.supplement = Some(snapshot.supplement());
                let mut data = snapshot.data.clone();
                data.game_mode = snapshot.base_game_mode.unwrap_or(data.game_mode);
                data.inventory_effects = actual.inventory_effects.clone();
                (data, source, Some(proof), "terminal")
            }
            Self::RetireConsumed(_) | Self::Inspect | Self::AdoptTerminal { .. } => {
                return Err(fail(Error::Conflict));
            }
        };
        source.generation = generation;
        source
            .validate(&candidate)
            .map_err(|_| fail(Error::InvalidState))?;
        candidate.source_actor = None;
        candidate.restoration_checkpoint = None;
        let key = format!("source:{}:{generation}:{label}", reference.nonce);
        use sha2::Digest;
        let encoded = bounded_encode(&(&candidate, &source)).map_err(fail)?;
        let hash = sha2::Sha256::digest(&encoded).into();
        let written = if let Some(handoff) = terminal_handoff {
            writer
                .commit_handoff_terminal(
                    reference.uuid,
                    &key,
                    hash,
                    candidate,
                    expected.ok_or_else(|| fail(Error::Conflict))?,
                    source,
                    handoff,
                )
                .await
        } else {
            writer
                .commit_source(reference.uuid, &key, hash, candidate, expected, source)
                .await
        }
        .map_err(write_error)?;
        if written.data.inventory_effects.get(&key) != Some(&written.effect.record) {
            writer.require_recovery(reference.uuid);
            return Err(Failure {
                error: Error::RecoveryRequired,
                uncertain: true,
            });
        }
        let source = written
            .data
            .source_actor
            .as_ref()
            .ok_or_else(|| fail(Error::RecoveryRequired))?
            .clone();
        Ok(Arc::new(Observation {
            data: Arc::new(written.data),
            source,
            _bytes: lease,
        }))
    }
}

/// A provider failure after attempted installation never resumes a source or reports consumption.
fn write_error(error: EffectWriteFailure) -> Failure {
    Failure {
        error: if error.kind == WriteFailureKind::RecoveryRequired {
            Error::RecoveryRequired
        } else {
            Error::Conflict
        },
        uncertain: error.kind == WriteFailureKind::RecoveryRequired || !error.resume_source,
    }
}

/// Project the ordinary native slot disposition without publishing any item or experience value.
fn post_drop(
    world: &World,
    player: Entity,
    pre: &PlayerSnapshot,
    keep_inventory: bool,
) -> Result<PlayerSnapshot, Error> {
    let mut post = pre.clone();
    if !keep_inventory {
        let main = world
            .get::<MainInventory>(player)
            .ok_or(Error::InvalidState)?;
        let armour = world
            .get::<ArmourInventory>(player)
            .ok_or(Error::InvalidState)?;
        let offhand = world
            .get::<OffhandSlot>(player)
            .ok_or(Error::InvalidState)?;
        let ui = world.get::<PlayerUi>(player).ok_or(Error::InvalidState)?;
        for (stored, actual) in post
            .data
            .inventory
            .iter_mut()
            .zip(main.0.slots())
            .chain(post.data.armour.iter_mut().zip(armour.slots()))
            .chain(std::iter::once((&mut post.data.offhand, &offhand.0)))
            .chain(post.ui.iter_mut().zip(ui.0.slots()))
        {
            if !actual.is_empty() && !crate::entity::player_life::death::keeps_on_death(actual) {
                *stored = StoredItemStack::default();
            }
        }
        post.data.ui = post.ui.clone();
        post.data.cursor = post.ui.first().cloned().ok_or(Error::InvalidState)?;
        post.data.experience = 0;
        post.data.experience_progress = 0.0;
    }
    post.validate().map_err(|_| Error::InvalidState)?;
    Ok(post)
}

/// A managed source cannot publish terminal values before the canonical post-drop tombstone ACK.
pub(crate) fn defer_death(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
) -> bool {
    let rules = world.resource::<crate::world::rules::GameRules>().clone();
    defer_death_with_rules(world, player, cause, attribution, rules)
}

/// Inventory continuations retain the actual original death rules across provider work.
pub(crate) fn defer_death_with_rules(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
) -> bool {
    if rebind::defer_death(world, player, cause, attribution, rules.clone()) {
        return true;
    }
    if live::defer_death(world, player, cause, attribution, rules.clone()) {
        return true;
    }
    let Some(managed) = world.get::<ManagedSource>(player) else {
        return unowned_terminal::ordinary(world, player, cause, attribution, rules);
    };
    if managed.record.phase != SourcePhase::Active {
        return false;
    }
    let proof = managed.proof();
    let reserve = managed.reserve.clone();
    let retention = managed.retention.clone();
    let source = managed.record.clone();
    let result = (|| {
        let reserve = reserve.ok_or(Error::RecoveryRequired)?;
        let size = super::super::death_capture::estimate(world, player)
            .map_err(|_| Error::LimitExceeded)?;
        let lease = reserve(size.checked_mul(3).ok_or(Error::LimitExceeded)?)?;
        let pre = Arc::new(capture(world, player).map_err(|_| Error::InvalidState)?);
        let post = Arc::new(post_drop(world, player, &pre, rules.keep_inventory)?);
        let reference = Reference {
            module: source.module.clone(),
            uuid: uuid::Uuid::parse_str(&pre.data.uuid).map_err(|_| Error::InvalidState)?,
            xuid: pre.xuid.clone(),
            name: source.name.clone(),
            nonce: source.nonce.clone(),
            world: uuid::Uuid::parse_str(&source.world).map_err(|_| Error::WrongWorld)?,
            dimension: source.dimension,
        };
        let combined = Arc::new((lease, retention));
        let hold = quarantine::hold(
            world,
            player,
            pre,
            quarantine::Role::Source,
            combined.clone(),
        )
        .map_err(|_| Error::Conflict)?;
        world
            .entity_mut(player)
            .insert((SourceBody(hold.clone()), TerminalPending));
        let prepared = prepare(
            world,
            Intent::Terminal {
                player,
                hold,
                post,
                proof,
                reference,
                death: Arc::new(TerminalDeath {
                    cause,
                    attribution,
                    rules,
                }),
            },
            combined,
            reserve,
        )?;
        submit(world, prepared);
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        if let Some(coordinator) = world.get_resource::<Coordinator>()
            && let Some(uuid) = world.get::<PlayerUuid>(player)
        {
            coordinator.writer.require_recovery(uuid.0);
        }
        tracing::error!(?error, "Native source terminal processing remains withheld");
    }
    true
}

/// An acknowledged inventory continuation uses one native death barrier for actual online or retained bodies.
#[allow(clippy::too_many_arguments)]
pub(crate) fn defer_death_after_inventory(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
    lineage: &crate::plugin::manager::players::PlayerLineage,
    id: &str,
    receipt: &crate::storage::InventoryEffectRecord,
) -> bool {
    if !lineage.matches(world, player)
        || !world
            .get::<crate::server::game::inventory_effects::InventoryPersistence>(player)
            .is_some_and(|state| state.receipts().get(id) == Some(receipt))
    {
        return false;
    }
    if defer_death_with_rules(world, player, cause, attribution, rules.clone()) {
        return true;
    }
    unowned_terminal::after_inventory(
        world,
        player,
        cause,
        attribution,
        rules,
        lineage,
        id,
        receipt,
    )
}

/// Definitive non-application releases no incoming value, but its actual fatal hit still settles.
#[allow(clippy::too_many_arguments)]
pub(crate) fn defer_death_after_inventory_rejection(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
    lineage: &crate::plugin::manager::players::PlayerLineage,
    epoch: crate::server::game::inventory_effects::InventoryEpoch,
    failure: &crate::storage::player_writer::EffectWriteFailure,
) -> bool {
    if failure.kind != crate::storage::player_writer::WriteFailureKind::NotApplied
        || !failure.resume_source
        || !lineage.matches(world, player)
        || !world
            .get::<crate::server::game::inventory_effects::InventoryPersistence>(player)
            .is_some_and(|state| state.matches_epoch(epoch))
    {
        return false;
    }
    if defer_death_with_rules(world, player, cause, attribution, rules.clone()) {
        return true;
    }
    unowned_terminal::begin(world, player, cause, attribution, rules)
}

/// Polling queues application through the aggregate router boundary, never guest reentry.
fn poll(world: &mut World) {
    let completed: Vec<_> =
        world
            .get_resource::<Coordinator>()
            .map_or_else(Vec::new, |coordinator| {
                let mut completions: Vec<_> = coordinator.rx.lock().try_iter().collect();
                completions.extend(std::mem::take(&mut *coordinator.deferred.lock()));
                completions
            });
    for completion in completed {
        let operation = completion.operation.clone();
        if !crate::server::worlds::queue_router_request(world, move |router, _| {
            apply(router, completion)
        }) {
            operation.progress.lock().status = Status::Uncertain;
        }
    }
}

/// Apply only to the original native lifetime after definitive canonical acknowledgement.
fn apply(router: &mut crate::server::worlds::DimensionRouter, mut completion: Completion) {
    let operation = completion.operation.clone();
    if matches!(completion.action, Apply::Live)
        && operation
            .live
            .as_ref()
            .is_some_and(|live| live.terminal.load(std::sync::atomic::Ordering::Acquire))
    {
        return;
    }
    let observed = match &completion.result {
        Ok(observed) => observed.clone(),
        Err(failure) => {
            if !failure.uncertain
                && let Apply::AdoptTerminal { player, hold } = &completion.action
                && let Some(server) = router.world_mut(operation.reference.dimension)
                && server.ecs.world().id() == operation.world
            {
                let world = server.ecs.world_mut();
                if quarantine::release_source(world, hold) {
                    world.entity_mut(*player).remove::<SourceBody>();
                    unowned_terminal::reject_adoption(world, *player, &operation);
                }
            }
            if !failure.uncertain
                && matches!(completion.action, Apply::Live)
                && let Some(server) = router.world_mut(operation.reference.dimension)
                && server.ecs.world().id() == operation.world
            {
                live::reject_pending(server.ecs.world_mut(), &operation);
            }
            if !failure.uncertain
                && let Apply::Retire {
                    player,
                    hold,
                    provisional: true,
                } = &completion.action
                && let Some(server) = router.world_mut(operation.reference.dimension)
                && server.ecs.world().id() == operation.world
            {
                let world = server.ecs.world_mut();
                if quarantine::release_source(world, hold) {
                    world.entity_mut(*player).remove::<SourceBody>();
                }
            }
            operation.progress.lock().status = if failure.uncertain {
                Status::Uncertain
            } else {
                Status::Rejected(failure.error)
            };
            return;
        }
    };
    if matches!(completion.action, Apply::Recreate)
        && duplicate(router, &operation.reference, None, false)
    {
        operation.complete(observed, None, false, Physical::Unavailable);
        return;
    }
    if matches!(completion.action, Apply::Recreate) {
        let feet =
            DVec3::from_array(observed.data.position) - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
        match router.prepare_state_position(operation.reference.dimension, feet) {
            Ok(true) => (),
            Ok(false) if completion.remaining > 0 => {
                completion.remaining -= 1;
                if let Some(server) = router.world_mut(operation.reference.dimension)
                    && server.ecs.world().id() == operation.world
                    && let Some(coordinator) = server.ecs.world().get_resource::<Coordinator>()
                {
                    coordinator.deferred.lock().push(completion);
                    return;
                }
                operation.complete(observed, None, false, Physical::Unavailable);
                return;
            }
            _ => {
                operation.complete(observed, None, false, Physical::Unavailable);
                return;
            }
        }
    }
    let Some(server) = router.world_mut(operation.reference.dimension) else {
        operation.complete(observed, None, false, Physical::Unavailable);
        return;
    };
    let world = server.ecs.world_mut();
    if world.id() != operation.world {
        operation.complete(observed, None, false, Physical::Unavailable);
        return;
    }
    let mut actor = None;
    let applied = match completion.action {
        Apply::AdoptTerminal { player, hold } if quarantine::matches(world, &hold) => {
            quarantine::acknowledge_source_receipts(world, &hold, &observed.data);
            if quarantine::release_source(world, &hold) {
                world.entity_mut(player).insert((
                    ManagedSource::acknowledged(observed.source.clone())
                        .retaining(operation.clone()),
                    crate::server::game::inventory_effects::InventoryPersistence::from_record(
                        Some(&observed.data),
                    ),
                ));
                world
                    .entity_mut(player)
                    .remove::<(SourceBody, unowned_terminal::Witness)>();
                unowned_terminal::reject_adoption(world, player, &operation);
                actor = Some(player);
                true
            } else {
                false
            }
        }
        Apply::RetireRebind {
            player,
            hold,
            operation: rebound,
        } if quarantine::matches(world, &hold) => {
            world.despawn(player);
            rebind::retired(world, &rebound, &observed);
            true
        }
        Apply::Live => {
            let (actual, applied) = live::apply(world, &operation, &observed);
            actor = actual;
            applied
        }
        Apply::Inspect => {
            let mut query = world.query::<(Entity, &ManagedSource)>();
            actor = query.iter(world).find_map(|(player, managed)| {
                (same_body(&observed.source, &managed.record)
                    && world
                        .get::<PlayerUuid>(player)
                        .is_some_and(|uuid| uuid.0 == operation.reference.uuid))
                .then_some(player)
            });
            false
        }
        Apply::Seal { player, hold } if quarantine::matches(world, &hold) => {
            quarantine::acknowledge_source_receipts(world, &hold, &observed.data);
            world.entity_mut(player).insert((
                ManagedSource::acknowledged(observed.source.clone()).retaining(operation.clone()),
                SourceBody(hold),
            ));
            world
                .entity_mut(player)
                .insert(crate::entity::components::OnGround::default());
            world.entity_mut(player).insert(
                super::super::super::inventory_effects::InventoryPersistence::from_record(Some(
                    &observed.data,
                )),
            );
            actor = Some(player);
            true
        }
        Apply::Recreate => {
            let result = observed.snapshot().and_then(|raw| {
                let player = spawn_detached(world, &operation.reference.name, &raw)?;
                world.entity_mut(player).insert(
                    super::super::super::inventory_effects::InventoryPersistence::from_record(
                        Some(&observed.data),
                    ),
                );
                let hold = match quarantine::hold(
                    world,
                    player,
                    Arc::new(raw),
                    quarantine::Role::Source,
                    operation.clone(),
                ) {
                    Ok(hold) => hold,
                    Err(error) => {
                        world.despawn(player);
                        return Err(error);
                    }
                };
                world.entity_mut(player).insert((
                    ManagedSource::acknowledged(observed.source.clone())
                        .retaining(operation.clone()),
                    SourceBody(hold),
                ));
                world
                    .entity_mut(player)
                    .insert(crate::entity::components::OnGround::default());
                Ok(player)
            });
            if let Ok(player) = result {
                actor = Some(player);
                true
            } else {
                false
            }
        }
        Apply::Activate { player, hold } if quarantine::matches(world, &hold) => {
            quarantine::acknowledge_source_receipts(world, &hold, &observed.data);
            if quarantine::release_source(world, &hold) {
                crate::plugin::manager::players::release_inactive_anchor(world, player);
                world.entity_mut(player).insert(
                    ManagedSource::acknowledged(observed.source.clone())
                        .retaining(operation.clone()),
                );
                world.entity_mut(player).remove::<SourceBody>();
                world.entity_mut(player).insert(
                    super::super::super::inventory_effects::InventoryPersistence::from_record(
                        Some(&observed.data),
                    ),
                );
                actor = Some(player);
                true
            } else {
                false
            }
        }
        Apply::Retire { player, hold, .. } if quarantine::matches(world, &hold) => {
            world.despawn(player);
            true
        }
        Apply::RetireAbsent => false,
        Apply::Terminal {
            player,
            hold,
            death,
        } if quarantine::matches(world, &hold) => {
            quarantine::acknowledge_source_receipts(world, &hold, &observed.data);
            if quarantine::release_source(world, &hold) {
                world.entity_mut(player).insert(
                    ManagedSource::acknowledged(observed.source.clone())
                        .retaining(operation.clone()),
                );
                world.entity_mut(player).remove::<SourceBody>();
                world.entity_mut(player).insert(
                    super::super::super::inventory_effects::InventoryPersistence::from_record(
                        Some(&observed.data),
                    ),
                );
                crate::entity::player_life::death::finish_death_with_rules(
                    world,
                    player,
                    death.cause,
                    death.attribution,
                    death.rules.clone(),
                );
                if let Ok(mut actor) = world.get_entity_mut(player) {
                    actor.remove::<TerminalPending>();
                    actor.remove::<live::Pending>();
                }
                actor = world.get_entity(player).is_ok().then_some(player);
                true
            } else {
                false
            }
        }
        _ => false,
    };
    let physical = actor.map_or(Physical::Absent, |actor| {
        if quarantine::is_held(world, actor) {
            Physical::Held
        } else {
            Physical::Present
        }
    });
    operation.complete(observed, actor, applied, physical);
}
