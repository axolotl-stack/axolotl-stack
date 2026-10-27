//! Java `TemplateStructurePiece`: a piece that places one template, then
//! handles its data markers and turns leftover jigsaw blocks into their final
//! state.

use super::context::StructureContext;
use super::piece::{PieceBase, StructurePiece};
use super::processor::Processor;
use super::template::{PlaceSettings, PlacedBlockEntity, place_in_world, template_box};
use super::template_data::{BlockEntityInfo, Template};
use super::transform::{BoundingBox, Mirror, Rotation, transform};
use crate::features::eval::BlockEntityData;
use crate::features::java_blocks::blocks as b;
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, FeatureWorld};

/// Data marker handler: `(piece, metadata, world position, world, random, chunk box)`.
pub type DataMarkerHandler =
    fn(&TemplatePiece, &str, BlockPos, &mut dyn FeatureWorld, &mut FeatureRandom, &BoundingBox);

/// Placement settings stored on a template piece.
#[derive(Debug, Clone)]
pub struct TemplatePlacement {
    pub rotation: Rotation,
    pub mirror: Mirror,
    pub pivot: BlockPos,
    pub processors: Vec<&'static Processor>,
    pub apply_waterlogging: bool,
    /// `setRandom` with the placement random (rare; most pieces leave it unset).
    pub shared_random: bool,
}

/// A template piece.
#[derive(Debug)]
pub struct TemplatePiece {
    pub base: PieceBase,
    pub template: &'static Template,
    pub template_position: BlockPos,
    pub placement: TemplatePlacement,
    /// Handler for `DATA` structure blocks; `None` ignores them.
    pub on_data_marker: Option<DataMarkerHandler>,
}

impl TemplatePiece {
    /// A piece whose bounding box is the transformed template box.
    pub fn new(
        gen_depth: i32,
        template: &'static Template,
        template_position: BlockPos,
        placement: TemplatePlacement,
        on_data_marker: Option<DataMarkerHandler>,
    ) -> Self {
        let bbox = template_box(
            template,
            template_position,
            placement.rotation,
            placement.pivot,
            placement.mirror,
        );
        Self {
            base: PieceBase::new(gen_depth, bbox),
            template,
            template_position,
            placement,
            on_data_marker,
        }
    }

    /// Add marker loot without discarding the template actor's other fields.
    /// Palette alternatives share block positions and actor records in Java's
    /// template format, so this lookup never needs to select a palette or draw.
    pub fn loot_data_at(&self, pos: BlockPos, table: &'static str, seed: i64) -> BlockEntityData {
        let mut data = BlockEntityData::loot(table, seed);
        let Some(palette) = self.template.palettes.first() else {
            return data;
        };
        for block in palette {
            let Some(index) = block.block_entity else {
                continue;
            };
            let at = transform(
                BlockPos::new(block.x as i32, block.y as i32, block.z as i32),
                self.placement.mirror,
                self.placement.rotation,
                self.placement.pivot,
            )
            .offset(
                self.template_position.x,
                self.template_position.y,
                self.template_position.z,
            );
            if at == pos {
                if let Some(BlockEntityInfo::Other {
                    actor,
                    spawner_entity,
                    ..
                }) = self.template.block_entities.get(index as usize)
                {
                    data.template_nbt = Some(*actor);
                    data.spawner_entity = *spawner_entity;
                }
                break;
            }
        }
        data
    }

    fn settings(&self, chunk_box: &BoundingBox) -> PlaceSettings {
        let mut s = PlaceSettings::new(self.template);
        s.rotation = self.placement.rotation;
        s.mirror = self.placement.mirror;
        s.pivot = self.placement.pivot;
        s.bounding_box = Some(*chunk_box);
        s.processors = self.placement.processors.clone();
        s.apply_waterlogging = self.placement.apply_waterlogging;
        s.shared_random = self.placement.shared_random;
        s
    }

    /// `filterBlocks(pos, settings, block)`: transformed positions of the
    /// palette's blocks of one type (palette chosen like `placeInWorld`).
    fn filter_blocks(
        &self,
        settings: &PlaceSettings,
        rng: &mut FeatureRandom,
        block: crate::features::JavaBlock,
    ) -> Vec<(BlockPos, Option<&'static BlockEntityInfo>)> {
        let t = self.template;
        let index =
            settings.next_int(rng, self.template_position, t.palettes.len() as i32) as usize;
        let mut out = Vec::new();
        for tb in &t.palettes[index] {
            if !tb.state.is(block) {
                continue;
            }
            let local = BlockPos::new(tb.x as i32, tb.y as i32, tb.z as i32);
            let rel = transform(local, settings.mirror, settings.rotation, settings.pivot);
            let pos = rel.offset(
                self.template_position.x,
                self.template_position.y,
                self.template_position.z,
            );
            out.push((
                pos,
                tb.block_entity
                    .and_then(|i| t.block_entities.get(i as usize)),
            ));
        }
        out
    }
}

/// Default hand-off of block entities produced by a template placement.
pub fn record_block_entities(world: &mut dyn FeatureWorld, placed: &[PlacedBlockEntity]) {
    for be in placed {
        let (template_nbt, template_loot, spawner_entity) = match be.data {
            Some(BlockEntityInfo::Other {
                actor,
                loot,
                spawner_entity,
            }) => (Some(*actor), *loot, *spawner_entity),
            _ => (None, None, None),
        };
        let loot = be
            .loot_table
            .or(template_loot.map(|(table, _)| table))
            .map(|table| {
                (
                    table,
                    be.loot_seed
                        .or(template_loot.map(|(_, seed)| seed))
                        .unwrap_or(0),
                )
            });
        if template_nbt.is_none() && loot.is_none() {
            continue;
        }
        world.set_block_entity(
            be.pos,
            BlockEntityData {
                loot,
                spawner_entity,
                template_nbt,
            },
        );
    }
}

impl StructurePiece for TemplatePiece {
    fn base(&self) -> &PieceBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.base
    }

    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.base.bbox = self.base.bbox.moved(dx, dy, dz);
        self.template_position = self.template_position.offset(dx, dy, dz);
    }

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        _ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        pivot: BlockPos,
    ) {
        let mut settings = self.settings(chunk_box);
        let mut placed = Vec::new();
        if !place_in_world(
            world,
            self.template_position,
            pivot,
            &mut settings,
            rng,
            &mut |be| placed.push(be),
        ) {
            return;
        }
        record_block_entities(world, &placed);
        for (pos, info) in self.filter_blocks(&settings, rng, b::STRUCTURE_BLOCK) {
            if let (Some(BlockEntityInfo::DataMarker { metadata }), Some(handler)) =
                (info, self.on_data_marker)
            {
                handler(self, metadata, pos, world, rng, chunk_box);
            }
        }
        for (pos, info) in self.filter_blocks(&settings, rng, b::JIGSAW) {
            if let Some(BlockEntityInfo::Jigsaw { final_state, .. }) = info {
                world.set_state(pos, *final_state);
            }
        }
    }
}

/// `RandomSource.create(Mth.getSeed(pos))`, for pieces that need it.
pub fn positional_random(pos: BlockPos) -> LegacyRandom {
    LegacyRandom::new(super::util::pos_seed(pos.x, pos.y, pos.z))
}
