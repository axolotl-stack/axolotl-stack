//! Java-style carvers: worm caves, cave rooms and canyons from the compiled
//! `unastar_noise::carvers` configurations.
//!
//! Every chunk within 8 chunks of the carved chunk may start carvers. For each start chunk
//! and each carver of the overworld biomes (index `i`), a legacy random seeded with
//! `setLargeFeatureSeed(seed + i, start)` decides whether the carver starts there and
//! drives its tunnels; only the carved chunk's blocks change. A per-chunk carving mask
//! stops two carvers from carving the same block twice.

use std::sync::LazyLock;

use unastar_noise::carver::{
    CanyonConfig, CaveConfig, ConfiguredCarver, cos, is_replaceable, set_large_feature_seed, sin,
};
use unastar_noise::xoroshiro::JavaRandom;
use unastar_noise::{FunctionContext, carvers, java_blocks, overworld_settings};

use super::aquifer::{FluidPicker, NoiseBasedAquifer};
use crate::world::ChunkPos;
use crate::world::chunk::MIN_Y;
use crate::world::generator::proto::DenseBlocks;

/// Default states of the blocks carving reads or writes, as Bedrock runtime ids.
struct CarverBlocks {
    grass_block: u32,
    mycelium: u32,
    dirt: u32,
    lava: u32,
}

static CARVER_BLOCKS: LazyLock<CarverBlocks> = LazyLock::new(|| {
    let id = |name: &str| {
        let java = java_blocks::default_state(name)
            .unwrap_or_else(|| panic!("{name} is not a Java block"));
        java_blocks::JAVA_TO_BEDROCK[java as usize]
    };
    CarverBlocks {
        grass_block: id("minecraft:grass_block"),
        mycelium: id("minecraft:mycelium"),
        dirt: id("minecraft:dirt"),
        lava: id("minecraft:lava"),
    }
});

/// Start chunks are searched within this many chunks of the carved chunk.
const START_RANGE: i32 = 8;
/// `WorldCarver.range`: tunnel length budget in chunks.
const CARVER_RANGE: i32 = 4;
/// Generation height and the margin carvers keep below the top.
const GEN_DEPTH: i32 = overworld_settings::HEIGHT;
const TOP_MARGIN: i32 = 7;

/// Blocks already carved in this chunk (`x`, `y - MIN_Y`, `z`).
struct CarvingMask {
    bits: Vec<u64>,
}

impl CarvingMask {
    fn new() -> Self {
        Self {
            bits: vec![0; (16 * 16 * GEN_DEPTH as usize).div_ceil(64)],
        }
    }

    #[inline]
    fn index(x: i32, y: i32, z: i32) -> usize {
        (((y - MIN_Y) as usize * 16) + z as usize) * 16 + x as usize
    }

    #[inline]
    fn get(&self, x: i32, y: i32, z: i32) -> bool {
        let i = Self::index(x, y, z);
        self.bits[i / 64] & (1 << (i % 64)) != 0
    }

    #[inline]
    fn set(&mut self, x: i32, y: i32, z: i32) {
        let i = Self::index(x, y, z);
        self.bits[i / 64] |= 1 << (i % 64);
    }
}

/// What carving needs from the generator besides the chunk.
pub trait CarveContext {
    /// The aquifer's substance for a carved (zero-density) position: `None` keeps the
    /// block (a barrier between aquifers).
    fn carve_substance(&mut self, x: i32, y: i32, z: i32) -> Option<u32>;
    /// The surface material for exposed dirt under a carved surface block.
    fn top_material(
        &mut self,
        blocks: &DenseBlocks,
        x: i32,
        y: i32,
        z: i32,
        has_fluid: bool,
    ) -> Option<u32>;
}

impl<F: FluidPicker> NoiseBasedAquifer<'_, F> {
    /// Substance of a carved position (density 0).
    pub fn carve_substance(&mut self, x: i32, y: i32, z: i32) -> Option<u32> {
        self.compute_substance(&FunctionContext::new(x, y, z), 0.0)
    }
}

struct Carving<'a, C: CarveContext> {
    chunk: &'a mut DenseBlocks,
    pos: ChunkPos,
    mask: CarvingMask,
    ctx: &'a mut C,
}

