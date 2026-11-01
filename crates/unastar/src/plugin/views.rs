//! Component-owned viewer projections, staged against exact current-world actor lineages.

use super::players::PlayerLineage;
use super::wit_bindings::unastar::plugin::views as wit;
use super::{HostContext, PluginManager};
use bevy_ecs::prelude::*;
use bevy_ecs::world::WorldId;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use wasmtime::component::Resource;

/// Displayed name-tag treatment; actor publication remains independently restricted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NameTagVisibility {
    Hidden,
    Normal,
    Always,
}
/// Owned formatting and restrictive visibility only, never physical actor state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Projection {
    pub name_tag: Option<String>,
    pub name_tag_visibility: Option<NameTagVisibility>,
    pub invisible: bool,
    pub hide_actor: bool,
    pub hide_equipment: bool,
}
#[derive(Clone)]
struct Owner {
    id: crate::plugin::manifest::PluginId,
    generation: uuid::Uuid,
    lease: Weak<()>,
}
impl Owner {
    /// Component replacement never inherits the previous store's pair ownership.
    fn matches(&self, context: &HostContext) -> bool {
        self.id == context.plugin_id
            && self.generation == context.generation
            && self.lease.upgrade().is_some()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    world: WorldId,
    subject: uuid::Uuid,
    viewer: uuid::Uuid,
}
struct Binding {
    subject: PlayerLineage,
    viewer: PlayerLineage,
    owner: Owner,
    provisional: Option<uuid::Uuid>,
    value: Option<Projection>,
}
impl Binding {
    /// A retained subject must still pass its original exact lineage; viewers require a transport.
    fn matches(&self, world: &World, subject: Entity, viewer: Entity) -> bool {
        self.subject.matches(world, subject)
            && self.viewer.matches(world, viewer)
            && world
                .get::<crate::entity::components::PlayerSession>(viewer)
                .is_some()
            && self.owner.lease.upgrade().is_some()
    }
}
/// Runtime-global ownership with explicit actual-world binding on every pair.
#[derive(Resource, Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<HashMap<Key, Binding>>>);

/// One callback's owned update; the engine never sees provisional formatting.
pub(super) struct Action {
    key: Key,
    reservation: Option<uuid::Uuid>,
    value: Option<Projection>,
}

impl HostContext {
    /// Stage a bounded pair without mutating native representation during guest execution.
    fn stage_view(
        &mut self,
        subject: PlayerLineage,
        viewer: PlayerLineage,
        value: Option<Projection>,
    ) -> Result<(), wit::ProjectionError> {
        if self.view_updates >= self.api_limits.max_view_updates_per_tick {
            return Err(wit::ProjectionError::LimitExceeded);
        }
        let key = Key {
            world: self.world_mut().id(),
            subject: subject.key(),
            viewer: viewer.key(),
        };
        let registry = self.view_registry.clone();
        let mut entries = registry.0.lock();
        entries.retain(|_, entry| entry.owner.lease.upgrade().is_some());
        if value.is_none() && !entries.contains_key(&key) {
            return Ok(());
        }
        let reservation = if let Some(entry) = entries.get(&key) {
            if !entry.owner.matches(self) {
                return Err(wit::ProjectionError::Conflict);
            }
            entry.provisional
        } else {
            let count = entries
                .values()
                .filter(|entry| entry.owner.matches(self))
                .count();
            if count >= self.api_limits.max_view_pairs as usize || entries.len() >= 65_536 {
                return Err(wit::ProjectionError::LimitExceeded);
            }
            let token = uuid::Uuid::new_v4();
            entries.insert(
                key,
                Binding {
                    subject,
                    viewer,
                    owner: Owner {
                        id: self.plugin_id.clone(),
                        generation: self.generation,
                        lease: Arc::downgrade(&self.view_lease),
                    },
                    provisional: Some(token),
                    value: None,
                },
            );
            Some(token)
        };
        self.view_updates += 1;
        let action = Action {
            key,
            reservation,
            value,
        };
        if let Some(previous) = self.pending_views.iter_mut().find(|old| old.key == key) {
            *previous = action;
        } else {
            self.pending_views.push(action);
        }
        Ok(())
    }
}

impl super::wit_bindings::unastar::plugin::views::Host for HostContext {
    fn project_player(
        &mut self,
        tx: Resource<super::wit_types::WorldTx>,
        subject: Resource<super::wit_types::Player>,
        viewer: Resource<super::wit_types::Player>,
        value: Option<wit::PlayerProjection>,
    ) -> Result<(), wit::ProjectionError> {
        self.validate_transaction(tx.rep())
            .map_err(wit::ProjectionError::Access)?;
        self.require(crate::plugin::manifest::PluginCapability::PlayerViews)
            .map_err(wit::ProjectionError::Access)?;
        let (subject, _) = self
            .live_player(subject.rep())
            .map_err(wit::ProjectionError::Access)?;
        let (viewer, actor) = self
            .live_player(viewer.rep())
            .map_err(wit::ProjectionError::Access)?;
        if self
            .world_mut()
            .get::<crate::entity::components::PlayerSession>(actor)
            .is_none()
        {
            return Err(wit::ProjectionError::Access(
                super::wit_types::AccessError::Unavailable,
            ));
        }
        let value = value
            .map(|value| {
                if value.name_tag.as_ref().is_some_and(|text| {
                    text.len() > 1024 || text.chars().any(|ch| ch.is_control() && ch != '\n')
                }) {
                    return Err(wit::ProjectionError::InvalidValue);
                }
                Ok(Projection {
                    name_tag: value.name_tag,
                    name_tag_visibility: value.name_tag_visibility.map(
                        |visibility| match visibility {
                            wit::NameTagVisibility::Hidden => NameTagVisibility::Hidden,
                            wit::NameTagVisibility::Normal => NameTagVisibility::Normal,
                            wit::NameTagVisibility::Always => NameTagVisibility::Always,
                        },
                    ),
                    invisible: value.invisible == Some(true),
                    hide_actor: value.actor_visible == Some(false),
                    hide_equipment: value.equipment_visible == Some(false),
                })
            })
            .transpose()?
            .filter(|value| *value != Projection::default());
        self.stage_view(subject, viewer, value)
    }
}

/// Commit formatting once after success; traps preserve a previous committed binding.
pub(super) fn finish_invocation(context: &mut HostContext, world: &mut World, succeeded: bool) {
    let actions = std::mem::take(&mut context.pending_views);
    let mut entries = context.view_registry.0.lock();
    for action in actions {
        let Some(binding) = entries.get(&action.key) else {
            continue;
        };
        if !binding.owner.matches(context) || binding.provisional != action.reservation {
            continue;
        }
        let current = action.key.world == world.id()
            && super::players::resolve(world, &binding.subject).is_ok_and(|subject| {
                super::players::resolve(world, &binding.viewer)
                    .is_ok_and(|viewer| binding.matches(world, subject, viewer))
            });
        if !succeeded || !current {
            if action.reservation.is_some() {
                entries.remove(&action.key);
            }
            continue;
        }
        if action.value.is_none() {
            entries.remove(&action.key);
        } else {
            let binding = entries.get_mut(&action.key).expect("validated pair owner");
            binding.provisional = None;
            binding.value = action.value;
        }
    }
}

/// Install an internal engine query resource, without a public native mutation API.
pub(super) fn install(registry: Registry, world: &mut World) {
    world.insert_resource(registry);
}
/// Prune only this world's stale pairs; other live worlds own their own actual actors.
pub(crate) fn prune(world: &mut World) {
    let Some(registry) = world.get_resource::<Registry>().cloned() else {
        return;
    };
    registry.0.lock().retain(|key, binding| {
        if binding.owner.lease.upgrade().is_none() {
            return false;
        }
        key.world != world.id()
            || super::players::resolve(world, &binding.subject).is_ok_and(|subject| {
                super::players::resolve(world, &binding.viewer)
                    .is_ok_and(|viewer| binding.matches(world, subject, viewer))
            })
    });
}
/// A world migration discards both source and recipient bindings for the old realm.
pub(crate) fn forget(world: &World, player: Entity) {
    let (Some(registry), Some(player)) = (
        world.get_resource::<Registry>(),
        world.get::<PlayerLineage>(player),
    ) else {
        return;
    };
    registry.0.lock().retain(|key, _| {
        key.world != world.id() || (key.subject != player.key() && key.viewer != player.key())
    });
}
/// Read a committed exact pair only, without allowing native policy to be bypassed.
pub(crate) fn projection(world: &World, subject: Entity, viewer: Entity) -> Option<Projection> {
    let registry = world.get_resource::<Registry>()?;
    let key = Key {
        world: world.id(),
        subject: world.get::<PlayerLineage>(subject)?.key(),
        viewer: world.get::<PlayerLineage>(viewer)?.key(),
    };
    let entries = registry.0.lock();
    let binding = entries.get(&key)?;
    binding
        .matches(world, subject, viewer)
        .then(|| binding.value.clone())
        .flatten()
}
/// A restrictive view never expands an explicit native actor audience.
pub(crate) fn actor_allowed(world: &World, subject: Entity, viewer: Entity) -> bool {
    subject == viewer || projection(world, subject, viewer).is_none_or(|value| !value.hide_actor)
}

impl PluginManager {
    /// Closed worlds cannot retain pair quotas indefinitely in a shared runtime.
    pub(crate) fn prune_player_views(&self, router: &crate::server::worlds::DimensionRouter) {
        let live: std::collections::HashSet<_> = router
            .iter()
            .map(|(_, server)| server.ecs.world().id())
            .collect();
        self.view_registry.0.lock().retain(|key, binding| {
            live.contains(&key.world) && binding.owner.lease.upgrade().is_some()
        });
    }
}
