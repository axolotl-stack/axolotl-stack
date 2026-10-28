//! Data-driven jigsaw structures (`worldgen/structures`, `template_pools`,
//! `structure_sets`, `processors`) and the template machinery shared with
//! `structure_template_feature` and `fossil_feature`.
//!
//! The tables ([`Template`], [`Pool`], [`ProcessorList`], [`Structure`],
//! [`StructureSet`]) are emitted by `unastar_bedrock_codegen` into
//! `generated::{STRUCTURE_TEMPLATES, TEMPLATE_POOLS, PROCESSOR_LISTS,
//! JIGSAW_STRUCTURES, STRUCTURE_SETS}`; template palettes are already
//! converted to runtime ids for each of the 4 clockwise rotations.
//!
//! **Placement (`random_spread`).** The world is cut into regions of
//! `spacing` x `spacing` chunks. Each region holds at most one start, in the
//! chunk `region * spacing + offset` with `offset` in `[0, spacing -
//! separation)` per axis (uniform for `linear`, the mean of two uniform draws
//! for `triangular`). The offsets are drawn from a [`BedrockRandom`] seeded
//! with `fold32(world_seed + region_x * 341873128712 + region_z *
//! 132897987541 + salt)` ([`region_random`]); BDS's exact salt mixing is not
//! measured: placement is unmeasured: to confirm by BDS fixture, including
//! grid offsets and spacing. The same random then picks the
//! set's structures by weight, without replacement, until one passes its
//! biome filter at the start piece's centre.
//!
//! **Assembly** ([`start_piece`], [`assemble`]) follows the vanilla jigsaw
//! algorithm: a weighted start element in a random rotation at the start
//! chunk's corner, lowered so its bottom layer sits one block below
//! `start_height` (plus the surface height at the piece centre for
//! `heightmap_projection: world_surface`); then breadth-first expansion by
//! placement priority up to `max_depth`. Each connector (shuffled, then by
//! selection priority) tries the pool's elements in weighted random order,
//! then the fallback pool's (only the fallback at `max_depth`); each element
//! in 4 shuffled rotations; each of its connectors that can attach (opposite
//! faces, `target` = `name`, equal tops unless the parent joint is
//! `rollable`). A child must fit inside its free space: the box of
//! `max_distance_from_center` around the start (clamped by
//! `dimension_padding`) minus every placed piece, or, when the connection
//! point lies inside the parent, the parent's own box minus the children
//! already placed inside it. Only `rigid` projection exists in the data.
//!
//! **Generation** ([`place_in_chunk`]) is a pure function of the seed and the
//! chunk: every start within reach is assembled again and only the blocks of
//! pieces inside the chunk are written. Terrain adaptation is approximated
//! after the fact (see [`adapt`]); processors use position-hashed randomness
//! so a piece spanning chunks is processed identically in each.
//!
//! Block actors carry AOT-compiled native NBT through the shared sparse sink.
//! Template entities and gameplay loot evaluation are separate concerns.

use super::generated::{self, bb, props};
use super::rng::BedrockRandom;
use super::world::{BlockPos, BlockWorld, Ctx, OUT_OF_BOUNDS};
use smallvec::SmallVec;
use std::cmp::Reverse;

/// "No index" marker in the tables (empty pool, no processors, no feature).
pub const NONE: u16 = u16::MAX;
/// Preserve this destination layer instead of placing a block.
pub const KEEP_LAYER: u32 = u32::MAX;
/// Bytes per template cell: `x, y, z` (u8) and the palette index (u16 LE).
pub const CELL_BYTES: usize = 5;
/// Largest `limit` of a capped processor (checked by the code generator).
pub const MAX_CAPPED_LIMIT: usize = 64;
/// Largest number of capped processors per list (checked by the code generator).
pub const MAX_CAPPED_PER_LIST: usize = 4;

/// Beard approximation: blocks cleared above the floor of a `beard_thin` piece.
const THIN_CLEAR: i32 = 6;
/// Beard approximation: support blocks filled below a `beard_thin` piece.
const THIN_SUPPORT: i32 = 6;
/// Beard approximation: support blocks filled below a `beard_box` piece.
const BOX_SUPPORT: i32 = 12;

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// The six block faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir6 {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir6 {
    /// Unit offset `(dx, dy, dz)`.
    #[inline]
    pub const fn delta(self) -> (i32, i32, i32) {
        match self {
            Dir6::Down => (0, -1, 0),
            Dir6::Up => (0, 1, 0),
            Dir6::North => (0, 0, -1),
            Dir6::South => (0, 0, 1),
            Dir6::West => (-1, 0, 0),
            Dir6::East => (1, 0, 0),
        }
    }

    #[inline]
    pub const fn opposite(self) -> Dir6 {
        match self {
            Dir6::Down => Dir6::Up,
            Dir6::Up => Dir6::Down,
            Dir6::North => Dir6::South,
            Dir6::South => Dir6::North,
            Dir6::West => Dir6::East,
            Dir6::East => Dir6::West,
        }
    }

    /// Rotate clockwise (seen from above) by `quarter_turns`; up/down are fixed.
    #[inline]
    pub fn rotate(self, quarter_turns: u8) -> Dir6 {
        let mut d = self;
        for _ in 0..quarter_turns % 4 {
            d = match d {
                Dir6::North => Dir6::East,
                Dir6::East => Dir6::South,
                Dir6::South => Dir6::West,
                Dir6::West => Dir6::North,
                vertical => vertical,
            };
        }
        d
    }
}

/// Rotate a horizontal offset clockwise by `quarter_turns`: `(x, z) -> (-z, x)`
/// per turn (east becomes south), matching the compiled palettes.
#[inline]
pub fn rotate_offset(x: i32, z: i32, quarter_turns: u8) -> (i32, i32) {
    match quarter_turns % 4 {
        0 => (x, z),
        1 => (-z, x),
        2 => (-x, -z),
        _ => (z, -x),
    }
}

/// World position of template-local `local` for a piece at `origin`.
#[inline]
pub fn world_pos(origin: BlockPos, local: [i32; 3], rotation: u8) -> BlockPos {
    let (x, z) = rotate_offset(local[0], local[2], rotation);
    BlockPos::new(origin.x + x, origin.y + local[1], origin.z + z)
}

/// An axis-aligned box of blocks with inclusive bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBox {
    pub min: BlockPos,
    pub max: BlockPos,
}

