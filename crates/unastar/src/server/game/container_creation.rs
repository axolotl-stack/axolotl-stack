//! Typed new container actors; callers never supply actor NBT or replace existing inventories.

use crate::{
    block_entity::{BlockContainer, ContainerKind},
    entity::components::Inventory,
    item::ItemStack,
    registry::{BlockRegistry, ItemRegistry},
    storage::column_writer::ColumnWriteError,
    world::{
        block_entity::BlockEntity,
        ecs::{ChunkData, ChunkManager, world_to_local_coords},
    },
};
use bevy_ecs::prelude::*;
use glam::IVec3;
use std::sync::Arc;

/// Native-owned actor and cache data travel with their acknowledged column image.
#[derive(Clone)]
pub(crate) struct Creation {
    pub position: IVec3,
    pub state: u32,
    pub actor: BlockEntity,
    pub decoded: BlockContainer,
    registry: Arc<BlockRegistry>,
    items: Arc<ItemRegistry>,
}
impl Creation {
    /// Construct only supported single containers, with fresh slot identities.
    pub(crate) fn new(
        kind: ContainerKind,
        position: IVec3,
        state: u32,
        slots: Vec<ItemStack>,
        registry: Arc<BlockRegistry>,
        items: Arc<ItemRegistry>,
    ) -> Result<Self, ColumnWriteError> {
        if (kind == ContainerKind::Chest
            && (position.x.checked_add(1).is_none()
                || position.x.checked_sub(1).is_none()
                || position.z.checked_add(1).is_none()
                || position.z.checked_sub(1).is_none()))
            || !matches!(kind, ContainerKind::Barrel | ContainerKind::Chest)
            || slots.len() != 27
            || registry
                .get_by_runtime_id(state)
                .and_then(|block| ContainerKind::from_block(&block.string_id))
                != Some(kind)
        {
            return Err(ColumnWriteError::Invalid);
        }
        if slots.iter().filter(|stack| !stack.is_empty()).any(|stack| {
            stack.damage < 0
                || stack.runtime_id < 0
                || !crate::item::enchant::valid_user_data(stack)
                || stack.item().is_none_or(|entry| {
                    stack.count > entry.max_stack_size()
                        || items
                            .item_type(stack.item_id())
                            .is_none_or(|current| !std::ptr::eq(entry, &*current))
                })
                || (stack.runtime_id != 0
                    && registry
                        .get_by_runtime_id(stack.runtime_id as u32)
                        .is_none())
        }) {
            return Err(ColumnWriteError::Invalid);
        }
        let mut decoded = BlockContainer::new(kind, position);
        decoded.items = Inventory::from_slots(
            slots
                .into_iter()
                .map(|stack| stack.with_net_id(0))
                .collect(),
        );
        let actor = BlockEntity::from_bedrock_compound(decoded.encode())
            .map_err(|_| ColumnWriteError::Invalid)?;
        Ok(Self {
            position,
            state,
            actor,
            decoded,
            registry,
            items,
        })
    }

    /// Charge both decoded slot values and the complete encoded block actor,
    /// even when immutable buffers happen to be shared with an original value.
    pub(crate) fn retained_bytes(&self) -> usize {
        4096 + self.actor.network_nbt().0.len()
            + self
                .decoded
                .items
                .slots()
                .iter()
                .map(|stack| {
                    128 + stack.item_id().len() + stack.nbt.as_ref().map_or(0, |data| data.len())
                })
                .sum::<usize>()
    }

    /// Single chests require every neighboring column loaded and actor-free;
    /// pairs are never inferred or repaired by creation.
    pub(crate) fn footprint(&self) -> Vec<IVec3> {
        let mut positions = vec![self.position];
        if self.decoded.kind == ContainerKind::Chest {
            positions.extend(
                [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
                    .map(|offset| self.position + offset),
            );
        }
        positions
    }

    /// Revalidate real cache ownership before IO and again before publication.
    pub(crate) fn available(&self, world: &World) -> bool {
        super::containers::creation_available(world, self.position)
    }

    /// Validate the complete loaded single-container footprint before storage acceptance.
    pub(crate) fn source_valid(&self, world: &World) -> bool {
        world
            .get_resource::<super::types::ItemRegistryResource>()
            .is_some_and(|items| Arc::ptr_eq(&items.0, &self.items))
            && world
                .get_resource::<super::types::BlockRegistryResource>()
                .is_some_and(|registry| Arc::ptr_eq(&registry.0, &self.registry))
            && self.available(world)
            && self.footprint().iter().all(|position| {
                if world
                    .get_resource::<super::ContainerMutationLocks>()
                    .is_some_and(|locks| locks.is_locked(*position))
                {
                    return false;
                }
                let Some(chunk) = world
                    .get_resource::<ChunkManager>()
                    .and_then(|manager| manager.get_by_coords(position.x >> 4, position.z >> 4))
                else {
                    return false;
                };
                let Some(data) = world.get::<ChunkData>(chunk) else {
                    return false;
                };
                let (x, y, z) = world_to_local_coords(position.x, position.y, position.z);
                let runtime = data.inner.get_block(x, y, z);
                let secondary = data.inner.get_block_layer(x, y, z, 1);
                let Some(block) = world
                    .get_resource::<super::types::BlockRegistryResource>()
                    .and_then(|registry| registry.0.get_by_runtime_id(runtime))
                else {
                    return false;
                };
                self.registry
                    .get_by_runtime_id(secondary)
                    .is_some_and(|block| block.string_id == "minecraft:air")
                    && matches!(
                        block.string_id.as_str(),
                        "minecraft:air"
                            | "minecraft:stone"
                            | "minecraft:cobblestone"
                            | "minecraft:dirt"
                            | "minecraft:obsidian"
                            | "minecraft:end_stone"
                            | "minecraft:netherrack"
                            | "minecraft:sand"
                            | "minecraft:gravel"
                    )
                    && !data
                        .inner
                        .block_entities()
                        .iter()
                        .any(|actor| actor.position() == position.to_array())
                    && !super::containers::has_decoded_state(world, *position)
            })
    }

    /// Install only after every terrain/actor buffer in the atomic set is present.
    pub(crate) fn install(&self, world: &mut World) {
        super::containers::install_created(world, &self.decoded);
    }

    /// Initial actor projection follows actual ACK and fence release.
    pub(crate) fn publish(&self, world: &mut World) {
        super::containers::publish_created(world, self.position);
    }

    /// Remove the temporary native-observer exemption after publication.
    pub(crate) fn finish(&self, world: &mut World) {
        super::containers::finish_created(world, self.position);
    }
}
