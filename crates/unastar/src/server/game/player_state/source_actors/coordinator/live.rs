//! Point-in-time ownership writes leave the genuine retained body's simulation running.
use super::*;
use crate::plugin::manager::players::PlayerLineage;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct State {
    pub(super) player: Entity,
    lineage: PlayerLineage,
    pub(super) snapshot: Arc<PlayerSnapshot>,
    pub(super) terminal: AtomicBool,
    pub(super) result: Mutex<Option<Result<Arc<Observation>, Failure>>>,
    pub(super) ready: tokio::sync::Notify,
}
impl State {
    pub(super) fn new(
        player: Entity,
        lineage: PlayerLineage,
        snapshot: Arc<PlayerSnapshot>,
    ) -> Self {
        Self {
            player,
            lineage,
            snapshot,
            terminal: AtomicBool::new(false),
            result: Mutex::new(None),
            ready: tokio::sync::Notify::new(),
        }
    }
    fn epoch_matches(&self, world: &World) -> bool {
        world
            .get::<crate::server::game::inventory_effects::InventoryPersistence>(self.player)
            .is_some_and(|persistence| {
                persistence.revision() == self.snapshot.data.inventory_revision
                    && persistence.receipts() == &self.snapshot.data.inventory_effects
            })
    }
    fn matches(&self, world: &World, operation: &Operation) -> bool {
        world.id() == operation.world
            && self.lineage.matches(world, self.player)
            && world
                .get::<PlayerUuid>(self.player)
                .is_some_and(|uuid| uuid.0 == operation.reference.uuid)
            && account_xuid(world, self.player) == operation.reference.xuid.as_deref()
    }
}
/// Pins actual lineage/residency only; it is not durable authority or a physical hold.
#[derive(Component)]
pub(crate) struct LiveSource(pub(crate) Arc<Operation>);
/// Serializes ownership writes without removing any native simulation components.
#[derive(Component)]
pub(super) struct Pending(Arc<Operation>);

/// Report only persistence ownership, independently of physical simulation eligibility.
pub(crate) fn pending(world: &World, player: Entity) -> bool {
    world.get::<Pending>(player).is_some()
}
/// Admission and competing durable writers serialize against accepted ownership work.
pub(crate) fn pending_account(world: &World, uuid: uuid::Uuid, xuid: Option<&str>) -> bool {
    world.try_query::<&Pending>().is_some_and(|mut query| {
        query.iter(world).any(|pending| {
            pending.0.reference.uuid == uuid
                || xuid
                    .filter(|xuid| !xuid.is_empty())
                    .is_some_and(|xuid| pending.0.reference.xuid.as_deref() == Some(xuid))
        })
    })
}
/// Callback success establishes pending ownership before another native hit can occur.
pub(super) fn accept(world: &mut World, operation: &Arc<Operation>) -> Result<(), Error> {
    let state = operation.live.as_ref().ok_or(Error::InvalidState)?;
    let player = state.player;
    if !state.matches(world, operation) {
        return Err(Error::SourceGone);
    }
    if !state.epoch_matches(world) {
        return Err(Error::Conflict);
    }
    if crate::server::game::player_state::source_actors::pending(world, player)
        || world.get::<Dead>(player).is_some()
        || crate::server::game::inventory_effects::is_quarantined(world, player)
        || crate::server::game::player_events::initial_state_pending(world, player)
    {
        return Err(Error::Busy);
    }
    if let Some(managed) = world.get::<ManagedSource>(player)
        && (managed.record.module != operation.reference.module
            || managed.record.nonce != operation.reference.nonce
            || managed.record.phase != SourcePhase::Active)
    {
        return Err(Error::NotOwned);
    }
    if let Some(live) = world.get::<LiveSource>(player)
        && (live.0.reference.module != operation.reference.module
            || live.0.reference.nonce != operation.reference.nonce)
    {
        return Err(Error::NotOwned);
    }
    world
        .entity_mut(player)
        .insert((Pending(operation.clone()), LiveSource(operation.clone())));
    Ok(())
}
pub(super) fn reject_pending(world: &mut World, operation: &Arc<Operation>) {
    let Some(state) = &operation.live else {
        return;
    };
    if world
        .get::<Pending>(state.player)
        .is_some_and(|pending| Arc::ptr_eq(&pending.0, operation))
    {
        world.entity_mut(state.player).remove::<Pending>();
    }
}