impl BoundingBox {
    #[inline]
    pub fn offset(self, dx: i32, dy: i32, dz: i32) -> BoundingBox {
        BoundingBox {
            min: self.min.offset(dx, dy, dz),
            max: self.max.offset(dx, dy, dz),
        }
    }

    /// `true` when the boxes share at least one block.
    #[inline]
    pub fn intersects(&self, o: &BoundingBox) -> bool {
        self.min.x <= o.max.x
            && self.max.x >= o.min.x
            && self.min.y <= o.max.y
            && self.max.y >= o.min.y
            && self.min.z <= o.max.z
            && self.max.z >= o.min.z
    }

    /// `true` when `o` lies entirely inside `self`.
    #[inline]
    pub fn contains(&self, o: &BoundingBox) -> bool {
        o.min.x >= self.min.x
            && o.max.x <= self.max.x
            && o.min.y >= self.min.y
            && o.max.y <= self.max.y
            && o.min.z >= self.min.z
            && o.max.z <= self.max.z
    }

    #[inline]
    pub fn contains_pos(&self, p: BlockPos) -> bool {
        (self.min.x..=self.max.x).contains(&p.x)
            && (self.min.y..=self.max.y).contains(&p.y)
            && (self.min.z..=self.max.z).contains(&p.z)
    }

    /// Horizontal centre (`min + span / 2`).
    #[inline]
    pub fn center_xz(&self) -> (i32, i32) {
        (
            self.min.x + (self.max.x - self.min.x + 1) / 2,
            self.min.z + (self.max.z - self.min.z + 1) / 2,
        )
    }
}

/// The 16x16 columns of one chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkColumns {
    pub x0: i32,
    pub z0: i32,
}

impl ChunkColumns {
    #[inline]
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            x0: chunk_x * 16,
            z0: chunk_z * 16,
        }
    }

    #[inline]
    pub fn contains(&self, x: i32, z: i32) -> bool {
        (self.x0..self.x0 + 16).contains(&x) && (self.z0..self.z0 + 16).contains(&z)
    }

    #[inline]
    pub fn overlaps(&self, b: &BoundingBox) -> bool {
        b.min.x < self.x0 + 16 && b.max.x >= self.x0 && b.min.z < self.z0 + 16 && b.max.z >= self.z0
    }
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// A jigsaw connector of a template (template-local, unrotated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Connector {
    pub pos: [i32; 3],
    /// The face the connector attaches through.
    pub front: Dir6,
    /// The connector's up direction (compared for `aligned` joints).
    pub top: Dir6,
    /// Interned jigsaw `name`.
    pub name: u16,
    /// Interned jigsaw `target`.
    pub target: u16,
    /// Pool of attached pieces, or [`NONE`] for `minecraft:empty`.
    pub pool: u16,
    pub rollable: bool,
    pub selection_priority: i32,
    pub placement_priority: i32,
}

/// Native actor fields compiled for one packed template cell.
#[derive(Debug, Clone, Copy)]
pub struct TemplateBlockEntity {
    pub cell: u32,
    pub id: &'static str,
    pub fields: &'static [u8],
    pub loot: Option<(&'static str, i64)>,
    pub java_loot: bool,
    pub spawner_entity: Option<&'static str>,
}

impl TemplateBlockEntity {
    fn place<W: BlockWorld>(&self, ctx: &mut Ctx<'_, W>, pos: BlockPos, before: u32, after: u32) {
        // A processor replacing the actor's block type invalidates its data.
        if before != after {
            let registry = crate::registry::BlockRegistry::vanilla();
            if registry
                .get_by_runtime_id(before)
                .zip(registry.get_by_runtime_id(after))
                .is_none_or(|(a, b)| a.string_id != b.string_id)
            {
                return;
            }
        }
        use crate::world::block_entity::{BlockEntity, LootTable};
        let loot = self.loot.map(|(table, seed)| {
            if self.java_loot {
                LootTable::Java(table, seed)
            } else {
                LootTable::Bedrock(table, seed)
            }
        });
        ctx.world.set_block_entity(BlockEntity::generated(
            [pos.x, pos.y, pos.z],
            self.id,
            self.fields,
            loot,
            self.spawner_entity,
        ));
    }
}

fn actor_for_cell<'a>(
    actors: &mut &'a [TemplateBlockEntity],
    cell: u32,
) -> Option<&'a TemplateBlockEntity> {
    while actors.first().is_some_and(|actor| actor.cell < cell) {
        *actors = &actors[1..];
    }
    if actors.first().is_some_and(|actor| actor.cell == cell) {
        let (first, rest) = actors.split_first().expect("actor cursor is nonempty");
        *actors = rest;
        Some(first)
    } else {
        None
    }
}

/// A paired block permutation for each clockwise quarter turn.
#[derive(Debug, Clone, Copy)]
pub struct TemplateState {
    pub primary: [u32; 4],
    pub secondary: [u32; 4],
}

/// A compiled structure template.
#[derive(Debug, Clone, Copy)]
pub struct Template {
    pub size: [i32; 3],
    /// Paired layer runtime ids per clockwise quarter turn.
    pub palette: &'static [TemplateState],
    /// Packed non-void cells, [`CELL_BYTES`] each.
    pub cells: &'static [u8],
    pub connectors: &'static [Connector],
    pub block_entities: &'static [TemplateBlockEntity],
}

impl Template {
    /// Every cell as `(cell index, template-local position, palette entry)`.
    #[inline]
    pub fn cells(&self) -> impl Iterator<Item = (u32, [i32; 3], u16)> + '_ {
        self.cells
            .chunks_exact(CELL_BYTES)
            .enumerate()
            .map(|(i, c)| {
                (
                    i as u32,
                    [i32::from(c[0]), i32::from(c[1]), i32::from(c[2])],
                    u16::from_le_bytes([c[3], c[4]]),
                )
            })
    }

    /// Both layer ids in `rotation`; [`KEEP_LAYER`] retains that layer.
    #[inline]
    pub fn state(&self, entry: u16, rotation: u8) -> [u32; 2] {
        let state = &self.palette[entry as usize];
        let rotation = (rotation % 4) as usize;
        [state.primary[rotation], state.secondary[rotation]]
    }

    /// Place unprocessed cells and their metadata, without a runtime NBT decoder.
    pub fn place<W: BlockWorld>(&self, ctx: &mut Ctx<'_, W>, origin: BlockPos, rotation: u8) {
        if self.block_entities.is_empty() {
            self.place_cells::<W, false>(ctx, origin, rotation);
        } else {
            self.place_cells::<W, true>(ctx, origin, rotation);
        }
    }

    fn place_cells<W: BlockWorld, const ACTORS: bool>(
        &self,
        ctx: &mut Ctx<'_, W>,
        origin: BlockPos,
        rotation: u8,
    ) {
        let mut actors = self.block_entities;
        for (cell, local, entry) in self.cells() {
            let pos = world_pos(origin, local, rotation);
            let state = self.state(entry, rotation);
            if let Some(primary) = place_state(ctx, pos, state)
                && ACTORS
                && let Some(actor) = actor_for_cell(&mut actors, cell)
            {
                actor.place(ctx, pos, state[0], primary);
            }
        }
    }

    /// Box covered by the template placed at `origin` in `rotation`.
    pub fn bounding_box(&self, origin: BlockPos, rotation: u8) -> BoundingBox {
        let [sx, sy, sz] = self.size;
        let a = world_pos(origin, [0, 0, 0], rotation);
        let b = world_pos(origin, [sx - 1, sy - 1, sz - 1], rotation);
        BoundingBox {
            min: BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)),
            max: BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)),
        }
    }
}

