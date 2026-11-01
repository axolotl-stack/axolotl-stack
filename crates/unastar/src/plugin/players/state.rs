//! Immutable typed state images retain original account and proof metadata internally.

use super::*;
use crate::entity::components::EffectType;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::player_state::{PlayerSnapshot as NativeSnapshot, SnapshotEffect};
use glam::DVec3;
use sha2::{Digest, Sha256};
use std::io::Write;

/// Only actual captures carry live expected-source authority; archives never manufacture it.
pub(super) struct Image {
    pub(super) binding: Option<(PlayerLineage, WorldReference)>,
    pub(super) expected: Arc<NativeSnapshot>,
    pub(super) candidate: Arc<NativeSnapshot>,
    pub(super) owner: wit::PlayerIdentity,
    pub(super) world: wit::ArchiveWorld,
    pub(super) destination: Option<WorldReference>,
    pub(super) registry: Arc<crate::registry::ItemRegistry>,
    pub(super) captured_tick: u64,
    provenance: [u8; 32],
    bytes: usize,
}
impl Image {
    /// Accepted requests account for complete original and candidate native state.
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }
    /// Only an exact immutable live observation may be the expected target or held source.
    pub(super) fn live_binding(&self) -> Result<(PlayerLineage, WorldReference), wit::AccessError> {
        self.binding
            .clone()
            .ok_or(wit::AccessError::InvalidArgument)
    }
    /// Stream complete semantics without constructing an unbounded encoded buffer.
    pub(super) fn fingerprint(&self) -> Result<[u8; 32], wit::AccessError> {
        let mut writer = HashWriter {
            hash: Sha256::new(),
            bytes: 0,
        };
        writer.hash.update(b"unastar.state.image.v1\0");
        writer.hash.update(self.provenance);
        serde_json::to_writer(&mut writer, &(&self.world.uuid, self.candidate.as_ref()))
            .map_err(|_| wit::AccessError::LimitExceeded)?;
        Ok(writer.hash.finalize().into())
    }
}
struct HashWriter {
    hash: Sha256,
    bytes: usize,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let used = self
            .bytes
            .checked_add(bytes.len())
            .filter(|bytes| *bytes <= 16 * 1024 * 1024)
            .ok_or_else(|| std::io::Error::other("state fingerprint exceeds bounded input"))?;
        self.bytes = used;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Framed original provenance remains immutable through every detached edit.
fn provenance(parts: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"unastar.state.origin.v1\0");
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.finalize().into()
}
/// Use the engine's stable stored mode IDs without exposing packet discriminants.
fn mode_id(mode: wit::GameMode) -> u8 {
    match mode {
        wit::GameMode::Survival => 0,
        wit::GameMode::Creative => 1,
        wit::GameMode::Adventure => 2,
        wit::GameMode::Spectator => 6,
    }
}
/// Validate native stored IDs before exposing semantic guest mode values.
fn mode(id: u8) -> Result<wit::GameMode, wit::AccessError> {
    match id {
        0 => Ok(wit::GameMode::Survival),
        1 => Ok(wit::GameMode::Creative),
        2 => Ok(wit::GameMode::Adventure),
        6 => Ok(wit::GameMode::Spectator),
        _ => Err(wit::AccessError::Unavailable),
    }
}
/// Copy current raw values; no pending request or historical actor is made live.
fn values(snapshot: &NativeSnapshot) -> Result<wit::StateValues, wit::AccessError> {
    let data = &snapshot.data;
    Ok(wit::StateValues {
        position: wit::Vec3 {
            x: data.position[0],
            y: data.position[1] - crate::entity::PLAYER_EYE_HEIGHT,
            z: data.position[2],
        },
        rotation: (data.rotation[0], data.rotation[1]),
        head_yaw: snapshot.head_yaw,
        velocity: wit::Vec3 {
            x: snapshot.velocity[0],
            y: snapshot.velocity[1],
            z: snapshot.velocity[2],
        },
        health: data.health,
        max_health: data.max_health,
        absorption: snapshot.absorption,
        food: data.food as u8,
        saturation: data.saturation,
        exhaustion: data.exhaustion,
        experience: data.experience as u32,
        experience_progress: data.experience_progress,
        base_mode: mode(snapshot.base_game_mode.unwrap_or(data.game_mode))?,
        mode_override: snapshot
            .base_game_mode
            .map(|_| mode(data.game_mode))
            .transpose()?,
        flight_override: snapshot.flight_override,
        air_current: snapshot.air_current,
        air_max: snapshot.air_max,
        on_fire_ticks: snapshot.on_fire_ticks,
        burning_remaining: snapshot.burning_remaining,
        burning_next: snapshot.burning_next,
        effects: snapshot
            .effects
            .iter()
            .map(|effect| {
                Ok(wit::ActiveStatusEffect {
                    kind: super::mechanics::kind(EffectType(effect.id))?,
                    amplifier: effect.level,
                    duration_ticks: effect.duration_ticks,
                    ambient: effect.ambient,
                    show_particles: effect.show_particles,
                    infinite: effect.infinite,
                })
            })
            .collect::<Result<_, wit::AccessError>>()?,
        age_ticks: data.age_ticks,
        cold_exposure: data.cold_exposure,
        dead: snapshot.dead,
    })
}
/// Set only gameplay values. Account identifiers and inventory proof fields cannot change.
fn put_values(
    snapshot: &mut NativeSnapshot,
    values: wit::StateValues,
) -> Result<(), wit::AccessError> {
    if values.effects.len() > 36 {
        return Err(wit::AccessError::InvalidArgument);
    }
    let previous_feet =
        DVec3::from_array(snapshot.data.position) - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    let feet = DVec3::new(values.position.x, values.position.y, values.position.z);
    if previous_feet != feet {
        snapshot.data.position = (feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT).to_array();
    }
    snapshot.data.rotation = [values.rotation.0, values.rotation.1];
    snapshot.head_yaw = values.head_yaw;
    snapshot.velocity = [values.velocity.x, values.velocity.y, values.velocity.z];
    snapshot.data.health = values.health;
    snapshot.data.max_health = values.max_health;
    snapshot.absorption = values.absorption;
    snapshot.data.food = values.food.into();
    snapshot.data.saturation = values.saturation;
    snapshot.data.exhaustion = values.exhaustion;
    snapshot.data.experience =
        i32::try_from(values.experience).map_err(|_| wit::AccessError::InvalidArgument)?;
    snapshot.data.experience_progress = values.experience_progress;
    snapshot.data.game_mode = mode_id(values.mode_override.unwrap_or(values.base_mode));
    snapshot.base_game_mode = values.mode_override.map(|_| mode_id(values.base_mode));
    snapshot.flight_override = values.flight_override;
    snapshot.air_current = values.air_current;
    snapshot.air_max = values.air_max;
    snapshot.on_fire_ticks = values.on_fire_ticks;
    snapshot.burning_remaining = values.burning_remaining;
    snapshot.burning_next = values.burning_next;
    snapshot.effects = values
        .effects
        .into_iter()
        .map(|effect| SnapshotEffect {
            id: super::mechanics::native_kind(effect.kind).0,
            level: effect.amplifier,
            duration_ticks: effect.duration_ticks,
            ambient: effect.ambient,
            show_particles: effect.show_particles,
            infinite: effect.infinite,
        })
        .collect();
    snapshot.data.age_ticks = values.age_ticks;
    snapshot.data.cold_exposure = values.cold_exposure;
    snapshot.dead = values.dead;
    snapshot
        .validate()
        .map_err(|_| wit::AccessError::InvalidArgument)
}

