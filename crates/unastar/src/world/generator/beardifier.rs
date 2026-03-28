use std::sync::LazyLock;

const BEARD_KERNEL_RADIUS: i32 = 12;
const BEARD_KERNEL_SIZE: usize = 24;

static BEARD_KERNEL: LazyLock<Vec<f32>> = LazyLock::new(|| {
    let mut kernel = vec![0.0; BEARD_KERNEL_SIZE * BEARD_KERNEL_SIZE * BEARD_KERNEL_SIZE];
    for zi in 0..BEARD_KERNEL_SIZE as i32 {
        for xi in 0..BEARD_KERNEL_SIZE as i32 {
            for yi in 0..BEARD_KERNEL_SIZE as i32 {
                kernel[(zi as usize) * BEARD_KERNEL_SIZE * BEARD_KERNEL_SIZE
                    + (xi as usize) * BEARD_KERNEL_SIZE
                    + yi as usize] = compute_beard_contribution(
                    xi - BEARD_KERNEL_RADIUS,
                    yi - BEARD_KERNEL_RADIUS,
                    zi - BEARD_KERNEL_RADIUS,
                ) as f32;
            }
        }
    }
    kernel
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerrainAdjustment {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

impl TerrainAdjustment {
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "bury" => Some(Self::Bury),
            "beard_thin" => Some(Self::BeardThin),
            "beard_box" => Some(Self::BeardBox),
            "encapsulate" => Some(Self::Encapsulate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntBoundingBox {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl IntBoundingBox {
    pub(crate) fn from_point(x: i32, y: i32, z: i32) -> Self {
        Self {
            min: [x, y, z],
            max: [x, y, z],
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            min: [
                self.min[0].min(other.min[0]),
                self.min[1].min(other.min[1]),
                self.min[2].min(other.min[2]),
            ],
            max: [
                self.max[0].max(other.max[0]),
                self.max[1].max(other.max[1]),
                self.max[2].max(other.max[2]),
            ],
        }
    }

    pub(crate) fn inflated_by(self, amount: i32) -> Self {
        Self {
            min: [
                self.min[0] - amount,
                self.min[1] - amount,
                self.min[2] - amount,
            ],
            max: [
                self.max[0] + amount,
                self.max[1] + amount,
                self.max[2] + amount,
            ],
        }
    }

    pub(crate) fn contains(self, x: i32, y: i32, z: i32) -> bool {
        x >= self.min[0]
            && x <= self.max[0]
            && y >= self.min[1]
            && y <= self.max[1]
            && z >= self.min[2]
            && z <= self.max[2]
    }

    pub(crate) fn close_to_chunk(self, chunk_x: i32, chunk_z: i32, padding: i32) -> bool {
        let chunk_min_x = chunk_x * 16 - padding;
        let chunk_max_x = chunk_x * 16 + 15 + padding;
        let chunk_min_z = chunk_z * 16 - padding;
        let chunk_max_z = chunk_z * 16 + 15 + padding;
        self.max[0] >= chunk_min_x
            && self.min[0] <= chunk_max_x
            && self.max[2] >= chunk_min_z
            && self.min[2] <= chunk_max_z
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BeardRigid {
    pub box_bounds: IntBoundingBox,
    pub terrain_adjustment: TerrainAdjustment,
    pub ground_level_delta: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BeardJunction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StructureBeardifier {
    pieces: Vec<BeardRigid>,
    junctions: Vec<BeardJunction>,
    affected_box: Option<IntBoundingBox>,
}

impl StructureBeardifier {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.affected_box.is_none()
    }

    pub(crate) fn add_rigid(&mut self, rigid: BeardRigid) {
        self.affected_box = Some(match self.affected_box {
            Some(current) => current.union(rigid.box_bounds),
            None => rigid.box_bounds,
        });
        self.pieces.push(rigid);
    }

    pub(crate) fn add_junction(&mut self, junction: BeardJunction) {
        let junction_box = IntBoundingBox::from_point(
            junction.source_x,
            junction.source_ground_y,
            junction.source_z,
        );
        self.affected_box = Some(match self.affected_box {
            Some(current) => current.union(junction_box),
            None => junction_box,
        });
        self.junctions.push(junction);
    }

    pub(crate) fn finish(mut self) -> Self {
        self.affected_box = self.affected_box.map(|bounds| bounds.inflated_by(24));
        self
    }

    pub(crate) fn compute(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        let Some(affected_box) = self.affected_box else {
            return 0.0;
        };
        if !affected_box.contains(block_x, block_y, block_z) {
            return 0.0;
        }

        let mut noise_value = 0.0;
        for rigid in &self.pieces {
            let box_bounds = rigid.box_bounds;
            let dx = (box_bounds.min[0] - block_x)
                .max(block_x - box_bounds.max[0])
                .max(0);
            let dz = (box_bounds.min[2] - block_z)
                .max(block_z - box_bounds.max[2])
                .max(0);
            let ground_y = box_bounds.min[1] + rigid.ground_level_delta;
            let dy_to_ground = block_y - ground_y;
            let dy = match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0,
                TerrainAdjustment::Bury | TerrainAdjustment::BeardThin => dy_to_ground,
                TerrainAdjustment::BeardBox => {
                    (ground_y - block_y).max(block_y - box_bounds.max[1]).max(0)
                }
                TerrainAdjustment::Encapsulate => (box_bounds.min[1] - block_y)
                    .max(block_y - box_bounds.max[1])
                    .max(0),
            };

            noise_value += match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0.0,
                TerrainAdjustment::Bury => {
                    get_bury_contribution(dx as f64, dy as f64 / 2.0, dz as f64)
                }
                TerrainAdjustment::BeardThin | TerrainAdjustment::BeardBox => {
                    get_beard_contribution(dx, dy, dz, dy_to_ground) * 0.8
                }
                TerrainAdjustment::Encapsulate => {
                    get_bury_contribution(dx as f64 / 2.0, dy as f64 / 2.0, dz as f64 / 2.0) * 0.8
                }
            };
        }

        for junction in &self.junctions {
            let dx = block_x - junction.source_x;
            let dy = block_y - junction.source_ground_y;
            let dz = block_z - junction.source_z;
            noise_value += get_beard_contribution(dx, dy, dz, dy) * 0.4;
        }

        noise_value
    }
}

fn get_bury_contribution(dx: f64, dy: f64, dz: f64) -> f64 {
    let distance = (dx * dx + dy * dy + dz * dz).sqrt();
    clamped_map(distance, 0.0, 6.0, 1.0, 0.0)
}

fn get_beard_contribution(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f64 {
    let xi = dx + BEARD_KERNEL_RADIUS;
    let yi = dy + BEARD_KERNEL_RADIUS;
    let zi = dz + BEARD_KERNEL_RADIUS;
    if !(0..BEARD_KERNEL_SIZE as i32).contains(&xi)
        || !(0..BEARD_KERNEL_SIZE as i32).contains(&yi)
        || !(0..BEARD_KERNEL_SIZE as i32).contains(&zi)
    {
        return 0.0;
    }

    let dy_with_offset = y_to_ground as f64 + 0.5;
    let distance_sqr =
        dx as f64 * dx as f64 + dy_with_offset * dy_with_offset + dz as f64 * dz as f64;
    let value = -dy_with_offset * fast_inv_sqrt(distance_sqr / 2.0) / 2.0;
    value
        * f64::from(
            BEARD_KERNEL[zi as usize * BEARD_KERNEL_SIZE * BEARD_KERNEL_SIZE
                + xi as usize * BEARD_KERNEL_SIZE
                + yi as usize],
        )
}

fn compute_beard_contribution(dx: i32, dy: i32, dz: i32) -> f64 {
    compute_beard_contribution_offset(dx, dy as f64 + 0.5, dz)
}

fn compute_beard_contribution_offset(dx: i32, dy: f64, dz: i32) -> f64 {
    let distance_sqr = dx as f64 * dx as f64 + dy * dy + dz as f64 * dz as f64;
    (-distance_sqr / 16.0).exp()
}

fn clamped_map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    if value <= from_min {
        return to_min;
    }
    if value >= from_max {
        return to_max;
    }
    let t = (value - from_min) / (from_max - from_min);
    to_min + t * (to_max - to_min)
}

fn fast_inv_sqrt(value: f64) -> f64 {
    value.sqrt().recip()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beardifier_empty_returns_zero() {
        let beardifier = StructureBeardifier::empty();
        assert_eq!(beardifier.compute(0, 64, 0), 0.0);
    }

    #[test]
    fn beardifier_bury_contribution_is_positive_near_piece() {
        let mut beardifier = StructureBeardifier::empty();
        beardifier.add_rigid(BeardRigid {
            box_bounds: IntBoundingBox {
                min: [0, 64, 0],
                max: [4, 68, 4],
            },
            terrain_adjustment: TerrainAdjustment::Bury,
            ground_level_delta: 0,
        });
        let beardifier = beardifier.finish();
        assert!(beardifier.compute(2, 64, 2) > 0.0);
    }
}