/// Apply a template's two independent layer actions with one atomic write.
pub fn place_state<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    mut state: [u32; 2],
) -> Option<u32> {
    if state[0] == KEEP_LAYER {
        state[0] = ctx.get(pos);
    }
    if state[1] == KEEP_LAYER {
        state[1] = ctx.get_layer(pos, 1);
    }
    if state.contains(&OUT_OF_BOUNDS) {
        return None;
    }
    let secondary = (state[1] != bb::AIR).then_some(state[1]);
    ctx.set_layers(pos, state[0], secondary).then_some(state[0])
}

/// A weighted pool element. Template elements have `feature == NONE`;
/// feature elements use their 1x1x1 template and place feature `feature`
/// through `generated::place_pool_feature`; the empty element has
/// `template == NONE`.
#[derive(Debug, Clone, Copy)]
pub struct Element {
    pub weight: u16,
    pub template: u16,
    pub processors: u16,
    pub feature: u16,
}

/// A template pool.
#[derive(Debug, Clone, Copy)]
pub struct Pool {
    pub elements: &'static [Element],
    /// Fallback pool, or [`NONE`].
    pub fallback: u16,
}

/// One processor rule: template blocks with ids in `input_first..=input_last`
/// become `output` with `probability`.
#[derive(Debug, Clone, Copy)]
pub struct ProcessorRule {
    pub input_first: u32,
    pub input_last: u32,
    pub probability: f32,
    pub output: u32,
}

/// One processor of a list.
#[derive(Debug, Clone, Copy)]
pub enum Processor {
    /// `minecraft:rule`: the first matching rule replaces the block.
    Rules(&'static [ProcessorRule]),
    /// `minecraft:capped`: the rules change at most `limit` blocks per piece.
    Capped {
        limit: u16,
        rules: &'static [ProcessorRule],
    },
}

/// A processor list.
#[derive(Debug, Clone, Copy)]
pub struct ProcessorList {
    pub processors: &'static [Processor],
}

/// `terrain_adaptation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adaptation {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

/// A vertical anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl Anchor {
    /// Absolute y for a world with buildable `min_y..max_y`.
    #[inline]
    pub fn resolve(self, min_y: i32, max_y: i32) -> i32 {
        match self {
            Anchor::Absolute(y) => y,
            Anchor::AboveBottom(d) => min_y + d,
            Anchor::BelowTop(d) => max_y - 1 - d,
        }
    }
}

/// A jigsaw structure.
#[derive(Debug, Clone, Copy)]
pub struct Structure {
    pub start_pool: u16,
    pub max_depth: u8,
    pub horizontal: i32,
    pub vertical: i32,
    pub start_height: Anchor,
    pub project_to_surface: bool,
    pub padding_bottom: i32,
    pub padding_top: i32,
    pub adaptation: Adaptation,
    /// Generation step order (structures place in ascending step order).
    pub step: u8,
    /// Compiled `biome_filters` over the numeric biome id.
    pub biome: fn(u16) -> bool,
    /// Largest chunk distance between the start chunk and any written block.
    pub reach: i32,
}

/// A `random_spread` structure set.
#[derive(Debug, Clone, Copy)]
pub struct StructureSet {
    /// `(structure index, weight)`.
    pub structures: &'static [(u16, u16)],
    pub spacing: i32,
    pub separation: i32,
    pub salt: i32,
    pub triangular: bool,
}

/// MT19937 seeded for one placement region (see the module docs).
pub fn region_random(world_seed: i64, region_x: i32, region_z: i32, salt: i32) -> BedrockRandom {
    let s = world_seed
        .wrapping_add(i64::from(region_x).wrapping_mul(341_873_128_712))
        .wrapping_add(i64::from(region_z).wrapping_mul(132_897_987_541))
        .wrapping_add(i64::from(salt));
    BedrockRandom::new((s as u32) ^ ((s >> 32) as u32))
}

impl StructureSet {
    /// Start chunk of region `(region_x, region_z)` and the random that
    /// continues with structure selection and assembly.
    pub fn start_chunk(
        &self,
        world_seed: i64,
        region_x: i32,
        region_z: i32,
    ) -> (i32, i32, BedrockRandom) {
        let mut rng = region_random(world_seed, region_x, region_z, self.salt);
        let span = self.spacing - self.separation;
        let (ox, oz) = if self.triangular {
            let x = (rng.next_int(span) + rng.next_int(span)) / 2;
            let z = (rng.next_int(span) + rng.next_int(span)) / 2;
            (x, z)
        } else {
            let x = rng.next_int(span);
            (x, rng.next_int(span))
        };
        (
            region_x * self.spacing + ox,
            region_z * self.spacing + oz,
            rng,
        )
    }
}

/// The tables an assembly reads: the compiled vanilla data or test tables.
#[derive(Debug, Clone, Copy)]
pub struct Tables<'t> {
    pub templates: &'t [Template],
    pub pools: &'t [Pool],
    pub processors: &'t [ProcessorList],
}

impl Tables<'static> {
    /// The compiled vanilla tables.
    pub fn vanilla() -> Self {
        Tables {
            templates: &generated::STRUCTURE_TEMPLATES,
            pools: &generated::TEMPLATE_POOLS,
            processors: &generated::PROCESSOR_LISTS,
        }
    }
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

