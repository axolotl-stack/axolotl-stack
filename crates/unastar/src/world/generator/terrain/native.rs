//! Concrete native preset kernels and source-native post-noise stages.

use noise::NoiseSource;
use noise::features::java_blocks::states;
use noise::java_biomes::JavaBiome;
use noise::profile::{self, Stage};
use unastar_noise as noise;

use super::{JavaDimension, JavaGenerator, JavaTerrainStage, java_aquifer_substance};
use crate::world::ChunkPos;
use crate::world::chunk::{BlockStateSpace, Chunk};
use crate::world::generator::aquifer::{NoiseBasedAquifer, OverworldFluidPicker};
use crate::world::generator::biome_source;
use crate::world::generator::carver::{self, CarveContext};
use crate::world::generator::ore_veinifier::NativeOreVeinifier;
use crate::world::generator::proto::{DenseBlocks, ProtoChunk, QuartBiomes};
use crate::world::generator::surface::SurfaceSystem;

/// Native createFluidPicker: lava's status is capped by the preset sea level.
/// This is the actual disabled-aquifer material rule, not a guessed block mapping.
pub(super) fn disabled_aquifer(y: i32, sea_level: i32, default_fluid: u32) -> u32 {
    if y < (-54).min(sea_level) {
        u32::from(states::LAVA.0)
    } else if y < sea_level {
        default_fluid
    } else {
        u32::from(states::AIR.0)
    }
}

