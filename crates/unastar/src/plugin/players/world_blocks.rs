//! Typed loaded terrain observations and callback-committed literal replacement batches.

use super::{Entry, HostContext, WorldReference, values, wit};
use crate::plugin::manifest::PluginCapability;
use crate::registry::BlockRegistry;
use crate::server::game::types::BlockRegistryResource;
use crate::server::game::{container_cell_locked, container_has_decoded_state};
use crate::world::chunk::BlockStateSpace;
use crate::world::ecs::{
    ChunkData, ChunkManager, ChunkState, world_to_chunk_coords, world_to_local_coords,
};
use bevy_ecs::prelude::*;
use glam::IVec3;
use std::sync::Arc;
use wasmtime::component::Resource;

#[path = "world_blocks/publication.rs"]
mod publication;
pub(crate) use publication::apply_pending;
pub(in crate::plugin::manager) use publication::{Request, RequestState, finish};

type Error = wit::WorldBlockError;
type OwnedCell = Arc<values::Value<Cell>>;
type OwnedReplacement = Arc<values::Value<Replacement>>;

/// Fixed owned cells contain no copied actor buffers or registry tables.
const CELL_BYTES: usize = 512;
const REPLACEMENT_BYTES: usize = 128;
const MAX_CELLS: usize = 512;

/// Closed authored support covers literal terrain with tested native ownership paths.
fn supported(identifier: &str) -> bool {
    matches!(
        identifier,
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
}

/// Exact primary/secondary native identities; neither one is a wire/network ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layers {
    primary: u32,
    secondary: u32,
}

/// Immutable observation and host-owned world/chunk witness.
pub(super) struct Cell {
    world: WorldReference,
    chunk: Entity,
    position: IVec3,
    layers: Layers,
    facts: wit::BlockFacts,
    registry: Arc<BlockRegistry>,
}

/// Detached candidates share their already charged immutable expected observations.
struct Replacement {
    expected: OwnedCell,
    proposed: Layers,
}

/// A builder may be retained, but application always needs a fresh transaction.
pub(super) struct Edit {
    world: WorldReference,
    cells: Vec<OwnedReplacement>,
    staged: bool,
}

/// Resolve only a real, loaded, unlocked cell in the actual configured bounds.
fn physical(world: &World, position: IVec3) -> Result<(Entity, Layers, bool, bool), Error> {
    let manager = world
        .get_resource::<ChunkManager>()
        .ok_or(Error::Access(wit::AccessError::Unavailable))?;
    let (x, z) = world_to_chunk_coords(position.x, position.z);
    if !manager
        .world_config()
        .bounds
        .contains(crate::world::ChunkPos::new(x, z))
    {
        return Err(Error::OutOfBounds);
    }
    let chunk = manager.get_by_coords(x, z).ok_or(Error::Unloaded)?;
    if world.get::<ChunkState>(chunk) != Some(&ChunkState::Loaded) {
        return Err(Error::Unloaded);
    }
    let data = world.get::<ChunkData>(chunk).ok_or(Error::Unloaded)?;
    if !data.inner.block_bounds().contains(&position.y) {
        return Err(Error::OutOfBounds);
    }
    if data.inner.state_space() != BlockStateSpace::Bedrock {
        return Err(Error::Unsupported);
    }
    if container_cell_locked(world, position) {
        return Err(Error::Reserved);
    }
    let (lx, ly, lz) = world_to_local_coords(position.x, position.y, position.z);
    Ok((
        chunk,
        Layers {
            primary: data.inner.get_block_layer(lx, ly, lz, 0),
            secondary: data.inner.get_block_layer(lx, ly, lz, 1),
        },
        data.inner
            .block_entities()
            .iter()
            .any(|actor| actor.position() == position.to_array()),
        container_has_decoded_state(world, position),
    ))
}

impl Cell {
    /// Match current physical content, native registry and the original chunk lifetime.
    fn current(&self, world: &World) -> Result<(), Error> {
        if !self.world.matches(world) {
            return Err(Error::Access(wit::AccessError::WrongWorld));
        }
        if !world
            .get_resource::<BlockRegistryResource>()
            .is_some_and(|registry| Arc::ptr_eq(&registry.0, &self.registry))
        {
            return Err(Error::Conflict);
        }
        let (chunk, layers, actor, cached) = physical(world, self.position)?;
        if chunk != self.chunk || layers != self.layers || actor || cached {
            return Err(Error::Conflict);
        }
        Ok(())
    }
}

