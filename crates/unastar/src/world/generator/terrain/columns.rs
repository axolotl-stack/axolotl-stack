//! Structure queries use native one-horizontal-cell column construction, never
//! a finished bulk chunk, a raw density point, or a synthetic default-fluid column.

use unastar_noise as noise;
use noise::features::{JavaState, java_blocks::states};

use super::{JavaDimension, JavaGenerator, TerrainBackend, java_aquifer_substance};
use crate::world::generator::aquifer::{NoiseBasedAquifer, OverworldFluidPicker};
use crate::world::generator::ore_veinifier::NativeOreVeinifier;
use crate::world::generator::structures::NativeStructureSource;

macro_rules! secondary_column {
    ($method:ident, $router:ident, $settings:ident) => {
        fn $method(&self, x: i32, z: i32, out: &mut [JavaState]) {
            use noise::$router as router;
            use noise::$settings as settings;
            out.fill(states::AIR);
            let generated = out.len().min(settings::HEIGHT as usize);
            if generated == 0 { return; }
            let source = self.native_noises().java_source();
            let mut sampler = router::NativeColumnSampler::new(x, z, &source);
            let in_z = z.rem_euclid(settings::CELL_WIDTH) as usize;
            let height = settings::CELL_HEIGHT as usize;
            sampler.initialize();
            sampler.advance_cell_x(0);
            for cell_y in (0..generated.div_ceil(height)).rev() {
                sampler.select_cell_yz(cell_y, 0);
                for in_y in (0..height).rev() {
                    let index = cell_y * height + in_y;
                    if index >= generated { continue; }
                    let y = settings::MIN_Y + index as i32;
                    sampler.update_y(y);
                    sampler.update_x(x);
                    let density = sampler.densities_4z(in_z & !3).to_array()[in_z & 3];
                    let block = if density > 0.0 {
                        settings::DEFAULT_BLOCK
                    } else {
                        super::native::disabled_aquifer(y, settings::SEA_LEVEL, settings::DEFAULT_FLUID)
                    };
                    out[index] = JavaState(u16::try_from(block).expect("native Java state exceeds its registry width"));
                }
            }
        }
    };
}

impl JavaGenerator {
    secondary_column!(nether_column, nether_compiled, nether_settings);
    secondary_column!(end_column, end_compiled, end_settings);

    fn overworld_column(&self, x: i32, z: i32, out: &mut [JavaState]) {
        use noise::overworld_settings as settings;
        out.fill(states::AIR);
        let generated = out.len().min(settings::HEIGHT as usize);
        if generated == 0 { return; }
        // Column interpolation has one horizontal cell. The actual aquifer still
        // uses its enclosing ChunkPos and full chunk-native preliminary caches.
        let source = self.native_noises().java_source();
        let flat = noise::FlatCacheGrid::new_native(x >> 4, z >> 4, &source);
        let columns = noise::ColumnContextGrid::new_native(x >> 4, z >> 4, &source, &flat);
        let mut aquifer = NoiseBasedAquifer::new(
            x >> 4, z >> 4, settings::MIN_Y, settings::HEIGHT, &source,
            &flat, &columns, self.seed, OverworldFluidPicker::new(settings::SEA_LEVEL),
        );
        let veins = NativeOreVeinifier::new(self.ore_random.clone());
        let mut sampler = noise::NativeColumnSampler::new(x, z, &source);
        let in_z = z.rem_euclid(settings::CELL_WIDTH) as usize;
        let height = settings::CELL_HEIGHT as usize;
        sampler.initialize();
        sampler.advance_cell_x(0);
        for cell_y in (0..generated.div_ceil(height)).rev() {
            sampler.select_cell_yz(cell_y, 0);
            for in_y in (0..height).rev() {
                let index = cell_y * height + in_y;
                if index >= generated { continue; }
                let y = settings::MIN_Y + index as i32;
                sampler.update_y(y);
                sampler.update_x(x);
                let density = sampler.densities_4z(in_z & !3).to_array()[in_z & 3];
                let ctx = noise::FunctionContext::new(x, y, z);
                let fluid = if density > 0.0 {
                    None
                } else {
                    aquifer.compute_substance(&ctx, density).map(java_aquifer_substance)
                };
                let block = fluid.unwrap_or_else(|| {
                    veins.compute_with_samples(
                        &ctx, sampler.vein_toggle(in_z),
                        || sampler.vein_ridged(in_z), || sampler.vein_gap(in_z),
                    ).unwrap_or(settings::DEFAULT_BLOCK)
                });
                out[index] = JavaState(u16::try_from(block).expect("native Java state exceeds its registry width"));
            }
        }
    }
}

impl NativeStructureSource for JavaGenerator {
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u8 {
        self.sample_biome(quart_x << 2, quart_y << 2, quart_z << 2) as u8
    }

    fn base_column(&self, x: i32, z: i32, out: &mut [JavaState]) {
        assert!(matches!(&self.backend, TerrainBackend::NativeJava(_)),
            "native structure columns require the native Java backend");
        match self.options.dimension {
            JavaDimension::Overworld => self.overworld_column(x, z, out),
            JavaDimension::Nether => self.nether_column(x, z, out),
            JavaDimension::End => self.end_column(x, z, out),
        }
    }
}