pub(super) fn run(router: &mut crate::server::worlds::DimensionRouter, prepared: Prepared) {
    let operation = prepared.operation.clone();
    let state = operation.live.as_ref().expect("live intent").clone();
    let outcome = (|| {
        if duplicate(
            router,
            &operation.reference,
            Some((operation.world, state.player)),
            true,
        ) {
            return Err(Error::Conflict);
        }
        let world = router
            .world_mut(operation.reference.dimension)
            .ok_or(Error::WrongWorld)?
            .ecs
            .world_mut();
        if !state.matches(world, &operation)
            || world.get::<PlayerSession>(state.player).is_some()
            || world
                .get::<crate::server::game::player_events::DetachedPlayer>(state.player)
                .is_none()
            || !world
                .get::<Pending>(state.player)
                .is_some_and(|pending| Arc::ptr_eq(&pending.0, &operation))
        {
            return Err(Error::SourceGone);
        }
        if !state.epoch_matches(world) {
            return Err(Error::Conflict);
        }
        let coordinator = world
            .get_resource::<Coordinator>()
            .ok_or(Error::NotInstalled)?;
        let writer = coordinator.writer.clone();
        let tx = coordinator.tx.clone();
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| Error::NotInstalled)?;
        let (snapshot, lease): (_, Arc<dyn Send + Sync>) = if state.terminal.load(Ordering::Acquire)
        {
            (state.snapshot.clone(), operation.clone())
        } else {
            let size =
                crate::server::game::player_state::death_capture::estimate(world, state.player)
                    .map_err(|_| Error::LimitExceeded)?;
            let lease = (prepared.reserve)(size.checked_mul(3).ok_or(Error::LimitExceeded)?)?;
            (
                Arc::new(capture(world, state.player).map_err(|_| Error::InvalidState)?),
                lease,
            )
        };
        let proof = world
            .get::<ManagedSource>(state.player)
            .map(ManagedSource::proof);
        operation.progress.lock().status = Status::Queued;
        let reply = Reply {
            operation: operation.clone(),
            writer: writer.clone(),
            tx,
            action: Some(Apply::Live),
        };
        let reserve = prepared.reserve;
        let state = state.clone();
        let operation = operation.clone();
        runtime.spawn(async move {
            let _lease = lease;
            let result = Work::Live(snapshot, proof)
                .execute(&writer, &operation.reference, reserve)
                .await;
            *state.result.lock() = Some(result.clone());
            state.ready.notify_waiters();
            reply.finish(result);
        });
        Ok::<_, Error>(())
    })();
    if let Err(error) = outcome {
        *state.result.lock() = Some(Err(Failure {
            error,
            uncertain: false,
        }));
        state.ready.notify_waiters();
        if let Some(server) = router.world_mut(operation.reference.dimension)
            && server.ecs.world().id() == operation.world
            && !state.terminal.load(Ordering::Acquire)
        {
            reject_pending(server.ecs.world_mut(), &operation);
        }
        operation.reject(error);
    }
}
/// Update persistence metadata only: changed native vitals, slots and timers stay untouched.
pub(super) fn apply(
    world: &mut World,
    operation: &Arc<Operation>,
    observed: &Arc<Observation>,
) -> (Option<Entity>, bool) {
    let Some(state) = operation.live.as_ref() else {
        return (None, false);
    };
    if !state.matches(world, operation)
        || world.get::<PlayerSession>(state.player).is_some()
        || !world
            .get::<Pending>(state.player)
            .is_some_and(|pending| Arc::ptr_eq(&pending.0, operation))
    {
        return (None, false);
    }
    let actor = Some(state.player);
    if !state.epoch_matches(world)
        || state.terminal.load(Ordering::Acquire)
        || crate::server::game::inventory_effects::is_quarantined(world, state.player)
        || world.get::<Dead>(state.player).is_some()
        || crate::server::game::player_events::initial_state_pending(world, state.player)
    {
        return (actor, false);
    }
    world.entity_mut(state.player).insert((
        ManagedSource::acknowledged(observed.source.clone()).retaining(operation.clone()),
        crate::server::game::inventory_effects::InventoryPersistence::from_record(Some(
            &observed.data,
        )),
    ));
    reject_pending(world, operation);
    (actor, true)
}
/// Hold only an actually dead body. Its terminal row is serialized after the initial write.
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
    let state = operation
        .live
        .as_ref()
        .expect("pending live source")
        .clone();
    if state.terminal.swap(true, Ordering::AcqRel) {
        return true;
    }
    let result = (|| {
        if !state.matches(world, &operation) || world.get::<Dead>(player).is_none() {
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
            Arc::new((lease, operation.clone())),
        )
        .map_err(|_| Error::Conflict)?;
        world
            .entity_mut(player)
            .insert((SourceBody(hold.clone()), TerminalPending));
        let coordinator = world
            .get_resource::<Coordinator>()
            .ok_or(Error::NotInstalled)?;
        let writer = coordinator.writer.clone();
        let reply = Reply {
            operation: operation.clone(),
            writer: writer.clone(),
            tx: coordinator.tx.clone(),
            action: Some(Apply::Terminal {
                player,
                hold,
                death: Arc::new(TerminalDeath {
                    cause,
                    attribution,
                    rules: world.resource::<crate::world::rules::GameRules>().clone(),
                }),
            }),
        };
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| Error::NotInstalled)?;
        let operation = operation.clone();
        runtime.spawn(async move {
            let initial = loop {
                let notified = state.ready.notified();
                if let Some(result) = state.result.lock().clone() {
                    break result;
                }
                notified.await;
            };
            let result = match initial {
                Ok(observed) => {
                    post.data.inventory_revision = observed.data.inventory_revision;
                    post.data.inventory_effects = observed.data.inventory_effects.clone();
                    Work::Terminal {
                        snapshot: Arc::new(post),
                        proof: SourceProof::from(&observed.source),
                    }
                    .execute(&writer, &operation.reference, operation.reserve.clone())
                    .await
                }
                Err(_) => Err(Failure {
                    error: Error::RecoveryRequired,
                    uncertain: true,
                }),
            };
            reply.finish(result);
        });
        Ok::<_, Error>(())
    })();
    if let Err(error) = result {
        if let Some(coordinator) = world.get_resource::<Coordinator>() {
            coordinator
                .writer
                .require_recovery(operation.reference.uuid);
        }
        operation.progress.lock().status = Status::Uncertain;
        tracing::error!(
            ?error,
            "Native pending source terminal processing remains withheld"
        );
    }
    true
}
