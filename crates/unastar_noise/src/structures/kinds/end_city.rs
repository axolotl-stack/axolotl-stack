//! End city (`EndCityStructure`, `EndCityPieces`).
//!
//! The start picks a random rotation and sits at the lowest `WORLD_SURFACE_WG`
//! height of a 5x5 box offset 7 blocks into the chunk (rejected below y 60).
//! Pieces are templates `minecraft:end_city/*` assembled by four recursive
//! section generators (house tower, tower, tower bridge, fat tower) with the
//! generation depth limit 8, at most one ship per city and Java's collision
//! rule: a section is discarded when one of its pieces hits a piece of a
//! different generation depth than the section's parent. Every random draw
//! happens in the same order as Java's.
//! Bridges attach to the tower piece retained by the generation random, which
//! may be below the topmost tower piece. Adding a ship does not move the
//! bridge's closing-end anchor.
//!
//! Data markers: `Chest` sets the `minecraft:chests/end_city_treasure` loot
//! table (with a fresh seed) on the container below the marker; `Sentry`
//! (shulker) and `Elytra` (item frame holding elytra) are entities spawned by
//! the runtime later; Java draws no structure random for them.
//!
//! Data-marker loot is merged with the template's AOT block-entity fields.

use crate::features::java_blocks::blocks as b;
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, FeatureWorld, JavaState};
use crate::structures::piece::StructurePiece;
use crate::structures::processor::Processor;
use crate::structures::start::{GenContext, GenerationStub, StructureKind, lowest_y_at};
use crate::structures::template_data;
use crate::structures::template_piece::{TemplatePiece, TemplatePlacement};
use crate::structures::transform::{BoundingBox, Mirror, Rotation, transform};

/// `EndCityPieces.MAX_GEN_DEPTH`.
const MAX_GEN_DEPTH: i32 = 8;
/// Starts below this Y are rejected.
const MIN_START_Y: i32 = 60;
/// Loot table set by `Chest` data markers.
const END_CITY_TREASURE: &str = "minecraft:chests/end_city_treasure";

/// `BlockIgnoreProcessor.STRUCTURE_BLOCK` (pieces that overwrite).
static IGNORE_STRUCTURE_BLOCK: Processor = Processor::BlockIgnore(&[b::STRUCTURE_BLOCK]);
/// `BlockIgnoreProcessor.STRUCTURE_AND_AIR` (pieces that keep existing blocks).
static IGNORE_STRUCTURE_AND_AIR: Processor = Processor::BlockIgnore(&[b::AIR, b::STRUCTURE_BLOCK]);

/// `TOWER_BRIDGES`: bridge rotation (relative) and attachment offset.
const TOWER_BRIDGES: [(Rotation, BlockPos); 4] = [
    (Rotation::None, BlockPos::new(1, -1, 0)),
    (Rotation::Clockwise90, BlockPos::new(6, -1, 1)),
    (Rotation::CounterClockwise90, BlockPos::new(0, -1, 5)),
    (Rotation::Clockwise180, BlockPos::new(5, -1, 6)),
];

/// `FAT_TOWER_BRIDGES`.
const FAT_TOWER_BRIDGES: [(Rotation, BlockPos); 4] = [
    (Rotation::None, BlockPos::new(4, -1, 0)),
    (Rotation::Clockwise90, BlockPos::new(12, -1, 4)),
    (Rotation::CounterClockwise90, BlockPos::new(0, -1, 8)),
    (Rotation::Clockwise180, BlockPos::new(8, -1, 12)),
];

