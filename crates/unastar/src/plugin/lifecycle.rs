//! Immediate WASM callbacks bridged through the native player lifecycle hooks.

use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::{Entity, World};
use glam::DVec3;
use tracing::error;
use unastar_data::actor_components::body::ActorDamageCause;
use wasmtime::Store;

use super::{HostContext, LoadedPlugin, PluginManager, UnastarPlugin, make_player_handle};
use super::{begin_fuel_budget, prepare_invocation, wit_types, with_world};
use crate::ecs::events::{ActionQueue, EventKind};
use crate::entity::components::Player;
use crate::entity::player_life::hooks::{
    PlayerDamage, PlayerDeath, PlayerLifecycleHandler, PlayerLifecycleHooks, PlayerRespawn,
    PlayerRespawned,
};

/// Shared manager adapter used while the runtime ticks its ECS world.
#[derive(Clone)]
pub(crate) struct LifecycleBridge {
    manager: Arc<Mutex<PluginManager>>,
}

impl LifecycleBridge {
    /// Attach synchronous WASM callbacks without borrowing the manager during simulation.
    pub(crate) fn install(manager: Arc<Mutex<PluginManager>>, world: &mut World) {
        world
            .resource_mut::<PlayerLifecycleHooks>()
            .register(Self { manager });
    }
}

impl PlayerLifecycleHandler for LifecycleBridge {
    fn on_damage(&self, world: &mut World, event: &mut PlayerDamage) {
        if event.is_cancelled() {
            return;
        }
        match self.manager.try_lock() {
            Ok(mut manager) => manager.player_damage(world, event),
            Err(error) => {
                error!(%error, "Plugin manager unavailable during player damage");
                event.cancel();
            }
        }
    }

    fn on_death(&self, world: &mut World, event: &PlayerDeath) {
        match self.manager.try_lock() {
            Ok(mut manager) => manager.player_death(world, event),
            Err(error) => error!(%error, "Plugin manager unavailable during player death"),
        }
    }

    fn on_respawn(&self, world: &mut World, event: &mut PlayerRespawn) {
        if event.is_cancelled() {
            return;
        }
        match self.manager.try_lock() {
            Ok(mut manager) => manager.player_respawn(world, event),
            Err(error) => {
                error!(%error, "Plugin manager unavailable during player respawn");
                event.cancel();
            }
        }
    }

    fn on_respawned(&self, world: &mut World, event: &PlayerRespawned) {
        match self.manager.try_lock() {
            Ok(mut manager) => manager.player_respawned(world, event),
            Err(error) => error!(%error, "Plugin manager unavailable after player respawn"),
        }
    }
}

impl PluginManager {
    /// Attach this shared manager to a custom runtime's player lifecycle hooks.
    ///
    /// Call once for each world, then call [`Self::begin_tick`] before its
    /// simulation and [`Self::tick`] afterward. `UnastarServer` attaches its
    /// manager automatically.
    pub fn attach_to_world(manager: Arc<Mutex<Self>>, world: &mut World) {
        world.init_resource::<PlayerLifecycleHooks>();
        LifecycleBridge::install(manager, world);
    }

    /// Initialize pending components and replenish fuel once before simulation.
    ///
    /// Immediate lifecycle calls and the following [`Self::tick`] share this
    /// budget. Repeated calls before `tick` completes cannot replenish fuel.
    /// Returns whether all component budgets are available; decisions fail
    /// closed when a budget cannot be initialized.
    pub fn begin_tick(&mut self, world: &mut World) -> bool {
        if self.tick_prepared {
            return self.tick_budget_valid;
        }
        self.tick_prepared = true;
        self.tick_budget_valid = true;
        self.initialize_pending(world);
        for plugin in self.plugins.values_mut() {
            if let Err(error) = begin_fuel_budget(&mut plugin.store, &plugin.manifest.limits) {
                error!(plugin = %plugin.manifest.id, %error, "Failed to set tick fuel budget");
                self.tick_budget_valid = false;
            }
        }
        self.tick_budget_valid
    }

