//! Bounded deferred component work, re-entered with fresh transaction scopes.

use super::players::{PlayerLineage, WorldReference};
use super::{
    HostContext, PluginManager, make_player_resource, make_world_tx, players, wit_types as wit,
    with_world,
};
use crate::plugin::manifest::PluginId;
use parking_lot::Mutex;
use std::sync::Arc;

/// A task is observable independently of whether its component keeps the resource handle.
pub(super) struct TaskState {
    pub(super) id: u64,
    status: Mutex<wit::TaskStatus>,
}
impl TaskState {
    /// Create pending state before publishing its owned component resource.
    pub(super) fn new(id: u64) -> Self {
        Self {
            id,
            status: Mutex::new(wit::TaskStatus::Pending),
        }
    }
    /// Read the exact recorded task outcome without touching a world.
    pub(super) fn status(&self) -> wit::TaskStatus {
        *self.status.lock()
    }
    /// Cancel pending work only; running and completed callbacks cannot be rolled back.
    pub(super) fn cancel(&self) -> bool {
        let mut status = self.status.lock();
        if matches!(*status, wit::TaskStatus::Pending) {
            *status = wit::TaskStatus::Failed(wit::AccessError::Cancelled);
            true
        } else {
            false
        }
    }
    /// Atomically claim an uncancelled task for one dispatcher.
    fn start(&self) -> bool {
        let mut status = self.status.lock();
        if matches!(*status, wit::TaskStatus::Pending) {
            *status = wit::TaskStatus::Running;
            true
        } else {
            false
        }
    }
    /// Publish a definitive callback or lifetime outcome exactly once.
    fn finish(&self, result: Result<(), wit::AccessError>) {
        *self.status.lock() = match result {
            Ok(()) => wit::TaskStatus::Completed,
            Err(error) => wit::TaskStatus::Failed(error),
        };
    }
}

/// Targets carry stable identity; they never retain borrowed live players or worlds.
#[derive(Clone)]
pub(super) enum TaskTarget {
    Player(Box<PlayerLineage>),
    World(WorldReference),
}

/// One component owns queued work, so removing a source world cannot silently drop it.
pub(super) struct TaskRequest {
    pub(super) owner: PluginId,
    pub(super) generation: uuid::Uuid,
    pub(super) callback: u64,
    pub(super) state: Arc<TaskState>,
    pub(super) target: TaskTarget,
}

impl Drop for TaskRequest {
    fn drop(&mut self) {
        self.state.cancel();
    }
}

/// Failed work has a typed notification even when no target world or player remains.
pub(super) struct TaskFailure {
    task: u64,
    callback: u64,
    error: wit::AccessError,
}

impl PluginManager {
    /// Execute one finite queue snapshot after world ticks, migrations and global callbacks.
    /// New work queued by these callbacks runs in a later drain, never synchronous re-entry.
    pub(crate) fn drain_scheduled_tasks(
        &mut self,
        router: &mut crate::server::worlds::DimensionRouter,
    ) {
        if self.readiness.check().is_err() {
            return;
        }
        let readiness = self.readiness.clone();
        let mut owners: Vec<_> = self.plugins.keys().cloned().collect();
        owners.sort_by(|a, b| a.0.cmp(&b.0));
        for owner in owners {
            let Some(plugin) = self.plugins.get_mut(&owner) else {
                continue;
            };
            let requests: Vec<_> = plugin.store.data_mut().task_requests.drain(..).collect();
            let mut failures: Vec<_> = plugin.store.data_mut().task_failures.drain(..).collect();
            for request in requests {
                if request.owner != owner || request.generation != plugin.store.data().generation {
                    request.state.finish(Err(wit::AccessError::Cancelled));
                    continue;
                }
                let result = if !request.state.start() {
                    Err(wit::AccessError::Cancelled)
                } else {
                    players::task_destination(router, &request.target).and_then(
                        |(dimension, player)| {
                            let server = router
                                .world_mut(dimension)
                                .ok_or(wit::AccessError::WorldClosed)?;
                            let world = server.ecs.world_mut();
                            let result = with_world(
                                &mut plugin.store,
                                world,
                                |store| -> anyhow::Result<()> {
                                    super::prepare_invocation(store);
                                    let tx = make_world_tx(store)?;
                                    match player {
                                        Some(entity) => {
                                            let player = make_player_resource(store, entity)?;
                                            plugin.bindings.call_on_player_task(
                                                store,
                                                tx,
                                                player,
                                                request.state.id,
                                                request.callback,
                                            )
                                        }
                                        None => plugin.bindings.call_on_world_task(
                                            store,
                                            tx,
                                            request.state.id,
                                            request.callback,
                                        ),
                                    }
                                },
                            );
                            super::finish_invocation(
                                plugin.store.data_mut(),
                                world,
                                result.is_ok(),
                            );
                            if let Err(error) = &result
                                && readiness.fail(&owner.0, &error.to_string())
                            {
                                super::readiness::fence_world(world);
                            }
                            result.map_err(|_| wit::AccessError::CallbackFailed)
                        },
                    )
                };
                request.state.finish(result);
                if readiness.check().is_err() {
                    self.tick_budget_valid = false;
                    return;
                }
                if let Err(error) = result {
                    failures.push(TaskFailure {
                        task: request.state.id,
                        callback: request.callback,
                        error,
                    });
                }
            }
            for failure in failures {
                if plugin.store.get_fuel().unwrap_or(0) == 0 {
                    plugin.store.data_mut().task_failures.push_back(failure);
                    continue;
                }
                let Some(primary) = router.world_mut(router.primary_dimension()) else {
                    plugin.store.data_mut().task_failures.push_back(failure);
                    continue;
                };
                let world = primary.ecs.world_mut();
                let result = with_world(&mut plugin.store, world, |store| {
                    super::prepare_invocation(store);
                    plugin.bindings.call_on_task_failed(
                        store,
                        failure.task,
                        failure.callback,
                        failure.error,
                    )
                });
                super::finish_invocation(plugin.store.data_mut(), world, result.is_ok());
                if let Err(error) = result {
                    tracing::error!(plugin = %owner, %error, "Scheduled task failure callback failed");
                    if readiness.fail(&owner.0, &error.to_string()) {
                        self.tick_budget_valid = false;
                        super::readiness::fence_world(world);
                        return;
                    }
                }
            }
        }
    }
}

/// Destroying a component cancels its pending tasks rather than exposing dangling job handles.
pub(super) fn cancel_pending(context: &mut HostContext) {
    for request in context
        .pending_tasks
        .iter()
        .chain(context.task_requests.iter())
    {
        request.state.cancel();
    }
    context.pending_tasks.clear();
    context.task_requests.clear();
    context.task_failures.clear();
}