const BASE_FLOOR: &str = "minecraft:end_city/base_floor";
const BASE_ROOF: &str = "minecraft:end_city/base_roof";
const BRIDGE_END: &str = "minecraft:end_city/bridge_end";
const BRIDGE_GENTLE_STAIRS: &str = "minecraft:end_city/bridge_gentle_stairs";
const BRIDGE_PIECE: &str = "minecraft:end_city/bridge_piece";
const BRIDGE_STEEP_STAIRS: &str = "minecraft:end_city/bridge_steep_stairs";
const FAT_TOWER_BASE: &str = "minecraft:end_city/fat_tower_base";
const FAT_TOWER_MIDDLE: &str = "minecraft:end_city/fat_tower_middle";
const FAT_TOWER_TOP: &str = "minecraft:end_city/fat_tower_top";
const SECOND_FLOOR_1: &str = "minecraft:end_city/second_floor_1";
const SECOND_FLOOR_2: &str = "minecraft:end_city/second_floor_2";
const SECOND_ROOF: &str = "minecraft:end_city/second_roof";
const SHIP: &str = "minecraft:end_city/ship";
const THIRD_FLOOR_1: &str = "minecraft:end_city/third_floor_1";
const THIRD_FLOOR_2: &str = "minecraft:end_city/third_floor_2";
const THIRD_ROOF: &str = "minecraft:end_city/third_roof";
const TOWER_BASE: &str = "minecraft:end_city/tower_base";
const TOWER_PIECE: &str = "minecraft:end_city/tower_piece";
const TOWER_TOP: &str = "minecraft:end_city/tower_top";

/// `Structure.findGenerationPoint` of `EndCityStructure`.
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::EndCity));
    let rotation = Rotation::random(&mut g.random);
    let position = lowest_y_in_5x5_offset_7(g, rotation);
    if position.y < MIN_START_Y {
        return None;
    }
    Some(GenerationStub {
        position,
        build: Box::new(move |g: &mut GenContext<'_>| {
            assemble(&mut g.random, position, rotation)
                .into_iter()
                .map(|p| Box::new(p) as Box<dyn StructurePiece>)
                .collect()
        }),
    })
}

/// `Structure.getLowestYIn5by5BoxOffset7Blocks(context, rotation)`.
fn lowest_y_in_5x5_offset_7(g: &GenContext<'_>, rotation: Rotation) -> BlockPos {
    let (dx, dz) = match rotation {
        Rotation::None => (5, 5),
        Rotation::Clockwise90 => (-5, 5),
        Rotation::Clockwise180 => (-5, -5),
        Rotation::CounterClockwise90 => (5, -5),
    };
    let x = g.min_block_x() + 7;
    let z = g.min_block_z() + 7;
    BlockPos::new(x, lowest_y_at(g.ctx, x, z, dx, dz), z)
}

/// `EndCityPieces.startHouseTower`: the pieces of a city starting at `start`.
///
/// # Panics
/// If the end city templates are not embedded (see
/// [`template_data::TEMPLATES_EMBEDDED`]).
pub fn assemble(rng: &mut LegacyRandom, start: BlockPos, rotation: Rotation) -> Vec<TemplatePiece> {
    let mut g = Assembly {
        rng,
        ship_created: false,
    };
    let mut pieces = Vec::new();
    let first = new_piece(BASE_FLOOR, start, rotation, true);
    let mut p = Anchor::of(&first);
    pieces.push(first);
    p = add(
        &mut pieces,
        p,
        BlockPos::new(-1, 0, -1),
        SECOND_FLOOR_1,
        rotation,
        false,
    );
    p = add(
        &mut pieces,
        p,
        BlockPos::new(-1, 4, -1),
        THIRD_FLOOR_1,
        rotation,
        false,
    );
    p = add(
        &mut pieces,
        p,
        BlockPos::new(-1, 8, -1),
        THIRD_ROOF,
        rotation,
        true,
    );
    g.recursive_children(Section::Tower, 1, p, None, &mut pieces);
    pieces
}

/// `new EndCityPiece(manager, name, position, rotation, overwrite)`.
fn new_piece(
    name: &'static str,
    position: BlockPos,
    rotation: Rotation,
    overwrite: bool,
) -> TemplatePiece {
    let template =
        template_data::template(name).unwrap_or_else(|| panic!("missing template {name}"));
    let processor = if overwrite {
        &IGNORE_STRUCTURE_BLOCK
    } else {
        &IGNORE_STRUCTURE_AND_AIR
    };
    TemplatePiece::new(
        0,
        template,
        position,
        TemplatePlacement {
            rotation,
            mirror: Mirror::None,
            pivot: BlockPos::new(0, 0, 0),
            processors: vec![processor],
            apply_waterlogging: true,
            shared_random: false,
        },
        Some(handle_data_marker),
    )
}

/// What a later piece needs from the piece it attaches to.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    position: BlockPos,
    rotation: Rotation,
    gen_depth: i32,
}

