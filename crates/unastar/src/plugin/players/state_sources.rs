//! Exact native source ownership remains held independently of guest observers.

use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::player_state::quarantine;
use parking_lot::Mutex;

pub(super) enum Phase {
    Staged,
    Held(quarantine::Handle),
    Consumed {
        handle: quarantine::Handle,
        id: String,
        hash: [u8; 32],
    },
    Cancelled,
}
/// Native retention, not a guest resource handle, owns an accepted source hold.
pub(super) struct Fence {
    pub(super) id: uuid::Uuid,
    pub(super) module: String,
    pub(super) reference: PlayerLineage,
    pub(super) world: WorldReference,
    pub(super) image: Arc<values::Value<super::state::Image>>,
    phase: Mutex<Phase>,
    _copies: Arc<values::Value<()>>,
}
impl Fence {
    /// An owned token never resolves by account, display name or historical session number.
    pub(super) fn matches(&self, world: &World) -> bool {
        self.world.matches(world)
            && match &*self.phase.lock() {
                Phase::Held(handle) | Phase::Consumed { handle, .. } => {
                    self.reference.matches(world, handle.player)
                        && quarantine::matches(world, handle)
                        && world
                            .get::<Anchor>(handle.player)
                            .is_some_and(|anchor| std::ptr::eq(anchor.0.as_ref(), self))
                }
                _ => false,
            }
    }
    /// A consumed source can reconcile its one immutable request, never grant another payload.
    pub(super) fn permits(&self, id: &str, hash: [u8; 32]) -> bool {
        match &*self.phase.lock() {
            Phase::Held(_) => true,
            Phase::Consumed {
                id: old,
                hash: old_hash,
                ..
            } => old == id && *old_hash == hash,
            _ => false,
        }
    }
    /// Mark native transfer ownership only after the coordinator accepted its durable worker.
    pub(super) fn consume(&self, id: String, hash: [u8; 32]) {
        let mut phase = self.phase.lock();
        if let Phase::Held(handle) = &*phase {
            *phase = Phase::Consumed {
                handle: handle.clone(),
                id,
                hash,
            };
        }
    }
    /// Status reports the actual holder lifetime without creating a live player capability.
    fn status(&self) -> wit::SourceFenceStatus {
        match &*self.phase.lock() {
            Phase::Staged => wit::SourceFenceStatus::Staged,
            Phase::Held(handle) | Phase::Consumed { handle, .. } if handle.is_gone() => {
                wit::SourceFenceStatus::Gone
            }
            Phase::Held(_) => wit::SourceFenceStatus::Held,
            Phase::Consumed { .. } => wit::SourceFenceStatus::Consumed,
            Phase::Cancelled => wit::SourceFenceStatus::Cancelled,
        }
    }
}
/// A real actor carries the original module-owned token through guest unloads.
#[derive(Component)]
pub(super) struct Anchor(pub(super) Arc<Fence>);

#[derive(Default)]
pub(in crate::plugin::manager) struct Pending(Vec<Arc<Fence>>);

impl HostContext {
    /// Reopening an archive can observe the same engine-held source, never recreate it.
    pub(super) fn reacquire_state_fence(
        &mut self,
        archive_rep: u32,
        tx_rep: u32,
    ) -> Result<Option<Resource<wit::SourceFence>>, wit::StateError> {
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        self.validate_transaction(tx_rep)
            .map_err(wit::StateError::Access)?;
        let archive = self
            .archived(archive_rep)
            .map_err(wit::StateError::Access)?;
        let Some(nonce) = archive.loaded.record.source_fence.as_deref() else {
            return Ok(None);
        };
        let world = unsafe { self.world() };
        if world
            .get_resource::<crate::world::WorldIdentity>()
            .is_none_or(|identity| {
                identity.id.to_string() != archive.loaded.record.world.uuid
                    || identity.dimension != archive.loaded.record.world.dimension
            })
        {
            return Err(wit::StateError::Access(wit::AccessError::WrongWorld));
        }
        let source = world.try_query::<&Anchor>().and_then(|mut query| {
            query
                .iter(world)
                .find(|anchor| anchor.0.id.to_string() == nonce)
                .map(|anchor| anchor.0.clone())
        });
        let Some(source) = source else {
            return Ok(None);
        };
        if source.module != self.plugin_id.0
            || !source.matches(world)
            || source.image.expected.as_ref() != &archive.loaded.record.state
        {
            return Err(wit::StateError::RecoveryRequired);
        }
        self.resources
            .allocate(
                Entry::SourceFence(source),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
            .map(Some)
            .map_err(wit::StateError::Access)
    }
    /// Validate a genuine current raw source and reserve all native holding bytes first.
    pub(super) fn stage_state_fence(
        &mut self,
        player_rep: u32,
        image_rep: u32,
    ) -> Result<Resource<wit::SourceFence>, wit::StateError> {
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        if !crate::server::game::player_state::restoration::available(unsafe { self.world() }) {
            return Err(wit::StateError::NotInstalled);
        }
        let (reference, player) = self
            .live_player(player_rep)
            .map_err(wit::StateError::Access)?;
        if !matches!(
            availability(unsafe { self.world() }, player),
            wit::PlayerAvailability::Ready
                | wit::PlayerAvailability::Retained
                | wit::PlayerAvailability::Dead
        ) {
            return Err(wit::StateError::Access(wit::AccessError::Unavailable));
        }
        let image = self
            .state_image(image_rep)
            .map_err(wit::StateError::Access)?;
        let (original, world) = image.live_binding().map_err(wit::StateError::Access)?;
        if original.id != reference.id
            || !world.matches(unsafe { self.world() })
            || image.candidate != image.expected
        {
            return Err(wit::StateError::InvalidImage);
        }
        let copies = self
            .resources
            .value_budget
            .own((), image.bytes())
            .map_err(wit::StateError::Access)?;
        let source = Arc::new(Fence {
            id: uuid::Uuid::new_v4(),
            module: self.plugin_id.0.clone(),
            reference,
            world,
            image,
            phase: Mutex::new(Phase::Staged),
            _copies: copies,
        });
        let resource = self
            .resources
            .allocate(
                Entry::SourceFence(source.clone()),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
            .map_err(wit::StateError::Access)?;
        self.state_sources.0.push(source);
        Ok(resource)
    }
    /// Resources belong to one module and never grant a replacement component a forged actor.
    pub(super) fn state_fence(&mut self, rep: u32) -> Result<Arc<Fence>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::RestorePlayerState)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::SourceFence(source)) if source.module == self.plugin_id.0 => {
                Ok(source.clone())
            }
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
}

