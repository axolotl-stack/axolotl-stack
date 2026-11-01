//! Owned inventory observations and candidate images, bound to their original actor.

use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::inventory_effects::InventoryState;
use crate::server::game::types::ItemRegistryResource;
use crate::storage::StoredItemStack;

/// A candidate retains an immutable expected image that guests cannot manufacture.
#[derive(Clone)]
pub(super) struct Image {
    pub(super) player: PlayerLineage,
    pub(super) world: WorldReference,
    pub(super) expected: Arc<InventoryState>,
    pub(super) candidate: Arc<InventoryState>,
    registry: Arc<crate::registry::ItemRegistry>,
}

/// Count retained dynamic data conservatively even when immutable buffers are shared.
fn state_bytes(state: &InventoryState) -> usize {
    256 + state
        .main
        .iter()
        .chain(state.armour.iter())
        .chain(std::iter::once(&state.offhand))
        .chain(state.ui.iter())
        .chain(state.ender_chest.iter())
        .map(|item| 128 + item.item_id.len() + item.nbt.as_ref().map_or(0, Vec::len))
        .sum::<usize>()
}

impl Image {
    /// Immutable images remain bounded independently of guest linear memory.
    fn bytes(&self) -> usize {
        state_bytes(&self.expected) + state_bytes(&self.candidate) + self.player.retained_bytes()
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
        let expected = Arc::new(
            InventoryState::capture(world, entity).map_err(|_| wit::AccessError::Unavailable)?,
        );
        let image = Image {
            player,
            world: WorldReference::of(world)?,
            candidate: expected.clone(),
            expected,
            registry: world
                .get_resource::<ItemRegistryResource>()
                .ok_or(wit::AccessError::Unavailable)?
                .0
                .clone(),
        };
        self.own_inventory(image)
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
    ) -> Result<Resource<wit::InventoryImage>, wit::AccessError> {
        image
            .expected
            .validate()
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        image
            .candidate
            .validate()
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        let bytes = image.bytes();
        let value = self.resources.value_budget.own(image, bytes)?;
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
            .map(|image| image.player.identity.clone())
    }
    fn dimension(&mut self, image: Resource<wit::InventoryImage>) -> Result<i32, wit::AccessError> {
        self.inventory_image(image.rep())
            .map(|image| image.world.dimension)
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
        let mut image = Image::clone(&original);
        let item = self.item_value(item.rep())?;
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
        self.own_inventory(image)
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
        let mut image = Image::clone(&original);
        Arc::make_mut(&mut image.candidate).held_slot = slot;
        self.own_inventory(image)
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