impl Anchor {
    fn of(p: &TemplatePiece) -> Self {
        Self {
            position: p.template_position,
            rotation: p.placement.rotation,
            gen_depth: p.base.gen_depth,
        }
    }
}

/// `addHelper(pieces, addPiece(manager, previous, offset, name, rotation,
/// overwrite))`: the new piece starts at the previous template position and
/// moves by `offset` rotated with the previous piece's rotation
/// (`calculateConnectedPosition` with zero pivots).
fn add(
    pieces: &mut Vec<TemplatePiece>,
    previous: Anchor,
    offset: BlockPos,
    name: &'static str,
    rotation: Rotation,
    overwrite: bool,
) -> Anchor {
    let mut piece = new_piece(name, previous.position, rotation, overwrite);
    let d = transform(
        offset,
        Mirror::None,
        previous.rotation,
        BlockPos::new(0, 0, 0),
    );
    piece.translate(d.x, d.y, d.z);
    let anchor = Anchor::of(&piece);
    pieces.push(piece);
    anchor
}

/// `setGenDepth(-1)` on the piece just added.
fn mark_bridge_end(pieces: &mut [TemplatePiece], anchor: &mut Anchor) {
    if let Some(last) = pieces.last_mut() {
        last.base.gen_depth = -1;
    }
    anchor.gen_depth = -1;
}

/// `EndCityPieces.SectionGenerator` instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    HouseTower,
    Tower,
    TowerBridge,
    FatTower,
}

/// Random plus the generators' shared state (`TOWER_BRIDGE_GENERATOR.shipCreated`).
struct Assembly<'r> {
    rng: &'r mut LegacyRandom,
    ship_created: bool,
}

