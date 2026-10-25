//! `StructureTemplate.placeInWorld`: transforms, processors, palette choice,
//! waterlogging and block-entity hand-off for one template placement.
//!
//! Neighbour shape updates at the template edge (`updateShapeAtEdge` and
//! `Block.updateFromNeighbourShapes` when the shape is not "known") are not
//! reproduced: connection states (fences, panes, walls) stay as stored in the
//! template. Jigsaw pieces place with a known shape, so this only affects
//! legacy template structures and fossils at their borders.

use super::processor::Processor;
use super::template_data::{BlockEntityInfo, Template};
use super::transform::{BoundingBox, Mirror, Rotation, mirror_state, rotate_state, transform};
use super::util::pos_seed;
use crate::features::java_blocks::states;
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, Direction, FeatureWorld, JavaState};

/// Where a placed block's block entity data comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEntityRef {
    /// Block entity `index` of the template being placed.
    Template(u16),
    /// A loot table set by a processor, optionally on top of template data.
    Loot {
        base: Option<u16>,
        loot_table: &'static str,
        seed: i64,
    },
}

impl BlockEntityRef {
    pub fn template_index(self) -> Option<u16> {
        match self {
            BlockEntityRef::Template(i) => Some(i),
            BlockEntityRef::Loot { base, .. } => base,
        }
    }

    /// `final_state` of a jigsaw block entity.
    pub fn jigsaw_final_state(self, template: &Template) -> Option<JavaState> {
        match template
            .block_entities
            .get(self.template_index()? as usize)?
        {
            BlockEntityInfo::Jigsaw { final_state, .. } => Some(*final_state),
            _ => None,
        }
    }
}

/// `StructureTemplate.StructureBlockInfo` (world position after transforms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockInfo {
    pub pos: BlockPos,
    pub state: JavaState,
    pub nbt: Option<BlockEntityRef>,
}

/// Block entity written by a placement, for the runtime to materialise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedBlockEntity {
    pub pos: BlockPos,
    pub state: JavaState,
    /// AOT-compiled template actor metadata when the template had one.
    pub data: Option<&'static BlockEntityInfo>,
    /// Loot table override from a processor.
    pub loot_table: Option<&'static str>,
    /// Loot seed drawn for randomizable containers.
    pub loot_seed: Option<i64>,
}

/// `StructurePlaceSettings`.
pub struct PlaceSettings {
    pub template: &'static Template,
    pub mirror: Mirror,
    pub rotation: Rotation,
    pub pivot: BlockPos,
    pub bounding_box: Option<BoundingBox>,
    /// `setRandom(random)` was called with the placement random: processors
    /// and palette choice draw from it. Otherwise they use a legacy random
    /// seeded from each position (`RandomSource.create(Mth.getSeed(pos))`).
    pub shared_random: bool,
    pub processors: Vec<&'static Processor>,
    pub apply_waterlogging: bool,
    pub ignore_entities: bool,
}

impl PlaceSettings {
    pub fn new(template: &'static Template) -> Self {
        Self {
            template,
            mirror: Mirror::None,
            rotation: Rotation::None,
            pivot: BlockPos::new(0, 0, 0),
            bounding_box: None,
            shared_random: false,
            processors: Vec::new(),
            apply_waterlogging: true,
            ignore_entities: true,
        }
    }

    /// `getRandom(pos).nextFloat()`.
    pub fn next_float(&self, rng: &mut FeatureRandom, pos: BlockPos) -> f32 {
        if self.shared_random {
            rng.next_float()
        } else {
            LegacyRandom::new(pos_seed(pos.x, pos.y, pos.z)).next_float()
        }
    }

    /// `getRandom(pos).nextInt(bound)`.
    pub fn next_int(&self, rng: &mut FeatureRandom, pos: BlockPos, bound: i32) -> i32 {
        if self.shared_random {
            rng.next_int_bounded(bound)
        } else {
            LegacyRandom::new(pos_seed(pos.x, pos.y, pos.z)).next_int_bounded(bound)
        }
    }
}

/// `Template.getSize(rotation)`.
pub fn rotated_size(t: &Template, rotation: Rotation) -> [i32; 3] {
    match rotation {
        Rotation::Clockwise90 | Rotation::CounterClockwise90 => [t.size[2], t.size[1], t.size[0]],
        _ => t.size,
    }
}

/// `getZeroPositionWithTransform(pos, mirror, rotation)`.
pub fn zero_position(t: &Template, pos: BlockPos, mirror: Mirror, rotation: Rotation) -> BlockPos {
    let sx = t.size[0] - 1;
    let sz = t.size[2] - 1;
    let i = if mirror == Mirror::FrontBack { sx } else { 0 };
    let j = if mirror == Mirror::LeftRight { sz } else { 0 };
    match rotation {
        Rotation::CounterClockwise90 => pos.offset(j, 0, sx - i),
        Rotation::Clockwise90 => pos.offset(sz - j, 0, i),
        Rotation::Clockwise180 => pos.offset(sx - i, 0, sz - j),
        Rotation::None => pos.offset(i, 0, j),
    }
}