/// Capture native observations without generation, inventory materialization or pair/loot repair.
pub(super) fn capture(
    context: &mut HostContext,
    tx: u32,
    position: wit::BlockPos,
) -> Result<Resource<wit::BlockCell>, Error> {
    let scope = context.transaction(tx).map_err(Error::Access)?;
    context
        .require(PluginCapability::ReadBlocks)
        .map_err(Error::Access)?;
    let world = unsafe { context.world() };
    let position = IVec3::new(position.x, position.y, position.z);
    let (chunk, layers, actor, cached) = physical(world, position)?;
    let registry = world
        .get_resource::<BlockRegistryResource>()
        .ok_or(Error::Access(wit::AccessError::Unavailable))?
        .0
        .clone();
    let block = registry
        .get_by_runtime_id(layers.primary)
        .ok_or(Error::Access(wit::AccessError::Unavailable))?;
    let secondary = registry
        .get_by_runtime_id(layers.secondary)
        .ok_or(Error::Access(wit::AccessError::Unavailable))?;
    let facts = wit::BlockFacts {
        solid: block.data.solid,
        blocks_motion: block.data.blocks_motion,
        liquid: block.data.liquid,
        replaceable: crate::server::game::is_replaceable(block, layers.primary),
        actor_present: actor,
        edit_support: if actor || cached {
            wit::BlockEditSupport::BlockActor
        } else if supported(&block.string_id) && secondary.string_id == "minecraft:air" {
            wit::BlockEditSupport::Supported
        } else {
            wit::BlockEditSupport::UnsupportedFamily
        },
    };
    let cell = context
        .resources
        .value_budget
        .own(
            Cell {
                world: scope.world,
                chunk,
                position,
                layers,
                facts,
                registry,
            },
            CELL_BYTES,
        )
        .map_err(Error::Access)?;
    context
        .resources
        .allocate(
            Entry::BlockCell(cell),
            context.api_limits.max_owned_resources,
        )
        .map(Resource::new_own)
        .map_err(Error::Access)
}

/// Construct a detached builder without touching terrain or acquiring a cell reservation.
pub(super) fn edit(context: &mut HostContext, tx: u32) -> Result<Resource<wit::BlockEdit>, Error> {
    let scope = context.transaction(tx).map_err(Error::Access)?;
    context
        .require(PluginCapability::WriteBlocks)
        .map_err(Error::Access)?;
    context
        .resources
        .allocate(
            Entry::BlockEdit(Edit {
                world: scope.world,
                cells: Vec::new(),
                staged: false,
            }),
            context.api_limits.max_owned_resources,
        )
        .map(Resource::new_own)
        .map_err(Error::Access)
}

/// Resolve an exact complete property assignment in the frozen native registry.
fn resolve(registry: &BlockRegistry, value: &wit::BlockValue) -> Result<u32, Error> {
    if value.identifier.len() > 256 || value.properties.len() > 64 {
        return Err(Error::Access(wit::AccessError::InvalidArgument));
    }
    if !supported(&value.identifier) {
        return Err(Error::Unsupported);
    }
    let block = registry
        .get_by_name(&value.identifier)
        .ok_or(Error::Unsupported)?;
    if value.properties.len() != block.data.properties.len() {
        return Err(Error::Access(wit::AccessError::InvalidArgument));
    }
    let mut properties = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for property in &value.properties {
        if property.name.len() > 128 || !names.insert(&property.name) {
            return Err(Error::Access(wit::AccessError::InvalidArgument));
        }
        let definition = block
            .data
            .properties
            .iter()
            .find(|native| native.name == property.name)
            .ok_or(Error::Access(wit::AccessError::InvalidArgument))?;
        let native = definition
            .values
            .iter()
            .copied()
            .find(|native| match (&property.value, native) {
                (wit::BlockPropertyValue::Byte(a), crate::registry::block::StateValue::Byte(b)) => {
                    a == b
                }
                (
                    wit::BlockPropertyValue::Integer(a),
                    crate::registry::block::StateValue::Int(b),
                ) => a == b,
                (wit::BlockPropertyValue::Text(a), crate::registry::block::StateValue::Str(b)) => {
                    a.len() <= 256 && a == b
                }
                _ => false,
            })
            .ok_or(Error::Access(wit::AccessError::InvalidArgument))?;
        properties.push((definition.name, native));
    }
    (block.min_state_id..=block.max_state_id)
        .find(|state| {
            properties
                .iter()
                .all(|(name, value)| block.state_value(*state, name) == Some(*value))
        })
        .ok_or(Error::Access(wit::AccessError::InvalidArgument))
}

/// Issue one owned request only after the whole builder, scope and quotas validate.
pub(super) fn stage(
    context: &mut HostContext,
    tx: u32,
    edit: u32,
) -> Result<Resource<wit::BlockEditRequest>, Error> {
    let scope = context.transaction(tx).map_err(Error::Access)?;
    context
        .require(PluginCapability::WriteBlocks)
        .map_err(Error::Access)?;
    let candidate = match context.resources.entries.get(&edit) {
        Some(Entry::BlockEdit(edit)) => edit,
        _ => return Err(Error::Access(wit::AccessError::InvalidArgument)),
    };
    if candidate.staged {
        return Err(Error::AlreadyStaged);
    }
    if candidate.world != scope.world {
        return Err(Error::Access(wit::AccessError::WrongWorld));
    }
    if candidate.cells.is_empty() {
        return Err(Error::EmptyBatch);
    }
    if context.resources.block_edits >= context.api_limits.max_block_edits_per_tick.min(128) {
        return Err(Error::Access(wit::AccessError::LimitExceeded));
    }
    for replacement in &candidate.cells {
        replacement.expected.current(unsafe { context.world() })?;
    }
    let reservation = publication::Count::acquire(
        &context.block_edit_count,
        1,
        context.api_limits.max_pending_block_edits.min(4096),
    )
    .map_err(Error::Access)?;
    let state = Arc::new(RequestState::new());
    let rep = context
        .resources
        .allocate(
            Entry::BlockEditRequest(state.clone()),
            context.api_limits.max_owned_resources,
        )
        .map_err(Error::Access)?;
    let Some(Entry::BlockEdit(candidate)) = context.resources.entries.get_mut(&edit) else {
        unreachable!()
    };
    candidate.staged = true;
    let request = Request::new(
        state,
        scope.world,
        std::mem::take(&mut candidate.cells),
        Arc::downgrade(&context.command_owner),
        reservation,
    );
    context.resources.block_edits += 1;
    context.pending_block_edits.push(request);
    Ok(Resource::new_own(rep))
}

