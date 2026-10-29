//! Server-side block light, for gameplay (clients light their own world).
//!
//! Every chunk column gets a [`ChunkBlockLight`] at the first
//! [`update_block_light`] after it enters the world. The column is flood
//! filled from its emitters, and the light already stored on the faces of
//! lit neighbouring columns spreads into it; its own light spreads into
//! them. From then on the column records every cell write, and the next
//! [`update_block_light`] relights around the cells whose light emission or
//! damping changed. It first unlights everything the old light could have
//! reached, then relights from the cells bordering that region and from the
//! emitters inside it, across column borders. Queries read the stored
//! nibbles and never search.
//!
//! Propagation matches BDS 1.26.51. A cell emits the larger emission of its
//! two layers (the block and its underlying liquid). Light entering a cell
//! loses that cell's larger light damping, but at least
//! [`LIGHT_MIN_ATTENUATION`]: a torch (14) lights the air next to it with
//! 13, and a full-damping block (15) lets nothing in. Light only spreads
//! between columns whose block light exists, so an unloaded neighbour
//! stays dark and pulls the light in when it loads.
//!
//! Emission and damping come from the block data. That data carries them
//! for each block's first canonical state, so separate lit and unlit blocks
//! (`lit_furnace`, `unlit_redstone_torch`, `lit_redstone_lamp`) differ. A
//! state property that changes light within one block (candles, campfires,
//! sea pickles, respawn anchors, copper bulbs) has no per-state source and
//! takes the first state's values.
//!
//! [`brightness`] is the raw brightness gameplay reads, as BDS 1.26.51 reads
//! it: 0 inside a block that damps all light, otherwise the larger of the
//! sky light after the time of day's darkening
//! ([`super::clock::sky_brightness`]) and the block light.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::chunk::{CellChange, Chunk, MIN_Y, SUBCHUNK_COUNT};
use super::clock::{SKY_LIGHT_MAX, sky_brightness_with_darken, sky_darken};
use super::ecs::systems::{world_to_chunk_coords, world_to_local_coords};
use super::ecs::{ChunkData, ChunkManager};
use crate::registry::BlockRegistry;
use crate::server::game::types::BlockRegistryResource;

/// The least light a cell takes from light entering it, also through a
/// block that damps none (confirmed).
pub const LIGHT_MIN_ATTENUATION: u8 = 1;
/// Light damping of a block that is dark inside: the brightness in it is 0
/// (confirmed).
pub const FULL_LIGHT_DAMPING: u8 = 15;

/// One above the highest block.
const MAX_Y: i32 = MIN_Y + SUBCHUNK_COUNT as i32 * 16;
const SECTIONS: usize = SUBCHUNK_COUNT as usize;
/// A section's light: one nibble per cell.
const SECTION_BYTES: usize = 2048;
/// Light levels a spread queue holds (0..=15).
const LEVELS: usize = 16;
/// The six face neighbours.
const FACES: [IVec3; 6] = [
    IVec3::X,
    IVec3::NEG_X,
    IVec3::Y,
    IVec3::NEG_Y,
    IVec3::Z,
    IVec3::NEG_Z,
];

/// The block light of one chunk column: a nibble per cell for every
/// section any light reached; a section no light reached allocates nothing.
#[derive(Component, Debug, Clone, Default)]
pub struct ChunkBlockLight {
    sections: [Option<Box<[u8; SECTION_BYTES]>>; SECTIONS],
}

impl ChunkBlockLight {
    /// The section and nibble index of local `x`/`z` at absolute `y`.
    fn index(x: u8, y: i32, z: u8) -> Option<(usize, usize)> {
        let dy = usize::try_from(y - MIN_Y).ok()?;
        (dy < SECTIONS * 16).then(|| {
            (
                dy >> 4,
                usize::from(x) | usize::from(z) << 4 | (dy & 15) << 8,
            )
        })
    }

    /// The light at local `x`/`z` and absolute `y` (0 outside the world).
    pub fn get(&self, x: u8, y: i32, z: u8) -> u8 {
        let Some((section, cell)) = Self::index(x, y, z) else {
            return 0;
        };
        self.sections[section]
            .as_ref()
            .map_or(0, |nibbles| nibbles[cell >> 1] >> ((cell & 1) * 4) & 15)
    }

    fn set(&mut self, x: u8, y: i32, z: u8, level: u8) {
        let Some((section, cell)) = Self::index(x, y, z) else {
            return;
        };
        let stored = &mut self.sections[section];
        if stored.is_none() && level == 0 {
            return;
        }
        let nibbles = stored.get_or_insert_with(|| Box::new([0; SECTION_BYTES]));
        let shift = (cell & 1) * 4;
        nibbles[cell >> 1] = nibbles[cell >> 1] & !(15 << shift) | level << shift;
    }