/// `getBoundingBox(startPos, rotation, pivot, mirror)`.
pub fn template_box(
    t: &Template,
    start: BlockPos,
    rotation: Rotation,
    pivot: BlockPos,
    mirror: Mirror,
) -> BoundingBox {
    let a = transform(BlockPos::new(0, 0, 0), mirror, rotation, pivot);
    let b = transform(
        BlockPos::new(t.size[0] - 1, t.size[1] - 1, t.size[2] - 1),
        mirror,
        rotation,
        pivot,
    );
    BoundingBox::from_corners(a, b).moved(start.x, start.y, start.z)
}

/// Whether a block entity of this kind takes a loot seed when placed
/// (`RandomizableContainer`).
fn is_randomizable_container(state: JavaState) -> bool {
    matches!(
        state.block().def().kind,
        "minecraft:chest"
            | "minecraft:trapped_chest"
            | "minecraft:copper_chest"
            | "minecraft:weathering_copper_chest"
            | "minecraft:barrel"
            | "minecraft:dispenser"
            | "minecraft:dropper"
            | "minecraft:hopper"
            | "minecraft:shulker_box"
            | "minecraft:decorated_pot"
            | "minecraft:crafter"
    )
}

/// `processBlockInfos`: transform each template block, run the processors in
/// order, then their finalisation pass.
pub fn process_blocks<W: FeatureWorld + ?Sized>(
    world: &mut W,
    offset: BlockPos,
    pivot: BlockPos,
    settings: &mut PlaceSettings,
    palette: &[super::template_data::TemplateBlock],
    rng: &mut FeatureRandom,
) -> Vec<BlockInfo> {
    // Only capped processors need the pre-transform list during finalization.
    let keep_originals = settings
        .processors
        .iter()
        .any(|p| matches!(p, Processor::Capped { .. }));
    let mut originals = if keep_originals {
        Vec::with_capacity(palette.len())
    } else {
        Vec::new()
    };
    let mut processed = Vec::with_capacity(palette.len());
    for tb in palette {
        let local = BlockPos::new(tb.x as i32, tb.y as i32, tb.z as i32);
        let original = BlockInfo {
            pos: local,
            state: tb.state,
            nbt: tb.block_entity.map(BlockEntityRef::Template),
        };
        let rel = transform(local, settings.mirror, settings.rotation, settings.pivot);
        let mut info = Some(BlockInfo {
            pos: rel.offset(offset.x, offset.y, offset.z),
            ..original
        });
        // Copy the static processor reference, not the per-placement Vec.
        for index in 0..settings.processors.len() {
            let p = settings.processors[index];
            let Some(cur) = info else { break };
            info = p.process(world, offset, pivot, &original, cur, settings, rng);
        }
        if let Some(i) = info {
            if keep_originals {
                originals.push(original);
            }
            processed.push(i);
        }
    }
    for index in 0..settings.processors.len() {
        let p = settings.processors[index];
        p.finalize(
            world,
            offset,
            pivot,
            &originals,
            &mut processed,
            settings,
            rng,
        );
    }
    processed
}

