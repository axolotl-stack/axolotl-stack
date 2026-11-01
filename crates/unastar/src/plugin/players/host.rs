//! Typed WIT imports, with no unscoped native actor mutations.

use super::*;
use crate::plugin::manifest::PluginCapability;

impl super::super::wit_bindings::unastar::plugin::types::HostPlayer for HostContext {
    fn to_handle(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<Resource<wit::PlayerHandle>, wit::AccessError> {
        let (reference, _) = self.live_player(player.rep())?;
        self.own_player(reference)
    }
    fn identity(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<wit::PlayerIdentity, wit::AccessError> {
        self.live_player(player.rep())
            .map(|(reference, _)| reference.identity)
    }
    fn position(&mut self, player: Resource<wit::Player>) -> Result<wit::Vec3, wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.position)
    }
    fn rotation(&mut self, player: Resource<wit::Player>) -> Result<(f32, f32), wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.rotation)
    }
    fn health(&mut self, player: Resource<wit::Player>) -> Result<f32, wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.health)
    }
    fn max_health(&mut self, player: Resource<wit::Player>) -> Result<f32, wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.max_health)
    }
    fn game_mode(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<wit::GameMode, wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.game_mode)
    }
    fn effective_game_mode(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<wit::GameMode, wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.effective_game_mode)
    }
    fn flags(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<wit::PlayerFlags, wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.flags)
    }
    fn availability(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<wit::PlayerAvailability, wit::AccessError> {
        let (_, player) = self.live_player(player.rep())?;
        Ok(availability(unsafe { self.world() }, player))
    }
    fn configured_admin(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<bool, wit::AccessError> {
        self.live_snapshot(player.rep())
            .map(|snapshot| snapshot.configured_admin)
    }
    fn snapshot(
        &mut self,
        player: Resource<wit::Player>,
    ) -> Result<wit::PlayerSnapshot, wit::AccessError> {
        self.live_snapshot(player.rep())
    }
    fn message(
        &mut self,
        player: Resource<wit::Player>,
        text: String,
    ) -> Result<(), wit::AccessError> {
        let (reference, player) = self.live_player(player.rep())?;
        self.require(PluginCapability::SendMessages)?;
        if text.len() > self.api_limits.max_message_bytes as usize
            || text.chars().any(|ch| ch.is_control() && ch != '\n')
        {
            return Err(wit::AccessError::InvalidArgument);
        }
        if unsafe { self.world() }
            .get::<crate::entity::components::PlayerSession>(player)
            .is_none()
        {
            return Err(wit::AccessError::Unavailable);
        }
        if self.resources.messages >= self.api_limits.max_messages_per_tick {
            return Err(wit::AccessError::LimitExceeded);
        }
        self.resources.messages += 1;
        let scope = self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        self.pending_messages.push(BoundMessage {
            reference,
            scope,
            text,
        });
        Ok(())
    }
    fn drop(&mut self, player: Resource<wit::Player>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&player.rep());
        Ok(())
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostWorldTx for HostContext {
    fn world(
        &mut self,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Resource<wit::WorldHandle>, wit::AccessError> {
        let scope = self.transaction(tx.rep())?;
        self.own_world(scope.world)
    }
    fn players(
        &mut self,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Vec<Resource<wit::PlayerHandle>>, wit::AccessError> {
        self.transaction(tx.rep())?;
        self.require(PluginCapability::ReadPlayers)?;
        let mut observed = observe(self.world_mut());
        observed.sort_by_key(|actor| actor.reference.uuid);
        if self.resources.entries.len() + observed.len()
            > self.api_limits.max_owned_resources as usize
        {
            return Err(wit::AccessError::LimitExceeded);
        }
        observed
            .into_iter()
            .map(|actor| self.own_player(actor.reference))
            .collect()
    }
    fn player(
        &mut self,
        tx: Resource<wit::WorldTx>,
        handle: Resource<wit::PlayerHandle>,
    ) -> Result<Resource<wit::Player>, wit::AccessError> {
        self.validate_transaction(tx.rep())?;
        let scope = self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        self.require(PluginCapability::ReadPlayers)?;
        let reference = self.stored_player(handle.rep())?;
        resolve(self.world_mut(), &reference)?;
        self.resources
            .allocate(
                Entry::Player {
                    reference,
                    scope,
                    transient: false,
                },
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
    }
    fn drop(&mut self, tx: Resource<wit::WorldTx>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&tx.rep());
        Ok(())
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostPlayerHandle for HostContext {
    fn identity(
        &mut self,
        handle: Resource<wit::PlayerHandle>,
    ) -> Result<wit::PlayerIdentity, wit::AccessError> {
        self.operation()?;
        self.stored_player(handle.rep())
            .map(|reference| reference.identity)
    }
    fn snapshot(
        &mut self,
        handle: Resource<wit::PlayerHandle>,
    ) -> Result<wit::PlayerSnapshot, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ReadPlayers)?;
        self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        let reference = self.stored_player(handle.rep())?;
        if let Ok(player) = resolve(self.world_mut(), &reference) {
            return snapshot(unsafe { self.world() }, player, &reference);
        }
        let directory = unsafe { self.world() }
            .get_resource::<ComponentDirectory>()
            .ok_or(wit::AccessError::StalePlayer)?;
        let actor = directory.actor(&reference)?;
        // A cached observation must not revive a stale actor in this same world.
        if actor.world.matches(unsafe { self.world() }) {
            return Err(wit::AccessError::StalePlayer);
        }
        actor.snapshot.clone()
    }
    fn schedule(
        &mut self,
        handle: Resource<wit::PlayerHandle>,
        callback_id: u64,
    ) -> Result<Resource<wit::Task>, wit::AccessError> {
        self.operation()?;
        let target =
            super::super::tasks::TaskTarget::Player(Box::new(self.stored_player(handle.rep())?));
        self.schedule_task(target, callback_id)
    }
    fn drop(&mut self, handle: Resource<wit::PlayerHandle>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&handle.rep());
        Ok(())
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostWorldHandle for HostContext {
    fn dimension(&mut self, handle: Resource<wit::WorldHandle>) -> Result<i32, wit::AccessError> {
        self.operation()?;
        self.stored_world(handle.rep()).map(|world| world.dimension)
    }
    fn schedule(
        &mut self,
        handle: Resource<wit::WorldHandle>,
        callback_id: u64,
    ) -> Result<Resource<wit::Task>, wit::AccessError> {
        self.operation()?;
        let target = super::super::tasks::TaskTarget::World(self.stored_world(handle.rep())?);
        self.schedule_task(target, callback_id)
    }
    fn drop(&mut self, handle: Resource<wit::WorldHandle>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&handle.rep());
        Ok(())
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostTask for HostContext {
    fn id(&mut self, task: Resource<wit::Task>) -> Result<u64, wit::AccessError> {
        self.operation()?;
        match self.resources.entries.get(&task.rep()) {
            Some(Entry::Task(task)) => Ok(task.id),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn status(&mut self, task: Resource<wit::Task>) -> Result<wit::TaskStatus, wit::AccessError> {
        self.operation()?;
        match self.resources.entries.get(&task.rep()) {
            Some(Entry::Task(task)) => Ok(task.status()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn cancel(&mut self, task: Resource<wit::Task>) -> Result<bool, wit::AccessError> {
        self.operation()?;
        match self.resources.entries.get(&task.rep()) {
            Some(Entry::Task(task)) => Ok(task.cancel()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn drop(&mut self, task: Resource<wit::Task>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&task.rep());
        Ok(())
    }
}

impl super::super::wit_bindings::unastar::plugin::types::Host for HostContext {}

impl super::super::wit_bindings::unastar::plugin::host::Host for HostContext {
    fn get_tick_id(&mut self) -> u64 {
        unsafe { self.world() }
            .get_resource::<crate::ecs::resources::TickCounter>()
            .map_or(0, |tick| tick.current)
    }
    fn unix_time_seconds(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }
    fn players(&mut self) -> Result<Vec<Resource<wit::PlayerHandle>>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ReadPlayers)?;
        self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        let current = observe(self.world_mut());
        let mut actors = unsafe { self.world() }
            .get_resource::<ComponentDirectory>()
            .map(|directory| directory.actors.clone())
            .unwrap_or_default();
        let world_id = unsafe { self.world() }.id();
        actors.retain(|actor| actor.world.id != world_id);
        actors.extend(current);
        actors.sort_by_key(|actor| (actor.world.dimension, actor.reference.uuid));
        if self.resources.entries.len() + actors.len()
            > self.api_limits.max_owned_resources as usize
        {
            return Err(wit::AccessError::LimitExceeded);
        }
        actors
            .into_iter()
            .map(|actor| self.own_player(actor.reference))
            .collect()
    }
    fn worlds(&mut self) -> Result<Vec<Resource<wit::WorldHandle>>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ReadPlayers)?;
        self.resources.scope.ok_or(wit::AccessError::ExpiredScope)?;
        let mut worlds = unsafe { self.world() }
            .get_resource::<ComponentDirectory>()
            .map(|directory| directory.worlds.clone())
            .unwrap_or_default();
        let current = WorldReference::of(unsafe { self.world() })?;
        if !worlds.contains(&current) {
            worlds.push(current);
        }
        if self.resources.entries.len() + worlds.len()
            > self.api_limits.max_owned_resources as usize
        {
            return Err(wit::AccessError::LimitExceeded);
        }
        worlds
            .into_iter()
            .map(|world| self.own_world(world))
            .collect()
    }
    fn log(&mut self, level: wit::LogLevel, message: String) {
        if self.operation().is_err() || message.len() > self.api_limits.max_message_bytes as usize {
            return;
        }
        #[cfg(test)]
        self.test_logs.push(message.clone());
        match level {
            wit::LogLevel::Info => tracing::info!(plugin = %self.plugin_id, "{message}"),
            wit::LogLevel::Warn => tracing::warn!(plugin = %self.plugin_id, "{message}"),
            wit::LogLevel::Error => tracing::error!(plugin = %self.plugin_id, "{message}"),
        }
    }
}