    /// Bytes of section storage this column allocated.
    pub fn section_bytes(&self) -> usize {
        self.sections.iter().flatten().count() * SECTION_BYTES
    }
}

/// System: light the chunk columns that entered the world since the last
/// run, then relight around the cells written since (module docs).
pub fn update_block_light(world: &mut World) {
    light_new_columns(world);
    relight_changed_cells(world);
}

/// Block light at `pos`: 0 outside the world height and in columns that
/// are not loaded or not lit yet.
pub fn block_light(world: &World, pos: IVec3) -> u8 {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world
        .get_resource::<ChunkManager>()
        .and_then(|manager| manager.get_by_coords(cx, cz))
        .and_then(|entity| world.get::<ChunkBlockLight>(entity))
        .map_or(0, |light| {
            light.get((pos.x & 15) as u8, pos.y, (pos.z & 15) as u8)
        })
}

/// Raw brightness at `pos` (0..=15) with the time of day's sky darkening.
pub fn brightness(world: &World, pos: IVec3) -> u8 {
    brightness_with_darken(world, pos, sky_darken(world))
}

/// [`brightness`] with an explicit sky darkening. Below the world it is 0;
/// above it and in unloaded columns it is the sky's; inside a block of
/// [`FULL_LIGHT_DAMPING`] it is 0; elsewhere the larger of the darkened
/// sky light and the block light.
pub fn brightness_with_darken(world: &World, pos: IVec3, darken: u8) -> u8 {
    let sky = sky_brightness_with_darken(world, pos, darken);
    if !(MIN_Y..MAX_Y).contains(&pos.y) {
        return sky;
    }
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    let Some(entity) = world
        .get_resource::<ChunkManager>()
        .and_then(|manager| manager.get_by_coords(cx, cz))
    else {
        return sky;
    };
    let Some(chunk) = world.get::<ChunkData>(entity) else {
        return sky;
    };
    let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
    let damping = registry(world)
        .get_by_runtime_id(chunk.inner.get_block(x, y, z))
        .map_or(0, |entry| entry.filter_light);
    if damping >= FULL_LIGHT_DAMPING {
        return 0;
    }
    let block = world
        .get::<ChunkBlockLight>(entity)
        .map_or(0, |light| light.get(x, pos.y, z));
    sky.max(block)
}

/// The block registry the world resolves states with.
fn registry(world: &World) -> &BlockRegistry {
    match world.get_resource::<BlockRegistryResource>() {
        Some(resource) => &resource.0,
        None => BlockRegistry::vanilla(),
    }
}

fn light_new_columns(world: &mut World) {
    let mut query = world.query_filtered::<(Entity, &ChunkData), Without<ChunkBlockLight>>();
    let fresh: Vec<(Entity, (i32, i32))> = query
        .iter(world)
        .map(|(entity, data)| (entity, (data.inner.x, data.inner.z)))
        .collect();
    for (entity, column) in fresh {
        world.entity_mut(entity).insert(ChunkBlockLight::default());
        if let Some(mut data) = world.get_mut::<ChunkData>(entity) {
            data.inner.track_cell_changes();
        }
        let indexed = world
            .get_resource::<ChunkManager>()
            .and_then(|manager| manager.get_by_coords(column.0, column.1))
            == Some(entity);
        if indexed {
            with_region(world, column, |region| region.light_column());
        }
    }
}

fn relight_changed_cells(world: &mut World) {
    let mut query = world.query_filtered::<&mut ChunkData, With<ChunkBlockLight>>();
    let mut batches: Vec<((i32, i32), Vec<CellChange>)> = Vec::new();
    for mut data in query.iter_mut(world) {
        if data.inner.has_cell_changes() {
            let changes = data.inner.take_cell_changes();
            batches.push(((data.inner.x, data.inner.z), changes));
        }
    }
    for (column, changes) in batches {
        with_region(world, column, |region| region.relight(column, &changes));
    }
}