impl HostContext {
    /// Resource ownership is bounded even when multiple references share immutable buffers.
    pub(super) fn own_state_image(
        &mut self,
        image: Arc<values::Value<Image>>,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        self.resources
            .allocate(
                Entry::StateImage(image),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
    }
    /// Detached values remain readable; only submission requires the restoration capability.
    pub(super) fn state_image(
        &mut self,
        rep: u32,
    ) -> Result<Arc<values::Value<Image>>, wit::AccessError> {
        self.operation()?;
        match self.resources.entries.get(&rep) {
            Some(Entry::StateImage(image)) => Ok(image.clone()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    /// Reserve all native copies before asking the engine for a complete current snapshot.
    pub(super) fn capture_state_image(
        &mut self,
        rep: u32,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        self.require(PluginCapability::RestorePlayerState)?;
        let (reference, player) = self.live_player(rep)?;
        let world = unsafe { self.world() };
        let origin = WorldReference::of(world)?;
        let captured_tick = world
            .get_resource::<crate::ecs::TickCounter>()
            .ok_or(wit::AccessError::Unavailable)?
            .current;
        let identity = world
            .get_resource::<crate::world::WorldIdentity>()
            .ok_or(wit::AccessError::Unavailable)?;
        if identity.id.is_nil()
            || identity.dimension != origin.dimension
            || identity.name.is_empty()
            || identity.name.len() > 128
            || identity.name.chars().any(char::is_control)
            || reference.identity.name.len() > 64
        {
            return Err(wit::AccessError::InvalidIdentity);
        }
        let mut bytes = super::inventory::borrowed_bytes(world, player)?
            .checked_mul(4)
            .ok_or(wit::AccessError::LimitExceeded)?;
        if let Some(persistence) =
            world.get::<crate::server::game::inventory_effects::InventoryPersistence>(player)
        {
            for key in persistence.receipts().keys() {
                bytes = bytes
                    .checked_add((128 + key.len()) * 2)
                    .ok_or(wit::AccessError::LimitExceeded)?;
            }
        }
        bytes = bytes
            .checked_add(reference.retained_bytes() + 16384)
            .ok_or(wit::AccessError::LimitExceeded)?;
        let reservation = self.resources.value_budget.reserve(bytes)?;
        let registry = world
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0
            .clone();
        let native = Arc::new(
            crate::server::game::player_state::capture(world, player)
                .map_err(|_| wit::AccessError::Unavailable)?,
        );
        let world = wit::ArchiveWorld {
            uuid: identity.id.to_string(),
            name: identity.name.clone(),
            dimension: identity.dimension,
        };
        let proof = provenance(&[
            b"capture",
            self.plugin_id.0.as_bytes(),
            native.data.uuid.as_bytes(),
        ]);
        self.own_state_image(reservation.bind(Image {
            binding: Some((reference.clone(), origin)),
            expected: native.clone(),
            candidate: native,
            owner: reference.identity,
            world,
            destination: Some(origin),
            registry,
            captured_tick,
            provenance: proof,
            bytes,
        }))
    }
    /// A module archive supplies immutable history, never a reconstructed live player binding.
    pub(super) fn state_from_archive(
        &mut self,
        rep: u32,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        let archive = self.archived(rep)?;
        let bytes = archive
            .loaded
            .bytes
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(16384))
            .ok_or(wit::AccessError::LimitExceeded)?;
        let reservation = self.resources.value_budget.reserve(bytes)?;
        let native = Arc::new(archive.loaded.record.state.clone());
        let source = &archive.loaded.record.world;
        let reference = &archive.loaded.reference;
        let proof = provenance(&[
            b"archive",
            self.plugin_id.0.as_bytes(),
            reference.key.as_bytes(),
            &reference.revision.to_be_bytes(),
            reference.fingerprint.as_bytes(),
        ]);
        self.own_state_image(reservation.bind(Image {
            binding: None,
            expected: native.clone(),
            candidate: native,
            owner: super::archive::identity(&archive.loaded.record.owner),
            world: wit::ArchiveWorld {
                uuid: source.uuid.clone(),
                name: source.name.clone(),
                dimension: source.dimension,
            },
            destination: None,
            registry: archive.registry.clone(),
            captured_tick: archive.loaded.record.captured_tick,
            provenance: proof,
            bytes,
        }))
    }
    /// Derivations retain original proof and account metadata while owning their native copy.
    fn derive_state(
        &mut self,
        image: &Image,
        edit: impl FnOnce(&mut NativeSnapshot) -> Result<(), wit::AccessError>,
        destination: Option<(WorldReference, wit::ArchiveWorld)>,
        extra: usize,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        let bytes = image
            .bytes()
            .checked_add(extra)
            .ok_or(wit::AccessError::LimitExceeded)?;
        let reservation = self.resources.value_budget.reserve(bytes)?;
        let mut candidate = (*image.candidate).clone();
        edit(&mut candidate)?;
        candidate
            .validate()
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        let (target, world) = destination.map_or_else(
            || (image.destination, image.world.clone()),
            |(reference, world)| (Some(reference), world),
        );
        self.own_state_image(reservation.bind(Image {
            binding: image.binding.clone(),
            expected: image.expected.clone(),
            candidate: Arc::new(candidate),
            owner: image.owner.clone(),
            world,
            destination: target,
            registry: image.registry.clone(),
            captured_tick: image.captured_tick,
            provenance: image.provenance,
            bytes,
        }))
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostPlayerStateImage for HostContext {
    fn values(
        &mut self,
        image: Resource<wit::PlayerStateImage>,
    ) -> Result<wit::StateValues, wit::AccessError> {
        values(&self.state_image(image.rep())?.candidate)
    }
    fn with_values(
        &mut self,
        image: Resource<wit::PlayerStateImage>,
        values: wit::StateValues,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        let image = self.state_image(image.rep())?;
        self.derive_state(&image, |snapshot| put_values(snapshot, values), None, 0)
    }
    fn at(
        &mut self,
        image: Resource<wit::PlayerStateImage>,
        tx: Resource<wit::WorldTx>,
        feet: wit::Vec3,
        rotation: (f32, f32),
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        self.validate_transaction(tx.rep())?;
        let image = self.state_image(image.rep())?;
        let reference = WorldReference::of(unsafe { self.world() })?;
        let native = unsafe { self.world() }
            .get_resource::<crate::world::WorldIdentity>()
            .ok_or(wit::AccessError::Unavailable)?;
        let world = wit::ArchiveWorld {
            uuid: native.id.to_string(),
            name: native.name.clone(),
            dimension: native.dimension,
        };
        self.derive_state(
            &image,
            |snapshot| {
                let feet = DVec3::new(feet.x, feet.y, feet.z);
                snapshot.data.position =
                    (feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT).to_array();
                snapshot.data.rotation = [rotation.0, rotation.1];
                snapshot.data.dimension = reference.dimension;
                Ok(())
            },
            Some((reference, world)),
            0,
        )
    }
    fn dimension(
        &mut self,
        image: Resource<wit::PlayerStateImage>,
    ) -> Result<i32, wit::AccessError> {
        Ok(self.state_image(image.rep())?.candidate.data.dimension)
    }
    fn inventory(
        &mut self,
        image: Resource<wit::PlayerStateImage>,
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        let image = self.state_image(image.rep())?;
        self.own_archived_inventory(
            &image.candidate,
            image.owner.clone(),
            image.candidate.data.dimension,
            image.registry.clone(),
        )
    }
    fn with_inventory(
        &mut self,
        image: Resource<wit::PlayerStateImage>,
        inventory: Resource<wit::InventoryImage>,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        let image = self.state_image(image.rep())?;
        let inventory = self.inventory_image(inventory.rep())?;
        let extra = inventory
            .bytes()
            .checked_mul(2)
            .ok_or(wit::AccessError::LimitExceeded)?;
        let owner = inventory.identity()?;
        if owner.uuid != image.owner.uuid
            || owner.authenticated_xuid != image.owner.authenticated_xuid
        {
            return Err(wit::AccessError::InvalidIdentity);
        }
        self.derive_state(
            &image,
            |snapshot| {
                let candidate = &inventory.candidate;
                snapshot.data.inventory = candidate.main.clone();
                snapshot.data.armour = candidate.armour.clone();
                snapshot.data.offhand = candidate.offhand.clone();
                snapshot.ui = candidate.ui.clone();
                snapshot.data.ui = candidate.ui.clone();
                snapshot.data.cursor = candidate
                    .ui
                    .first()
                    .cloned()
                    .ok_or(wit::AccessError::InvalidArgument)?;
                snapshot.data.ender_chest = candidate.ender_chest.clone();
                snapshot.data.held_slot = candidate.held_slot;
                Ok(())
            },
            None,
            extra,
        )
    }
    fn fingerprint(
        &mut self,
        image: Resource<wit::PlayerStateImage>,
    ) -> Result<String, wit::AccessError> {
        Ok(self
            .state_image(image.rep())?
            .fingerprint()?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    fn drop(&mut self, image: Resource<wit::PlayerStateImage>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&image.rep());
        Ok(())
    }
}
