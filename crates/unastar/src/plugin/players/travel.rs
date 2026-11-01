//! Scoped component intents over native durable relocation and world migration.

use super::*;
use crate::entity::components::{PlayerSession, Position};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::player_events::ConnectionIdentity;
use crate::server::worlds::{self, DimensionRouter, TransferError};
use glam::DVec3;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Weak;
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_OUTSTANDING: usize = 64;

/// One component instance owns both pending operations and undelivered outcomes.
#[derive(Default)]
pub(in crate::plugin::manager) struct Results {
    notices: Mutex<VecDeque<Notice>>,
    outstanding: Arc<AtomicUsize>,
}

/// Keep a bound until its terminal notification is drained or discarded.
struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Notice {
    event: wit::TravelResult,
    _permit: Permit,
}

/// No borrowed resource or ECS borrow escapes the original callback.
pub(in crate::plugin::manager) struct Request {
    source: WorldReference,
    destination: WorldReference,
    player: Entity,
    reference: PlayerLineage,
    connection: ConnectionIdentity,
    owner: Weak<()>,
    radius: u32,
    feet: DVec3,
    event: wit::TravelResult,
    results: Arc<Results>,
    permit: Permit,
}

/// Native queue ownership makes world closure a definite failed operation.
struct Operation(Option<Request>);
impl Operation {
    /// Publish exactly one owned result to its still-existing component instance.
    fn finish(mut self, outcome: wit::TravelOutcome) {
        let Some(mut request) = self.0.take() else {
            return;
        };
        if request.owner.upgrade().is_some() {
            request.event.outcome = outcome;
            request.results.notices.lock().push_back(Notice {
                event: request.event,
                _permit: request.permit,
            });
        }
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if let Some(mut request) = self.0.take()
            && request.owner.upgrade().is_some()
        {
            request.event.outcome = not_applied(wit::TravelError::SourceClosed);
            request.results.notices.lock().push_back(Notice {
                event: request.event,
                _permit: request.permit,
            });
        }
    }
}

#[derive(Resource, Default)]
struct Pending(HashMap<String, Operation>);
#[derive(Resource)]
struct Installed;

/// Attach read-only native outcome listeners once per world, without manager reentry.
pub(in crate::plugin::manager) fn install(world: &mut World) {
    if world.contains_resource::<Installed>() {
        return;
    }
    world.insert_resource(Installed);
    world.init_resource::<Pending>();
    world
        .get_resource_or_init::<worlds::RelocationHooks>()
        .register(Bridge);
    world
        .get_resource_or_init::<worlds::WorldTransferHooks>()
        .register(Bridge);
}

/// Detach the matching operation before publishing an outcome or running any callback.
fn take(world: &mut World, id: &str) -> Option<Operation> {
    world.get_resource_mut::<Pending>()?.0.remove(id)
}

struct Bridge;
impl worlds::RelocationHandler for Bridge {
    fn on_committed(&self, world: &mut World, event: &worlds::PlayerRelocationCommitted) {
        if let Some(operation) = take(world, &event.request_id) {
            let dimension = operation
                .0
                .as_ref()
                .expect("owned operation")
                .source
                .dimension;
            operation.finish(wit::TravelOutcome::Moved(wit::TravelMoved {
                from: point(event.from),
                destination: location(dimension, event.feet),
            }));
        }
    }
    fn on_failed(&self, world: &mut World, event: &worlds::PlayerRelocationFailed) {
        if let Some(operation) = take(world, &event.request_id) {
            operation.finish(failure(&event.error));
        }
    }
}
impl worlds::WorldTransferHandler for Bridge {
    fn on_committed(
        &self,
        source: &mut World,
        _destination: &mut World,
        event: &worlds::PlayerWorldTransferred,
    ) {
        if let Some(operation) = take(source, &event.request_id) {
            operation.finish(wit::TravelOutcome::Moved(wit::TravelMoved {
                from: point(event.from),
                destination: location(event.to_dimension, event.feet),
            }));
        }
    }
    fn on_failed(&self, source: &mut World, event: &worlds::WorldTransferFailed) {
        if let Some(operation) = take(source, &event.request_id) {
            operation.finish(failure(&event.error));
        }
    }
}