impl Assembly<'_> {
    /// `EndCityPieces.recursiveChildren`.
    fn recursive_children(
        &mut self,
        section: Section,
        depth: i32,
        parent: Anchor,
        offset: Option<BlockPos>,
        pieces: &mut Vec<TemplatePiece>,
    ) -> bool {
        if depth > MAX_GEN_DEPTH {
            return false;
        }
        let mut children = Vec::new();
        if !self.generate(section, depth, parent, offset, &mut children) {
            return false;
        }
        let gen_depth = self.rng.next_int();
        for child in &mut children {
            child.base.gen_depth = gen_depth;
            if let Some(hit) = pieces
                .iter()
                .find(|p| p.base.bbox.intersects(&child.base.bbox))
                && hit.base.gen_depth != parent.gen_depth
            {
                return false;
            }
        }
        pieces.append(&mut children);
        true
    }

    /// `SectionGenerator.generate`.
    fn generate(
        &mut self,
        section: Section,
        depth: i32,
        parent: Anchor,
        offset: Option<BlockPos>,
        pieces: &mut Vec<TemplatePiece>,
    ) -> bool {
        match section {
            Section::HouseTower => {
                self.house_tower(depth, parent, offset.unwrap_or_default(), pieces)
            }
            Section::Tower => self.tower(depth, parent, pieces),
            Section::TowerBridge => self.tower_bridge(depth, parent, pieces),
            Section::FatTower => self.fat_tower(depth, parent, pieces),
        }
    }

    /// `HOUSE_TOWER_GENERATOR`.
    fn house_tower(
        &mut self,
        depth: i32,
        parent: Anchor,
        offset: BlockPos,
        pieces: &mut Vec<TemplatePiece>,
    ) -> bool {
        if depth > MAX_GEN_DEPTH {
            return false;
        }
        let rot = parent.rotation;
        let mut p = add(pieces, parent, offset, BASE_FLOOR, rot, true);
        match self.rng.next_int_bounded(3) {
            0 => {
                add(pieces, p, BlockPos::new(-1, 4, -1), BASE_ROOF, rot, true);
            }
            1 => {
                p = add(
                    pieces,
                    p,
                    BlockPos::new(-1, 0, -1),
                    SECOND_FLOOR_2,
                    rot,
                    false,
                );
                p = add(pieces, p, BlockPos::new(-1, 8, -1), SECOND_ROOF, rot, false);
                self.recursive_children(Section::Tower, depth + 1, p, None, pieces);
            }
            _ => {
                p = add(
                    pieces,
                    p,
                    BlockPos::new(-1, 0, -1),
                    SECOND_FLOOR_2,
                    rot,
                    false,
                );
                p = add(
                    pieces,
                    p,
                    BlockPos::new(-1, 4, -1),
                    THIRD_FLOOR_2,
                    rot,
                    false,
                );
                p = add(pieces, p, BlockPos::new(-1, 8, -1), THIRD_ROOF, rot, true);
                self.recursive_children(Section::Tower, depth + 1, p, None, pieces);
            }
        }
        true
    }

    /// `TOWER_GENERATOR`.
    fn tower(&mut self, depth: i32, parent: Anchor, pieces: &mut Vec<TemplatePiece>) -> bool {
        let rot = parent.rotation;
        let x = 3 + self.rng.next_int_bounded(2);
        let z = 3 + self.rng.next_int_bounded(2);
        let mut p = add(
            pieces,
            parent,
            BlockPos::new(x, -3, z),
            TOWER_BASE,
            rot,
            true,
        );
        p = add(pieces, p, BlockPos::new(0, 7, 0), TOWER_PIECE, rot, true);
        // Retain the selected attachment piece separately from the growing
        // tower: its topmost piece is still the anchor for the tower roof.
        let mut bridge_anchor = (self.rng.next_int_bounded(3) == 0).then_some(p);
        let height = 1 + self.rng.next_int_bounded(3);
        for i in 0..height {
            p = add(pieces, p, BlockPos::new(0, 4, 0), TOWER_PIECE, rot, true);
            if i < height - 1 && self.rng.next_bool() {
                bridge_anchor = Some(p);
            }
        }
        if let Some(bridge_anchor) = bridge_anchor {
            for (r, off) in TOWER_BRIDGES {
                if self.rng.next_bool() {
                    let start = add(pieces, bridge_anchor, off, BRIDGE_END, rot.then(r), true);
                    self.recursive_children(Section::TowerBridge, depth + 1, start, None, pieces);
                }
            }
        } else if depth != 7 {
            return self.recursive_children(Section::FatTower, depth + 1, p, None, pieces);
        }
        add(pieces, p, BlockPos::new(-1, 4, -1), TOWER_TOP, rot, true);
        true
    }

    /// `TOWER_BRIDGE_GENERATOR`.
    fn tower_bridge(
        &mut self,
        depth: i32,
        parent: Anchor,
        pieces: &mut Vec<TemplatePiece>,
    ) -> bool {
        let rot = parent.rotation;
        let length = self.rng.next_int_bounded(4) + 1;
        let mut p = add(
            pieces,
            parent,
            BlockPos::new(0, 0, -4),
            BRIDGE_PIECE,
            rot,
            true,
        );
        mark_bridge_end(pieces, &mut p);
        let mut y = 0;
        for _ in 0..length {
            if self.rng.next_bool() {
                p = add(pieces, p, BlockPos::new(0, y, -4), BRIDGE_PIECE, rot, true);
                y = 0;
            } else {
                p = if self.rng.next_bool() {
                    add(
                        pieces,
                        p,
                        BlockPos::new(0, y, -4),
                        BRIDGE_STEEP_STAIRS,
                        rot,
                        true,
                    )
                } else {
                    add(
                        pieces,
                        p,
                        BlockPos::new(0, y, -8),
                        BRIDGE_GENTLE_STAIRS,
                        rot,
                        true,
                    )
                };
                y = 4;
            }
        }
        if !self.ship_created && self.rng.next_int_bounded(10 - depth) == 0 {
            let x = -8 + self.rng.next_int_bounded(8);
            let z = -70 + self.rng.next_int_bounded(10);
            // The ship branches from the bridge without replacing its anchor.
            add(pieces, p, BlockPos::new(x, y, z), SHIP, rot, true);
            self.ship_created = true;
        } else if !self.recursive_children(
            Section::HouseTower,
            depth + 1,
            p,
            Some(BlockPos::new(-3, y + 1, -11)),
            pieces,
        ) {
            return false;
        }
        let mut end = add(
            pieces,
            p,
            BlockPos::new(4, y, 0),
            BRIDGE_END,
            rot.then(Rotation::Clockwise180),
            true,
        );
        mark_bridge_end(pieces, &mut end);
        true
    }

    /// `FAT_TOWER_GENERATOR`.
    fn fat_tower(&mut self, depth: i32, parent: Anchor, pieces: &mut Vec<TemplatePiece>) -> bool {
        let rot = parent.rotation;
        let mut p = add(
            pieces,
            parent,
            BlockPos::new(-3, 4, -3),
            FAT_TOWER_BASE,
            rot,
            true,
        );
        p = add(
            pieces,
            p,
            BlockPos::new(0, 4, 0),
            FAT_TOWER_MIDDLE,
            rot,
            true,
        );
        let mut i = 0;
        while i < 2 && self.rng.next_int_bounded(3) != 0 {
            p = add(
                pieces,
                p,
                BlockPos::new(0, 8, 0),
                FAT_TOWER_MIDDLE,
                rot,
                true,
            );
            for (r, off) in FAT_TOWER_BRIDGES {
                if self.rng.next_bool() {
                    let start = add(pieces, p, off, BRIDGE_END, rot.then(r), true);
                    self.recursive_children(Section::TowerBridge, depth + 1, start, None, pieces);
                }
            }
            i += 1;
        }
        add(
            pieces,
            p,
            BlockPos::new(-2, 8, -2),
            FAT_TOWER_TOP,
            rot,
            true,
        );
        true
    }
}

