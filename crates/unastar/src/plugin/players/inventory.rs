//! Owned inventory observations and candidate images, bound to their original actor.

use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::inventory_effects::InventoryState;
use crate::server::game::types::ItemRegistryResource;
use crate::storage::StoredItemStack;

/// A historical archive never grants a capability for an old or replacement actor.
#[derive(Clone)]
enum Origin {
    Captured {
        player: PlayerLineage,
        world: WorldReference,
    },
    Archived {
        identity: wit::PlayerIdentity,
        dimension: i32,
    },
}

/// A candidate retains an immutable expected image that guests cannot manufacture.
#[derive(Clone)]
pub(super) struct Image {
    origin: Origin,
    pub(super) expected: Arc<InventoryState>,
    pub(super) candidate: Arc<InventoryState>,
    registry: Arc<crate::registry::ItemRegistry>,
}

/// Both borrowed and stored estimates use the same conservative per-stack charge.
pub(super) fn stack_bytes(identifier: &str, nbt: Option<&[u8]>) -> usize {
    128 + identifier.len() + nbt.map_or(0, <[u8]>::len)
}

/// A borrowed native stack uses the same charge as its stored representation.
pub(super) fn native_stack_bytes(item: &crate::item::ItemStack) -> usize {
    stack_bytes(item.item_id(), item.nbt.as_deref())
}

/// Count retained dynamic data even when immutable buffers happen to be shared.
fn state_bytes(state: &InventoryState) -> usize {
    256 + state
        .main
        .iter()
        .chain(state.armour.iter())
        .chain(std::iter::once(&state.offhand))
        .chain(state.ui.iter())
        .chain(state.ender_chest.iter())
        .map(|item| stack_bytes(&item.item_id, item.nbt.as_deref()))
        .sum::<usize>()
}

/// Check complete physical stores and estimate without cloning any native item buffer.
pub(super) fn borrowed_bytes(world: &World, entity: Entity) -> Result<usize, wit::AccessError> {
    use crate::entity::components::{
        ArmourInventory, EnderChestInventory, HeldSlot, MainInventory, OffhandSlot, PlayerUi,
    };
    if crate::server::game::inventory_effects::is_quarantined(world, entity) {
        return Err(wit::AccessError::Unavailable);
    }
    let main = world
        .get::<MainInventory>(entity)
        .ok_or(wit::AccessError::Unavailable)?;
    let armour = world
        .get::<ArmourInventory>(entity)
        .ok_or(wit::AccessError::Unavailable)?;
    let offhand = world
        .get::<OffhandSlot>(entity)
        .ok_or(wit::AccessError::Unavailable)?;
    let ui = world
        .get::<PlayerUi>(entity)
        .ok_or(wit::AccessError::Unavailable)?;
    let ender = world
        .get::<EnderChestInventory>(entity)
        .ok_or(wit::AccessError::Unavailable)?;
    world
        .get::<HeldSlot>(entity)
        .ok_or(wit::AccessError::Unavailable)?;
    main.0
        .slots()
        .iter()
        .chain(armour.slots())
        .chain(std::iter::once(&offhand.0))
        .chain(ui.0.slots())
        .chain(ender.0.slots())
        .try_fold(256usize, |used, item| {
            used.checked_add(native_stack_bytes(item))
                .ok_or(wit::AccessError::LimitExceeded)
        })
}

/// Historical images use the very same slot charges while keeping native buffers borrowed.
fn snapshot_bytes(snapshot: &crate::server::game::player_state::PlayerSnapshot) -> usize {
    let data = &snapshot.data;
    256 + data
        .inventory
        .iter()
        .chain(data.armour.iter())
        .chain(std::iter::once(&data.offhand))
        .chain(snapshot.ui.iter())
        .chain(data.ender_chest.iter())
        .map(|item| stack_bytes(&item.item_id, item.nbt.as_deref()))
        .sum::<usize>()
}

/// Cloned identity and connection strings are part of the historical image's allowance.
fn identity_bytes(identity: &wit::PlayerIdentity) -> usize {
    128 + identity.uuid.len() + identity.name.len()
}

impl Image {
    /// Immutable images remain bounded independently of guest linear memory.
    pub(super) fn bytes(&self) -> usize {
        let identity_bytes = match &self.origin {
            Origin::Captured { player, .. } => player.retained_bytes(),
            Origin::Archived { identity, .. } => identity_bytes(identity),
        };
        state_bytes(&self.expected) + state_bytes(&self.candidate) + identity_bytes
    }

    /// Ordinary live submission accepts only a genuine capture of a current actor lineage.
    pub(in crate::plugin::manager) fn captured_binding(
        &self,
    ) -> Result<(PlayerLineage, WorldReference), wit::AccessError> {
        match &self.origin {
            Origin::Captured { player, world } => Ok((player.clone(), *world)),
            Origin::Archived { .. } => Err(wit::AccessError::InvalidArgument),
        }
    }