impl HostContext {
    /// Resource lookup remains owned-data access, independently of live world availability.
    fn block_cell(&mut self, rep: u32) -> Result<OwnedCell, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ReadBlocks)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::BlockCell(cell)) => Ok(cell.clone()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
}

impl wit::HostBlockCell for HostContext {
    fn position(
        &mut self,
        cell: Resource<wit::BlockCell>,
    ) -> Result<wit::BlockPos, wit::AccessError> {
        let position = self.block_cell(cell.rep())?.position;
        Ok(wit::BlockPos {
            x: position.x,
            y: position.y,
            z: position.z,
        })
    }
    fn layers(
        &mut self,
        cell: Resource<wit::BlockCell>,
    ) -> Result<wit::BlockLayers, wit::AccessError> {
        let cell = self.block_cell(cell.rep())?;
        Ok(wit::BlockLayers {
            primary: super::super::blocks::registry_value(&cell.registry, cell.layers.primary)?,
            secondary: super::super::blocks::registry_value(&cell.registry, cell.layers.secondary)?,
        })
    }
    fn facts(
        &mut self,
        cell: Resource<wit::BlockCell>,
    ) -> Result<wit::BlockFacts, wit::AccessError> {
        Ok(self.block_cell(cell.rep())?.facts)
    }
    fn drop(&mut self, cell: Resource<wit::BlockCell>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&cell.rep());
        Ok(())
    }
}

impl wit::HostBlockEdit for HostContext {
    fn replace(
        &mut self,
        edit: Resource<wit::BlockEdit>,
        expected: Resource<wit::BlockCell>,
        proposed: wit::BlockLayers,
    ) -> Result<(), Error> {
        self.operation().map_err(Error::Access)?;
        self.require(PluginCapability::WriteBlocks)
            .map_err(Error::Access)?;
        let cell = self.block_cell(expected.rep()).map_err(Error::Access)?;
        if cell.facts.edit_support != wit::BlockEditSupport::Supported {
            return Err(Error::Unsupported);
        }
        let primary = resolve(&cell.registry, &proposed.primary)?;
        if proposed.secondary.identifier != "minecraft:air" {
            return Err(Error::Unsupported);
        }
        let secondary = resolve(&cell.registry, &proposed.secondary)?;
        let maximum = (self.api_limits.max_block_cells as usize).min(MAX_CELLS);
        let Some(Entry::BlockEdit(candidate)) = self.resources.entries.get(&edit.rep()) else {
            return Err(Error::Access(wit::AccessError::InvalidArgument));
        };
        if candidate.staged {
            return Err(Error::AlreadyStaged);
        }
        if candidate.world != cell.world {
            return Err(Error::Access(wit::AccessError::WrongWorld));
        }
        if candidate.cells.len() >= maximum {
            return Err(Error::Access(wit::AccessError::LimitExceeded));
        }
        if candidate
            .cells
            .iter()
            .any(|previous| previous.expected.position == cell.position)
        {
            return Err(Error::Conflict);
        }
        let replacement = self
            .resources
            .value_budget
            .own(
                Replacement {
                    expected: cell,
                    proposed: Layers { primary, secondary },
                },
                REPLACEMENT_BYTES,
            )
            .map_err(Error::Access)?;
        let Some(Entry::BlockEdit(candidate)) = self.resources.entries.get_mut(&edit.rep()) else {
            unreachable!()
        };
        candidate.cells.push(replacement);
        Ok(())
    }
    fn drop(&mut self, edit: Resource<wit::BlockEdit>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&edit.rep());
        Ok(())
    }
}

impl wit::HostBlockEditRequest for HostContext {
    fn status(
        &mut self,
        request: Resource<wit::BlockEditRequest>,
    ) -> Result<wit::BlockEditStatus, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::WriteBlocks)?;
        match self.resources.entries.get(&request.rep()) {
            Some(Entry::BlockEditRequest(state)) => Ok(state.status()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn cancel(
        &mut self,
        request: Resource<wit::BlockEditRequest>,
    ) -> Result<bool, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::WriteBlocks)?;
        match self.resources.entries.get(&request.rep()) {
            Some(Entry::BlockEditRequest(state)) => Ok(state.cancel()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn drop(&mut self, request: Resource<wit::BlockEditRequest>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&request.rep());
        Ok(())
    }
}
