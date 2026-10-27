//! Java 1.21.11 structure terrain adaptation. Collection preserves the incoming
//! native start/piece/junction order; sampling uses only compact numeric data.
//! The selection context must use unadapted native base columns, not this field.

use std::sync::LazyLock;

use super::jigsaw::{JigsawJunction, Projection};
use super::start::{StructureStart, TerrainAdjustment};
use super::transform::BoundingBox;
use crate::features::BlockPos;

const RADIUS: i32 = 12;
const SIZE: usize = 24;

// Native stores the Gaussian part as floats, including its half-block Y offset.
// Keep that rounding boundary separate from the double directional factor.
static KERNEL: LazyLock<[f32; SIZE * SIZE * SIZE]> = LazyLock::new(|| {
    let mut values = [0.0; SIZE * SIZE * SIZE];
    for z in 0..SIZE {
        for x in 0..SIZE {
            for y in 0..SIZE {
                let dx = x as f64 - RADIUS as f64;
                let dy = y as f64 - RADIUS as f64 + 0.5;
                let dz = z as f64 - RADIUS as f64;
                let squared = dx * dx + dy * dy + dz * dz;
                values[z * SIZE * SIZE + x * SIZE + y] =
                    std::f64::consts::E.powf(-squared / 16.0) as f32;
            }
        }
    }
    values
});

/// A native `Beardifier.Rigid`. Ordinary (non-pool) pieces use ground delta 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rigid {
    pub bbox: BoundingBox,
    pub adjustment: TerrainAdjustment,
    pub ground_level_delta: i32,
}

/// Immutable per-chunk density inputs. No random state is read or changed.
#[derive(Debug, Clone, Default)]
pub struct Beardifier {
    pieces: Vec<Rigid>,
    junctions: Vec<JigsawJunction>,
    affected_box: Option<BoundingBox>,
}

impl Beardifier {
    /// Capture starts returned by native structure-reference selection, in that
    /// exact order. Piece boxes are culled against the chunk expanded by 12;
    /// they are never clipped to the chunk (that would move their influence).
    pub fn for_chunk<'a>(
        starts: impl IntoIterator<Item = &'a StructureStart>,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Self {
        let min_x = chunk_x.wrapping_mul(16);
        let min_z = chunk_z.wrapping_mul(16);
        let mut result = Self::default();
        for start in starts {
            let adjustment = start.def().terrain_adaptation;
            if adjustment == TerrainAdjustment::None {
                continue;
            }
            for piece in &start.pieces {
                let bbox = piece.bbox();
                if !bbox.intersects_xz(
                    min_x - RADIUS,
                    min_z - RADIUS,
                    min_x + 15 + RADIUS,
                    min_z + 15 + RADIUS,
                ) {
                    continue;
                }
                if let Some(pool) = piece.pool_terrain() {
                    if pool.projection == Projection::Rigid {
                        result.push_piece(Rigid {
                            bbox,
                            adjustment,
                            ground_level_delta: pool.ground_level_delta,
                        });
                    }
                    // Terrain-matching pool pieces still supply junctions. The
                    // destination projection/delta do not filter this list.
                    for &junction in pool.junctions {
                        if junction.source_x > min_x - RADIUS
                            && junction.source_x < min_x + 15 + RADIUS
                            && junction.source_z > min_z - RADIUS
                            && junction.source_z < min_z + 15 + RADIUS
                        {
                            let p = BlockPos::new(
                                junction.source_x,
                                junction.source_ground_y,
                                junction.source_z,
                            );
                            result.include_box(BoundingBox::from_corners(p, p));
                            result.junctions.push(junction);
                        }
                    }
                } else {
                    result.push_piece(Rigid {
                        bbox,
                        adjustment,
                        ground_level_delta: 0,
                    });
                }
            }
        }
        // This is the union of *collected* geometry, not StructureStart's
        // already-inflated reference box. Java 1.21.11 clips compute to it.
        result.affected_box = result.affected_box.map(|bbox| bbox.inflated(24));
        result
    }

    fn include_box(&mut self, bbox: BoundingBox) {
        match &mut self.affected_box {
            Some(affected) => affected.encapsulate(&bbox),
            None => self.affected_box = Some(bbox),
        }
    }

    fn push_piece(&mut self, piece: Rigid) {
        self.include_box(piece.bbox);
        self.pieces.push(piece);
    }

    pub fn pieces(&self) -> &[Rigid] {
        &self.pieces
    }

    pub fn junctions(&self) -> &[JigsawJunction] {
        &self.junctions
    }

    pub fn affected_box(&self) -> Option<BoundingBox> {
        self.affected_box
    }

    /// Sample the native field without allocations or piece virtual dispatch.
    #[inline]
    pub fn sample(&self, x: i32, y: i32, z: i32) -> f64 {
        let Some(affected) = self.affected_box else {
            return 0.0;
        };
        if !affected.is_inside(BlockPos::new(x, y, z)) {
            return 0.0;
        }
        let mut value = 0.0;
        for piece in &self.pieces {
            let bbox = piece.bbox;
            let dx = (bbox.min_x - x).max(x - bbox.max_x).max(0);
            let dz = (bbox.min_z - z).max(z - bbox.max_z).max(0);
            let ground_y = bbox.min_y + piece.ground_level_delta;
            let ground_distance = y - ground_y;
            value += match piece.adjustment {
                TerrainAdjustment::None => 0.0,
                TerrainAdjustment::Bury => bury(dx as f64, ground_distance as f64 / 2.0, dz as f64),
                TerrainAdjustment::BeardThin => {
                    beard(dx, ground_distance, dz, ground_distance) * 0.8
                }
                TerrainAdjustment::BeardBox => {
                    let dy = (ground_y - y).max(y - bbox.max_y).max(0);
                    beard(dx, dy, dz, ground_distance) * 0.8
                }
                TerrainAdjustment::Encapsulate => {
                    let dy = (bbox.min_y - y).max(y - bbox.max_y).max(0);
                    bury(dx as f64 / 2.0, dy as f64 / 2.0, dz as f64 / 2.0) * 0.8
                }
            };
        }
        for junction in &self.junctions {
            let dy = y - junction.source_ground_y;
            value += beard(x - junction.source_x, dy, z - junction.source_z, dy) * 0.4;
        }
        value
    }
}

