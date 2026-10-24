//! Java structure processors (`StructureProcessor`): per-block rewriting of
//! template blocks before placement, plus the `capped` finalisation pass.

use super::template::{BlockInfo, PlaceSettings};
use super::util::{clamped_lerp, inverse_lerp, pos_seed, shuffle};
use crate::features::blocks::{BlockSet, JavaBlock, RuleTest};
use crate::features::eval::{FeatureWorld, test_rule};
use crate::features::java_blocks::blocks as b;
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::values::IntProvider;
use crate::features::{BlockPos, Heightmap, JavaState};

/// Java `PosRuleTest`.
#[derive(Debug, Clone, Copy)]
pub enum PosRuleTest {
    AlwaysTrue,
    LinearPos {
        min_chance: f32,
        max_chance: f32,
        min_dist: i32,
        max_dist: i32,
    },
    AxisAlignedLinearPos {
        /// 0 = x, 1 = y, 2 = z.
        axis: u8,
        min_chance: f32,
        max_chance: f32,
        min_dist: i32,
        max_dist: i32,
    },
}

impl PosRuleTest {
    fn test(
        &self,
        local: BlockPos,
        relative: BlockPos,
        structure: BlockPos,
        rng: &mut LegacyRandom,
    ) -> bool {
        let _ = local;
        match *self {
            PosRuleTest::AlwaysTrue => true,
            PosRuleTest::LinearPos {
                min_chance,
                max_chance,
                min_dist,
                max_dist,
            } => {
                let d = (relative.x - structure.x).abs()
                    + (relative.y - structure.y).abs()
                    + (relative.z - structure.z).abs();
                let f = rng.next_float();
                f <= clamped_lerp(
                    min_chance,
                    max_chance,
                    inverse_lerp(d as f32, min_dist as f32, max_dist as f32),
                )
            }
            PosRuleTest::AxisAlignedLinearPos {
                axis,
                min_chance,
                max_chance,
                min_dist,
                max_dist,
            } => {
                let d = match axis {
                    0 => (relative.x - structure.x).abs(),
                    1 => (relative.y - structure.y).abs(),
                    _ => (relative.z - structure.z).abs(),
                } as f32;
                let f = rng.next_float();
                f <= clamped_lerp(
                    min_chance,
                    max_chance,
                    inverse_lerp(d, min_dist as f32, max_dist as f32),
                )
            }
        }
    }
}