/// One placed piece of an assembled structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub template: u16,
    pub processors: u16,
    pub feature: u16,
    pub origin: BlockPos,
    pub rotation: u8,
    pub bbox: BoundingBox,
    pub depth: u8,
}

/// Free space for children: `bounds` minus the `taken` boxes.
struct Space {
    bounds: BoundingBox,
    taken: Vec<BoundingBox>,
}

impl Space {
    fn fits(&self, b: &BoundingBox) -> bool {
        self.bounds.contains(b) && self.taken.iter().all(|t| !t.intersects(b))
    }
}

/// Can `child` (in `child_rotation`) attach to `parent` (in `parent_rotation`)?
#[inline]
pub fn can_attach(
    parent: &Connector,
    parent_rotation: u8,
    child: &Connector,
    child_rotation: u8,
) -> bool {
    parent.front.rotate(parent_rotation) == child.front.rotate(child_rotation).opposite()
        && (parent.rollable
            || parent.top.rotate(parent_rotation) == child.top.rotate(child_rotation))
        && parent.target == child.name
}

fn shuffle<T>(rng: &mut BedrockRandom, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = rng.next_int(i as i32 + 1) as usize;
        items.swap(i, j);
    }
}

/// Weighted pick of one element index.
fn pick_weighted(rng: &mut BedrockRandom, elements: &[Element]) -> Option<usize> {
    let total: i32 = elements.iter().map(|e| i32::from(e.weight)).sum();
    if total <= 0 {
        return None;
    }
    let mut r = rng.next_int(total);
    for (i, e) in elements.iter().enumerate() {
        r -= i32::from(e.weight);
        if r < 0 {
            return Some(i);
        }
    }
    None
}

/// Append `(pool, element)` for every element of `pool` in weighted random
/// order (sequential weighted draws without replacement, which orders
/// elements like the first occurrences of a shuffled weight-repeated list).
fn weighted_order(
    rng: &mut BedrockRandom,
    elements: &[Element],
    pool: u16,
    out: &mut Vec<(u16, u16)>,
) {
    let start = out.len();
    out.extend((0..elements.len() as u16).map(|i| (pool, i)));
    for k in start..out.len() {
        let total: i32 = out[k..]
            .iter()
            .map(|&(_, i)| i32::from(elements[i as usize].weight))
            .sum();
        if total <= 0 {
            return;
        }
        let mut r = rng.next_int(total);
        let mut chosen = k;
        for (j, &(_, i)) in out.iter().enumerate().skip(k) {
            r -= i32::from(elements[i as usize].weight);
            if r < 0 {
                chosen = j;
                break;
            }
        }
        out.swap(k, chosen);
    }
}

/// Connector indices of `template` shuffled, then stably ordered by
/// descending selection priority.
fn ordered_connectors(rng: &mut BedrockRandom, template: &Template, out: &mut Vec<u16>) {
    out.clear();
    out.extend(0..template.connectors.len() as u16);
    shuffle(rng, out);
    out.sort_by_key(|&c| Reverse(template.connectors[c as usize].selection_priority));
}

/// The start piece of `structure` in chunk `(chunk_x, chunk_z)`, or `None`
/// for an empty start element. `surface(x, z)` is the world-surface height.
#[allow(clippy::too_many_arguments)]
pub fn start_piece(
    t: Tables<'_>,
    structure: &Structure,
    rng: &mut BedrockRandom,
    chunk_x: i32,
    chunk_z: i32,
    surface: &dyn Fn(i32, i32) -> i32,
    min_y: i32,
    max_y: i32,
) -> Option<Piece> {
    let pool = t.pools.get(structure.start_pool as usize)?;
    let element = pool.elements[pick_weighted(rng, pool.elements)?];
    if element.template == NONE {
        return None;
    }
    let template = &t.templates[element.template as usize];
    let rotation = rng.next_int(4) as u8;
    let origin = BlockPos::new(chunk_x * 16, 0, chunk_z * 16);
    let bbox = template.bounding_box(origin, rotation);
    let (cx, cz) = bbox.center_xz();
    let anchor = structure.start_height.resolve(min_y, max_y);
    let ground = if structure.project_to_surface {
        surface(cx, cz) + anchor
    } else {
        anchor
    };
    // Pool elements have a ground level delta of one: the bottom layer is
    // embedded one block below the ground height.
    let dy = ground - 1 - bbox.min.y;
    Some(Piece {
        template: element.template,
        processors: element.processors,
        feature: element.feature,
        origin: origin.offset(0, dy, 0),
        rotation,
        bbox: bbox.offset(0, dy, 0),
        depth: 0,
    })
}

