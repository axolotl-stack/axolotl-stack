//! Jigsaw structures (Java `JigsawStructure`, `JigsawPlacement.addPieces`,
//! `PoolElementStructurePiece`): villages, pillager outposts, bastions,
//! ancient cities, trail ruins and trial chambers.
//!
//! # Generation point
//! [`find_generation_point`] samples the start height, resolves the pool
//! aliases ([`PoolAliasLookup`]), draws the start rotation and element, anchors
//! the start on `start_jigsaw_name`, projects it to the heightmap and rejects
//! starts too close to the world height limits (`dimension_padding`). The stub's
//! builder runs the assembly with the same context random.
//!
//! # Assembly
//! Pieces are expanded breadth first through a priority queue keyed by the
//! parent jigsaw's placement priority (FIFO within a priority, highest first).
//! Each piece's jigsaws are shuffled and stably sorted by selection priority;
//! candidate elements are the pool's weighted list (each element repeated
//! `weight` times) shuffled, followed by the shuffled fallback pool; each
//! candidate tries its four rotations in shuffled order and its own shuffled
//! jigsaws. Every random draw happens in Java's order, including the shuffles
//! whose results go unused.
//!
//! Java tracks free space with voxel shapes: the `max_distance_from_center` box
//! minus the start piece, or the parent's box for jigsaws pointing into their
//! own piece. A candidate fits when its box shrunk by 0.25 lies inside the free
//! shape, and its box is then removed from the shape. All boxes are block
//! aligned, so the shape is exactly a set of whole blocks; [`FreeShape`] keeps
//! it as one box minus the boxes carved out of it, which accepts and rejects
//! the same candidates.
//!
//! # Placement
//! [`PoolElementPiece::post_process`] places single elements as templates with
//! Java's processor order (structure block ignore, jigsaw replacement, the
//! element's processors, gravity for terrain matching pieces; legacy elements
//! ignore structure blocks and air last), feature elements as their placed
//! feature, list elements one after another, and nothing for empty elements.
//!
//! Not reproduced (the framework cannot express them):
//! - Template entities (villagers, armor stands, ...) are not spawned; Java
//!   spawns them without drawing from the placement random.
//! - Java also runs the template's data markers through the processors after
//!   placement, but the structure block ignore processor always removes them
//!   and the pool element's data marker handler does nothing, so no blocks
//!   change and no draws are made on the placement random; it is skipped.

use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{LazyLock, OnceLock};

use crate::features::java_blocks::blocks as b;
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, Direction, FeatureWorld, Heightmap, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::jigsaw::{
    self, JigsawStructure, PoolAlias, PoolElement, Projection, TemplatePool,
};
use crate::structures::piece::{PieceBase, StructurePiece};
use crate::structures::processor::Processor;
use crate::structures::start::{GenContext, GenerationStub, StructureKind};
use crate::structures::template::{PlaceSettings, place_in_world, template_box};
use crate::structures::template_data::{
    self, BlockEntityInfo, JointType, TEMPLATE_NAMES, Template,
};
use crate::structures::template_piece::record_block_entities;
use crate::structures::transform::{BoundingBox, Mirror, Rotation, transform};
use crate::structures::util::{pos_seed, shuffle};

/// Id of Java's `Pools.EMPTY`, the only empty pool jigsaws may target.
const EMPTY_POOL: &str = "minecraft:empty";

/// `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
static IGNORE_STRUCTURE_BLOCK: Processor = Processor::BlockIgnore(&[b::STRUCTURE_BLOCK]);
/// `BlockIgnoreProcessor.STRUCTURE_AND_AIR` (legacy single elements).
static IGNORE_STRUCTURE_AND_AIR: Processor = Processor::BlockIgnore(&[b::AIR, b::STRUCTURE_BLOCK]);
/// `JigsawReplacementProcessor.INSTANCE`.
static JIGSAW_REPLACEMENT: Processor = Processor::JigsawReplacement;
/// The processor of `Projection.TERRAIN_MATCHING`.
static TERRAIN_MATCHING_GRAVITY: Processor = Processor::Gravity {
    heightmap: Heightmap::WorldSurfaceWg,
    offset: -1,
};
/// `EmptyPoolElement.INSTANCE`, returned for empty pools.
static EMPTY_ELEMENT: PoolElement = PoolElement::Empty;

// ---------------------------------------------------------------------------
// Pool aliases

/// `PoolAliasLookup`: pool id substitutions resolved once per start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolAliasLookup {
    map: Vec<(&'static str, &'static str)>,
}

impl PoolAliasLookup {
    /// `PoolAliasLookup.create(aliases, pos, seed)`: random bindings draw from
    /// `RandomSource.create(seed).forkPositional().at(pos)`.
    pub fn create(aliases: &'static [PoolAlias], pos: BlockPos, seed: i64) -> Self {
        if aliases.is_empty() {
            return Self::default();
        }
        let factory_seed = LegacyRandom::new(seed).next_long();
        let mut rng = LegacyRandom::new(pos_seed(pos.x, pos.y, pos.z) ^ factory_seed);
        let mut map = Vec::new();
        for alias in aliases {
            resolve_alias(alias, &mut rng, &mut map);
        }
        Self { map }
    }

    /// The pool id `key` stands for (itself when it is not an alias).
    pub fn lookup<'s>(&self, key: &'s str) -> &'s str {
        self.map
            .iter()
            .find(|(alias, _)| *alias == key)
            .map_or(key, |(_, target)| *target)
    }
}

/// `PoolAliasBinding.forEachResolved`.
fn resolve_alias(
    alias: &'static PoolAlias,
    rng: &mut LegacyRandom,
    out: &mut Vec<(&'static str, &'static str)>,
) {
    match *alias {
        PoolAlias::Direct { alias, target } => out.push((alias, target)),
        PoolAlias::Random { alias, targets } => {
            if let Some(&(target, _)) = weighted_choice(targets, rng) {
                out.push((alias, target));
            }
        }
        PoolAlias::RandomGroup { groups } => {
            if let Some(&(group, _)) = weighted_choice(groups, rng) {
                for a in group {
                    resolve_alias(a, rng, out);
                }
            }
        }
    }
}