/// Block entities that take a loot table (`RandomizableContainer`).
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

/// `EndCityPiece.handleDataMarker`.
fn handle_data_marker(
    piece: &TemplatePiece,
    marker: &str,
    pos: BlockPos,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    chunk_box: &BoundingBox,
) {
    if marker.starts_with("Chest") {
        let chest = pos.below();
        if chunk_box.is_inside(chest) && is_randomizable_container(world.get_state(chest)) {
            let seed = rng.next_long();
            world.set_block_entity(chest, piece.loot_data_at(chest, END_CITY_TREASURE, seed));
        }
    }
    // `Sentry` (shulker) and `Elytra` (item frame) are entities spawned by
    // the runtime later; Java draws nothing from the structure random here.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::Heightmap;
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;
    use crate::structures::context::StructureContext;
    use crate::structures::generated::STRUCTURES;

    /// Flat end-like context with a configurable surface.
    struct FlatContext {
        seed: i64,
        surface: i32,
    }

    impl StructureContext for FlatContext {
        fn seed(&self) -> i64 {
            self.seed
        }
        fn min_y(&self) -> i32 {
            0
        }
        fn height(&self) -> i32 {
            256
        }
        fn sea_level(&self) -> i32 {
            0
        }
        fn noise_biome(&self, _qx: i32, _qy: i32, _qz: i32) -> u8 {
            0
        }
        fn base_height(&self, _x: i32, _z: i32, _kind: Heightmap) -> i32 {
            self.surface
        }
        fn base_column(&self, _x: i32, _z: i32, out: &mut [JavaState]) {
            for (y, s) in out.iter_mut().enumerate() {
                *s = if (y as i32) < self.surface {
                    states::END_STONE
                } else {
                    states::AIR
                };
            }
        }
    }

    fn end_city_def() -> &'static crate::structures::start::StructureDef {
        STRUCTURES
            .iter()
            .find(|d| d.name == "minecraft:end_city")
            .expect("end city structure")
    }

    /// Actual native assembly at the corpus's End-city start and an independent
    /// origin. Both include a ship, exposing the closing-end anchor as well as
    /// the randomly retained tower bridge anchor.
    #[test]
    fn pinned_native_end_city_template_geometry_and_attachment_anchors() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        struct CapturedContext<'a> {
            seed: i64,
            heights: &'a serde_json::Value,
        }
        impl StructureContext for CapturedContext<'_> {
            fn seed(&self) -> i64 { self.seed }
            fn min_y(&self) -> i32 { 0 }
            fn height(&self) -> i32 { 128 }
            fn level_height(&self) -> i32 { 256 }
            fn sea_level(&self) -> i32 { 0 }
            fn noise_biome(&self, _: i32, _: i32, _: i32) -> u8 {
                panic!("End-city assembly unexpectedly queried a biome")
            }
            fn base_height(&self, x: i32, z: i32, kind: Heightmap) -> i32 {
                assert!(matches!(kind, Heightmap::WorldSurfaceWg));
                self.heights.as_array().unwrap().iter()
                    .find(|row| row["x"] == x && row["z"] == z && row["kind"] == "WORLD_SURFACE_WG")
                    .unwrap_or_else(|| panic!("uncaptured native base-height query {x},{z}"))
                    ["height"].as_i64().unwrap() as i32
            }
            fn base_column(&self, _: i32, _: i32, _: &mut [JavaState]) {
                panic!("End-city assembly unexpectedly queried a column")
            }
        }
        let capture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/data/native_end_city_1_21_11.json"
        )).unwrap();
        assert_eq!(capture["denied_network_attempts"], serde_json::json!([]));
        assert_eq!(capture["server_constructed"], false);
        let mut count = 0;
        for case in capture["cases"].as_array().unwrap() {
            let ctx = CapturedContext {
                seed: capture["seed"].as_i64().unwrap(),
                heights: &case["height_queries"],
            };
            let cx = case["chunk"][0].as_i64().unwrap() as i32;
            let cz = case["chunk"][1].as_i64().unwrap() as i32;
            let mut g = GenContext::new(&ctx, end_city_def(), cx, cz);
            let rotation = Rotation::random(&mut g.random.clone());
            let stub = find_generation_point(&mut g);
            assert_eq!(stub.is_some(), case["present"].as_bool().unwrap());
            let Some(stub) = stub else { continue };
            assert_eq!(serde_json::json!([stub.position.x, stub.position.y, stub.position.z]), case["position"]);
            let pieces = assemble(&mut g.random, stub.position, rotation);
            let native = case["pieces"].as_array().unwrap();
            assert_eq!(pieces.len(), native.len(), "{cx},{cz}");
            for (index, (piece, native)) in pieces.iter().zip(native).enumerate() {
                let context = (cx, cz, index);
                let pos = piece.template_position;
                let b = piece.base.bbox;
                assert_eq!(piece.template.name, native["template"].as_str().unwrap(), "{context:?} template");
                assert_eq!(serde_json::json!([pos.x, pos.y, pos.z]), native["position"], "{context:?} origin");
                assert_eq!(serde_json::json!([b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z]), native["bounds"], "{context:?} bounds");
                assert_eq!(piece.base.gen_depth, native["depth"].as_i64().unwrap() as i32, "{context:?} collision group");
                let rotation = match piece.placement.rotation {
                    Rotation::None => "NONE",
                    Rotation::Clockwise90 => "CLOCKWISE_90",
                    Rotation::Clockwise180 => "CLOCKWISE_180",
                    Rotation::CounterClockwise90 => "COUNTERCLOCKWISE_90",
                };
                assert_eq!(rotation, native["rotation"].as_str().unwrap(), "{context:?} rotation");
            }
            assert_eq!(pieces.iter().filter(|piece| piece.template.name == SHIP).count(), 1);
            count += pieces.len();
        }
        assert_eq!(count, 148);
    }

    #[test]
    fn start_height_gate_and_position() {
        // No templates needed: the stub is computed without building pieces.
        for (seed, surface, expect) in [
            (1i64, 70, true),
            (2, 61, true),
            (3, 60, false),
            (4, 40, false),
        ] {
            let ctx = FlatContext { seed, surface };
            let mut g = GenContext::new(&ctx, end_city_def(), 4, -9);
            let mut reference = g.random.clone();
            let stub = find_generation_point(&mut g);
            assert_eq!(stub.is_some(), expect, "surface {surface}");
            // Exactly one draw (the rotation) before the stub.
            reference.next_int_bounded(4);
            assert_eq!(reference.next_long(), g.random.clone().next_long());
            if let Some(stub) = stub {
                assert_eq!(
                    stub.position,
                    BlockPos::new(4 * 16 + 7, surface - 1, -9 * 16 + 7)
                );
            }
        }
    }

    /// The pieces of the city started in chunk (0, 0), plus its stub position.
    /// Also checks the boxed builder yields the same piece boxes.
    fn city(seed: i64) -> (BlockPos, Rotation, Vec<TemplatePiece>) {
        let ctx = FlatContext { seed, surface: 70 };
        let mut g = GenContext::new(&ctx, end_city_def(), 0, 0);
        let rotation = Rotation::random(&mut g.random.clone());
        let stub = find_generation_point(&mut g).expect("start above y 60");
        let position = stub.position;
        let mut r = g.random.clone();
        let pieces = assemble(&mut r, position, rotation);
        let boxed = (stub.build)(&mut g);
        assert_eq!(boxed.len(), pieces.len());
        assert!(
            boxed
                .iter()
                .zip(&pieces)
                .all(|(a, b)| a.bbox() == b.base.bbox)
        );
        (position, rotation, pieces)
    }

    fn summary(pieces: &[TemplatePiece]) -> Vec<(&'static str, BlockPos, Rotation, BoundingBox)> {
        pieces
            .iter()
            .map(|p| {
                (
                    p.template.name,
                    p.template_position,
                    p.placement.rotation,
                    p.base.bbox,
                )
            })
            .collect()
    }

    #[test]
    fn assembly_is_deterministic() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let a = summary(&city(42).2);
        assert_eq!(a, summary(&city(42).2));
        assert_ne!(a, summary(&city(43).2));
    }

    #[test]
    fn city_shape() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let mut ships = 0;
        let mut fat_towers = 0;
        for seed in 0..40 {
            let (position, rotation, pieces) = city(seed);
            let names: Vec<_> = pieces.iter().map(|p| p.template.name).collect();
            assert_eq!(
                &names[..4],
                &[BASE_FLOOR, SECOND_FLOOR_1, THIRD_FLOOR_1, THIRD_ROOF]
            );
            assert_eq!(pieces[0].template_position, position);
            assert!(pieces[..4].iter().all(|p| p.placement.rotation == rotation));
            // The tower section, when kept, follows the starting house.
            if let Some(name) = names.get(4) {
                assert_eq!(*name, TOWER_BASE);
            }
            let ship_count = names.iter().filter(|n| **n == SHIP).count();
            assert!(ship_count <= 1, "seed {seed}");
            ships += ship_count;
            fat_towers += names.iter().filter(|n| **n == FAT_TOWER_TOP).count();
            for p in &pieces {
                assert!(p.base.bbox.min_y >= position.y - 20);
                assert!(p.base.bbox.max_y < position.y + 200);
            }
        }
        assert!(ships > 0, "no ship in 40 cities");
        assert!(fat_towers > 0, "no fat tower in 40 cities");
    }

    #[test]
    fn post_process_stays_in_chunk_and_sets_loot() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let mut loot = 0;
        for seed in 0..6 {
            let pieces = city(seed).2;
            let mut all = pieces[0].base.bbox;
            for p in &pieces[1..] {
                all.encapsulate(&p.base.bbox);
            }
            let ctx = FlatContext { seed, surface: 70 };
            for cx in all.min_x.div_euclid(16)..=all.max_x.div_euclid(16) {
                for cz in all.min_z.div_euclid(16)..=all.max_z.div_euclid(16) {
                    let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                    let mut world = TestWorld::flat(70);
                    world.fill = states::END_STONE;
                    let mut rng = FeatureRandom::new(seed ^ ((cx as i64) << 20) ^ cz as i64);
                    let pivot = BlockPos::new(cx * 16, 0, cz * 16);
                    for p in &pieces {
                        if p.base.bbox.intersects(&chunk_box) {
                            p.post_process(&mut world, &ctx, &mut rng, &chunk_box, (cx, cz), pivot);
                        }
                    }
                    for pos in world.blocks.keys() {
                        assert!(chunk_box.is_inside(*pos), "write outside chunk at {pos:?}");
                    }
                    for (pos, data) in &world.block_entities {
                        assert!(chunk_box.is_inside(*pos));
                        if data.loot.map(|l| l.0) == Some(END_CITY_TREASURE) {
                            assert!(is_randomizable_container(world.blocks[pos]));
                            loot += 1;
                        }
                    }
                    assert!(world.blocks.values().all(|s| !s.is(b::STRUCTURE_BLOCK)));
                }
            }
        }
        assert!(loot > 0, "no end city treasure chest in 6 cities");
    }
}