#[inline]
fn bury(x: f64, y: f64, z: f64) -> f64 {
    let distance = (x * x + y * y + z * z).sqrt();
    if distance > 6.0 {
        0.0
    } else {
        1.0 - distance / 6.0
    }
}

#[inline]
fn beard(x: i32, y: i32, z: i32, ground_distance: i32) -> f64 {
    if !(-RADIUS..RADIUS).contains(&x)
        || !(-RADIUS..RADIUS).contains(&y)
        || !(-RADIUS..RADIUS).contains(&z)
    {
        return 0.0;
    }
    let dy = ground_distance as f64 + 0.5;
    let squared = x as f64 * x as f64 + dy * dy + z as f64 * z as f64;
    let inverse = fast_inv_sqrt(squared / 2.0);
    let direction = -dy * inverse / 2.0;
    let index = (z + RADIUS) as usize * SIZE * SIZE
        + (x + RADIUS) as usize * SIZE
        + (y + RADIUS) as usize;
    direction * KERNEL[index] as f64
}

#[inline]
fn fast_inv_sqrt(value: f64) -> f64 {
    // Native Mth's one-step inverse square root, not hardware reciprocal sqrt.
    let estimate = f64::from_bits(6_910_469_410_427_058_090 - (value.to_bits() >> 1));
    estimate * (1.5 - value * 0.5 * estimate * estimate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use super::super::generated::STRUCTURES;
    use super::super::jigsaw::PoolElement;
    use super::super::kinds::jigsaw_place::PoolElementPiece;
    use super::super::kinds::scattered::BuriedTreasurePiece;
    use super::super::piece::StructurePiece;
    use super::super::transform::Rotation;

    #[derive(Deserialize)]
    struct Capture {
        version: String,
        primitives: Primitives,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Primitives {
        beard: Vec<[f64; 5]>,
        bury: Vec<[f64; 4]>,
    }

    #[derive(Deserialize)]
    struct Case {
        adjustment: String,
        chunk: [i32; 2],
        inputs: Vec<Input>,
        pieces: Vec<Collected>,
        junctions: Vec<[i32; 5]>,
        affected_box: Option<[i32; 6]>,
        samples: Vec<[f64; 4]>,
    }

    #[derive(Deserialize)]
    struct Input {
        kind: String,
        #[serde(rename = "box")]
        bbox: [i32; 6],
        projection: Option<String>,
        #[serde(rename = "groundDelta")]
        ground_delta: i32,
        junctions: Vec<[i32; 5]>,
    }

    #[derive(Deserialize)]
    struct Collected {
        #[serde(rename = "box")]
        bbox: [i32; 6],
        adjustment: String,
        ground_delta: i32,
    }

    fn capture() -> Capture {
        serde_json::from_str(include_str!("../../tests/data/native_beardifier_1_21_11.json"))
            .unwrap()
    }

    fn adjustment(name: &str) -> TerrainAdjustment {
        match name {
            "NONE" => TerrainAdjustment::None,
            "BURY" => TerrainAdjustment::Bury,
            "BEARD_THIN" => TerrainAdjustment::BeardThin,
            "BEARD_BOX" => TerrainAdjustment::BeardBox,
            "ENCAPSULATE" => TerrainAdjustment::Encapsulate,
            _ => panic!("unknown native terrain adjustment {name}"),
        }
    }

    fn bbox(b: [i32; 6]) -> BoundingBox {
        BoundingBox::new(b[0], b[1], b[2], b[3], b[4], b[5])
    }

    fn junction(j: [i32; 5]) -> JigsawJunction {
        JigsawJunction {
            source_x: j[0],
            source_ground_y: j[1],
            source_z: j[2],
            delta_y: j[3],
            dest_projection: if j[4] == 0 {
                Projection::TerrainMatching
            } else {
                Projection::Rigid
            },
        }
    }

    fn assert_native(actual: f64, expected: f64, context: impl std::fmt::Debug) {
        // Decimal JSON and host libm may round double arithmetic by an ULP.
        // The kernel's native float boundary and geometric predicates remain
        // exact; this is not a state-comparison or world-parity tolerance.
        assert!(
            (actual - expected).abs() <= 4.0 * f64::EPSILON * expected.abs().max(1.0),
            "{context:?}: native {expected:.17}, runtime {actual:.17}"
        );
    }

    #[test]
    fn pinned_native_numeric_contributions() {
        let capture = capture();
        assert_eq!(capture.version, "1.21.11");
        assert_eq!(capture.primitives.beard.len(), 1120);
        assert_eq!(capture.primitives.bury.len(), 105);
        for row in capture.primitives.beard {
            assert_native(
                beard(row[0] as i32, row[1] as i32, row[2] as i32, row[3] as i32),
                row[4],
                row,
            );
        }
        for row in capture.primitives.bury {
            assert_native(bury(row[0], row[1], row[2]), row[3], row);
        }
    }

    #[test]
    fn pinned_native_collection_and_consumer() {
        static RIGID_ELEMENT: PoolElement = PoolElement::Feature {
            feature: 0,
            projection: Projection::Rigid,
        };
        static MATCHING_ELEMENT: PoolElement = PoolElement::Feature {
            feature: 0,
            projection: Projection::TerrainMatching,
        };
        let capture = capture();
        assert_eq!(capture.cases.len(), 15);
        for case in capture.cases {
            let terrain_adjustment = adjustment(&case.adjustment);
            let structure = STRUCTURES
                .iter()
                .position(|def| def.terrain_adaptation == terrain_adjustment)
                .expect("vanilla data contains every terrain adjustment") as u16;
            let mut pieces: Vec<Box<dyn StructurePiece>> = Vec::new();
            for input in &case.inputs {
                let b = bbox(input.bbox);
                let pos = BlockPos::new(b.min_x, b.min_y, b.min_z);
                if input.kind == "ordinary" {
                    let mut piece = BuriedTreasurePiece::new(pos);
                    piece.base.bbox = b;
                    assert!(piece.pool_terrain().is_none());
                    pieces.push(Box::new(piece));
                } else {
                    let element = match input.projection.as_deref() {
                        Some("RIGID") => &RIGID_ELEMENT,
                        Some("TERRAIN_MATCHING") => &MATCHING_ELEMENT,
                        other => panic!("unknown native projection {other:?}"),
                    };
                    let mut piece = PoolElementPiece::new(
                        element,
                        pos,
                        input.ground_delta,
                        Rotation::None,
                        b,
                        true,
                    );
                    piece.junctions = input.junctions.iter().copied().map(junction).collect();
                    let metadata = piece.pool_terrain().unwrap();
                    assert_eq!(metadata.ground_level_delta, input.ground_delta);
                    assert_eq!(metadata.junctions, piece.junctions);
                    pieces.push(Box::new(piece));
                }
            }
            let start = StructureStart::new(structure, case.chunk[0], case.chunk[1], pieces).unwrap();
            let field = Beardifier::for_chunk([&start], case.chunk[0], case.chunk[1]);
            let expected: Vec<_> = case.pieces.iter().map(|p| Rigid {
                bbox: bbox(p.bbox),
                adjustment: adjustment(&p.adjustment),
                ground_level_delta: p.ground_delta,
            }).collect();
            assert_eq!(field.pieces(), expected, "{} {:?}", case.adjustment, case.chunk);
            let expected_junctions: Vec<_> = case.junctions.iter().copied().map(junction).collect();
            assert_eq!(field.junctions(), expected_junctions);
            assert_eq!(field.affected_box(), case.affected_box.map(bbox));
            assert_eq!(case.samples.len(), 520);
            for row in case.samples {
                assert_native(
                    field.sample(row[0] as i32, row[1] as i32, row[2] as i32),
                    row[3],
                    (&case.adjustment, case.chunk, row),
                );
            }
        }
    }
}