/// `WeightedList.getRandom`: `nextInt(totalWeight)` walked over the weights.
fn weighted_choice<'t, T>(entries: &'t [(T, i32)], rng: &mut LegacyRandom) -> Option<&'t (T, i32)> {
    let total: i32 = entries.iter().map(|e| e.1).sum();
    if total == 0 {
        return None;
    }
    let mut i = rng.next_int_bounded(total);
    for e in entries {
        if i < e.1 {
            return Some(e);
        }
        i -= e.1;
    }
    unreachable!("weighted index below the total weight")
}

// ---------------------------------------------------------------------------
// Jigsaw blocks

/// A jigsaw block of a template, as stored (palette 0, palette order).
#[derive(Debug)]
struct TemplateJigsaw {
    pos: BlockPos,
    front: Direction,
    top: Direction,
    joint: JointType,
    name: Cow<'static, str>,
    target: Cow<'static, str>,
    pool: Cow<'static, str>,
    selection_priority: i32,
    placement_priority: i32,
}

/// `StructureTemplate.JigsawBlockInfo` placed with a rotation and offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Jigsaw {
    pos: BlockPos,
    front: Direction,
    top: Direction,
    joint: JointType,
    name: &'static str,
    target: &'static str,
    pool: &'static str,
    selection_priority: i32,
    placement_priority: i32,
}