/// A trap cancels provisional work; accepted native source holds have no implicit release.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    for source in std::mem::take(&mut context.state_sources.0) {
        if !succeeded
            || !source.world.matches(world)
            || world
                .get_resource::<crate::ecs::TickCounter>()
                .is_none_or(|tick| tick.current != source.image.captured_tick)
        {
            *source.phase.lock() = Phase::Cancelled;
            continue;
        }
        let result = resolve(world, &source.reference).and_then(|player| {
            quarantine::hold(
                world,
                player,
                source.image.expected.clone(),
                quarantine::Role::Source,
                source.clone(),
            )
            .map_err(|_| wit::AccessError::Unavailable)
        });
        match result {
            Ok(handle) => {
                let player = handle.player;
                *source.phase.lock() = Phase::Held(handle);
                world.entity_mut(player).insert(Anchor(source));
            }
            Err(_) => *source.phase.lock() = Phase::Cancelled,
        }
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostSourceFence for HostContext {
    fn status(
        &mut self,
        source: Resource<wit::SourceFence>,
    ) -> Result<wit::SourceFenceStatus, wit::AccessError> {
        Ok(self.state_fence(source.rep())?.status())
    }
    fn state_image(
        &mut self,
        source: Resource<wit::SourceFence>,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        let source = self.state_fence(source.rep())?;
        if matches!(
            source.status(),
            wit::SourceFenceStatus::Staged | wit::SourceFenceStatus::Cancelled
        ) {
            return Err(wit::AccessError::Unavailable);
        }
        self.own_state_image(source.image.clone())
    }
    fn archive(
        &mut self,
        source: Resource<wit::SourceFence>,
        key: String,
    ) -> Result<wit::ArchiveReference, wit::ArchiveError> {
        let source = self
            .state_fence(source.rep())
            .map_err(wit::ArchiveError::Access)?;
        // A genuine staged image may be persisted before the callback commits its hold.
        // Durable archive installation does not acknowledge native source withholding.
        if matches!(source.status(), wit::SourceFenceStatus::Cancelled) {
            return Err(wit::ArchiveError::Access(wit::AccessError::Unavailable));
        }
        let image = &source.image;
        let _archive_copy = self
            .resources
            .value_budget
            .reserve(image.bytes())
            .map_err(wit::ArchiveError::Access)?;
        let owner = crate::plugin::archive_store::Owner {
            authenticated_xuid: image.owner.authenticated_xuid,
            uuid: image.owner.uuid.clone(),
            name: image.owner.name.clone(),
            connection_id: image.owner.connection_id,
        };
        let world = crate::plugin::archive_store::SourceWorld {
            uuid: image.world.uuid.clone(),
            name: image.world.name.clone(),
            dimension: image.world.dimension,
        };
        let record = crate::plugin::archive_store::Record::new(
            &self.plugin_id.0,
            owner,
            world,
            image.captured_tick,
            (*image.expected).clone(),
        )
        .map_err(|_| wit::ArchiveError::InvalidSnapshot)?
        .with_source_fence(source.id);
        let reference = self.archive_call(true, |namespace| namespace.capture(&key, &record))?;
        Ok(wit::ArchiveReference {
            key: reference.key,
            revision: reference.revision,
            fingerprint: reference.fingerprint,
            owner: super::archive::identity(&reference.owner),
        })
    }
    fn drop(&mut self, source: Resource<wit::SourceFence>) -> wasmtime::Result<()> {
        // The actual native holder keeps its state and byte lease until explicit retirement.
        self.resources.entries.remove(&source.rep());
        Ok(())
    }
}