/// `StructureTemplate.placeInWorld(level, offset, pivot, settings, random, flags)`.
///
/// Returns `false` only when the template has no palette. Block entities are
/// reported through `on_block_entity` in placement order.
pub fn place_in_world<W: FeatureWorld + ?Sized>(
    world: &mut W,
    offset: BlockPos,
    pivot: BlockPos,
    settings: &mut PlaceSettings,
    rng: &mut FeatureRandom,
    on_block_entity: &mut dyn FnMut(PlacedBlockEntity),
) -> bool {
    let _stage = crate::profile::scope(crate::profile::Stage::StructureTemplates);
    let t = settings.template;
    if t.palettes.is_empty() {
        return false;
    }
    let palette_index = settings.next_int(rng, offset, t.palettes.len() as i32) as usize;
    let palette = &t.palettes[palette_index];
    if palette.is_empty() && (settings.ignore_entities || t.entities.is_empty()) {
        return true;
    }
    if t.size.iter().any(|s| *s < 1) {
        return true;
    }
    let infos = process_blocks(world, offset, pivot, settings, palette, rng);
    let mut need_liquid: Vec<BlockPos> = Vec::new();
    let mut sources: Vec<BlockPos> = Vec::new();
    for info in infos {
        if let Some(bb) = settings.bounding_box
            && !bb.is_inside(info.pos)
        {
            continue;
        }
        let fluid_before = settings
            .apply_waterlogging
            .then(|| world.get_state(info.pos).fluid());
        let state = rotate_state(mirror_state(info.state, settings.mirror), settings.rotation);
        if info.nbt.is_some() {
            // Java clears any existing block entity with a barrier first.
            world.set_state(info.pos, states::BARRIER);
        }
        if !world.set_state(info.pos, state) {
            continue;
        }
        if let Some(nbt) = info.nbt {
            let loot_seed = is_randomizable_container(state).then(|| rng.next_long());
            let (data, loot_table, seed) = match nbt {
                BlockEntityRef::Template(i) => (t.block_entities.get(i as usize), None, loot_seed),
                BlockEntityRef::Loot {
                    base,
                    loot_table,
                    seed,
                } => (
                    base.and_then(|i| t.block_entities.get(i as usize)),
                    Some(loot_table),
                    loot_seed.or(Some(seed)),
                ),
            };
            on_block_entity(PlacedBlockEntity {
                pos: info.pos,
                state,
                data,
                loot_table,
                loot_seed: seed,
            });
        }
        if let Some(fluid) = fluid_before {
            let own = state.fluid();
            if own.source {
                sources.push(info.pos);
            } else if state.has("waterlogged") {
                if fluid.is_water() && fluid.source && state.value("waterlogged") == Some("false") {
                    world.set_state(info.pos, state.with_bool("waterlogged", true));
                }
                if !fluid.source {
                    need_liquid.push(info.pos);
                }
            }
        }
    }
    // Spread source water into waterloggable blocks next to sources placed
    // outside this template (Java's liquid propagation loop).
    let dirs = [
        Direction::Up,
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    let mut changed = true;
    while changed && !need_liquid.is_empty() {
        changed = false;
        need_liquid.retain(|&p| {
            let mut fluid = world.get_state(p).fluid();
            let mut i = 0;
            while i < dirs.len() && !fluid.source {
                let n = p.rel(dirs[i]);
                let nf = world.get_state(n).fluid();
                if nf.source && !sources.contains(&n) {
                    fluid = nf;
                }
                i += 1;
            }
            if fluid.source {
                let s = world.get_state(p);
                if s.has("waterlogged") {
                    if fluid.is_water() && s.value("waterlogged") == Some("false") {
                        world.set_state(p, s.with_bool("waterlogged", true));
                    }
                    changed = true;
                    return false;
                }
            }
            true
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::RuleTest;
    use crate::features::java_blocks::blocks;
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;
    use crate::structures::processor::{BlockEntityModifier, PosRuleTest, ProcessorRule};
    use crate::structures::template_data::TemplateBlock;
    use std::sync::LazyLock;

    static TEMPLATE: LazyLock<Template> = LazyLock::new(|| Template {
        name: "test:capped",
        size: [3, 1, 1],
        palettes: Vec::new(),
        block_entities: Vec::new(),
        entities: Vec::new(),
    });
    static REWRITES: Processor = Processor::Rule(&[
        ProcessorRule {
            input: RuleTest::BlockMatch(blocks::STONE),
            location: RuleTest::AlwaysTrue,
            position: PosRuleTest::AlwaysTrue,
            output: states::DIRT,
            block_entity: BlockEntityModifier::Passthrough,
        },
        ProcessorRule {
            input: RuleTest::BlockMatch(blocks::DIRT),
            location: RuleTest::AlwaysTrue,
            position: PosRuleTest::AlwaysTrue,
            output: states::GOLD_BLOCK,
            block_entity: BlockEntityModifier::Passthrough,
        },
    ]);
    static IGNORE: Processor = Processor::BlockIgnore(&[blocks::AIR]);
    static CAPPED: Processor = Processor::Capped {
        delegate: &REWRITES,
        limit: IntProvider::Constant(1),
    };

    #[test]
    fn capped_finalization_keeps_originals_aligned_after_filtering() {
        let palette =
            [(0, states::AIR), (1, states::STONE), (2, states::STONE)].map(|(x, state)| {
                TemplateBlock {
                    x,
                    y: 0,
                    z: 0,
                    state,
                    block_entity: None,
                }
            });
        let mut world = TestWorld::flat(0);
        let mut settings = PlaceSettings::new(&TEMPLATE);
        settings.processors.extend([&IGNORE, &CAPPED]);
        let mut rng = FeatureRandom::new(12345);
        let blocks = process_blocks(
            &mut world,
            BlockPos::new(10, 40, 20),
            BlockPos::new(0, 0, 0),
            &mut settings,
            &palette,
            &mut rng,
        );
        assert_eq!(
            blocks
                .iter()
                .filter(|b| b.state == states::GOLD_BLOCK)
                .count(),
            1
        );
        assert_eq!(blocks.iter().filter(|b| b.state == states::DIRT).count(), 1);
        assert_eq!(
            blocks.iter().map(|b| b.pos).collect::<Vec<_>>(),
            [BlockPos::new(11, 40, 20), BlockPos::new(12, 40, 20)],
        );
    }
}