/// `ResourceLocation.parse`: ids without a namespace are in `minecraft`.
fn normalize_id(s: &'static str) -> Cow<'static, str> {
    if s.contains(':') {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(format!("minecraft:{s}"))
    }
}

fn direction_named(s: &str) -> Direction {
    match s {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        other => panic!("bad jigsaw orientation direction {other}"),
    }
}

/// `JigsawBlock.getFrontFacing` / `getTopFacing` of a jigsaw state.
fn jigsaw_orientation(state: JavaState) -> (Direction, Direction) {
    let o = state
        .value("orientation")
        .expect("jigsaw block without orientation");
    let (front, top) = o
        .split_once('_')
        .expect("jigsaw orientation is <front>_<top>");
    (direction_named(front), direction_named(top))
}

/// Jigsaw blocks of each template, decoded on first use.
static TEMPLATE_JIGSAWS: LazyLock<Vec<OnceLock<Vec<TemplateJigsaw>>>> =
    LazyLock::new(|| TEMPLATE_NAMES.iter().map(|_| OnceLock::new()).collect());

/// A template by id with its jigsaw blocks (`Palette.jigsaws()` of palette 0).
fn template_with_jigsaws(name: &str) -> (&'static Template, &'static [TemplateJigsaw]) {
    let template = template_data::template_or_empty(name);
    // Missing ids (one in vanilla data) are Java's empty template: no jigsaws.
    let Ok(index) = TEMPLATE_NAMES.binary_search(&name) else {
        return (template, &[]);
    };
    let jigsaws = TEMPLATE_JIGSAWS[index].get_or_init(|| {
        let palette = template
            .palettes
            .first()
            .unwrap_or_else(|| panic!("template {name} has no palette"));
        palette
            .iter()
            .filter(|tb| tb.state.is(b::JIGSAW))
            .map(|tb| {
                let info = tb
                    .block_entity
                    .and_then(|i| template.block_entities.get(i as usize));
                let Some(BlockEntityInfo::Jigsaw {
                    name: jname,
                    target,
                    pool,
                    joint,
                    selection_priority,
                    placement_priority,
                    ..
                }) = info
                else {
                    panic!("jigsaw block without jigsaw data in template {name}");
                };
                let (front, top) = jigsaw_orientation(tb.state);
                TemplateJigsaw {
                    pos: BlockPos::new(tb.x as i32, tb.y as i32, tb.z as i32),
                    front,
                    top,
                    joint: (*joint).unwrap_or(if front.is_horizontal() {
                        JointType::Aligned
                    } else {
                        JointType::Rollable
                    }),
                    name: normalize_id(jname),
                    target: normalize_id(target),
                    pool: normalize_id(pool),
                    selection_priority: *selection_priority,
                    placement_priority: *placement_priority,
                }
            })
            .collect()
    });
    (template, jigsaws)
}

/// `JigsawBlock.canAttach(source, target)`.
fn can_attach(source: &Jigsaw, target: &Jigsaw) -> bool {
    source.front == target.front.opposite()
        && (source.joint == JointType::Rollable || source.top == target.top)
        && source.target == target.name
}

// ---------------------------------------------------------------------------
// Pool elements and pools

/// `StructurePoolElement.getProjection()`.
fn projection_of(element: &PoolElement) -> Projection {
    match *element {
        PoolElement::Single { projection, .. }
        | PoolElement::Feature { projection, .. }
        | PoolElement::List { projection, .. } => projection,
        PoolElement::Empty => Projection::Rigid,
    }
}

/// `StructurePoolElement.getGroundLevelDelta()` (the same for every element type).
const GROUND_LEVEL_DELTA: i32 = 1;

/// `StructurePoolElement.getBoundingBox(manager, pos, rotation)`.
fn element_box(element: &PoolElement, pos: BlockPos, rotation: Rotation) -> BoundingBox {
    match *element {
        PoolElement::Single { template, .. } => {
            let (t, _) = template_with_jigsaws(template);
            template_box(t, pos, rotation, BlockPos::new(0, 0, 0), Mirror::None)
        }
        PoolElement::Feature { .. } => BoundingBox::new(pos.x, pos.y, pos.z, pos.x, pos.y, pos.z),
        PoolElement::List { elements, .. } => {
            let mut boxes = elements
                .iter()
                .filter(|e| !matches!(e, PoolElement::Empty))
                .map(|e| element_box(e, pos, rotation));
            let mut out = boxes.next().expect("list pool element without elements");
            for bx in boxes {
                out.encapsulate(&bx);
            }
            out
        }
        PoolElement::Empty => panic!("empty pool element has no bounding box"),
    }
}

/// `StructurePoolElement.getShuffledJigsawBlocks(manager, pos, rotation, random)`.
fn element_jigsaws(
    element: &PoolElement,
    pos: BlockPos,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<Jigsaw> {
    match *element {
        PoolElement::Single { template, .. } => {
            let (_, stored) = template_with_jigsaws(template);
            let mut list: Vec<Jigsaw> = stored
                .iter()
                .map(|j| {
                    let p = transform(j.pos, Mirror::None, rotation, BlockPos::new(0, 0, 0));
                    Jigsaw {
                        pos: p.offset(pos.x, pos.y, pos.z),
                        front: rotation.rotate_dir(j.front),
                        top: rotation.rotate_dir(j.top),
                        joint: j.joint,
                        name: static_str(&j.name),
                        target: static_str(&j.target),
                        pool: static_str(&j.pool),
                        selection_priority: j.selection_priority,
                        placement_priority: j.placement_priority,
                    }
                })
                .collect();
            shuffle(&mut list, rng);
            list.sort_by_key(|j| std::cmp::Reverse(j.selection_priority));
            list
        }
        // FeaturePoolElement's built-in jigsaw: facing down, pointing nowhere.
        PoolElement::Feature { .. } => vec![Jigsaw {
            pos,
            front: Direction::Down,
            top: Direction::South,
            joint: JointType::Rollable,
            name: "minecraft:bottom",
            target: EMPTY_POOL,
            pool: EMPTY_POOL,
            selection_priority: 0,
            placement_priority: 0,
        }],
        PoolElement::List { elements, .. } => element_jigsaws(&elements[0], pos, rotation, rng),
        PoolElement::Empty => Vec::new(),
    }
}

/// Borrow a cached id for the program's lifetime (the cache is a static).
#[allow(clippy::ptr_arg)]
fn static_str(s: &'static Cow<'static, str>) -> &'static str {
    s
}

/// Number of entries of `StructureTemplatePool.templates` (weights expanded).
fn pool_size(pool: &TemplatePool) -> i32 {
    pool.elements.iter().map(|e| e.1).sum()
}

/// `StructureTemplatePool.getRandomTemplate(random)`.
fn random_template(pool: &'static TemplatePool, rng: &mut LegacyRandom) -> &'static PoolElement {
    let size = pool_size(pool);
    if size == 0 {
        return &EMPTY_ELEMENT;
    }
    let mut i = rng.next_int_bounded(size);
    for (element, weight) in pool.elements {
        if i < *weight {
            return element;
        }
        i -= weight;
    }
    unreachable!("template index below the pool size")
}

/// `StructureTemplatePool.getShuffledTemplates(random)`, appended to `out`.
fn push_shuffled_templates(
    pool: &'static TemplatePool,
    rng: &mut LegacyRandom,
    out: &mut Vec<&'static PoolElement>,
) {
    let start = out.len();
    for (element, weight) in pool.elements {
        for _ in 0..*weight {
            out.push(element);
        }
    }
    shuffle(&mut out[start..], rng);
}

/// `StructureTemplatePool.getMaxSize(manager)`: tallest element at rotation none.
fn pool_max_size(pool: &TemplatePool) -> i32 {
    pool.elements
        .iter()
        .filter(|(e, _)| !matches!(e, PoolElement::Empty))
        .map(|(e, _)| element_box(e, BlockPos::new(0, 0, 0), Rotation::None).y_span())
        .max()
        .unwrap_or(0)
}

/// Pool by id, as Java's registry lookup.
fn find_pool(name: &str) -> Option<&'static TemplatePool> {
    jigsaw::pool(name)
}

// ---------------------------------------------------------------------------
// Free space

/// The free space a piece may occupy: whole blocks of `area` not covered by
/// any `carved` box (Java's voxel shape of the same blocks).
#[derive(Debug, Clone)]
struct FreeShape {
    /// Inclusive block box; inverted (min > max) when Java's shape is empty.
    area: BoundingBox,
    carved: Vec<BoundingBox>,
}

impl FreeShape {
    fn new(area: BoundingBox) -> Self {
        Self {
            area,
            carved: Vec::new(),
        }
    }

    /// `!joinIsNotEmpty(shape, create(AABB.of(bx).deflate(0.25)), ONLY_SECOND)`:
    /// every block of `bx` is free.
    fn fits(&self, bx: &BoundingBox) -> bool {
        let a = &self.area;
        bx.min_x >= a.min_x
            && bx.max_x <= a.max_x
            && bx.min_y >= a.min_y
            && bx.max_y <= a.max_y
            && bx.min_z >= a.min_z
            && bx.max_z <= a.max_z
            && !self.carved.iter().any(|c| c.intersects(bx))
    }

    /// `joinUnoptimized(shape, create(AABB.of(bx)), ONLY_FIRST)`.
    fn carve(&mut self, bx: BoundingBox) {
        self.carved.push(bx);
    }
}

// ---------------------------------------------------------------------------
// Pieces

/// `JigsawJunction`: where two pieces meet, for terrain adaptation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JigsawJunction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
    pub delta_y: i32,
    pub dest_projection: Projection,
}

/// `PoolElementStructurePiece`: one pool element placed at a position and
/// rotation (orientation-free piece, generation depth 0).
#[derive(Debug, Clone)]
pub struct PoolElementPiece {
    pub base: PieceBase,
    pub element: &'static PoolElement,
    pub position: BlockPos,
    pub ground_level_delta: i32,
    pub rotation: Rotation,
    pub junctions: Vec<JigsawJunction>,
    /// The structure's `liquid_settings` (`apply_waterlogging`).
    pub apply_waterlogging: bool,
}

impl PoolElementPiece {
    /// A piece with an explicit bounding box (possibly expanded beyond the
    /// element's own box).
    pub fn new(
        element: &'static PoolElement,
        position: BlockPos,
        ground_level_delta: i32,
        rotation: Rotation,
        bbox: BoundingBox,
        apply_waterlogging: bool,
    ) -> Self {
        Self {
            base: PieceBase::new(0, bbox),
            element,
            position,
            ground_level_delta,
            rotation,
            junctions: Vec::new(),
            apply_waterlogging,
        }
    }
}