    /// Historical identity is an observation, never a newly manufactured live capability.
    fn identity(&self) -> Result<wit::PlayerIdentity, wit::AccessError> {
        match &self.origin {
            Origin::Captured { .. } => self.captured_binding().map(|(player, _)| player.identity),
            Origin::Archived { identity, .. } => Ok(identity.clone()),
        }
    }

    /// Dimension getters retain their original source independently of world lifetime.
    fn dimension(&self) -> i32 {
        match self.origin {
            Origin::Captured { world, .. } => world.dimension,
            Origin::Archived { dimension, .. } => dimension,
        }
    }

    /// Resolve one semantic inventory area without leaking native container numbers.
    fn slots(state: &InventoryState, area: wit::InventoryArea) -> &[StoredItemStack] {
        match area {
            wit::InventoryArea::Main => &state.main,
            wit::InventoryArea::Armour => &state.armour,
            wit::InventoryArea::Offhand => std::slice::from_ref(&state.offhand),
            wit::InventoryArea::Ui => &state.ui,
            wit::InventoryArea::EnderChest => &state.ender_chest,
        }
    }

    /// Candidate writes use the same semantic area mapping as reads.
    fn slots_mut(state: &mut InventoryState, area: wit::InventoryArea) -> &mut [StoredItemStack] {
        match area {
            wit::InventoryArea::Main => &mut state.main,
            wit::InventoryArea::Armour => &mut state.armour,
            wit::InventoryArea::Offhand => std::slice::from_mut(&mut state.offhand),
            wit::InventoryArea::Ui => &mut state.ui,
            wit::InventoryArea::EnderChest => &mut state.ender_chest,
        }
    }
}