/// Run `pass` over the 3 × 3 columns around `center`: every change within
/// a column stays within its neighbours, since light falls by at least 1
/// per block and a column is 16 wide.
fn with_region(world: &mut World, center: (i32, i32), pass: impl FnOnce(&mut Region)) {
    let base = (center.0 - 1, center.1 - 1);
    let entities: [Option<Entity>; 9] = std::array::from_fn(|slot| {
        let (dx, dz) = (slot as i32 / 3, slot as i32 % 3);
        world
            .get_resource::<ChunkManager>()?
            .get_by_coords(base.0 + dx, base.1 + dz)
    });
    let mut light: [Option<ChunkBlockLight>; 9] = std::array::from_fn(|slot| {
        let entity = entities[slot]?;
        world.get::<ChunkData>(entity)?;
        let mut stored = world.get_mut::<ChunkBlockLight>(entity)?;
        Some(std::mem::take(&mut *stored))
    });
    {
        let blocks: Arc<BlockRegistry>;
        let registry = match world.get_resource::<BlockRegistryResource>() {
            Some(resource) => {
                blocks = Arc::clone(&resource.0);
                &*blocks
            }
            None => BlockRegistry::vanilla(),
        };
        let world = &*world;
        let chunks: [Option<&Chunk>; 9] = std::array::from_fn(|slot| {
            light[slot].as_ref()?;
            world
                .get::<ChunkData>(entities[slot]?)
                .map(|data| &data.inner)
        });
        let mut region = Region {
            base,
            chunks,
            light: &mut light,
            registry,
            queues: Default::default(),
            unlight: Vec::new(),
        };
        pass(&mut region);
    }
    for (entity, light) in entities.into_iter().zip(light) {
        if let (Some(entity), Some(light)) = (entity, light)
            && let Some(mut stored) = world.get_mut::<ChunkBlockLight>(entity)
        {
            *stored = light;
        }
    }
}

/// The 3 × 3 columns a light pass reads and writes.
struct Region<'a> {
    /// Chunk coordinates of slot 0; slot `dx * 3 + dz`.
    base: (i32, i32),
    chunks: [Option<&'a Chunk>; 9],
    /// Taken out of the columns for the pass; `None` where a column is not
    /// loaded or not lit (light does not enter it).
    light: &'a mut [Option<ChunkBlockLight>; 9],
    registry: &'a BlockRegistry,
    /// Cells to spread from, by their light level.
    queues: [Vec<IVec3>; LEVELS],
    /// Cells to unlight from, with the light they had.
    unlight: Vec<(IVec3, u8)>,
}