/// Expand `start` into the full piece list (`out[0]` is the start).
pub fn assemble(
    t: Tables<'_>,
    structure: &Structure,
    rng: &mut BedrockRandom,
    start: Piece,
    min_y: i32,
    max_y: i32,
    out: &mut Vec<Piece>,
) {
    out.clear();
    out.push(start);
    if structure.max_depth == 0 {
        return;
    }
    let (cx, cz) = start.bbox.center_xz();
    let cy = start.bbox.min.y + 1;
    let bounds = BoundingBox {
        min: BlockPos::new(
            cx - structure.horizontal,
            (cy - structure.vertical).max(min_y + structure.padding_bottom),
            cz - structure.horizontal,
        ),
        max: BlockPos::new(
            cx + structure.horizontal,
            (cy + structure.vertical).min(max_y - 1 - structure.padding_top),
            cz + structure.horizontal,
        ),
    };
    let mut spaces = vec![Space {
        bounds,
        taken: vec![start.bbox],
    }];
    // Interior space of each piece, created when a connector points inside it.
    let mut interiors: Vec<Option<usize>> = vec![None];
    // (piece, free space, placement priority); highest priority first, FIFO.
    let mut queue: Vec<(usize, usize, i32)> = vec![(0, 0, 0)];
    let mut parent_connectors: Vec<u16> = Vec::new();
    let mut child_connectors: Vec<u16> = Vec::new();
    let mut candidates: Vec<(u16, u16)> = Vec::new();

    while !queue.is_empty() {
        let mut best = 0;
        for (i, q) in queue.iter().enumerate() {
            if q.2 > queue[best].2 {
                best = i;
            }
        }
        let (pi, piece_space, _) = queue.remove(best);
        let piece = out[pi];
        let template = &t.templates[piece.template as usize];
        ordered_connectors(rng, template, &mut parent_connectors);
        'connectors: for &ci in &parent_connectors {
            let c = &template.connectors[ci as usize];
            if c.pool == NONE {
                continue;
            }
            let Some(pool) = t.pools.get(c.pool as usize) else {
                continue;
            };
            let (dx, dy, dz) = c.front.rotate(piece.rotation).delta();
            let target = world_pos(piece.origin, c.pos, piece.rotation).offset(dx, dy, dz);
            let space = if piece.bbox.contains_pos(target) {
                match interiors[pi] {
                    Some(i) => i,
                    None => {
                        spaces.push(Space {
                            bounds: piece.bbox,
                            taken: Vec::new(),
                        });
                        interiors[pi] = Some(spaces.len() - 1);
                        spaces.len() - 1
                    }
                }
            } else {
                piece_space
            };
            candidates.clear();
            if piece.depth != structure.max_depth {
                weighted_order(rng, pool.elements, c.pool, &mut candidates);
            }
            if let Some(fallback) = t.pools.get(pool.fallback as usize) {
                weighted_order(rng, fallback.elements, pool.fallback, &mut candidates);
            }
            for &(cp, ce) in &candidates {
                let element = t.pools[cp as usize].elements[ce as usize];
                if element.template == NONE {
                    // The empty element ends the attempts for this connector.
                    break;
                }
                let child = &t.templates[element.template as usize];
                let mut rotations = [0u8, 1, 2, 3];
                shuffle(rng, &mut rotations);
                for rotation in rotations {
                    ordered_connectors(rng, child, &mut child_connectors);
                    let local_box = child.bounding_box(BlockPos::new(0, 0, 0), rotation);
                    for &cc in &child_connectors {
                        let cc = &child.connectors[cc as usize];
                        if !can_attach(c, piece.rotation, cc, rotation) {
                            continue;
                        }
                        let local = world_pos(BlockPos::new(0, 0, 0), cc.pos, rotation);
                        let origin = BlockPos::new(
                            target.x - local.x,
                            target.y - local.y,
                            target.z - local.z,
                        );
                        let bbox = local_box.offset(origin.x, origin.y, origin.z);
                        if !spaces[space].fits(&bbox) {
                            continue;
                        }
                        spaces[space].taken.push(bbox);
                        out.push(Piece {
                            template: element.template,
                            processors: element.processors,
                            feature: element.feature,
                            origin,
                            rotation,
                            bbox,
                            depth: piece.depth + 1,
                        });
                        interiors.push(None);
                        if piece.depth < structure.max_depth {
                            queue.push((out.len() - 1, space, c.placement_priority));
                        }
                        continue 'connectors;
                    }
                }
            }
        }
    }
}

/// Pick, check and assemble the structure of one start of `set`; `None` when
/// no structure of the set passes its biome filter there.
#[allow(clippy::too_many_arguments)]
pub fn generate_start(
    t: Tables<'_>,
    structures: &[Structure],
    set: &StructureSet,
    rng: &mut BedrockRandom,
    chunk_x: i32,
    chunk_z: i32,
    surface: &dyn Fn(i32, i32) -> i32,
    biome: &dyn Fn(i32, i32, i32) -> u16,
    min_y: i32,
    max_y: i32,
) -> Option<(usize, Vec<Piece>)> {
    // Vanilla sets contain one or 18 entries; larger custom sets still spill.
    let mut remaining: SmallVec<[(u16, u16); 32]> = SmallVec::from_slice(set.structures);
    while !remaining.is_empty() {
        let total: i32 = remaining.iter().map(|&(_, w)| i32::from(w)).sum();
        let mut r = rng.next_int(total.max(1));
        let mut chosen = 0;
        for (i, &(_, w)) in remaining.iter().enumerate() {
            r -= i32::from(w);
            if r < 0 {
                chosen = i;
                break;
            }
        }
        let (index, _) = remaining.remove(chosen);
        let structure = &structures[index as usize];
        let Some(start) = start_piece(t, structure, rng, chunk_x, chunk_z, surface, min_y, max_y)
        else {
            continue;
        };
        let (cx, cz) = start.bbox.center_xz();
        if !(structure.biome)(biome(cx, start.bbox.min.y + 1, cz)) {
            continue;
        }
        let mut pieces = Vec::new();
        assemble(t, structure, rng, start, min_y, max_y, &mut pieces);
        return Some((index as usize, pieces));
    }
    None
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

/// Place every vanilla jigsaw structure piece that intersects chunk
/// `(chunk_x, chunk_z)`. `ctx.world` must expose the chunk (writes are
/// limited to it, except features of feature pool elements, which spill into
/// neighbours like any decoration). `surface(x, z)` is the terrain's
/// world-surface height (first free y above the highest non-air block) and
/// `biome(x, y, z)` the numeric biome id; both must be valid far outside the
/// region, since starts up to [`Structure::reach`] chunks away contribute.
/// Call it before the vegetation passes so terrain adaptation only reshapes
/// terrain.
pub fn place_in_chunk<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    surface: &dyn Fn(i32, i32) -> i32,
    biome: &dyn Fn(i32, i32, i32) -> u16,
    chunk_x: i32,
    chunk_z: i32,
) {
    let t = Tables::vanilla();
    let structures: &[Structure] = &generated::JIGSAW_STRUCTURES;
    let (min_y, max_y) = (ctx.world.min_y(), ctx.world.max_y());
    let columns = ChunkColumns::new(chunk_x, chunk_z);
    let mut starts: Vec<(u8, usize, Vec<Piece>)> = Vec::new();
    for set in generated::STRUCTURE_SETS.iter() {
        let reach = set
            .structures
            .iter()
            .map(|&(s, _)| structures[s as usize].reach)
            .max()
            .unwrap_or(0);
        let rx0 = (chunk_x - reach).div_euclid(set.spacing);
        let rx1 = (chunk_x + reach).div_euclid(set.spacing);
        let rz0 = (chunk_z - reach).div_euclid(set.spacing);
        let rz1 = (chunk_z + reach).div_euclid(set.spacing);
        for rx in rx0..=rx1 {
            for rz in rz0..=rz1 {
                let (sx, sz, mut rng) = set.start_chunk(ctx.seed, rx, rz);
                if (sx - chunk_x).abs() > reach || (sz - chunk_z).abs() > reach {
                    continue;
                }
                let Some((index, pieces)) = generate_start(
                    t, structures, set, &mut rng, sx, sz, surface, biome, min_y, max_y,
                ) else {
                    continue;
                };
                if pieces.iter().any(|p| columns.overlaps(&p.bbox)) {
                    starts.push((structures[index].step, index, pieces));
                }
            }
        }
    }
    starts.sort_by_key(|&(step, _, _)| step);
    for (_, index, pieces) in &starts {
        place_start(ctx, t, &structures[*index], pieces, columns);
    }
}