/// Block entity rewrite of a matching rule.
#[derive(Debug, Clone, Copy)]
pub enum BlockEntityModifier {
    Passthrough,
    Clear,
    /// Sets the loot table and draws a loot seed (`nextLong`).
    AppendLoot(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessorRule {
    pub input: RuleTest,
    pub location: RuleTest,
    pub position: PosRuleTest,
    pub output: JavaState,
    pub block_entity: BlockEntityModifier,
}

/// Java `StructureProcessor`.
#[derive(Debug, Clone, Copy)]
pub enum Processor {
    /// Keeps a block with probability `integrity` (only blocks in `rottable`
    /// when given).
    BlockRot {
        integrity: f32,
        rottable: Option<BlockSet>,
    },
    Rule(&'static [ProcessorRule]),
    /// Applies `delegate` to at most `limit` random blocks after processing.
    Capped {
        delegate: &'static Processor,
        limit: IntProvider,
    },
    /// Drops blocks whose target position holds a block in the set.
    ProtectedBlocks(BlockSet),
    /// Drops blocks of these types.
    BlockIgnore(&'static [JavaBlock]),
    /// Moves blocks to the heightmap at their column (`terrain_matching`).
    Gravity {
        heightmap: Heightmap,
        offset: i32,
    },
    /// Replaces jigsaw blocks with their `final_state` (structure void drops them).
    JigsawReplacement,
    Nop,
    /// Ruined portal blackstone swap.
    BlackstoneReplace,
    /// Keeps lava where the template has a non-full block.
    LavaSubmergedBlock,
    /// Ruined portal ageing.
    BlockAge {
        mossiness: f32,
    },
}

impl Processor {
    /// `processBlock`. `original` is the untransformed template block.
    pub fn process<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        offset: BlockPos,
        pivot: BlockPos,
        original: &BlockInfo,
        current: BlockInfo,
        settings: &mut PlaceSettings,
        rng: &mut FeatureRandom,
    ) -> Option<BlockInfo> {
        let _ = offset;
        match *self {
            Processor::BlockRot {
                integrity,
                rottable,
            } => {
                if let Some(set) = rottable
                    && !original.state.is_in(set)
                {
                    return Some(current);
                }
                let f = settings.next_float(rng, current.pos);
                (f <= integrity).then_some(current)
            }
            Processor::Rule(rules) => {
                let mut rng =
                    LegacyRandom::new(pos_seed(current.pos.x, current.pos.y, current.pos.z));
                let existing = world.get_state(current.pos);
                for rule in rules {
                    if test_rule(&rule.input, current.state, &mut rng)
                        && test_rule(&rule.location, existing, &mut rng)
                        && rule
                            .position
                            .test(original.pos, current.pos, pivot, &mut rng)
                    {
                        let nbt = match rule.block_entity {
                            BlockEntityModifier::Passthrough => current.nbt,
                            BlockEntityModifier::Clear => None,
                            BlockEntityModifier::AppendLoot(table) => {
                                Some(super::template::BlockEntityRef::Loot {
                                    base: current.nbt.and_then(|n| n.template_index()),
                                    loot_table: table,
                                    seed: rng.next_long(),
                                })
                            }
                        };
                        return Some(BlockInfo {
                            pos: current.pos,
                            state: rule.output,
                            nbt,
                        });
                    }
                }
                Some(current)
            }
            Processor::Capped { delegate, .. } => {
                delegate.process(world, offset, pivot, original, current, settings, rng)
            }
            Processor::ProtectedBlocks(set) => {
                (!world.get_state(current.pos).is_in(set)).then_some(current)
            }
            Processor::BlockIgnore(blocks) => {
                (!blocks.contains(&current.state.block())).then_some(current)
            }
            Processor::Gravity { heightmap, offset } => {
                let y = world.get_height(heightmap, current.pos.x, current.pos.z) + offset;
                Some(BlockInfo {
                    pos: BlockPos::new(current.pos.x, y + original.pos.y, current.pos.z),
                    ..current
                })
            }
            Processor::JigsawReplacement => {
                if !current.state.is(b::JIGSAW) {
                    return Some(current);
                }
                let final_state = current
                    .nbt
                    .and_then(|n| n.jigsaw_final_state(settings.template))
                    .unwrap_or(crate::features::java_blocks::states::AIR);
                (!final_state.is(b::STRUCTURE_VOID)).then_some(BlockInfo {
                    pos: current.pos,
                    state: final_state,
                    nbt: None,
                })
            }
            Processor::Nop => Some(current),
            Processor::BlackstoneReplace => Some(BlockInfo {
                state: super::ruined::blackstone_replace(current.state),
                ..current
            }),
            Processor::LavaSubmergedBlock => {
                let lava = world.get_state(current.pos).is(b::LAVA);
                if lava && !current.state.is_collision_full_cube() {
                    Some(BlockInfo {
                        state: crate::features::java_blocks::states::LAVA,
                        ..current
                    })
                } else {
                    Some(current)
                }
            }
            Processor::BlockAge { mossiness } => {
                let aged = if settings.shared_random {
                    super::ruined::block_age(current.state, mossiness, rng)
                } else {
                    let mut r =
                        LegacyRandom::new(pos_seed(current.pos.x, current.pos.y, current.pos.z));
                    super::ruined::block_age(current.state, mossiness, &mut r)
                };
                Some(BlockInfo {
                    state: aged.unwrap_or(current.state),
                    ..current
                })
            }
        }
    }

    /// `finalizeProcessing` (only `capped` does anything).
    pub fn finalize<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        offset: BlockPos,
        pivot: BlockPos,
        originals: &[BlockInfo],
        processed: &mut [BlockInfo],
        settings: &mut PlaceSettings,
        place_rng: &mut FeatureRandom,
    ) {
        let Processor::Capped { delegate, limit } = *self else {
            return;
        };
        if limit.max_value() == 0 || processed.is_empty() || originals.len() != processed.len() {
            return;
        }
        // RandomSource.create(seed).forkPositional().at(offset)
        let factory_seed = LegacyRandom::new(world.seed()).next_long();
        let mut rng = LegacyRandom::new(pos_seed(offset.x, offset.y, offset.z) ^ factory_seed);
        let n = limit.sample(&mut rng).min(processed.len() as i32);
        if n < 1 {
            return;
        }
        let mut order: Vec<usize> = (0..processed.len()).collect();
        shuffle(&mut order, &mut rng);
        let mut changed = 0;
        for k in order {
            if changed >= n {
                break;
            }
            let before = processed[k];
            if let Some(after) = delegate.process(
                world,
                offset,
                pivot,
                &originals[k],
                before,
                settings,
                place_rng,
            ) && after != before
            {
                changed += 1;
                processed[k] = after;
            }
        }
    }
}