    /// Return subscribed plugin identities in a stable order for mutable decisions.
    fn subscribers(&self, kind: EventKind) -> Vec<super::PluginId> {
        let mut ids: Vec<_> = self
            .plugins
            .iter()
            .filter(|(_, plugin)| plugin.manifest.subscriptions.contains(&kind))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        ids
    }

    /// Apply ordered damage decisions while the hit has no committed side effects.
    fn player_damage(&mut self, world: &mut World, event: &mut PlayerDamage) {
        if !self.begin_tick(world) {
            event.cancel();
            return;
        }
        for id in self.subscribers(EventKind::PlayerDamage) {
            let plugin = self.plugins.get_mut(&id).expect("subscribed plugin exists");
            let player = event.player();
            let attacker = player_attacker(world, event.hit.attacker);
            let cause = damage_cause(event.hit.cause);
            let damage = event.hit.damage;
            let result = invoke(plugin, world, |store, bindings| {
                let player = make_player_handle(store, player);
                let attacker = attacker.map(|entity| make_player_handle(store, entity));
                bindings.call_on_player_damage(store, player, attacker, cause, damage)
            });
            match result {
                Ok(Some(damage)) if damage.is_finite() && damage >= 0.0 => {
                    event.hit.damage = damage;
                }
                Ok(_) => {
                    event.cancel();
                    break;
                }
                Err(error) => {
                    error!(plugin = %id, %error, "Player damage callback failed");
                    event.cancel();
                    break;
                }
            }
        }
    }

    /// Notify subscribers after the native death has committed.
    fn player_death(&mut self, world: &mut World, event: &PlayerDeath) {
        if !self.begin_tick(world) {
            return;
        }
        for id in self.subscribers(EventKind::PlayerDeath) {
            let plugin = self.plugins.get_mut(&id).expect("subscribed plugin exists");
            let attacker = player_attacker(world, event.cause.attacker);
            let cause = damage_cause(event.cause.cause);
            if let Err(error) = invoke(plugin, world, |store, bindings| {
                let player = make_player_handle(store, event.player);
                let attacker = attacker.map(|entity| make_player_handle(store, entity));
                bindings.call_on_player_death(store, player, attacker, cause)
            }) {
                error!(plugin = %id, %error, "Player death callback failed");
            }
        }
    }

    /// Apply ordered respawn decisions before acknowledging or restoring the player.
    fn player_respawn(&mut self, world: &mut World, event: &mut PlayerRespawn) {
        if !self.begin_tick(world) {
            event.cancel();
            return;
        }
        for id in self.subscribers(EventKind::PlayerRespawn) {
            let plugin = self.plugins.get_mut(&id).expect("subscribed plugin exists");
            let player = event.player();
            let position = to_wit(event.position);
            let result = invoke(plugin, world, |store, bindings| {
                let player = make_player_handle(store, player);
                bindings.call_on_player_respawn(store, player, position)
            });
            match result {
                Ok(Some(position)) => {
                    let position = DVec3::new(position.x, position.y, position.z);
                    if position.is_finite() && position.as_vec3().is_finite() {
                        event.position = position;
                    } else {
                        event.cancel();
                        break;
                    }
                }
                Ok(None) => {
                    event.cancel();
                    break;
                }
                Err(error) => {
                    error!(plugin = %id, %error, "Player respawn callback failed");
                    event.cancel();
                    break;
                }
            }
        }
    }

    /// Notify subscribers once the player is alive at its committed destination.
    fn player_respawned(&mut self, world: &mut World, event: &PlayerRespawned) {
        if !self.begin_tick(world) {
            return;
        }
        for id in self.subscribers(EventKind::PlayerRespawned) {
            let plugin = self.plugins.get_mut(&id).expect("subscribed plugin exists");
            let position = to_wit(event.position);
            if let Err(error) = invoke(plugin, world, |store, bindings| {
                let player = make_player_handle(store, event.player);
                bindings.call_on_player_respawned(store, player, position)
            }) {
                error!(plugin = %id, %error, "Player respawned callback failed");
            }
        }
    }
}

