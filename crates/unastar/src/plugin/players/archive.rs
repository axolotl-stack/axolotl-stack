//! Typed historical reads over module-owned immutable native captures.

use super::*;
use crate::plugin::{archive_store as store, manifest::PluginCapability};
use crate::server::game::inventory_effects::InventoryPersistence;
use crate::server::game::types::ItemRegistryResource;
use std::time::Instant;

pub(super) struct Archived {
    pub(super) loaded: store::Loaded,
    pub(super) registry: Arc<crate::registry::ItemRegistry>,
}

impl HostContext {
    /// Keep the leased archive namespace alive for the complete component instance.
    fn archive_namespace(&mut self) -> Result<store::SharedNamespace, store::ArchiveError> {
        if let Some(namespace) = &self.archive_namespace {
            return Ok(namespace.clone());
        }
        let namespace = self
            .archive_registry
            .lock()
            .map_err(|_| store::ArchiveError::IoError)?
            .namespace(&self.plugin_id.0)?;
        self.archive_namespace = Some(namespace.clone());
        Ok(namespace)
    }

    /// Share the bounded trusted I/O allowance, preserving ambiguous installation errors.
    pub(super) fn archive_call<T>(
        &mut self,
        may_write: bool,
        call: impl FnOnce(&mut store::Namespace) -> Result<T, store::ArchiveError>,
    ) -> Result<T, wit::ArchiveError> {
        self.operation().map_err(wit::ArchiveError::Access)?;
        self.require(PluginCapability::PlayerStateArchives)
            .map_err(wit::ArchiveError::Access)?;
        let start = self
            .io_budget
            .begin(Instant::now())
            .ok_or(wit::ArchiveError::QuotaExceeded)?;
        let result = self.archive_namespace().and_then(|namespace| {
            let mut namespace = namespace.lock().map_err(|_| store::ArchiveError::IoError)?;
            call(&mut namespace)
        });
        if !self.io_budget.finish(start, Instant::now()) {
            if may_write && matches!(&result, Ok(_) | Err(store::ArchiveError::Uncertain)) {
                return Err(wit::ArchiveError::Uncertain);
            }
            return result
                .map_err(error)
                .and(Err(wit::ArchiveError::QuotaExceeded));
        }
        result.map_err(error)
    }

    /// Capture only a genuine, fresh native actor; no guest-supplied state is accepted.
    pub(super) fn capture_archive(
        &mut self,
        rep: u32,
        key: String,
    ) -> Result<wit::ArchiveReference, wit::ArchiveError> {
        let (player, entity) = self.live_player(rep).map_err(wit::ArchiveError::Access)?;
        self.require(PluginCapability::PlayerStateArchives)
            .map_err(wit::ArchiveError::Access)?;
        let world = unsafe { self.world() };
        let identity = world
            .get_resource::<crate::world::WorldIdentity>()
            .ok_or(wit::ArchiveError::Access(wit::AccessError::Unavailable))?;
        let tick = world
            .get_resource::<crate::ecs::TickCounter>()
            .ok_or(wit::ArchiveError::Access(wit::AccessError::Unavailable))?
            .current;
        if player.identity.name.len() > 64 || identity.name.len() > 128 {
            return Err(wit::ArchiveError::InvalidSnapshot);
        }
        let mut bytes =
            super::inventory::borrowed_bytes(world, entity).map_err(wit::ArchiveError::Access)?;
        let ui = world
            .get::<crate::entity::components::PlayerUi>(entity)
            .ok_or(wit::ArchiveError::Access(wit::AccessError::Unavailable))?;
        // Native snapshots retain both the complete UI and the saved cursor/UI record.
        for item in ui.0.slots().iter().chain(ui.0.slots().first()) {
            bytes = bytes
                .checked_add(super::inventory::native_stack_bytes(item))
                .ok_or(wit::ArchiveError::QuotaExceeded)?;
        }
        if let Some(persistence) = world.get::<InventoryPersistence>(entity) {
            for key in persistence.receipts().keys() {
                bytes = bytes
                    .checked_add(128 + key.len())
                    .ok_or(wit::ArchiveError::QuotaExceeded)?;
            }
        }
        bytes = bytes
            .checked_add(player.retained_bytes())
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or(wit::ArchiveError::QuotaExceeded)?;
        let limit = self
            .archive_registry
            .lock()
            .map_err(|_| wit::ArchiveError::IoError)?
            .limits()
            .value_bytes;
        if bytes > limit {
            return Err(wit::ArchiveError::QuotaExceeded);
        }
        let _capture_reservation = self
            .resources
            .value_budget
            .reserve(bytes)
            .map_err(wit::ArchiveError::Access)?;
        let state = crate::server::game::player_state::capture(world, entity)
            .map_err(|_| wit::ArchiveError::Access(wit::AccessError::Unavailable))?;
        let record = store::Record::new(
            &self.plugin_id.0,
            owner(&player.identity),
            store::SourceWorld {
                uuid: identity.id.to_string(),
                name: identity.name.clone(),
                dimension: identity.dimension,
            },
            tick,
            state,
        )
        .map_err(error)?;
        self.archive_call(true, |namespace| namespace.capture(&key, &record))
            .map(|reference| reference_wit(&reference))
    }