/// Carve a chunk with every carver started within 8 chunks.
pub fn carve_chunk<C: CarveContext>(
    seed: i64,
    chunk: &mut DenseBlocks,
    pos: ChunkPos,
    ctx: &mut C,
) {
    let mut carving = Carving {
        chunk,
        pos,
        mask: CarvingMask::new(),
        ctx,
    };
    let mut random = JavaRandom::from_seed(0);
    for dx in -START_RANGE..=START_RANGE {
        for dz in -START_RANGE..=START_RANGE {
            let start = ChunkPos::new(pos.x + dx, pos.z + dz);
            for (index, carver) in carvers::OVERWORLD_CARVERS.iter().enumerate() {
                set_large_feature_seed(
                    &mut random,
                    seed.wrapping_add(index as i64),
                    start.x,
                    start.z,
                );
                if random.next_float() > carver.probability() {
                    continue;
                }
                match carver {
                    ConfiguredCarver::Cave(config) => carving.cave(config, &mut random, start),
                    ConfiguredCarver::Canyon(config) => carving.canyon(config, &mut random, start),
                    ConfiguredCarver::NetherCave(_) => {
                        unreachable!("the overworld lists no nether carver (checked at build time)")
                    }
                }
            }
        }
    }
}

/// Java's tunnel thickness draw.
fn thickness(random: &mut JavaRandom) -> f32 {
    let mut value = random.next_float() * 2.0 + random.next_float();
    if random.next_int(10) == 0 {
        value *= random.next_float() * random.next_float() * 3.0 + 1.0;
    }
    value
}

/// Whether a tunnel at `(x, z)` with `remaining` steps can still reach the chunk.
fn can_reach(chunk: ChunkPos, x: f64, z: f64, step: i32, steps: i32, width: f32) -> bool {
    let dx = x - (chunk.x * 16 + 8) as f64;
    let dz = z - (chunk.z * 16 + 8) as f64;
    let remaining = (steps - step) as f64;
    let reach = (width + 2.0 + 16.0) as f64;
    dx * dx + dz * dz - remaining * remaining <= reach * reach
}

/// How an ellipsoid decides to skip a block (relative coordinates, absolute y).
enum Skip<'w> {
    Cave { floor_level: f64 },
    Canyon { widths: &'w [f32] },
}

impl Skip<'_> {
    #[inline]
    fn should_skip(&self, rx: f64, ry: f64, rz: f64, y: i32) -> bool {
        match self {
            Skip::Cave { floor_level } => ry <= *floor_level || rx * rx + ry * ry + rz * rz >= 1.0,
            Skip::Canyon { widths } => {
                let i = (y - MIN_Y) as usize;
                (rx * rx + rz * rz) * widths[i - 1] as f64 + ry * ry / 6.0 >= 1.0
            }
        }
    }
}

