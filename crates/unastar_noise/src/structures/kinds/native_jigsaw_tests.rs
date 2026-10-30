//! Captured native consumers, independent of the runtime's density adapter.
use super::*;
use crate::structures::generated::STRUCTURES;
use serde_json::{Value, json};
use std::sync::Mutex;

fn number(value: &Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

fn heightmap_name(kind: Heightmap) -> &'static str {
    match kind {
        Heightmap::WorldSurfaceWg => "WORLD_SURFACE_WG",
        Heightmap::WorldSurface => "WORLD_SURFACE",
        Heightmap::OceanFloorWg => "OCEAN_FLOOR_WG",
        Heightmap::OceanFloor => "OCEAN_FLOOR",
        Heightmap::MotionBlocking => "MOTION_BLOCKING",
        Heightmap::MotionBlockingNoLeaves => "MOTION_BLOCKING_NO_LEAVES",
    }
}

struct NativeContext<'a> {
    capture: &'a Value,
    case: &'a Value,
    queries: Mutex<Vec<Value>>,
}

impl StructureContext for NativeContext<'_> {
    fn seed(&self) -> i64 {
        self.capture["seed"].as_i64().unwrap()
    }
    fn min_y(&self) -> i32 {
        number(&self.capture["min_y"])
    }
    fn height(&self) -> i32 {
        number(&self.capture["generator_height"]).min(self.level_height())
    }
    fn level_height(&self) -> i32 {
        number(&self.capture["level_height"])
    }
    fn sea_level(&self) -> i32 {
        match self.capture["dimension"].as_str().unwrap() {
            "overworld" => 63,
            "nether" => 32,
            "end" => 0,
            other => panic!("unknown native dimension {other}"),
        }
    }
    fn noise_biome(&self, _: i32, _: i32, _: i32) -> u8 {
        panic!("jigsaw assembly unexpectedly sampled a biome")
    }
    fn base_height(&self, x: i32, z: i32, kind: Heightmap) -> i32 {
        let kind = heightmap_name(kind);
        let row = self.case["height_queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| number(&row["x"]) == x && number(&row["z"]) == z && row["kind"] == kind)
            .unwrap_or_else(|| {
                panic!(
                    "uncaptured native height query {x},{z}/{kind} for {}",
                    self.case["structure"]
                )
            });
        self.queries.lock().unwrap().push(row.clone());
        number(&row["height"])
    }
    fn base_column(&self, _: i32, _: i32, _: &mut [JavaState]) {
        panic!("jigsaw assembly unexpectedly sampled a base column")
    }
}

fn element_identity(element: &PoolElement) -> Value {
    match element {
        PoolElement::Single {
            template, legacy, ..
        } => json!({
            "kind": if *legacy { "legacy_single" } else { "single" },
            "template": template,
        }),
        PoolElement::Feature { feature, .. } => json!({
            "kind": "feature",
            "feature": crate::features::generated::PLACED_NAMES[*feature as usize],
        }),
        PoolElement::List { elements, .. } => json!({
            "kind": "list",
            "elements": elements.iter().map(element_identity).collect::<Vec<_>>(),
        }),
        PoolElement::Empty => json!({"kind": "empty"}),
    }
}

fn projection_name(projection: Projection) -> &'static str {
    match projection {
        Projection::Rigid => "RIGID",
        Projection::TerrainMatching => "TERRAIN_MATCHING",
    }
}

fn rotation_name(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::None => "NONE",
        Rotation::Clockwise90 => "CLOCKWISE_90",
        Rotation::Clockwise180 => "CLOCKWISE_180",
        Rotation::CounterClockwise90 => "COUNTERCLOCKWISE_90",
    }
}

fn box_json(b: BoundingBox) -> Value {
    json!([b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z])
}

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/data/native_jigsaw_1_21_11.json"
    ))
    .unwrap()
}