    /// Own historical data without granting any old actor or world capability.
    fn own_archive(
        &mut self,
        loaded: store::Loaded,
    ) -> Result<Resource<wit::ArchivedPlayerState>, wit::AccessError> {
        if self.world_ptr.is_null() {
            return Err(wit::AccessError::Unavailable);
        }
        let registry = unsafe { self.world() }
            .get_resource::<ItemRegistryResource>()
            .ok_or(wit::AccessError::Unavailable)?
            .0
            .clone();
        let bytes = loaded
            .bytes
            .checked_add(8192)
            .ok_or(wit::AccessError::LimitExceeded)?;
        let value = self
            .resources
            .value_budget
            .own(Archived { loaded, registry }, bytes)?;
        let rep = self.resources.allocate(
            Entry::ArchivedState(value),
            self.api_limits.max_owned_resources,
        )?;
        Ok(Resource::new_own(rep))
    }

    /// Retained immutable reads are still charged to this instance's resource and operation bounds.
    pub(super) fn archived(
        &mut self,
        rep: u32,
    ) -> Result<Arc<values::Value<Archived>>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::PlayerStateArchives)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::ArchivedState(archive)) => Ok(archive.clone()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
}

impl super::super::wit_bindings::unastar::plugin::archives::Host for HostContext {
    fn limits(&mut self) -> wit::ArchiveLimits {
        let limits = self
            .archive_registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .limits();
        wit::ArchiveLimits {
            max_snapshot_bytes: limits.value_bytes as u64,
            max_namespace_bytes: limits.namespace_bytes,
            max_snapshots: limits.keys as u64,
        }
    }
    fn lookup(&mut self, key: String) -> Result<Option<wit::ArchiveReference>, wit::ArchiveError> {
        self.archive_call(false, |namespace| namespace.lookup(&key))
            .map(|reference| reference.as_ref().map(reference_wit))
    }
    fn read(
        &mut self,
        reference: wit::ArchiveReference,
    ) -> Result<Resource<wit::ArchivedPlayerState>, wit::ArchiveError> {
        let reference = store::Reference {
            key: reference.key,
            revision: reference.revision,
            fingerprint: reference.fingerprint,
            owner: owner(&reference.owner),
        };
        let loaded = self.archive_call(false, |namespace| namespace.read(&reference))?;
        self.own_archive(loaded).map_err(wit::ArchiveError::Access)
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostArchivedPlayerState for HostContext {
    fn rebind(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
        tx: Resource<wit::WorldTx>,
        request_id: String,
    ) -> Result<Resource<wit::SourceRebindOperation>, wit::StateError> {
        self.inspect_source_rebind(archive.rep(), tx.rep(), request_id)
    }

    fn source(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.inspect_source(archive.rep(), tx.rep())
    }
    fn death_facts(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
    ) -> Result<Option<wit::DeathFacts>, wit::AccessError> {
        let archive = self.archived(archive.rep())?;
        Ok(archive
            .loaded
            .record
            .death
            .as_ref()
            .map(super::death_archives::facts))
    }
    fn source_fence(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Option<Resource<wit::SourceFence>>, wit::StateError> {
        self.reacquire_state_fence(archive.rep(), tx.rep())
    }
    fn state_image(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        self.state_from_archive(archive.rep())
    }
    fn reference(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
    ) -> Result<wit::ArchiveReference, wit::AccessError> {
        self.archived(archive.rep())
            .map(|archive| reference_wit(&archive.loaded.reference))
    }
    fn facts(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
    ) -> Result<wit::ArchiveFacts, wit::AccessError> {
        let archive = self.archived(archive.rep())?;
        let record = &archive.loaded.record;
        let state = &record.state;
        let data = &state.data;
        Ok(wit::ArchiveFacts {
            owner: identity(&record.owner),
            source_world: wit::ArchiveWorld {
                uuid: record.world.uuid.clone(),
                name: record.world.name.clone(),
                dimension: record.world.dimension,
            },
            captured_tick: record.captured_tick,
            position: wit::Vec3 {
                x: data.position[0],
                y: data.position[1] - crate::entity::PLAYER_EYE_HEIGHT,
                z: data.position[2],
            },
            rotation: (data.rotation[0], data.rotation[1]),
            velocity: wit::Vec3 {
                x: state.velocity[0],
                y: state.velocity[1],
                z: state.velocity[2],
            },
            health: data.health,
            max_health: data.max_health,
            absorption: state.absorption,
            food: data.food as u8,
            saturation: data.saturation,
            exhaustion: data.exhaustion,
            experience: data.experience as u32,
            experience_progress: data.experience_progress,
            base_game_mode: mode(state.base_game_mode.unwrap_or(data.game_mode))?,
            effective_game_mode: mode(data.game_mode)?,
            dead: state.dead,
        })
    }
    fn inventory(
        &mut self,
        archive: Resource<wit::ArchivedPlayerState>,
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        let archive = self.archived(archive.rep())?;
        let record = &archive.loaded.record;
        self.own_archived_inventory(
            &record.state,
            identity(&record.owner),
            record.world.dimension,
            archive.registry.clone(),
        )
    }
    fn drop(&mut self, archive: Resource<wit::ArchivedPlayerState>) -> wasmtime::Result<()> {
        if !matches!(
            self.resources.entries.get(&archive.rep()),
            Some(Entry::ArchivedState(_))
        ) {
            anyhow::bail!("invalid archived state resource");
        }
        self.resources.entries.remove(&archive.rep());
        Ok(())
    }
}

/// Preserve semantic modes while rejecting unsupported stored values.
fn mode(mode: u8) -> Result<wit::GameMode, wit::AccessError> {
    match mode {
        0 => Ok(wit::GameMode::Survival),
        1 => Ok(wit::GameMode::Creative),
        2 => Ok(wit::GameMode::Adventure),
        6 => Ok(wit::GameMode::Spectator),
        _ => Err(wit::AccessError::InvalidArgument),
    }
}
/// Copy only verified original identity fields into the native archive envelope.
fn owner(identity: &wit::PlayerIdentity) -> store::Owner {
    store::Owner {
        uuid: identity.uuid.clone(),
        authenticated_xuid: identity.authenticated_xuid,
        name: identity.name.clone(),
        connection_id: identity.connection_id,
    }
}
/// Historical identity never resolves an actor or manufactures a live capability.
pub(super) fn identity(owner: &store::Owner) -> wit::PlayerIdentity {
    wit::PlayerIdentity {
        uuid: owner.uuid.clone(),
        authenticated_xuid: owner.authenticated_xuid,
        name: owner.name.clone(),
        connection_id: owner.connection_id,
    }
}
/// Native buffers remain behind the host; only the immutable bounded reference crosses WIT.
fn reference_wit(reference: &store::Reference) -> wit::ArchiveReference {
    wit::ArchiveReference {
        key: reference.key.clone(),
        revision: reference.revision,
        fingerprint: reference.fingerprint.clone(),
        owner: identity(&reference.owner),
    }
}
/// Keep disk errors typed without leaking native fields or host paths.
fn error(error: store::ArchiveError) -> wit::ArchiveError {
    match error {
        store::ArchiveError::InvalidKey => wit::ArchiveError::InvalidKey,
        store::ArchiveError::Conflict => wit::ArchiveError::Conflict,
        store::ArchiveError::NotFound => wit::ArchiveError::NotFound,
        store::ArchiveError::InvalidReference => wit::ArchiveError::InvalidReference,
        store::ArchiveError::InvalidSnapshot => wit::ArchiveError::InvalidSnapshot,
        store::ArchiveError::QuotaExceeded => wit::ArchiveError::QuotaExceeded,
        store::ArchiveError::IoError => wit::ArchiveError::IoError,
        store::ArchiveError::Uncertain => wit::ArchiveError::Uncertain,
    }
}