/// Write the parts of one assembled start inside `columns`.
fn place_start<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    t: Tables<'_>,
    structure: &Structure,
    pieces: &[Piece],
    columns: ChunkColumns,
) {
    if structure.adaptation != Adaptation::None {
        for p in pieces
            .iter()
            .filter(|p| p.feature == NONE && columns.overlaps(&p.bbox))
        {
            adapt(ctx, structure.adaptation, &p.bbox, columns);
        }
    }
    for p in pieces {
        if p.feature != NONE {
            // A feature piece places its feature once, from the chunk that
            // holds its position.
            if columns.contains(p.origin.x, p.origin.z) && ctx.in_height(p.origin.y) {
                let _ = generated::place_pool_feature(ctx, p.feature, p.origin);
            }
        } else if columns.overlaps(&p.bbox) {
            place_piece(ctx, t, p, columns);
        }
    }
}

/// Ground material under a column: the first non-air, non-liquid block at
/// or below `y` (grass-like tops become dirt), or stone.
fn ground_material<W: BlockWorld>(ctx: &Ctx<'_, W>, x: i32, y: i32, z: i32) -> u32 {
    let min_y = ctx.world.min_y();
    let mut y = y;
    while y >= min_y {
        let id = ctx.get(BlockPos::new(x, y, z));
        if id == OUT_OF_BOUNDS {
            break;
        }
        if id != bb::AIR && !props::is_liquid(id) {
            return if id == bb::GRASS_BLOCK || id == bb::PODZOL || id == bb::MYCELIUM {
                bb::DIRT
            } else {
                id
            };
        }
        y -= 1;
    }
    bb::STONE
}

#[inline]
fn is_open(id: u32) -> bool {
    id == bb::AIR || props::is_liquid(id)
}