impl<C: CarveContext> Carving<'_, C> {
    fn cave(&mut self, config: &CaveConfig, random: &mut JavaRandom, start: ChunkPos) {
        let range = (CARVER_RANGE * 2 - 1) * 16;
        let bound = random.next_int(15) as u32 + 1;
        let bound = random.next_int(bound) as u32 + 1;
        let count = random.next_int(bound);
        for _ in 0..count {
            let x = (start.x * 16 + random.next_int(16)) as f64;
            let y = config.y.sample(random) as f64;
            let z = (start.z * 16 + random.next_int(16)) as f64;
            let horizontal = config.horizontal_radius_multiplier.sample(random) as f64;
            let vertical = config.vertical_radius_multiplier.sample(random) as f64;
            let floor_level = config.floor_level.sample(random) as f64;
            let skip = Skip::Cave { floor_level };
            let mut tunnels = 1;
            if random.next_int(4) == 0 {
                let y_scale = config.y_scale.sample(random) as f64;
                let radius = 1.0 + random.next_float() * 6.0;
                let width = 1.5 + (sin(std::f32::consts::FRAC_PI_2) * radius) as f64;
                let height = width * y_scale;
                self.ellipsoid(
                    config.replaceable,
                    config.lava_level,
                    x + 1.0,
                    y,
                    z,
                    width,
                    height,
                    &skip,
                );
                tunnels += random.next_int(4);
            }
            for _ in 0..tunnels {
                let yaw = random.next_float() * (std::f32::consts::PI * 2.0);
                let pitch = (random.next_float() - 0.5) / 4.0;
                let thickness = thickness(random);
                let steps = range - random.next_int((range / 4) as u32);
                let tunnel_seed = random.next_long();
                self.tunnel(
                    config,
                    tunnel_seed,
                    x,
                    y,
                    z,
                    horizontal,
                    vertical,
                    thickness,
                    yaw,
                    pitch,
                    0,
                    steps,
                    1.0,
                    &skip,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tunnel(
        &mut self,
        config: &CaveConfig,
        seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        horizontal: f64,
        vertical: f64,
        thickness: f32,
        mut yaw: f32,
        mut pitch: f32,
        first_step: i32,
        steps: i32,
        y_scale: f64,
        skip: &Skip<'_>,
    ) {
        let mut random = JavaRandom::from_seed(seed);
        let branch_step = random.next_int((steps / 2) as u32) + steps / 4;
        let steep = random.next_int(6) == 0;
        let mut yaw_change = 0.0f32;
        let mut pitch_change = 0.0f32;
        for step in first_step..steps {
            let width =
                1.5 + (sin(std::f32::consts::PI * step as f32 / steps as f32) * thickness) as f64;
            let height = width * y_scale;
            let horizontal_step = cos(pitch);
            x += (cos(yaw) * horizontal_step) as f64;
            y += sin(pitch) as f64;
            z += (sin(yaw) * horizontal_step) as f64;
            pitch *= if steep { 0.92 } else { 0.7 };
            pitch += pitch_change * 0.1;
            yaw += yaw_change * 0.1;
            pitch_change *= 0.9;
            yaw_change *= 0.75;
            pitch_change += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
            yaw_change += (random.next_float() - random.next_float()) * random.next_float() * 4.0;
            if step == branch_step && thickness > 1.0 {
                let left = random.next_long();
                let left_thickness = random.next_float() * 0.5 + 0.5;
                self.tunnel(
                    config,
                    left,
                    x,
                    y,
                    z,
                    horizontal,
                    vertical,
                    left_thickness,
                    yaw - std::f32::consts::FRAC_PI_2,
                    pitch / 3.0,
                    step,
                    steps,
                    1.0,
                    skip,
                );
                let right = random.next_long();
                let right_thickness = random.next_float() * 0.5 + 0.5;
                self.tunnel(
                    config,
                    right,
                    x,
                    y,
                    z,
                    horizontal,
                    vertical,
                    right_thickness,
                    yaw + std::f32::consts::FRAC_PI_2,
                    pitch / 3.0,
                    step,
                    steps,
                    1.0,
                    skip,
                );
                return;
            }
            if random.next_int(4) == 0 {
                continue;
            }
            if !can_reach(self.pos, x, z, step, steps, thickness) {
                return;
            }
            self.ellipsoid(
                config.replaceable,
                config.lava_level,
                x,
                y,
                z,
                width * horizontal,
                height * vertical,
                skip,
            );
        }
    }

    fn canyon(&mut self, config: &CanyonConfig, random: &mut JavaRandom, start: ChunkPos) {
        let range = (CARVER_RANGE * 2 - 1) * 16;
        let x = (start.x * 16 + random.next_int(16)) as f64;
        let y = config.y.sample(random) as f64;
        let z = (start.z * 16 + random.next_int(16)) as f64;
        let yaw = random.next_float() * (std::f32::consts::PI * 2.0);
        let pitch = config.vertical_rotation.sample(random);
        let y_scale = config.y_scale.sample(random) as f64;
        let thickness = config.thickness.sample(random);
        let steps = (range as f32 * config.distance_factor.sample(random)) as i32;
        let seed = random.next_long();
        self.canyon_tunnel(config, seed, x, y, z, thickness, yaw, pitch, steps, y_scale);
    }

    #[allow(clippy::too_many_arguments)]
    fn canyon_tunnel(
        &mut self,
        config: &CanyonConfig,
        seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        thickness: f32,
        mut yaw: f32,
        mut pitch: f32,
        steps: i32,
        y_scale: f64,
    ) {
        let mut random = JavaRandom::from_seed(seed);
        let mut widths = vec![0f32; GEN_DEPTH as usize];
        let mut width_factor = 1.0f32;
        for (j, slot) in widths.iter_mut().enumerate() {
            if j == 0 || random.next_int(config.width_smoothness as u32) == 0 {
                width_factor = 1.0 + random.next_float() * random.next_float();
            }
            *slot = width_factor * width_factor;
        }
        let skip = Skip::Canyon { widths: &widths };
        let mut yaw_change = 0.0f32;
        let mut pitch_change = 0.0f32;
        for step in 0..steps {
            let mut width =
                1.5 + (sin(step as f32 * std::f32::consts::PI / steps as f32) * thickness) as f64;
            let mut height = width * y_scale;
            width *= config.horizontal_radius_factor.sample(&mut random) as f64;
            // Vertical radius shaped along the canyon's length.
            let along = 1.0 - (0.5 - step as f32 / steps as f32).abs() * 2.0;
            let factor = config.vertical_radius_default_factor
                + config.vertical_radius_center_factor * along;
            height = factor as f64 * height * (random.next_float() * 0.25 + 0.75) as f64;
            let horizontal_step = cos(pitch);
            let vertical_step = sin(pitch);
            x += (cos(yaw) * horizontal_step) as f64;
            y += vertical_step as f64;
            z += (sin(yaw) * horizontal_step) as f64;
            pitch *= 0.7;
            pitch += pitch_change * 0.05;
            yaw += yaw_change * 0.05;
            pitch_change *= 0.8;
            yaw_change *= 0.5;
            pitch_change += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
            yaw_change += (random.next_float() - random.next_float()) * random.next_float() * 4.0;
            if random.next_int(4) == 0 {
                continue;
            }
            if !can_reach(self.pos, x, z, step, steps, thickness) {
                return;
            }
            self.ellipsoid(
                config.replaceable,
                config.lava_level,
                x,
                y,
                z,
                width,
                height,
                &skip,
            );
        }
    }

    /// Carve the ellipsoid around `(x, y, z)` within the carved chunk.
    #[allow(clippy::too_many_arguments)]
    fn ellipsoid(
        &mut self,
        replaceable: &[u64],
        lava_level: i32,
        x: f64,
        y: f64,
        z: f64,
        horizontal_radius: f64,
        vertical_radius: f64,
        skip: &Skip<'_>,
    ) {
        let (mid_x, mid_z) = ((self.pos.x * 16 + 8) as f64, (self.pos.z * 16 + 8) as f64);
        let reach = 16.0 + horizontal_radius * 2.0;
        if (x - mid_x).abs() > reach || (z - mid_z).abs() > reach {
            return;
        }
        let (min_x, min_z) = (self.pos.x * 16, self.pos.z * 16);
        let x0 = ((x - horizontal_radius).floor() as i32 - min_x - 1).max(0);
        let x1 = ((x + horizontal_radius).floor() as i32 - min_x).min(15);
        let y0 = ((y - vertical_radius).floor() as i32 - 1).max(MIN_Y + 1);
        let y1 = ((y + vertical_radius).floor() as i32 + 1).min(MIN_Y + GEN_DEPTH - 1 - TOP_MARGIN);
        let z0 = ((z - horizontal_radius).floor() as i32 - min_z - 1).max(0);
        let z1 = ((z + horizontal_radius).floor() as i32 - min_z).min(15);
        for lx in x0..=x1 {
            let rx = ((min_x + lx) as f64 + 0.5 - x) / horizontal_radius;
            for lz in z0..=z1 {
                let rz = ((min_z + lz) as f64 + 0.5 - z) / horizontal_radius;
                if rx * rx + rz * rz >= 1.0 {
                    continue;
                }
                let mut reached_surface = false;
                let mut by = y1;
                while by > y0 {
                    let ry = (by as f64 - 0.5 - y) / vertical_radius;
                    if !skip.should_skip(rx, ry, rz, by) && !self.mask.get(lx, by, lz) {
                        self.mask.set(lx, by, lz);
                        self.carve_block(replaceable, lava_level, lx, by, lz, &mut reached_surface);
                    }
                    by -= 1;
                }
            }
        }
    }

    fn carve_block(
        &mut self,
        replaceable: &[u64],
        lava_level: i32,
        lx: i32,
        y: i32,
        lz: i32,
        reached_surface: &mut bool,
    ) {
        let (local_x, local_z) = (lx as u8, lz as u8);
        let materials = &*CARVER_BLOCKS;
        let block = self.chunk.get_block(local_x, y as i16, local_z);
        if block == materials.grass_block || block == materials.mycelium {
            *reached_surface = true;
        }
        if !is_replaceable(replaceable, block) {
            return;
        }
        let (x, z) = (self.pos.x * 16 + lx, self.pos.z * 16 + lz);
        let carved = if y <= lava_level {
            materials.lava
        } else {
            match self.ctx.carve_substance(x, y, z) {
                Some(state) => state,
                None => return,
            }
        };
        self.chunk.set_block(local_x, y as i16, local_z, carved);
        if *reached_surface
            && self.chunk.get_block(local_x, (y - 1) as i16, local_z) == materials.dirt
        {
            let has_fluid = java_blocks_has_fluid(carved);
            if let Some(top) = self.ctx.top_material(self.chunk, x, y - 1, z, has_fluid) {
                self.chunk.set_block(local_x, (y - 1) as i16, local_z, top);
            }
        }
    }
}

#[inline]
fn java_blocks_has_fluid(block: u32) -> bool {
    java_blocks::BEDROCK_STATE_CLASSES
        .get(block as usize)
        .is_some_and(|class| class & unastar_noise::block_class::FLUID != 0)
}