impl StructurePiece for PoolElementPiece {
    fn base(&self) -> &PieceBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.base
    }

    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.base.bbox = self.base.bbox.moved(dx, dy, dz);
        self.position = self.position.offset(dx, dy, dz);
    }

    /// `place(level, ..., random, box, pivot, keepJigsaws = false)`.
    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        _ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        pivot: BlockPos,
    ) {
        place_element(
            self.element,
            projection_of(self.element),
            world,
            rng,
            self.position,
            pivot,
            self.rotation,
            chunk_box,
            self.apply_waterlogging,
        );
    }
}

/// `StructurePoolElement.place(...)`. `projection` is the element's own, or
/// the enclosing list's (Java copies a list's projection onto its elements).
#[allow(clippy::too_many_arguments)]
fn place_element(
    element: &'static PoolElement,
    projection: Projection,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    offset: BlockPos,
    pivot: BlockPos,
    rotation: Rotation,
    chunk_box: &BoundingBox,
    apply_waterlogging: bool,
) -> bool {
    match *element {
        PoolElement::Single {
            template,
            processors,
            override_liquid,
            legacy,
            ..
        } => {
            let (t, _) = template_with_jigsaws(template);
            let mut settings = PlaceSettings::new(t);
            settings.rotation = rotation;
            settings.bounding_box = Some(*chunk_box);
            settings.ignore_entities = false;
            settings.apply_waterlogging = override_liquid.unwrap_or(apply_waterlogging);
            let mut list: Vec<&'static Processor> = Vec::with_capacity(processors.len() + 3);
            if !legacy {
                list.push(&IGNORE_STRUCTURE_BLOCK);
            }
            list.push(&JIGSAW_REPLACEMENT);
            list.extend(processors.iter());
            if projection == Projection::TerrainMatching {
                list.push(&TERRAIN_MATCHING_GRAVITY);
            }
            if legacy {
                list.push(&IGNORE_STRUCTURE_AND_AIR);
            }
            settings.processors = list;
            let mut placed = Vec::new();
            if !place_in_world(world, offset, pivot, &mut settings, rng, &mut |be| {
                placed.push(be)
            }) {
                return false;
            }
            record_block_entities(world, &placed);
            true
        }
        PoolElement::Feature { feature, .. } => {
            crate::features::generated::place_placed(feature, world, rng, offset)
        }
        PoolElement::List {
            elements,
            projection: list_projection,
        } => elements.iter().all(|e| {
            place_element(
                e,
                list_projection,
                world,
                rng,
                offset,
                pivot,
                rotation,
                chunk_box,
                apply_waterlogging,
            )
        }),
        PoolElement::Empty => true,
    }
}

// ---------------------------------------------------------------------------
// Generation point

/// The start found by [`find_generation_point`], before assembly.
#[derive(Debug, Clone)]
struct Start {
    piece: PoolElementPiece,
    /// Stub position: centre of the start box at the anchor height.
    center: BlockPos,
    aliases: PoolAliasLookup,
}

/// `JigsawStructure.findGenerationPoint` for `StructureKind::Jigsaw`.
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let StructureKind::Jigsaw(structure) = g.def.kind else {
        panic!("{} is not a jigsaw structure", g.def.name);
    };
    let start = find_start(g, structure)?;
    let center = start.center;
    Some(GenerationStub {
        position: center,
        build: Box::new(move |g: &mut GenContext<'_>| {
            let ctx = g.ctx;
            assemble(ctx, &mut g.random, structure, start)
                .pieces
                .into_iter()
                .map(|p| Box::new(p) as Box<dyn StructurePiece>)
                .collect()
        }),
    })
}

/// `JigsawPlacement.addPieces` up to the generation stub.
fn find_start(g: &mut GenContext<'_>, s: &'static JigsawStructure) -> Option<Start> {
    let ctx = g.ctx;
    let y = s
        .start_height
        .sample(&mut g.random, ctx.min_y(), ctx.height());
    let pos = BlockPos::new(g.min_block_x(), y, g.min_block_z());
    let aliases = PoolAliasLookup::create(s.pool_aliases, pos, ctx.seed());
    let rotation = Rotation::random(&mut g.random);
    let start_pool = find_pool(aliases.lookup(s.start_pool))
        .or_else(|| find_pool(s.start_pool))
        .unwrap_or_else(|| panic!("missing start pool {}", s.start_pool));
    let element = random_template(start_pool, &mut g.random);
    if matches!(element, PoolElement::Empty) {
        return None;
    }
    let anchor = match s.start_jigsaw_name {
        Some(name) => {
            let name = normalize_id(name);
            let jigsaws = element_jigsaws(element, pos, rotation, &mut g.random);
            jigsaws.iter().find(|j| j.name == name).map(|j| j.pos)?
        }
        None => pos,
    };
    let anchor_offset = BlockPos::new(anchor.x - pos.x, anchor.y - pos.y, anchor.z - pos.z);
    let origin = BlockPos::new(
        pos.x - anchor_offset.x,
        pos.y - anchor_offset.y,
        pos.z - anchor_offset.z,
    );
    let mut piece = PoolElementPiece::new(
        element,
        origin,
        GROUND_LEVEL_DELTA,
        rotation,
        element_box(element, origin, rotation),
        s.apply_waterlogging,
    );
    let bx = piece.base.bbox;
    let cx = (bx.max_x + bx.min_x) / 2;
    let cz = (bx.max_z + bx.min_z) / 2;
    let ground = match s.project_start_to_heightmap {
        None => origin.y,
        Some(kind) => pos.y + ctx.first_free_height(cx, cz, kind),
    };
    let piece_ground = bx.min_y + piece.ground_level_delta;
    piece.translate(0, ground - piece_ground, 0);
    if start_too_close_to_limits(ctx, s.dimension_padding, &piece.base.bbox) {
        return None;
    }
    Some(Start {
        piece,
        center: BlockPos::new(cx, ground + anchor_offset.y, cz),
        aliases,
    })
}

/// `isStartTooCloseToWorldHeightLimits`.
fn start_too_close_to_limits(
    ctx: &dyn StructureContext,
    padding: (i32, i32),
    bx: &BoundingBox,
) -> bool {
    if padding == (0, 0) {
        return false;
    }
    let bottom = ctx.min_y() + padding.0;
    let top = ctx.min_y() + ctx.height() - padding.1;
    bx.min_y < bottom || bx.max_y > top
}