impl HostContext {
    /// Capture all authoritative areas during an exact current-player transaction.
    pub(super) fn capture_inventory(
        &mut self,
        rep: u32,
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        let (player, entity) = self.live_player(rep)?;
        self.require(PluginCapability::ReadInventory)?;
        let world = unsafe { self.world() };
        let bytes = borrowed_bytes(world, entity)?
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(player.retained_bytes()))
            .ok_or(wit::AccessError::LimitExceeded)?;
        let reservation = self.resources.value_budget.reserve(bytes)?;
        let expected = Arc::new(
            InventoryState::capture(world, entity).map_err(|_| wit::AccessError::Unavailable)?,
        );
        let image = Image {
            origin: Origin::Captured {
                player,
                world: WorldReference::of(world)?,
            },
            candidate: expected.clone(),
            expected,
            registry: world
                .get_resource::<ItemRegistryResource>()
                .ok_or(wit::AccessError::Unavailable)?
                .0
                .clone(),
        };
        self.own_inventory(image, reservation)
    }

    /// Read a genuine host archive without synthesizing a current actor or world binding.
    pub(super) fn own_archived_inventory(
        &mut self,
        snapshot: &crate::server::game::player_state::PlayerSnapshot,
        identity: wit::PlayerIdentity,
        dimension: i32,
        registry: Arc<crate::registry::ItemRegistry>,
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        self.require(PluginCapability::ReadInventory)?;
        let bytes = snapshot_bytes(snapshot)
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(identity_bytes(&identity)))
            .ok_or(wit::AccessError::LimitExceeded)?;
        let reservation = self.resources.value_budget.reserve(bytes)?;
        let data = &snapshot.data;
        let expected = Arc::new(InventoryState {
            revision: data.inventory_revision,
            main: data.inventory.clone(),
            armour: data.armour.clone(),
            offhand: data.offhand.clone(),
            ui: snapshot.ui.clone(),
            ender_chest: data.ender_chest.clone(),
            held_slot: data.held_slot,
        });
        self.own_inventory(
            Image {
                origin: Origin::Archived {
                    identity,
                    dimension,
                },
                candidate: expected.clone(),
                expected,
                registry,
            },
            reservation,
        )
    }

    /// Owned observations need no borrowed ECS world after their original capture.
    fn inventory_image(
        &mut self,
        rep: u32,
    ) -> Result<Arc<super::values::Value<Image>>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ReadInventory)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::InventoryImage(image)) => Ok(image.clone()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }

    /// Account for complete images before publishing any owned resource.
    fn own_inventory(
        &mut self,
        image: Image,
        reservation: super::values::Reservation,
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        image
            .expected
            .validate()
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        image
            .candidate
            .validate()
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        if reservation.bytes() != image.bytes() {
            return Err(wit::AccessError::InvalidArgument);
        }
        let value = reservation.bind(image);
        let rep = self.resources.allocate(
            Entry::InventoryImage(value),
            self.api_limits.max_owned_resources,
        )?;
        Ok(Resource::new_own(rep))
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostInventoryImage for HostContext {
    fn identity(
        &mut self,
        image: Resource<wit::InventoryImage>,
    ) -> Result<wit::PlayerIdentity, wit::AccessError> {
        self.inventory_image(image.rep())
            .and_then(|image| image.identity())
    }
    fn dimension(&mut self, image: Resource<wit::InventoryImage>) -> Result<i32, wit::AccessError> {
        self.inventory_image(image.rep())
            .map(|image| image.dimension())
    }
    fn revision(&mut self, image: Resource<wit::InventoryImage>) -> Result<u64, wit::AccessError> {
        self.inventory_image(image.rep())
            .map(|image| image.expected.revision)
    }
    fn held_slot(&mut self, image: Resource<wit::InventoryImage>) -> Result<u8, wit::AccessError> {
        self.inventory_image(image.rep())
            .map(|image| image.candidate.held_slot)
    }
    fn slot_count(
        &mut self,
        image: Resource<wit::InventoryImage>,
        area: wit::InventoryArea,
    ) -> Result<u32, wit::AccessError> {
        self.inventory_image(image.rep())
            .map(|image| Image::slots(&image.candidate, area).len() as u32)
    }
    fn slot(
        &mut self,
        image: Resource<wit::InventoryImage>,
        area: wit::InventoryArea,
        index: u32,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        let image = self.inventory_image(image.rep())?;
        self.require(PluginCapability::ItemValues)?;
        let stack = Image::slots(&image.candidate, area)
            .get(index as usize)
            .ok_or(wit::AccessError::InvalidArgument)?
            .clone()
            .restore(&image.registry)
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        self.own_item(stack)
    }
    fn with_slot(
        &mut self,
        image: Resource<wit::InventoryImage>,
        area: wit::InventoryArea,
        index: u32,
        item: Resource<wit::Stack>,
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        let original = self.inventory_image(image.rep())?;
        let item = self.item_value(item.rep())?;
        let old = Image::slots(&original.candidate, area)
            .get(index as usize)
            .ok_or(wit::AccessError::InvalidArgument)?;
        let bytes = original
            .bytes()
            .checked_sub(stack_bytes(&old.item_id, old.nbt.as_deref()))
            .and_then(|bytes| bytes.checked_add(stack_bytes(item.item_id(), item.nbt.as_deref())))
            .ok_or(wit::AccessError::LimitExceeded)?;
        let reservation = self.resources.value_budget.reserve(bytes)?;
        let mut image = Image::clone(&original);
        // Rebind through this snapshot's frozen registry and validate before copying.
        let stored = StoredItemStack::from(&item);
        stored
            .clone()
            .restore(&image.registry)
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        let candidate = Arc::make_mut(&mut image.candidate);
        *Image::slots_mut(candidate, area)
            .get_mut(index as usize)
            .ok_or(wit::AccessError::InvalidArgument)? = stored;
        self.own_inventory(image, reservation)
    }
    fn with_held_slot(
        &mut self,
        image: Resource<wit::InventoryImage>,
        slot: u8,
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        if slot > 8 {
            return Err(wit::AccessError::InvalidArgument);
        }
        let original = self.inventory_image(image.rep())?;
        let reservation = self.resources.value_budget.reserve(original.bytes())?;
        let mut image = Image::clone(&original);
        Arc::make_mut(&mut image.candidate).held_slot = slot;
        self.own_inventory(image, reservation)
    }
    fn drop(&mut self, image: Resource<wit::InventoryImage>) -> wasmtime::Result<()> {
        if !matches!(
            self.resources.entries.get(&image.rep()),
            Some(Entry::InventoryImage(_))
        ) {
            anyhow::bail!("invalid inventory image resource");
        }
        self.resources.entries.remove(&image.rep());
        Ok(())
    }
}

#[cfg(test)]
mod archive_origin_tests {
    use super::*;

    #[test]
    fn derived_archived_images_never_gain_a_current_actor_or_world_binding() {
        let mut registry = crate::registry::ItemRegistry::new();
        registry.load_vanilla();
        let state = Arc::new(InventoryState {
            revision: 7,
            main: vec![StoredItemStack::default(); 36],
            armour: Default::default(),
            offhand: Default::default(),
            ui: vec![StoredItemStack::default(); crate::entity::components::PLAYER_UI_SIZE],
            ender_chest: vec![
                StoredItemStack::default();
                crate::entity::components::ENDER_CHEST_SIZE
            ],
            held_slot: 1,
        });
        let original = Image {
            origin: Origin::Archived {
                identity: wit::PlayerIdentity {
                    uuid: uuid::Uuid::from_u128(1).to_string(),
                    authenticated_xuid: Some(12345),
                    name: "Original".into(),
                    connection_id: Some(3),
                },
                dimension: 2,
            },
            expected: state.clone(),
            candidate: state,
            registry: Arc::new(registry),
        };
        let mut derived = original.clone();
        Arc::make_mut(&mut derived.candidate).held_slot = 5;
        for image in [&original, &derived] {
            assert!(matches!(
                image.captured_binding(),
                Err(wit::AccessError::InvalidArgument)
            ));
            assert_eq!(image.identity().unwrap().authenticated_xuid, Some(12345));
            assert_eq!(image.dimension(), 2);
            assert_eq!(image.expected.revision, 7);
        }
        assert_eq!(original.candidate.held_slot, 1);
        assert_eq!(derived.candidate.held_slot, 5);
    }
}