#[test]
fn pinned_native_jigsaw_start_piece_geometry_and_query_order() {
    if !template_data::TEMPLATES_EMBEDDED {
        return;
    }
    let fixture = fixture();
    let mut checked = 0;
    for capture in fixture["captures"].as_array().unwrap() {
        assert_eq!(capture["version"], "1.21.11");
        assert_eq!(capture["denied_network_attempts"], json!([]));
        assert_eq!(capture["server_constructed"], false);
        for case in capture["cases"].as_array().unwrap() {
            let def = STRUCTURES
                .iter()
                .find(|def| Some(def.name) == case["structure"].as_str())
                .unwrap();
            let StructureKind::Jigsaw(structure) = def.kind else {
                continue;
            };
            let ctx = NativeContext {
                capture,
                case,
                queries: Mutex::new(Vec::new()),
            };
            let cx = number(&case["chunk"][0]);
            let cz = number(&case["chunk"][1]);
            let mut g = GenContext::new(&ctx, def, cx, cz);
            let start = find_start(&mut g, structure);
            assert_eq!(
                start.is_some(),
                case["present"].as_bool().unwrap(),
                "{} start",
                def.name
            );
            let Some(start) = start else { continue };
            assert_eq!(
                json!([start.center.x, start.center.y, start.center.z]),
                case["position"],
                "{} stub",
                def.name
            );
            let assembly = assemble(&ctx, &mut g.random, structure, start);
            let native_pieces = case["pieces"].as_array().unwrap();
            assert_eq!(
                assembly.pieces.len(),
                native_pieces.len(),
                "{} pieces",
                def.name
            );
            for (index, (piece, native)) in assembly.pieces.iter().zip(native_pieces).enumerate() {
                let identity = (def.name, cx, cz, index);
                assert_eq!(
                    box_json(piece.base.bbox),
                    native["bounds"],
                    "{identity:?} bounds"
                );
                assert_eq!(
                    json!([piece.position.x, piece.position.y, piece.position.z]),
                    native["position"],
                    "{identity:?} origin"
                );
                assert_eq!(
                    piece.ground_level_delta,
                    number(&native["ground_level_delta"]),
                    "{identity:?} ground delta"
                );
                assert_eq!(
                    rotation_name(piece.rotation),
                    native["rotation"].as_str().unwrap(),
                    "{identity:?} rotation"
                );
                assert_eq!(
                    element_identity(piece.element),
                    native["element_identity"],
                    "{identity:?} template/feature"
                );
                assert_eq!(
                    projection_name(projection_of(piece.element)),
                    native["projection"].as_str().unwrap(),
                    "{identity:?} projection"
                );
                let junctions: Vec<_> = piece
                    .junctions
                    .iter()
                    .map(|j| {
                        json!({
                            "getSourceX": j.source_x,
                            "getSourceGroundY": j.source_ground_y,
                            "getSourceZ": j.source_z,
                            "getDeltaY": j.delta_y,
                            "getDestProjection": projection_name(j.dest_projection),
                        })
                    })
                    .collect();
                assert_eq!(
                    json!(junctions),
                    native["junctions"],
                    "{identity:?} junctions"
                );
            }
            assert_eq!(
                json!(*ctx.queries.lock().unwrap()),
                case["height_queries"],
                "{} native query sequence",
                def.name
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 6);
}

#[test]
fn pinned_native_jigsaw_level_padding_boundaries() {
    let fixture = fixture();
    let capture = &fixture["captures"][0];
    for row in capture["padding_cases"].as_array().unwrap() {
        let context_capture = json!({
            "seed": 0, "min_y": row["min_y"], "generator_height": 128,
            "level_height": row["level_height"], "dimension": "nether",
        });
        let ctx = NativeContext {
            capture: &context_capture,
            case: row,
            queries: Mutex::new(Vec::new()),
        };
        let b = &row["bounds"];
        let bounds = BoundingBox::new(
            number(&b[0]),
            number(&b[1]),
            number(&b[2]),
            number(&b[3]),
            number(&b[4]),
            number(&b[5]),
        );
        let padding = (number(&row["padding"][0]), number(&row["padding"][1]));
        assert_eq!(
            start_too_close_to_limits(&ctx, padding, &bounds),
            row["rejected"].as_bool().unwrap(),
            "{row}"
        );
    }
}