/// Approximate `terrain_adaptation` for one piece inside `columns`. Vanilla
/// reshapes the terrain density around pieces before surface building; here
/// the finished terrain is edited instead:
/// - `beard_thin`: clear non-liquid blocks up to [`THIN_CLEAR`] blocks above
///   the piece floor (its bottom layer) and fill air/liquid gaps up to
///   [`THIN_SUPPORT`] blocks below it with the ground material;
/// - `beard_box`: clear the whole box above the floor and fill gaps up to
///   [`BOX_SUPPORT`] blocks below;
/// - `bury` / `encapsulate`: fill every air/liquid block of the box with the
///   ground material, so only the template's own cells (including its air)
///   stay open.
pub fn adapt<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    mode: Adaptation,
    bbox: &BoundingBox,
    columns: ChunkColumns,
) {
    let floor = bbox.min.y;
    let x0 = bbox.min.x.max(columns.x0);
    let x1 = bbox.max.x.min(columns.x0 + 15);
    let z0 = bbox.min.z.max(columns.z0);
    let z1 = bbox.max.z.min(columns.z0 + 15);
    for x in x0..=x1 {
        for z in z0..=z1 {
            match mode {
                Adaptation::None => return,
                Adaptation::BeardThin | Adaptation::BeardBox => {
                    let (top, support) = if mode == Adaptation::BeardThin {
                        (bbox.max.y.min(floor + THIN_CLEAR), THIN_SUPPORT)
                    } else {
                        (bbox.max.y, BOX_SUPPORT)
                    };
                    for y in floor + 1..=top {
                        let pos = BlockPos::new(x, y, z);
                        if !ctx.in_height(y) {
                            continue;
                        }
                        let id = ctx.get(pos);
                        if id != OUT_OF_BOUNDS && !is_open(id) {
                            ctx.set(pos, bb::AIR);
                        }
                    }
                    let fill = ground_material(ctx, x, floor - 1, z);
                    for y in (floor - support..floor).rev() {
                        let pos = BlockPos::new(x, y, z);
                        if !ctx.in_height(y) || !is_open(ctx.get(pos)) {
                            break;
                        }
                        ctx.set(pos, fill);
                    }
                }
                Adaptation::Bury | Adaptation::Encapsulate => {
                    let fill = ground_material(ctx, x, floor - 1, z);
                    for y in floor..=bbox.max.y {
                        let pos = BlockPos::new(x, y, z);
                        if ctx.in_height(y) && is_open(ctx.get(pos)) {
                            ctx.set(pos, fill);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Processors
// ---------------------------------------------------------------------------

#[inline]
fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Position-seeded hash: processors draw from it so the result of a block
/// does not depend on which chunk places it.
#[inline]
pub fn position_hash(seed: i64, pos: BlockPos, salt: u64) -> u64 {
    let xz = u64::from(pos.x as u32) | (u64::from(pos.z as u32) << 32);
    mix64(seed as u64 ^ mix64(xz) ^ mix64(u64::from(pos.y as u32) ^ salt.rotate_left(32)))
}

#[inline]
fn unit_float(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// First matching rule's output for `state` at `pos`. `random_block_match`
/// rules draw only when the block matches, like vanilla.
pub fn apply_rules(
    rules: &[ProcessorRule],
    state: u32,
    seed: i64,
    pos: BlockPos,
    salt: u64,
) -> Option<u32> {
    let mut draw = 0u64;
    for r in rules {
        if state < r.input_first || state > r.input_last {
            continue;
        }
        if r.probability < 1.0 {
            let roll = unit_float(position_hash(
                seed,
                pos,
                salt.wrapping_mul(64).wrapping_add(draw),
            ));
            draw += 1;
            if roll >= r.probability {
                continue;
            }
        }
        return Some(r.output);
    }
    None
}

/// One cell chosen by a capped processor.
#[derive(Debug, Clone, Copy, Default)]
struct Pick {
    key: u64,
    cell: u32,
    output: u32,
}

/// Cells chosen by the capped processors of one piece: per capped processor
/// the `limit` changing cells with the smallest piece-seeded keys, which is
/// a uniformly random choice independent of the chunk being generated.
struct Caps {
    picks: [[Pick; MAX_CAPPED_LIMIT]; MAX_CAPPED_PER_LIST],
    len: [usize; MAX_CAPPED_PER_LIST],
}

impl Caps {
    fn selected(&self, capped: usize, cell: u32) -> Option<u32> {
        self.picks[capped][..self.len[capped]]
            .iter()
            .find(|p| p.cell == cell)
            .map(|p| p.output)
    }

    fn offer(&mut self, capped: usize, limit: usize, pick: Pick) {
        let limit = limit.min(MAX_CAPPED_LIMIT);
        let len = self.len[capped];
        let picks = &mut self.picks[capped];
        if len < limit {
            picks[len] = pick;
            self.len[capped] = len + 1;
            return;
        }
        let mut worst = 0;
        for i in 1..len {
            if picks[i].key > picks[worst].key {
                worst = i;
            }
        }
        if len > 0 && pick.key < picks[worst].key {
            picks[worst] = pick;
        }
    }
}

/// Layer actions after `list[..upto]`; processors replace only the primary id.
fn process(
    list: &[Processor],
    caps: &Caps,
    upto: usize,
    cell: u32,
    pos: BlockPos,
    state: [u32; 2],
    seed: i64,
) -> [u32; 2] {
    let mut state = state;
    let mut capped = 0;
    for (k, p) in list.iter().take(upto).enumerate() {
        match p {
            Processor::Rules(rules) => {
                if let Some(out) = apply_rules(rules, state[0], seed, pos, k as u64) {
                    state[0] = out;
                }
            }
            Processor::Capped { .. } => {
                if let Some(out) = caps.selected(capped, cell) {
                    state[0] = out;
                }
                capped += 1;
            }
        }
    }
    state
}

/// Write the cells of `piece` inside `columns`, processors applied.
fn place_piece<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    t: Tables<'_>,
    piece: &Piece,
    columns: ChunkColumns,
) {
    let template = &t.templates[piece.template as usize];
    let list: &[Processor] = match t.processors.get(piece.processors as usize) {
        Some(l) => l.processors,
        None => &[],
    };
    let seed = ctx.seed;
    let mut caps = Caps {
        picks: [[Pick::default(); MAX_CAPPED_LIMIT]; MAX_CAPPED_PER_LIST],
        len: [0; MAX_CAPPED_PER_LIST],
    };
    // Capped processors look at the whole piece, so they are resolved over
    // every cell first (deterministically, whatever chunk is generated).
    let piece_seed = position_hash(
        seed,
        piece.origin,
        (u64::from(piece.template) << 8) | u64::from(piece.rotation),
    );
    let mut capped = 0;
    for (k, p) in list.iter().enumerate() {
        let Processor::Capped { limit, rules } = p else {
            continue;
        };
        if capped >= MAX_CAPPED_PER_LIST {
            break;
        }
        for (cell, local, entry) in template.cells() {
            let pos = world_pos(piece.origin, local, piece.rotation);
            let before = process(
                list,
                &caps,
                k,
                cell,
                pos,
                template.state(entry, piece.rotation),
                seed,
            );
            if let Some(output) = apply_rules(rules, before[0], seed, pos, k as u64)
                && output != before[0]
            {
                let key = mix64(piece_seed ^ u64::from(cell).wrapping_mul(0x2545_f491_4f6c_dd1d));
                caps.offer(capped, *limit as usize, Pick { key, cell, output });
            }
        }
        capped += 1;
    }
    if template.block_entities.is_empty() {
        place_piece_cells::<W, false>(ctx, template, piece, columns, list, &caps);
    } else {
        place_piece_cells::<W, true>(ctx, template, piece, columns, list, &caps);
    }
}

fn place_piece_cells<W: BlockWorld, const ACTORS: bool>(
    ctx: &mut Ctx<'_, W>,
    template: &Template,
    piece: &Piece,
    columns: ChunkColumns,
    list: &[Processor],
    caps: &Caps,
) {
    let mut actors = template.block_entities;
    for (cell, local, entry) in template.cells() {
        let pos = world_pos(piece.origin, local, piece.rotation);
        if !columns.contains(pos.x, pos.z) || !ctx.in_height(pos.y) {
            continue;
        }
        let original = template.state(entry, piece.rotation);
        let state = process(list, caps, list.len(), cell, pos, original, ctx.seed);
        if let Some(primary) = place_state(ctx, pos, state)
            && ACTORS
            && let Some(actor) = actor_for_cell(&mut actors, cell)
        {
            actor.place(ctx, pos, original[0], primary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME_ROOM: u16 = 0;
    const NAME_DOOR: u16 = 1;
    const NAME_HALL: u16 = 2;

    // Template 0: a 3x1x3 room with two east-facing connectors one block
    // apart in z. Template 1: a 3x1x3 annex attaching through its west side.
    static PARENT_CONNECTORS: [Connector; 2] = [
        Connector {
            pos: [2, 0, 0],
            front: Dir6::East,
            top: Dir6::Up,
            name: NAME_ROOM,
            target: NAME_DOOR,
            pool: 0,
            rollable: true,
            selection_priority: 0,
            placement_priority: 0,
        },
        Connector {
            pos: [2, 0, 2],
            front: Dir6::East,
            top: Dir6::Up,
            name: NAME_ROOM,
            target: NAME_DOOR,
            pool: 0,
            rollable: true,
            selection_priority: 0,
            placement_priority: 0,
        },
    ];
    static CHILD_CONNECTORS: [Connector; 1] = [Connector {
        pos: [0, 0, 1],
        front: Dir6::West,
        top: Dir6::Up,
        name: NAME_DOOR,
        target: NAME_HALL,
        pool: NONE,
        rollable: true,
        selection_priority: 0,
        placement_priority: 0,
    }];
    static TEMPLATES: [Template; 2] = [
        Template {
            size: [3, 1, 3],
            palette: &[],
            cells: &[],
            connectors: &PARENT_CONNECTORS,
            block_entities: &[],
        },
        Template {
            size: [3, 1, 3],
            palette: &[],
            cells: &[],
            connectors: &CHILD_CONNECTORS,
            block_entities: &[],
        },
    ];
    static CHILDREN: [Element; 1] = [Element {
        weight: 1,
        template: 1,
        processors: NONE,
        feature: NONE,
    }];
    static STARTS: [Element; 1] = [Element {
        weight: 1,
        template: 0,
        processors: NONE,
        feature: NONE,
    }];
    static POOLS: [Pool; 2] = [
        Pool {
            elements: &CHILDREN,
            fallback: NONE,
        },
        Pool {
            elements: &STARTS,
            fallback: NONE,
        },
    ];

    fn any_biome(_: u16) -> bool {
        true
    }

    fn structure() -> Structure {
        Structure {
            start_pool: 1,
            max_depth: 1,
            horizontal: 32,
            vertical: 32,
            start_height: Anchor::Absolute(0),
            project_to_surface: true,
            padding_bottom: 0,
            padding_top: 0,
            adaptation: Adaptation::None,
            step: 4,
            biome: any_biome,
            reach: 4,
        }
    }

    fn tables() -> Tables<'static> {
        Tables {
            templates: &TEMPLATES,
            pools: &POOLS,
            processors: &[],
        }
    }

    #[test]
    fn random_spread_starts_stay_in_their_region_and_repeat() {
        for triangular in [false, true] {
            let set = StructureSet {
                structures: &[],
                spacing: 34,
                separation: 8,
                salt: 83_469_867,
                triangular,
            };
            let mut seen = std::collections::HashSet::new();
            for rx in -6..6 {
                for rz in -6..6 {
                    let (x, z, _) = set.start_chunk(12345, rx, rz);
                    assert!((rx * 34..rx * 34 + 26).contains(&x), "x {x} region {rx}");
                    assert!((rz * 34..rz * 34 + 26).contains(&z), "z {z} region {rz}");
                    let (x2, z2, _) = set.start_chunk(12345, rx, rz);
                    assert_eq!((x, z), (x2, z2));
                    seen.insert((x - rx * 34, z - rz * 34));
                }
            }
            // Offsets vary between regions.
            assert!(seen.len() > 60);
        }
    }

    #[test]
    fn rotated_positions_and_connectors_stay_consistent() {
        let origin = BlockPos::new(10, 5, -3);
        assert_eq!(world_pos(origin, [2, 1, 0], 0), BlockPos::new(12, 6, -3));
        assert_eq!(world_pos(origin, [2, 1, 0], 1), BlockPos::new(10, 6, -1));
        assert_eq!(world_pos(origin, [2, 1, 0], 2), BlockPos::new(8, 6, -3));
        assert_eq!(world_pos(origin, [2, 1, 0], 3), BlockPos::new(10, 6, -5));
        // The direction of a rotated connector matches its rotated offset.
        for r in 0..4 {
            let (dx, _, dz) = Dir6::East.rotate(r).delta();
            assert_eq!(rotate_offset(1, 0, r), (dx, dz));
        }
        // Rotated boxes cover the same volume.
        let t = &TEMPLATES[0];
        for r in 0..4 {
            let b = t.bounding_box(origin, r);
            assert_eq!((b.max.x - b.min.x + 1) * (b.max.z - b.min.z + 1), 9);
            assert!(b.contains_pos(origin));
        }
        // An east connector accepts a west child only in matching rotations.
        let parent = &PARENT_CONNECTORS[0];
        let child = &CHILD_CONNECTORS[0];
        assert!(can_attach(parent, 0, child, 0));
        assert!(!can_attach(parent, 0, child, 1));
        assert!(can_attach(parent, 1, child, 1));
        assert!(
            !can_attach(child, 0, parent, 0),
            "target must match the child's name"
        );
    }

    #[test]
    fn overlapping_children_are_rejected() {
        let s = structure();
        let t = tables();
        let mut rng = BedrockRandom::new(7);
        let start = start_piece(t, &s, &mut rng, 0, 0, &|_, _| 64, -64, 320).unwrap();
        assert_eq!(start.bbox.min.y, 63, "bottom layer one below the surface");
        let mut pieces = Vec::new();
        assemble(t, &s, &mut rng, start, -64, 320, &mut pieces);
        // Both connectors want an annex; the second would overlap the first.
        assert_eq!(pieces.len(), 2);
        let child = pieces[1];
        assert!(!child.bbox.intersects(&start.bbox));
        assert_eq!(child.depth, 1);
        // The child touches the parent through the connector.
        let gap_x = (
            child.bbox.min.x - start.bbox.max.x,
            start.bbox.min.x - child.bbox.max.x,
        );
        let gap_z = (
            child.bbox.min.z - start.bbox.max.z,
            start.bbox.min.z - child.bbox.max.z,
        );
        assert!(gap_x.0 == 1 || gap_x.1 == 1 || gap_z.0 == 1 || gap_z.1 == 1);
    }

    #[test]
    fn assembly_is_deterministic() {
        let s = structure();
        let t = tables();
        let run = || {
            let mut rng = BedrockRandom::new(99);
            let start = start_piece(t, &s, &mut rng, 3, -2, &|_, _| 70, -64, 320).unwrap();
            let mut pieces = Vec::new();
            assemble(t, &s, &mut rng, start, -64, 320, &mut pieces);
            pieces
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn space_rejects_overlaps_and_out_of_bounds() {
        let b = |x0, x1| BoundingBox {
            min: BlockPos::new(x0, 0, 0),
            max: BlockPos::new(x1, 0, 0),
        };
        let space = Space {
            bounds: b(0, 20),
            taken: vec![b(0, 4)],
        };
        assert!(space.fits(&b(5, 9)));
        assert!(!space.fits(&b(4, 9)), "overlaps the taken box");
        assert!(!space.fits(&b(18, 21)), "leaves the bounds");
    }

    #[test]
    fn rules_are_position_deterministic_and_respect_probability() {
        let rules = [ProcessorRule {
            input_first: 5,
            input_last: 5,
            probability: 0.25,
            output: 9,
        }];
        let mut hits = 0;
        for x in 0..4000 {
            let pos = BlockPos::new(x, 64, x / 7);
            let a = apply_rules(&rules, 5, 1, pos, 0);
            assert_eq!(a, apply_rules(&rules, 5, 1, pos, 0));
            assert_eq!(apply_rules(&rules, 4, 1, pos, 0), None);
            hits += usize::from(a.is_some());
        }
        assert!((800..1200).contains(&hits), "{hits} hits");
    }

    #[test]
    fn capped_keeps_the_smallest_keys() {
        let mut caps = Caps {
            picks: [[Pick::default(); MAX_CAPPED_LIMIT]; MAX_CAPPED_PER_LIST],
            len: [0; MAX_CAPPED_PER_LIST],
        };
        for (cell, key) in [(0u32, 50u64), (1, 10), (2, 40), (3, 5)] {
            caps.offer(
                0,
                2,
                Pick {
                    key,
                    cell,
                    output: 7,
                },
            );
        }
        assert_eq!(caps.len[0], 2);
        assert_eq!(caps.selected(0, 3), Some(7));
        assert_eq!(caps.selected(0, 1), Some(7));
        assert_eq!(caps.selected(0, 0), None);
    }
}
