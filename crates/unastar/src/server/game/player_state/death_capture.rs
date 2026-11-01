//! Bounded genuine raw evidence captured after death commits and before ordinary drops.

use super::*;
use std::sync::Arc;

const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;

/// Incomplete and over-limit evidence remain distinct without exposing encoded state.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Error {
    Unavailable,
    TooLarge,
    Invalid,
}

/// Native evidence owns its source registry and world facts independently of a live actor.
pub(crate) struct Capture {
    pub snapshot: Arc<PlayerSnapshot>,
    pub world: crate::world::WorldIdentity,
    pub tick: u64,
    pub registry: Arc<crate::registry::ItemRegistry>,
    pub bytes: usize,
    pub nonce: uuid::Uuid,
}
impl std::fmt::Debug for Capture {
    /// Diagnostics describe ownership without formatting complete item buffers.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeathStateCapture")
            .field("tick", &self.tick)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

/// Estimate borrowed dynamic data before the full snapshot clones any item or receipt buffers.
fn estimate(world: &World, player: Entity) -> Result<usize, Error> {
    let main = world
        .get::<MainInventory>(player)
        .ok_or(Error::Unavailable)?;
    let armour = world
        .get::<ArmourInventory>(player)
        .ok_or(Error::Unavailable)?;
    let offhand = world.get::<OffhandSlot>(player).ok_or(Error::Unavailable)?;
    let ui = world.get::<PlayerUi>(player).ok_or(Error::Unavailable)?;
    let ender = world
        .get::<EnderChestInventory>(player)
        .ok_or(Error::Unavailable)?;
    let mut bytes = 16384usize;
    for item in main
        .0
        .slots()
        .iter()
        .chain(armour.slots())
        .chain(std::iter::once(&offhand.0))
        .chain(ui.0.slots())
        .chain(ui.0.slots())
        .chain(ui.0.slots().first())
        .chain(ender.0.slots())
    {
        bytes = bytes
            .checked_add(128)
            .and_then(|bytes| bytes.checked_add(item.item_id().len()))
            .and_then(|bytes| bytes.checked_add(item.nbt.as_deref().map_or(0, <[u8]>::len)))
            .filter(|bytes| *bytes <= MAX_CAPTURE_BYTES)
            .ok_or(Error::TooLarge)?;
    }
    if let Some(persistence) =
        world.get::<super::super::inventory_effects::InventoryPersistence>(player)
    {
        for key in persistence.receipts().keys() {
            bytes = bytes
                .checked_add(256)
                .and_then(|bytes| bytes.checked_add(key.len().saturating_mul(2)))
                .filter(|bytes| *bytes <= MAX_CAPTURE_BYTES)
                .ok_or(Error::TooLarge)?;
        }
    }
    Ok(bytes)
}

/// Capture the genuine committed dead state before drops or observers may remove the source.
pub(crate) fn capture(world: &World, player: Entity) -> Result<Arc<Capture>, Error> {
    if !crate::plugin::manager::readiness::death_capture_requested(world) {
        return Err(Error::Unavailable);
    }
    let source = world
        .get_resource::<crate::world::WorldIdentity>()
        .ok_or(Error::Unavailable)?;
    if source.id.is_nil()
        || source.name.is_empty()
        || source.name.len() > 128
        || source.name.chars().any(char::is_control)
    {
        return Err(Error::Invalid);
    }
    let bytes = estimate(world, player)?;
    let snapshot = super::capture(world, player).map_err(|_| Error::Invalid)?;
    if !snapshot.dead || snapshot.data.health != 0.0 || snapshot.data.dimension != source.dimension
    {
        return Err(Error::Invalid);
    }
    Ok(Arc::new(Capture {
        snapshot: Arc::new(snapshot),
        world: source.clone(),
        tick: world
            .get_resource::<TickCounter>()
            .ok_or(Error::Unavailable)?
            .current,
        registry: world
            .get_resource::<super::super::types::ItemRegistryResource>()
            .ok_or(Error::Unavailable)?
            .0
            .clone(),
        bytes,
        nonce: uuid::Uuid::new_v4(),
    }))
}