/// Never classify a potentially acknowledged native write as safely unapplied.
fn failure(error: &TransferError) -> wit::TravelOutcome {
    let error = match error {
        TransferError::RecoveryRequired => return wit::TravelOutcome::Uncertain,
        TransferError::InvalidDimension | TransferError::InvalidRequest => {
            wit::TravelError::InvalidRequest
        }
        TransferError::UnknownDimension => wit::TravelError::DestinationClosed,
        TransferError::IncompatibleRegistries => wit::TravelError::IncompatibleWorlds,
        TransferError::SourceUnavailable => wit::TravelError::Unavailable,
        TransferError::StaleSource => wit::TravelError::StaleSource,
        TransferError::UnsafeDestination => wit::TravelError::UnsafeDestination,
        TransferError::Rejected => wit::TravelError::Rejected,
        TransferError::Pending => wit::TravelError::Pending,
        TransferError::UnsupportedComponents(_) => wit::TravelError::UnsupportedState,
        TransferError::Backpressure => wit::TravelError::Backpressure,
        TransferError::Timeout => wit::TravelError::Timeout,
        TransferError::Persistence => wit::TravelError::Persistence,
    };
    not_applied(error)
}

/// Convert finite native feet coordinates without eye-height adjustment.
fn point(feet: DVec3) -> wit::Vec3 {
    wit::Vec3 {
        x: feet.x,
        y: feet.y,
        z: feet.z,
    }
}
/// Capture a descriptive location; dimensions never grant access to a world.
fn location(dimension: i32, feet: DVec3) -> wit::TravelLocation {
    wit::TravelLocation {
        dimension,
        feet: point(feet),
    }
}
/// Construct a definite native failure that did not perform a possible write.
fn not_applied(error: wit::TravelError) -> wit::TravelOutcome {
    wit::TravelOutcome::NotApplied(error)
}
/// Native loading, publication and inventory barriers remain authoritative.
fn ready(world: &World, player: Entity) -> bool {
    matches!(availability(world, player), wit::PlayerAvailability::Ready)
        && world.get::<PlayerSession>(player).is_some()
        && super::super::readiness::admission_allowed(world)
        && world
            .get_resource::<crate::server::game::types::PlayerProviderResource>()
            .is_some_and(|provider| provider.0.atomic_durable_records())
}

impl HostContext {
    /// Validate and reserve bounded callback-owned work; no native request exists yet.
    pub(in crate::plugin::manager) fn stage_travel(
        &mut self,
        player: u32,
        destination: u32,
        feet: wit::Vec3,
        radius: u16,
        correlation: u64,
    ) -> Result<wit::TravelAcknowledgement, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::PlayerTravel)?;
        let (reference, player) = self.live_player(player)?;
        let destination = self.stored_world(destination)?;
        let feet = DVec3::new(feet.x, feet.y, feet.z);
        if !feet.is_finite() || !feet.as_vec3().is_finite() || radius > 256 {
            return Err(wit::AccessError::InvalidArgument);
        }
        let world = unsafe { self.world() };
        let source = WorldReference::of(world)?;
        let directory = world
            .get_resource::<ComponentDirectory>()
            .ok_or(wit::AccessError::Unavailable)?;
        if !directory.worlds.contains(&destination) {
            return Err(wit::AccessError::WorldClosed);
        }
        if !ready(world, player) {
            return Err(wit::AccessError::Unavailable);
        }
        let connection =
            ConnectionIdentity::capture(world, player).ok_or(wit::AccessError::StalePlayer)?;
        let from = world
            .get::<Position>(player)
            .ok_or(wit::AccessError::Unavailable)?
            .0
            - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
        if !from.is_finite() {
            return Err(wit::AccessError::Unavailable);
        }
        let counter = &self.travel_results.outstanding;
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value < MAX_OUTSTANDING).then_some(value + 1)
            })
            .map_err(|_| wit::AccessError::LimitExceeded)?;
        self.pending_travel.push(Request {
            source,
            destination,
            player,
            connection,
            owner: Arc::downgrade(&self.mechanics_owner),
            radius: radius.into(),
            feet,
            event: wit::TravelResult {
                correlation,
                identity: reference.identity.clone(),
                origin: location(source.dimension, from),
                requested: location(destination.dimension, feet),
                outcome: not_applied(wit::TravelError::Cancelled),
            },
            reference,
            results: self.travel_results.clone(),
            permit: Permit(counter.clone()),
        });
        Ok(wit::TravelAcknowledgement::Staged)
    }
}