// ---------------------------------------------------------------------------
// Assembly

/// Assembled pieces with per-piece bookkeeping (the bookkeeping is only read
/// by the tests' structural checks).
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
struct Assembly {
    pieces: Vec<PoolElementPiece>,
    /// Generation depth of each piece (start = 0).
    depths: Vec<i32>,
    /// Free shape each piece was carved from (the start: shape 0).
    shapes: Vec<usize>,
}

/// `PieceState`.
#[derive(Debug, Clone, Copy)]
struct PieceState {
    piece: usize,
    free: usize,
    depth: i32,
}

/// `SequencedPriorityIterator`: highest priority first, FIFO within one.
#[derive(Debug)]
struct PriorityQueue<T> {
    queues: BTreeMap<i32, VecDeque<T>>,
}

impl<T> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self {
            queues: BTreeMap::new(),
        }
    }
}

impl<T> PriorityQueue<T> {
    fn push(&mut self, value: T, priority: i32) {
        self.queues.entry(priority).or_default().push_back(value);
    }

    fn pop(&mut self) -> Option<T> {
        let mut entry = self.queues.last_entry()?;
        let value = entry.get_mut().pop_front();
        if entry.get().is_empty() {
            entry.remove();
        }
        value
    }
}

/// `JigsawPlacement.Placer`.
struct Placer<'r> {
    ctx: &'r dyn StructureContext,
    random: &'r mut LegacyRandom,
    max_depth: i32,
    use_expansion_hack: bool,
    apply_waterlogging: bool,
    aliases: &'r PoolAliasLookup,
    pieces: Vec<PoolElementPiece>,
    depths: Vec<i32>,
    piece_shapes: Vec<usize>,
    shapes: Vec<FreeShape>,
    placing: PriorityQueue<PieceState>,
}

/// The builder half of `JigsawPlacement.addPieces`.
fn assemble(
    ctx: &dyn StructureContext,
    random: &mut LegacyRandom,
    s: &'static JigsawStructure,
    start: Start,
) -> Assembly {
    let Start {
        piece,
        center,
        aliases,
    } = start;
    let start_box = piece.base.bbox;
    let mut placer = Placer {
        ctx,
        random,
        max_depth: s.size,
        use_expansion_hack: s.use_expansion_hack,
        apply_waterlogging: s.apply_waterlogging,
        aliases: &aliases,
        pieces: vec![piece],
        depths: vec![0],
        piece_shapes: vec![0],
        shapes: Vec::new(),
        placing: PriorityQueue::default(),
    };
    if s.size > 0 {
        let d = s.max_distance_from_center;
        let min_y = ctx.min_y();
        let max_y_exclusive = min_y + ctx.height();
        let low = (center.y - d).max(min_y + s.dimension_padding.0);
        let high = (center.y + d + 1).min(max_y_exclusive - s.dimension_padding.1);
        let mut global = FreeShape::new(BoundingBox::new(
            center.x - d,
            low,
            center.z - d,
            center.x + d,
            high - 1,
            center.z + d,
        ));
        global.carve(start_box);
        placer.shapes.push(global);
        placer.try_placing_children(0, 0, 0);
        while let Some(state) = placer.placing.pop() {
            placer.try_placing_children(state.piece, state.free, state.depth);
        }
    }
    Assembly {
        pieces: placer.pieces,
        depths: placer.depths,
        shapes: placer.piece_shapes,
    }
}