macro_rules! secondary_stage {
    ($method:ident, $router:ident, $settings:ident, $biomes:ident) => {
        pub(super) fn $method(&self, pos: ChunkPos, stage: JavaTerrainStage) -> ProtoChunk {
            use noise::$router as router;
            use noise::$settings as settings;
            let _noise = profile::scope(Stage::Noise);
            let source = self.terrain_source(pos);
            let biomes = {
                let _biomes = profile::scope(Stage::Biomes);
                biome_source::$biomes(&source, pos)
            };
            if stage == JavaTerrainStage::Biomes {
                return self.biomes_stage(pos, biomes);
            }
            let flat = router::FlatCacheGrid::new_native(pos.x, pos.z, &source);
            let columns = router::ColumnContextGrid::new_native(pos.x, pos.z, &source, &flat);
            let mut sampler =
                router::NativeCellSampler::new(pos.x, pos.z, &flat, &columns, &source);
            let mut dense = DenseBlocks::filled_with_bounds(
                u32::from(states::AIR.0),
                BlockStateSpace::Java,
                self.options.dimension.block_bounds(),
            );
            sampler.initialize();
            let width = settings::CELL_WIDTH as usize;
            let height = settings::CELL_HEIGHT as usize;
            for cell_x in 0..16 / width {
                sampler.advance_cell_x(cell_x);
                for cell_z in 0..16 / width {
                    for cell_y in (0..settings::HEIGHT as usize / height).rev() {
                        sampler.select_cell_yz(cell_y, cell_z);
                        for in_y in (0..height).rev() {
                            let y = settings::MIN_Y + (cell_y * height + in_y) as i32;
                            sampler.update_y(y);
                            for in_x in 0..width {
                                let x = cell_x * width + in_x;
                                sampler.update_x(pos.x * 16 + x as i32);
                                for z_batch in (0..width).step_by(4) {
                                    let densities = sampler.densities_4z(z_batch).to_array();
                                    for (lane, density) in densities.into_iter().enumerate() {
                                        let z = cell_z * width + z_batch + lane;
                                        let block = if density > 0.0 {
                                            settings::DEFAULT_BLOCK
                                        } else {
                                            disabled_aquifer(
                                                y,
                                                settings::SEA_LEVEL,
                                                settings::DEFAULT_FLUID,
                                            )
                                        };
                                        if block != u32::from(states::AIR.0) {
                                            dense.set_block(x as u8, y as i16, z as u8, block);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            self.finish_native_stage(pos, stage, dense, biomes, &source, |_, y, _| {
                Some(disabled_aquifer(
                    y,
                    settings::SEA_LEVEL,
                    settings::DEFAULT_FLUID,
                ))
            })
        }
    };
}

impl JavaGenerator {
    fn prepare_carver_start(&self, start: ChunkPos) {
        let mut visited = self
            .carver_biomes
            .lock()
            .expect("carver biome cache poisoned");
        if visited.insert(start) {
            biome_source::sample_biome_native(
                self.options.dimension,
                &self.native_noises().java_source(),
                start.x.wrapping_mul(4),
                0,
                start.z.wrapping_mul(4),
            );
        }
    }

    pub(super) fn generate_overworld_native(
        &self,
        pos: ChunkPos,
        stage: JavaTerrainStage,
    ) -> ProtoChunk {
        use noise::overworld_settings as settings;
        let _noise = profile::scope(Stage::Noise);
        let source = self.terrain_source(pos);
        let flat = noise::FlatCacheGrid::new_native(pos.x, pos.z, &source);
        let columns = noise::ColumnContextGrid::new_native(pos.x, pos.z, &source, &flat);
        let biomes = {
            let _biomes = profile::scope(Stage::Biomes);
            biome_source::fill_overworld_native(&source, pos, &flat, &columns)
        };
        if stage == JavaTerrainStage::Biomes {
            return self.biomes_stage(pos, biomes);
        }
        let mut aquifer = NoiseBasedAquifer::new(
            pos.x,
            pos.z,
            settings::MIN_Y,
            settings::HEIGHT,
            &source,
            &flat,
            &columns,
            self.seed,
            OverworldFluidPicker::new(settings::SEA_LEVEL),
        );
        let veins = NativeOreVeinifier::new(self.ore_random.clone());
        let mut sampler = noise::NativeCellSampler::new(pos.x, pos.z, &flat, &columns, &source);
        let mut dense = DenseBlocks::filled_with_bounds(
            u32::from(states::AIR.0),
            BlockStateSpace::Java,
            self.options.dimension.block_bounds(),
        );
        sampler.initialize();
        let width = settings::CELL_WIDTH as usize;
        let height = settings::CELL_HEIGHT as usize;
        for cell_x in 0..16 / width {
            sampler.advance_cell_x(cell_x);
            for cell_z in 0..16 / width {
                for cell_y in (0..settings::HEIGHT as usize / height).rev() {
                    sampler.select_cell_yz(cell_y, cell_z);
                    for in_y in (0..height).rev() {
                        let y = settings::MIN_Y + (cell_y * height + in_y) as i32;
                        sampler.update_y(y);
                        for in_x in 0..width {
                            let x = cell_x * width + in_x;
                            let block_x = pos.x * 16 + x as i32;
                            sampler.update_x(block_x);
                            for z_batch in (0..width).step_by(4) {
                                let densities = sampler.densities_4z(z_batch).to_array();
                                let _fluids = profile::scope(Stage::AquiferVeins);
                                for (lane, density) in densities.into_iter().enumerate() {
                                    let in_z = z_batch + lane;
                                    let z = cell_z * width + in_z;
                                    let ctx = noise::FunctionContext::new(
                                        block_x,
                                        y,
                                        pos.z * 16 + z as i32,
                                    );
                                    let fluid = if density > 0.0 {
                                        None
                                    } else {
                                        aquifer
                                            .compute_substance(&ctx, density)
                                            .map(java_aquifer_substance)
                                    };
                                    let block = fluid.unwrap_or_else(|| {
                                        veins
                                            .compute_with_samples(
                                                &ctx,
                                                sampler.vein_toggle(in_z),
                                                || sampler.vein_ridged(in_z),
                                                || sampler.vein_gap(in_z),
                                            )
                                            .unwrap_or(settings::DEFAULT_BLOCK)
                                    });
                                    if block != u32::from(states::AIR.0) {
                                        dense.set_block(x as u8, y as i16, z as u8, block);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.finish_native_stage(pos, stage, dense, biomes, &source, |x, y, z| {
            aquifer.carve_substance(x, y, z).map(java_aquifer_substance)
        })
    }

    secondary_stage!(
        generate_nether_native,
        nether_compiled,
        nether_settings,
        fill_nether_native
    );
    secondary_stage!(
        generate_end_native,
        end_compiled,
        end_settings,
        fill_end_native
    );

    pub(super) fn biomes_stage(&self, pos: ChunkPos, mut biomes: QuartBiomes) -> ProtoChunk {
        biomes.set_block_bounds(self.options.dimension.block_bounds());
        ProtoChunk {
            pos,
            chunk: Chunk::new_with_bounds(
                pos.x,
                pos.z,
                BlockStateSpace::Java,
                self.options.dimension.block_bounds(),
            ),
            biomes,
        }
    }

    pub(super) fn generate_native_biomes(&self, pos: ChunkPos) -> QuartBiomes {
        let source = self.terrain_source(pos);
        match self.options.dimension {
            JavaDimension::Overworld => {
                let flat = noise::FlatCacheGrid::new_native(pos.x, pos.z, &source);
                let columns = noise::ColumnContextGrid::new_native(pos.x, pos.z, &source, &flat);
                biome_source::fill_overworld_native(&source, pos, &flat, &columns)
            }
            JavaDimension::Nether => biome_source::fill_nether_native(&source, pos),
            JavaDimension::End => biome_source::fill_end_native(&source, pos),
        }
    }

    fn preliminary_corners<N: NoiseSource>(&self, pos: ChunkPos, source: &N) -> [f64; 4] {
        let sample = |dx, dz| {
            let ctx = noise::FunctionContext::new(pos.x * 16 + dx, 0, pos.z * 16 + dz);
            match self.options.dimension {
                JavaDimension::Overworld => {
                    noise::compute_preliminary_surface_level_native_uncached(&ctx, source)
                }
                JavaDimension::Nether => {
                    noise::nether_compiled::compute_preliminary_surface_level_native_uncached(
                        &ctx, source,
                    )
                }
                JavaDimension::End => {
                    noise::end_compiled::compute_preliminary_surface_level_native_uncached(
                        &ctx, source,
                    )
                }
            }
        };
        [sample(0, 0), sample(16, 0), sample(0, 16), sample(16, 16)]
    }

    /// Surface's BiomeManager reads real biome-filled neighbor chunks. Do not
    /// replace those reads with a new nearest-point search: native equal-fitness
    /// winners depend on the preceding section traversal.
    pub(super) fn finish_native_stage<N, A>(
        &self,
        pos: ChunkPos,
        stage: JavaTerrainStage,
        mut dense: DenseBlocks,
        biomes: QuartBiomes,
        source: &N,
        mut carve_substance: A,
    ) -> ProtoChunk
    where
        N: NoiseSource,
        A: FnMut(i32, i32, i32) -> Option<u32>,
    {
        if stage >= JavaTerrainStage::Surface {
            let surface = profile::scope(Stage::Surface);
            // Native reference profile: center BIOMES/NOISE, then additional
            // BIOMES chunks dz-outer/dx-inner. The center is borrowed, not copied.
            let neighbors: [Option<QuartBiomes>; 9] = std::array::from_fn(|slot| {
                let dx = slot as i32 % 3 - 1;
                let dz = slot as i32 / 3 - 1;
                (slot != 4)
                    .then(|| self.generate_native_biomes(ChunkPos::new(pos.x + dx, pos.z + dz)))
            });
            let zoom_seed = biome_source::zoom_seed(self.seed);
            let mut biome_at = |x, y, z| {
                biome_source::zoomed_biome(zoom_seed, x, y, z, |qx, qy, qz| {
                    let dx = (qx >> 2) - pos.x;
                    let dz = (qz >> 2) - pos.z;
                    assert!(
                        (-1..=1).contains(&dx) && (-1..=1).contains(&dz),
                        "native surface queried outside its biome neighborhood"
                    );
                    let slot = ((dz + 1) * 3 + dx + 1) as usize;
                    let column = neighbors[slot].as_ref().unwrap_or(&biomes);
                    JavaBiome::from_index(column.at_block(
                        ((qx & 3) * 4) as u8,
                        qy * 4,
                        ((qz & 3) * 4) as u8,
                    ))
                    .expect("native biome neighborhood contains an invalid Java biome")
                })
            };
            let corners = self.preliminary_corners(pos, source);
            self.surface_system
                .build_surface(&mut dense, pos, source, corners, &mut biome_at);
            drop(surface);
            if stage == JavaTerrainStage::Carvers {
                let _carvers = profile::scope(Stage::Carvers);
                let mut carving = NativeCarveContext {
                    generator: self,
                    surface: &self.surface_system,
                    source,
                    pos,
                    corners,
                    biome_at: &mut biome_at,
                    substance: &mut carve_substance,
                };
                carver::carve_chunk(
                    self.options.dimension,
                    self.seed,
                    &mut dense,
                    pos,
                    &mut carving,
                );
            }
        }
        let _packing = profile::scope(Stage::Packing);
        ProtoChunk {
            pos,
            chunk: dense.to_chunk(pos),
            biomes,
        }
    }
}

struct NativeCarveContext<'a, N, B, A> {
    generator: &'a JavaGenerator,
    surface: &'a SurfaceSystem,
    source: &'a N,
    pos: ChunkPos,
    corners: [f64; 4],
    biome_at: &'a mut B,
    substance: &'a mut A,
}

impl<N, B, A> CarveContext for NativeCarveContext<'_, N, B, A>
where
    N: NoiseSource,
    B: FnMut(i32, i32, i32) -> JavaBiome,
    A: FnMut(i32, i32, i32) -> Option<u32>,
{
    fn prepare_start_chunk(&mut self, start: ChunkPos) {
        self.generator.prepare_carver_start(start);
    }

    fn carve_substance(&mut self, x: i32, y: i32, z: i32) -> Option<u32> {
        (self.substance)(x, y, z)
    }

    fn top_material(
        &mut self,
        blocks: &DenseBlocks,
        x: i32,
        y: i32,
        z: i32,
        has_fluid: bool,
    ) -> Option<u32> {
        self.surface.top_material(
            blocks,
            self.pos,
            self.source,
            self.corners,
            self.biome_at,
            x,
            y,
            z,
            has_fluid,
        )
    }
}