/// Successful callbacks publish queueing only, independently of default cancellation.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    successful: bool,
) {
    let pending = std::mem::take(&mut context.pending_travel);
    if !successful {
        return;
    }
    for request in pending {
        let operation = Arc::new(Mutex::new(Some(Operation(Some(request)))));
        let queued = operation.clone();
        // The closure owns the failure guard, including when its world is closed.
        if !worlds::queue_router_request(world, move |router, _| {
            if let Some(operation) = queued.lock().take() {
                start(router, operation);
            }
        }) && let Some(operation) = operation.lock().take()
        {
            operation.finish(not_applied(wit::TravelError::Backpressure));
        }
    }
}

/// Enter native loading outside all component-manager and local world borrows.
fn start(router: &mut DimensionRouter, operation: Operation) {
    let request = operation.0.as_ref().expect("owned operation");
    if request.owner.upgrade().is_none() {
        operation.finish(not_applied(wit::TravelError::Cancelled));
        return;
    }
    if !router
        .world(request.source.dimension)
        .is_some_and(|server| request.source.matches(server.ecs.world()))
    {
        operation.finish(not_applied(wit::TravelError::SourceClosed));
        return;
    }
    if !router
        .world(request.destination.dimension)
        .is_some_and(|server| request.destination.matches(server.ecs.world()))
    {
        operation.finish(not_applied(wit::TravelError::DestinationClosed));
        return;
    }
    let world = router
        .world_mut(request.source.dimension)
        .expect("validated source")
        .ecs
        .world_mut();
    if !request.reference.matches(world, request.player)
        || !request.connection.matches(world, request.player)
        || !ready(world, request.player)
    {
        operation.finish(not_applied(wit::TravelError::StaleSource));
        return;
    }
    let source = request.source;
    let destination = request.destination;
    let reference = request.reference.clone();
    let connection = request.connection.clone();
    let player = request.player;
    let owner = request.owner.clone();
    let feet = request.feet;
    let radius = request.radius;
    let id = uuid::Uuid::new_v4().to_string();
    let accepted = if source == destination {
        worlds::request_relocation_checked(
            world,
            player,
            feet,
            radius,
            id.clone(),
            move |world, _| {
                owner.upgrade().is_some()
                    && source.matches(world)
                    && reference.matches(world, player)
                    && connection.matches(world, player)
                    && ready(world, player)
            },
        )
    } else {
        worlds::request_transfer_checked(
            world,
            player,
            destination.dimension,
            feet,
            radius,
            id.clone(),
            move |world, target, _| {
                owner.upgrade().is_some()
                    && source.matches(world)
                    && destination.matches(target)
                    && reference.matches(world, player)
                    && connection.matches(world, player)
                    && ready(world, player)
            },
        )
    };
    match accepted {
        Ok(()) => {
            world
                .get_resource_or_init::<Pending>()
                .0
                .insert(id, operation);
        }
        Err(error) => operation.finish(failure(&error)),
    }
}

impl super::super::PluginManager {
    /// Dispatch a bounded outcome generation globally with no fabricated live context.
    pub(crate) fn drain_travel_results(&mut self, router: &mut DimensionRouter) {
        if self.readiness.check().is_err() {
            return;
        }
        let Some(server) = router.world_mut(router.primary_dimension()) else {
            return;
        };
        let world = server.ecs.world_mut();
        let mut owners: Vec<_> = self.plugins.keys().cloned().collect();
        owners.sort_by(|a, b| a.0.cmp(&b.0));
        for owner in owners {
            let plugin = self.plugins.get_mut(&owner).expect("listed component");
            let notices: Vec<_> = plugin
                .store
                .data()
                .travel_results
                .notices
                .lock()
                .drain(..)
                .collect();
            for notice in notices {
                let result = super::super::lifecycle::invoke(plugin, world, |store, bindings| {
                    bindings.call_on_player_travel_result(store, &notice.event)
                });
                if let Err(error) = result {
                    self.callback_failed(world, &owner, &error.to_string());
                    if self.readiness.check().is_err() {
                        return;
                    }
                    break;
                }
            }
        }
    }
}