impl Placer<'_> {
    /// Surface height at a parent jigsaw (`getFirstFreeHeight(WORLD_SURFACE_WG)`).
    fn surface(&self, pos: BlockPos) -> i32 {
        self.ctx
            .first_free_height(pos.x, pos.z, Heightmap::WorldSurfaceWg)
    }

    /// Height the expansion hack reserves above a candidate: the tallest
    /// element of the pools its inward-facing jigsaws target.
    fn expansion_height(&self, jigsaws: &[Jigsaw], bx: &BoundingBox) -> i32 {
        jigsaws
            .iter()
            .map(|j| {
                if !bx.is_inside(j.pos.rel(j.front)) {
                    return 0;
                }
                let Some(pool) = find_pool(self.aliases.lookup(j.pool)) else {
                    return 0;
                };
                let fallback = find_pool(pool.fallback).map_or(0, pool_max_size);
                pool_max_size(pool).max(fallback)
            })
            .max()
            .unwrap_or(0)
    }

    /// `Placer.tryPlacingChildren`.
    fn try_placing_children(&mut self, piece_index: usize, free: usize, depth: i32) {
        let parent = &self.pieces[piece_index];
        let element = parent.element;
        let position = parent.position;
        let rotation = parent.rotation;
        let parent_ground_delta = parent.ground_level_delta;
        let parent_box = parent.base.bbox;
        let parent_projection = projection_of(element);
        let parent_rigid = parent_projection == Projection::Rigid;
        let parent_min_y = parent_box.min_y;
        let mut interior: Option<usize> = None;
        let mut candidates: Vec<&'static PoolElement> = Vec::new();

        let jigsaws = element_jigsaws(element, position, rotation, self.random);
        'jigsaws: for jigsaw in &jigsaws {
            let front = jigsaw.front;
            let jigsaw_pos = jigsaw.pos;
            let target_pos = jigsaw_pos.rel(front);
            let rel_y = jigsaw_pos.y - parent_min_y;
            let mut surface = i32::MIN;
            let Some(pool) = find_pool(self.aliases.lookup(jigsaw.pool)) else {
                continue;
            };
            if pool_size(pool) == 0 && pool.name != EMPTY_POOL {
                continue;
            }
            let Some(fallback) = find_pool(pool.fallback) else {
                continue;
            };
            if pool_size(fallback) == 0 && fallback.name != EMPTY_POOL {
                continue;
            }
            let shape = if parent_box.is_inside(target_pos) {
                *interior.get_or_insert_with(|| {
                    self.shapes.push(FreeShape::new(parent_box));
                    self.shapes.len() - 1
                })
            } else {
                free
            };
            candidates.clear();
            if depth != self.max_depth {
                push_shuffled_templates(pool, self.random, &mut candidates);
            }
            push_shuffled_templates(fallback, self.random, &mut candidates);
            let placement_priority = jigsaw.placement_priority;

            for &candidate in &candidates {
                if matches!(candidate, PoolElement::Empty) {
                    break;
                }
                let candidate_projection = projection_of(candidate);
                let candidate_rigid = candidate_projection == Projection::Rigid;
                for candidate_rotation in Rotation::shuffled(self.random) {
                    let origin = BlockPos::new(0, 0, 0);
                    let candidate_jigsaws =
                        element_jigsaws(candidate, origin, candidate_rotation, self.random);
                    let origin_box = element_box(candidate, origin, candidate_rotation);
                    let expansion = if self.use_expansion_hack && origin_box.y_span() <= 16 {
                        self.expansion_height(&candidate_jigsaws, &origin_box)
                    } else {
                        0
                    };
                    for candidate_jigsaw in &candidate_jigsaws {
                        if !can_attach(jigsaw, candidate_jigsaw) {
                            continue;
                        }
                        let local = candidate_jigsaw.pos;
                        let candidate_pos = BlockPos::new(
                            target_pos.x - local.x,
                            target_pos.y - local.y,
                            target_pos.z - local.z,
                        );
                        let candidate_box =
                            element_box(candidate, candidate_pos, candidate_rotation);
                        let candidate_min_y = candidate_box.min_y;
                        let local_y = local.y;
                        let step = rel_y - local_y + front.offset().1;
                        let ground = if parent_rigid && candidate_rigid {
                            parent_min_y + step
                        } else {
                            if surface == i32::MIN {
                                surface = self.surface(jigsaw_pos);
                            }
                            surface - local_y
                        };
                        let dy = ground - candidate_min_y;
                        let mut placed_box = candidate_box.moved(0, dy, 0);
                        let placed_pos = candidate_pos.offset(0, dy, 0);
                        if expansion > 0 {
                            let height = (expansion + 1).max(placed_box.max_y - placed_box.min_y);
                            let top = placed_box.min_y + height;
                            placed_box.encapsulate(&BoundingBox::new(
                                placed_box.min_x,
                                top,
                                placed_box.min_z,
                                placed_box.min_x,
                                top,
                                placed_box.min_z,
                            ));
                        }
                        if !self.shapes[shape].fits(&placed_box) {
                            continue;
                        }
                        self.shapes[shape].carve(placed_box);
                        let child_ground_delta = if candidate_rigid {
                            parent_ground_delta - step
                        } else {
                            GROUND_LEVEL_DELTA
                        };
                        let mut child = PoolElementPiece::new(
                            candidate,
                            placed_pos,
                            child_ground_delta,
                            candidate_rotation,
                            placed_box,
                            self.apply_waterlogging,
                        );
                        let junction_y = if parent_rigid {
                            parent_min_y + rel_y
                        } else if candidate_rigid {
                            ground + local_y
                        } else {
                            if surface == i32::MIN {
                                surface = self.surface(jigsaw_pos);
                            }
                            surface + step / 2
                        };
                        self.pieces[piece_index].junctions.push(JigsawJunction {
                            source_x: target_pos.x,
                            source_ground_y: junction_y - rel_y + parent_ground_delta,
                            source_z: target_pos.z,
                            delta_y: step,
                            dest_projection: candidate_projection,
                        });
                        child.junctions.push(JigsawJunction {
                            source_x: jigsaw_pos.x,
                            source_ground_y: junction_y - local_y + child_ground_delta,
                            source_z: jigsaw_pos.z,
                            delta_y: -step,
                            dest_projection: parent_projection,
                        });
                        self.pieces.push(child);
                        self.depths.push(depth + 1);
                        self.piece_shapes.push(shape);
                        if depth < self.max_depth {
                            self.placing.push(
                                PieceState {
                                    piece: self.pieces.len() - 1,
                                    free: shape,
                                    depth: depth + 1,
                                },
                                placement_priority,
                            );
                        }
                        continue 'jigsaws;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;
    use crate::structures::generated::STRUCTURES;
    use crate::structures::start::{StructureDef, TerrainAdjustment};

    /// Flat stone terrain up to `surface - 1`.
    struct Flat {
        seed: i64,
        surface: i32,
    }

    impl StructureContext for Flat {
        fn seed(&self) -> i64 {
            self.seed
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn height(&self) -> i32 {
            384
        }
        fn sea_level(&self) -> i32 {
            63
        }
        fn noise_biome(&self, _: i32, _: i32, _: i32) -> u8 {
            0
        }
        fn base_height(&self, _: i32, _: i32, _: Heightmap) -> i32 {
            self.surface
        }
        fn base_column(&self, _: i32, _: i32, out: &mut [JavaState]) {
            for (i, s) in out.iter_mut().enumerate() {
                *s = if -64 + (i as i32) < self.surface {
                    states::STONE
                } else {
                    states::AIR
                };
            }
        }
    }

    fn def(name: &str) -> &'static StructureDef {
        STRUCTURES
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("no structure {name}"))
    }

    fn jigsaw_of(def: &StructureDef) -> &'static JigsawStructure {
        match def.kind {
            StructureKind::Jigsaw(j) => j,
            _ => panic!("{} is not jigsaw", def.name),
        }
    }

    /// Start + assembly with bookkeeping, as the stub builder runs it.
    fn assemble_at(
        ctx: &Flat,
        def: &'static StructureDef,
        cx: i32,
        cz: i32,
    ) -> Option<(Start, Assembly)> {
        let mut g = GenContext::new(ctx, def, cx, cz);
        let s = jigsaw_of(def);
        let start = find_start(&mut g, s)?;
        let out = assemble(ctx, &mut g.random, s, start.clone());
        Some((start, out))
    }

    type Summary = Vec<(BoundingBox, BlockPos, Rotation, i32, usize)>;

    fn summary(a: &Assembly) -> Summary {
        a.pieces
            .iter()
            .zip(&a.depths)
            .map(|(p, d)| {
                (
                    p.base.bbox,
                    p.position,
                    p.rotation,
                    *d,
                    p.element as *const PoolElement as usize,
                )
            })
            .collect()
    }

    /// Structural checks every assembled start must pass.
    fn check_assembly(s: &JigsawStructure, start: &Start, a: &Assembly, ctx: &Flat) {
        let d = s.max_distance_from_center;
        let c = start.center;
        let top = ctx.min_y() + ctx.height();
        let low = (c.y - d).max(ctx.min_y() + s.dimension_padding.0);
        let high = (c.y + d + 1).min(top - s.dimension_padding.1) - 1;
        for (i, p) in a.pieces.iter().enumerate() {
            let bx = p.base.bbox;
            if i == 0 {
                continue;
            }
            assert!(
                bx.min_x >= c.x - d
                    && bx.max_x <= c.x + d
                    && bx.min_z >= c.z - d
                    && bx.max_z <= c.z + d,
                "piece {i} {bx:?} outside max distance of {c:?}"
            );
            assert!(
                bx.min_y >= low && bx.max_y <= high,
                "piece {i} {bx:?} outside vertical range"
            );
            assert!(a.depths[i] <= s.size + 1, "piece {i} deeper than the limit");
        }
        // Pieces carved from one free shape never overlap each other, and
        // pieces carved from the outer shape never overlap the start.
        for i in 0..a.pieces.len() {
            for j in (i + 1)..a.pieces.len() {
                let same_shape = a.shapes[i] == a.shapes[j];
                if same_shape {
                    let (bi, bj) = (a.pieces[i].base.bbox, a.pieces[j].base.bbox);
                    assert!(
                        !bi.intersects(&bj),
                        "pieces {i} {bi:?} and {j} {bj:?} overlap"
                    );
                }
            }
        }
        // Every piece but the start is connected to its parent by a junction pair.
        assert_eq!(
            a.pieces.iter().map(|p| p.junctions.len()).sum::<usize>(),
            2 * (a.pieces.len() - 1)
        );
    }

    fn run_structure(name: &str, min_pieces: usize) {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let def = def(name);
        let s = jigsaw_of(def);
        let ctx = Flat {
            seed: 12345,
            surface: 70,
        };
        let mut largest = 0;
        for (cx, cz) in [(0, 0), (7, -3), (-20, 11), (101, 55)] {
            let (start, a) = assemble_at(&ctx, def, cx, cz).expect("start");
            let (_, again) = assemble_at(&ctx, def, cx, cz).expect("start");
            assert_eq!(summary(&a), summary(&again), "{name} not deterministic");
            check_assembly(s, &start, &a, &ctx);
            largest = largest.max(a.pieces.len());
        }
        assert!(largest >= min_pieces, "{name}: only {largest} pieces");
    }

    #[test]
    fn village_plains_assembles() {
        run_structure("minecraft:village_plains", 20);
    }

    #[test]
    fn pillager_outpost_assembles() {
        run_structure("minecraft:pillager_outpost", 2);
    }

    #[test]
    fn trail_ruins_assembles() {
        run_structure("minecraft:trail_ruins", 5);
    }

    #[test]
    fn trial_chambers_assemble() {
        run_structure("minecraft:trial_chambers", 50);
    }

    #[test]
    fn bastion_and_ancient_city_assemble() {
        run_structure("minecraft:bastion_remnant", 10);
        run_structure("minecraft:ancient_city", 20);
    }

    #[test]
    fn start_projects_to_surface() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = Flat {
            seed: 99,
            surface: 80,
        };
        // Trail ruins: start height -15 below the surface, ground delta 1.
        let (start, _) = assemble_at(&ctx, def("minecraft:trail_ruins"), 3, 4).expect("start");
        assert_eq!(start.piece.base.bbox.min_y, 80 - 15 - 1);
        assert_eq!(start.center.y, 80 - 15);
        // Villages: start height 0, so the start sits one below the surface.
        let (start, _) = assemble_at(&ctx, def("minecraft:village_plains"), 3, 4).expect("start");
        assert_eq!(start.piece.base.bbox.min_y, 79);
        assert_eq!(start.center.y, 80);
    }

    #[test]
    fn depth_limit_bounds_the_village() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        static SHALLOW: JigsawStructure = JigsawStructure {
            start_pool: "minecraft:village/plains/town_centers",
            start_jigsaw_name: None,
            size: 1,
            start_height: crate::structures::start::StructureHeight::Constant(
                crate::structures::start::Anchor::Absolute(0),
            ),
            use_expansion_hack: true,
            project_start_to_heightmap: Some(Heightmap::WorldSurfaceWg),
            max_distance_from_center: 80,
            pool_aliases: &[],
            dimension_padding: (0, 0),
            apply_waterlogging: true,
        };
        static SHALLOW_DEF: StructureDef = StructureDef {
            name: "test:shallow_village",
            biomes: u128::MAX,
            step: 4,
            terrain_adaptation: TerrainAdjustment::BeardThin,
            kind: StructureKind::Jigsaw(&SHALLOW),
        };
        let ctx = Flat {
            seed: 4242,
            surface: 70,
        };
        let full = def("minecraft:village_plains");
        for (cx, cz) in [(0, 0), (5, 9), (-30, 2)] {
            let (start, shallow) = assemble_at(&ctx, &SHALLOW_DEF, cx, cz).expect("start");
            check_assembly(&SHALLOW, &start, &shallow, &ctx);
            assert!(shallow.depths.iter().all(|d| *d <= 2));
            // The full-size village grows past the fallback-only depth.
            let (_, deep) = assemble_at(&ctx, full, cx, cz).expect("start");
            assert!(deep.pieces.len() > shallow.pieces.len());
            assert!(deep.depths.iter().any(|d| *d > 2));
        }
    }

    #[test]
    fn start_jigsaw_anchors_the_start() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        // Ancient city starts anchor on `city_anchor`: the anchor lands on the
        // sampled start position, then the rigid start drops by its ground
        // level delta (1).
        let ctx = Flat {
            seed: 7,
            surface: 70,
        };
        let def = def("minecraft:ancient_city");
        let s = jigsaw_of(def);
        let mut g = GenContext::new(&ctx, def, 2, 3);
        let start = find_start(&mut g, s).expect("ancient city start");
        assert_eq!(start.center.y, -27);
        let mut rng = LegacyRandom::new(0);
        let anchors: Vec<_> = element_jigsaws(
            start.piece.element,
            start.piece.position,
            start.piece.rotation,
            &mut rng,
        )
        .into_iter()
        .filter(|j| j.name == "minecraft:city_anchor")
        .map(|j| j.pos)
        .collect();
        assert!(anchors.contains(&BlockPos::new(32, -28, 48)), "{anchors:?}");
    }

    #[test]
    fn pool_aliases_resolve_consistently() {
        let s = jigsaw_of(def("minecraft:trial_chambers"));
        let ranged = "minecraft:trial_chambers/spawner/contents/ranged";
        let slow = "minecraft:trial_chambers/spawner/contents/slow_ranged";
        let melee = "minecraft:trial_chambers/spawner/contents/melee";
        let mut seen = std::collections::HashSet::new();
        for i in 0..64 {
            let pos = BlockPos::new(i * 16, -30, i * -48);
            let lookup = PoolAliasLookup::create(s.pool_aliases, pos, 1000 + i as i64);
            assert_eq!(
                lookup,
                PoolAliasLookup::create(s.pool_aliases, pos, 1000 + i as i64)
            );
            let r = lookup.lookup(ranged);
            let mob = r.rsplit('/').next().unwrap();
            assert!(
                ["skeleton", "stray", "poison_skeleton"].contains(&mob),
                "{r}"
            );
            // A random group binds both aliases from the same group.
            assert_eq!(
                lookup.lookup(slow),
                format!("minecraft:trial_chambers/spawner/slow_ranged/{mob}")
            );
            assert!(
                lookup
                    .lookup(melee)
                    .starts_with("minecraft:trial_chambers/spawner/melee/")
            );
            assert_eq!(lookup.lookup("minecraft:other"), "minecraft:other");
            seen.insert(mob.to_string());
        }
        assert_eq!(seen.len(), 3, "every group is reachable");
        assert_eq!(
            PoolAliasLookup::create(&[], BlockPos::new(0, 0, 0), 1).lookup(ranged),
            ranged
        );
    }

    #[test]
    fn free_shape_matches_block_sets() {
        // Compare against an explicit set of free blocks in a small grid.
        let mut rng = LegacyRandom::new(31337);
        let rand_box = |rng: &mut LegacyRandom| {
            let a = [
                rng.next_int_bounded(12),
                rng.next_int_bounded(12),
                rng.next_int_bounded(12),
            ];
            let s = [
                1 + rng.next_int_bounded(5),
                1 + rng.next_int_bounded(5),
                1 + rng.next_int_bounded(5),
            ];
            BoundingBox::new(
                a[0],
                a[1],
                a[2],
                a[0] + s[0] - 1,
                a[1] + s[1] - 1,
                a[2] + s[2] - 1,
            )
        };
        for _ in 0..50 {
            let area = BoundingBox::new(2, 2, 2, 11, 11, 11);
            let mut shape = FreeShape::new(area);
            let mut free = std::collections::HashSet::new();
            for x in 2..=11 {
                for y in 2..=11 {
                    for z in 2..=11 {
                        free.insert((x, y, z));
                    }
                }
            }
            for _ in 0..40 {
                let bx = rand_box(&mut rng);
                let mut all_free = true;
                for x in bx.min_x..=bx.max_x {
                    for y in bx.min_y..=bx.max_y {
                        for z in bx.min_z..=bx.max_z {
                            all_free &= free.contains(&(x, y, z));
                        }
                    }
                }
                assert_eq!(shape.fits(&bx), all_free, "{bx:?}");
                if all_free || rng.next_bool() {
                    shape.carve(bx);
                    for x in bx.min_x..=bx.max_x {
                        for y in bx.min_y..=bx.max_y {
                            for z in bx.min_z..=bx.max_z {
                                free.remove(&(x, y, z));
                            }
                        }
                    }
                }
            }
        }
        // An inverted area (Java's empty shape) accepts nothing.
        let empty = FreeShape::new(BoundingBox::new(0, 5, 0, 10, 4, 10));
        assert!(!empty.fits(&BoundingBox::new(1, 4, 1, 1, 4, 1)));
    }

    #[test]
    fn priority_queue_is_highest_first_then_fifo() {
        let mut q = PriorityQueue::default();
        q.push('a', 0);
        q.push('b', 5);
        q.push('c', 0);
        q.push('d', 5);
        assert_eq!(q.pop(), Some('b'));
        q.push('e', 7);
        q.push('f', 5);
        let rest: Vec<_> = std::iter::from_fn(|| q.pop()).collect();
        assert_eq!(rest, vec!['e', 'd', 'f', 'a', 'c']);
    }

    #[test]
    fn outpost_post_process_stays_in_chunk() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = Flat {
            seed: 555,
            surface: 70,
        };
        let def = def("minecraft:pillager_outpost");
        let (_, a) = assemble_at(&ctx, def, 10, -6).expect("start");
        let mut total =
            BoundingBox::new(i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN);
        for p in &a.pieces {
            total.encapsulate(&p.base.bbox);
        }
        let first = a.pieces[0].base.bbox;
        let pivot = BlockPos::new(first.center().x, first.min_y, first.center().z);
        let mut written = 0;
        for cx in (total.min_x >> 4)..=(total.max_x >> 4) {
            for cz in (total.min_z >> 4)..=(total.max_z >> 4) {
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                let mut world = TestWorld::flat(70);
                world.seed = ctx.seed;
                let mut rng = FeatureRandom::new(cx as i64 * 31 + cz as i64);
                for p in a
                    .pieces
                    .iter()
                    .filter(|p| p.base.bbox.intersects(&chunk_box))
                {
                    p.post_process(&mut world, &ctx, &mut rng, &chunk_box, (cx, cz), pivot);
                }
                for (pos, state) in &world.blocks {
                    assert!(chunk_box.is_inside(*pos), "write outside chunk at {pos:?}");
                    assert!(
                        !state.is(b::JIGSAW) && !state.is(b::STRUCTURE_BLOCK),
                        "{pos:?}"
                    );
                }
                written += world.blocks.len();
            }
        }
        assert!(written > 100, "outpost placed {written} blocks");
    }
}