impl Region<'_> {
    /// The slot of the lit column holding `pos`, if any.
    fn slot(&self, pos: IVec3) -> Option<usize> {
        if !(MIN_Y..MAX_Y).contains(&pos.y) {
            return None;
        }
        let dx = (pos.x >> 4) - self.base.0;
        let dz = (pos.z >> 4) - self.base.1;
        if !(0..3).contains(&dx) || !(0..3).contains(&dz) {
            return None;
        }
        let slot = (dx * 3 + dz) as usize;
        self.light[slot].is_some().then_some(slot)
    }

    fn light(&self, slot: usize, pos: IVec3) -> u8 {
        self.light[slot].as_ref().map_or(0, |light| {
            light.get((pos.x & 15) as u8, pos.y, (pos.z & 15) as u8)
        })
    }

    fn set_light(&mut self, slot: usize, pos: IVec3, level: u8) {
        if let Some(light) = &mut self.light[slot] {
            light.set((pos.x & 15) as u8, pos.y, (pos.z & 15) as u8, level);
        }
    }

    /// Emission and damping of a state.
    fn state(&self, state: u32) -> (u8, u8) {
        self.registry
            .get_by_runtime_id(state)
            .map_or((0, 0), |entry| (entry.emit_light, entry.filter_light))
    }

    /// Emission and damping of two layers: the larger of each.
    fn layers(&self, primary: u32, secondary: u32) -> (u8, u8) {
        let (a, b) = (self.state(primary), self.state(secondary));
        (a.0.max(b.0), a.1.max(b.1))
    }

    /// Emission and damping of the cell at `pos` in column `slot`.
    fn cell(&self, slot: usize, pos: IVec3) -> (u8, u8) {
        let Some(chunk) = self.chunks[slot] else {
            return (0, 0);
        };
        let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
        self.layers(chunk.get_block(x, y, z), chunk.get_block_layer(x, y, z, 1))
    }

    /// Raise `pos` to `level` and spread from it.
    fn seed(&mut self, slot: usize, pos: IVec3, level: u8) {
        if level > self.light(slot, pos) {
            self.set_light(slot, pos, level);
            self.queues[usize::from(level)].push(pos);
        }
    }

    /// Spread from `pos` with the light it holds.
    fn spread_from(&mut self, slot: usize, pos: IVec3) {
        let level = self.light(slot, pos);
        if level > LIGHT_MIN_ATTENUATION {
            self.queues[usize::from(level)].push(pos);
        }
    }

    /// Spread every queued cell, brightest first, so each cell settles once.
    fn spread(&mut self) {
        for level in (LIGHT_MIN_ATTENUATION + 1..=SKY_LIGHT_MAX).rev() {
            while let Some(pos) = self.queues[usize::from(level)].pop() {
                let Some(slot) = self.slot(pos) else {
                    continue;
                };
                if self.light(slot, pos) != level {
                    continue;
                }
                for face in FACES {
                    let next = pos + face;
                    let Some(next_slot) = self.slot(next) else {
                        continue;
                    };
                    let damping = self.cell(next_slot, next).1.max(LIGHT_MIN_ATTENUATION);
                    if level > damping {
                        self.seed(next_slot, next, level - damping);
                    }
                }
            }
        }
        for queue in &mut self.queues {
            queue.clear();
        }
    }

    /// Light the centre column from scratch: its emitters, and the light
    /// on the faces of its lit neighbours.
    fn light_column(&mut self) {
        const CENTER: usize = 4;
        let Some(chunk) = self.chunks[CENTER] else {
            return;
        };
        let origin = IVec3::new((self.base.0 + 1) << 4, MIN_Y, (self.base.1 + 1) << 4);
        for section in 0..SECTIONS {
            let emits = chunk
                .sub_chunk(section)
                .palette_states()
                .any(|state| self.state(state).0 > 0);
            if !emits {
                continue;
            }
            for i in 0..4096i32 {
                let pos = origin + IVec3::new(i & 15, (section as i32) << 4 | i >> 8, i >> 4 & 15);
                let emit = self.cell(CENTER, pos).0;
                if emit > 0 {
                    self.seed(CENTER, pos, emit);
                }
            }
        }
        // The neighbour cells touching each face of the column: the slot,
        // the first cell's offset from the column origin, and the direction
        // along the face.
        let faces = [
            (1, IVec3::new(-1, 0, 0), IVec3::Z),
            (7, IVec3::new(16, 0, 0), IVec3::Z),
            (3, IVec3::new(0, 0, -1), IVec3::X),
            (5, IVec3::new(0, 0, 16), IVec3::X),
        ];
        for (slot, start, along_face) in faces {
            let Some(light) = &self.light[slot] else {
                continue;
            };
            let lit: Vec<usize> = (0..SECTIONS)
                .filter(|&section| light.sections[section].is_some())
                .collect();
            for section in lit {
                for along in 0..16 {
                    for dy in 0..16 {
                        let y = MIN_Y + ((section as i32) << 4 | dy);
                        self.spread_from(slot, origin.with_y(y) + start + along_face * along);
                    }
                }
            }
        }
        self.spread();
    }

    /// Relight around the cells of column `column` written in `changes`
    /// whose emission or damping changed: unlight what their old light
    /// reached, then relight from the region's border and its emitters.
    fn relight(&mut self, column: (i32, i32), changes: &[CellChange]) {
        let origin = IVec3::new(column.0 << 4, 0, column.1 << 4);
        let changed: Vec<(usize, IVec3)> = changes
            .iter()
            .filter_map(|change| {
                let pos = origin
                    + IVec3::new(
                        i32::from(change.x),
                        i32::from(change.y),
                        i32::from(change.z),
                    );
                let slot = self.slot(pos)?;
                let old = self.layers(change.old_primary, change.old_secondary);
                (old != self.cell(slot, pos)).then_some((slot, pos))
            })
            .collect();
        for &(slot, pos) in &changed {
            let level = self.light(slot, pos);
            if level > 0 {
                self.set_light(slot, pos, 0);
                self.unlight.push((pos, level));
            }
        }
        let mut emitters = Vec::new();
        while let Some((pos, level)) = self.unlight.pop() {
            for face in FACES {
                let next = pos + face;
                let Some(slot) = self.slot(next) else {
                    continue;
                };
                let held = self.light(slot, next);
                if held == 0 {
                    continue;
                }
                if held < level {
                    self.set_light(slot, next, 0);
                    self.unlight.push((next, held));
                    let emit = self.cell(slot, next).0;
                    if emit > 0 {
                        emitters.push((slot, next, emit));
                    }
                } else {
                    self.spread_from(slot, next);
                }
            }
        }
        for (slot, pos, emit) in emitters {
            self.seed(slot, pos, emit);
        }
        for (slot, pos) in changed {
            let emit = self.cell(slot, pos).0;
            self.seed(slot, pos, emit);
            for face in FACES {
                let next = pos + face;
                if let Some(next_slot) = self.slot(next) {
                    self.spread_from(next_slot, next);
                }
            }
        }
        self.spread();
    }
}

#[cfg(test)]
mod tests;