/// Scope handles to a single call and discard actions from a trapped callback.
fn invoke<R>(
    plugin: &mut LoadedPlugin,
    world: &mut World,
    call: impl FnOnce(&mut Store<HostContext>, &UnastarPlugin) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    plugin.store.data_mut().pending_actions.clear();
    let result = with_world(&mut plugin.store, world, |store| {
        prepare_invocation(store);
        call(store, &plugin.bindings)
    });
    let actions = std::mem::take(&mut plugin.store.data_mut().pending_actions);
    if result.is_ok() && !actions.is_empty() {
        for action in actions {
            world.resource_mut::<ActionQueue>().push(action);
        }
    }
    result
}

/// Only player attackers have a player resource handle in the component API.
fn player_attacker(world: &World, attacker: Option<Entity>) -> Option<Entity> {
    attacker.filter(|entity| world.get::<Player>(*entity).is_some())
}

/// Preserve the native eye-height position convention at the component boundary.
fn to_wit(position: DVec3) -> wit_types::Vec3 {
    wit_types::Vec3 {
        x: position.x,
        y: position.y,
        z: position.z,
    }
}

/// Map semantic causes explicitly so Rust enum numbering is never an API contract.
fn damage_cause(cause: ActorDamageCause) -> wit_types::DamageCause {
    match cause {
        ActorDamageCause::None => wit_types::DamageCause::None,
        ActorDamageCause::Override => wit_types::DamageCause::Override,
        ActorDamageCause::Contact => wit_types::DamageCause::Contact,
        ActorDamageCause::EntityAttack => wit_types::DamageCause::EntityAttack,
        ActorDamageCause::Projectile => wit_types::DamageCause::Projectile,
        ActorDamageCause::Suffocation => wit_types::DamageCause::Suffocation,
        ActorDamageCause::Fall => wit_types::DamageCause::Fall,
        ActorDamageCause::Fire => wit_types::DamageCause::Fire,
        ActorDamageCause::FireTick => wit_types::DamageCause::FireTick,
        ActorDamageCause::Lava => wit_types::DamageCause::Lava,
        ActorDamageCause::Drowning => wit_types::DamageCause::Drowning,
        ActorDamageCause::BlockExplosion => wit_types::DamageCause::BlockExplosion,
        ActorDamageCause::EntityExplosion => wit_types::DamageCause::EntityExplosion,
        ActorDamageCause::Void => wit_types::DamageCause::Void,
        ActorDamageCause::SelfDestruct => wit_types::DamageCause::SelfDestruct,
        ActorDamageCause::Magic => wit_types::DamageCause::Magic,
        ActorDamageCause::Wither => wit_types::DamageCause::Wither,
        ActorDamageCause::Starve => wit_types::DamageCause::Starve,
        ActorDamageCause::Anvil => wit_types::DamageCause::Anvil,
        ActorDamageCause::Thorns => wit_types::DamageCause::Thorns,
        ActorDamageCause::FallingBlock => wit_types::DamageCause::FallingBlock,
        ActorDamageCause::Piston => wit_types::DamageCause::Piston,
        ActorDamageCause::FlyIntoWall => wit_types::DamageCause::FlyIntoWall,
        ActorDamageCause::Magma => wit_types::DamageCause::Magma,
        ActorDamageCause::Fireworks => wit_types::DamageCause::Fireworks,
        ActorDamageCause::Lightning => wit_types::DamageCause::Lightning,
        ActorDamageCause::Charging => wit_types::DamageCause::Charging,
        ActorDamageCause::Temperature => wit_types::DamageCause::Temperature,
        ActorDamageCause::Freezing => wit_types::DamageCause::Freezing,
        ActorDamageCause::Stalactite => wit_types::DamageCause::Stalactite,
        ActorDamageCause::Stalagmite => wit_types::DamageCause::Stalagmite,
        ActorDamageCause::RamAttack => wit_types::DamageCause::RamAttack,
        ActorDamageCause::SonicBoom => wit_types::DamageCause::SonicBoom,
        ActorDamageCause::Campfire => wit_types::DamageCause::Campfire,
        ActorDamageCause::SoulCampfire => wit_types::DamageCause::SoulCampfire,
        ActorDamageCause::MaceSmash => wit_types::DamageCause::MaceSmash,
        ActorDamageCause::All => wit_types::DamageCause::All,
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
