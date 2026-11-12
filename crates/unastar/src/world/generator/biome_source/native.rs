//! Native preset climate arithmetic and source-specific biome selection.

use std::ops::Range;

use noise::NoiseSource;
use noise::java_biomes::JavaBiome;
use unastar_noise as noise;

use super::super::JavaDimension;
use super::super::proto::{NativeBiome, QuartBiomes};
use crate::world::ChunkPos;
use crate::world::chunk::MIN_Y;

macro_rules! climate_target {
    ($router:path, $ctx:expr, $noises:expr) => {{
        use $router as router;
        noise::overworld_biome_source::target(
            router::compute_temperature_native_uncached($ctx, $noises),
            router::compute_vegetation_native_uncached($ctx, $noises),
            router::compute_continents_native_uncached($ctx, $noises),
            router::compute_erosion_native_uncached($ctx, $noises),
            router::compute_depth_native_uncached($ctx, $noises),
            router::compute_ridges_native_uncached($ctx, $noises),
        )
    }};
}

/// A native quart query uses the selected source's real previous-candidate history.
/// End selection is not a nearest-climate search and does not touch either tree.
pub fn sample_biome_native<N: NoiseSource>(
    dimension: JavaDimension,
    noises: &N,
    quart_x: i32,
    quart_y: i32,
    quart_z: i32,
) -> JavaBiome {
    let ctx = noise::FunctionContext::new(quart_x << 2, quart_y << 2, quart_z << 2);
    match dimension {
        JavaDimension::Overworld => noise::overworld_biome_source::find(&overworld_target_native(
            noises, quart_x, quart_y, quart_z,
        )),
        JavaDimension::Nether => noise::nether_biome_source::find(&climate_target!(
            unastar_noise::nether_compiled,
            &ctx,
            noises
        )),
        JavaDimension::End => super::sample_end_biome(quart_x, quart_z, |x, z| {
            noise::end_compiled::compute_erosion_native_uncached(
                &noise::FunctionContext::new(x, 0, z),
                noises,
            )
        }),
    }
}

/// Fill sections in native ascending-section/X/Y/Z order. Only adjacent identical
/// targets may reuse a result: another query can change an equal-fitness winner.
pub(super) fn fill_lattice(
    bounds: Range<i32>,
    mut target: impl FnMut(usize, i32, usize) -> [i64; 7],
    mut lookup: impl FnMut(&[i64; 7]) -> JavaBiome,
) -> QuartBiomes {
    let mut biomes = QuartBiomes::filled(0);
    biomes.set_block_bounds(bounds.clone());
    let mut previous = None;
    for section_y in (bounds.start..bounds.end).step_by(16) {
        for qx in 0..4 {
            for local_y in 0..4 {
                let y = section_y + local_y * 4;
                let qy = ((y - MIN_Y) >> 2) as usize;
                for qz in 0..4 {
                    let target = target(qx, y, qz);
                    let biome = match previous {
                        Some((last, biome)) if last == target => biome,
                        _ => lookup(&target),
                    };
                    previous = Some((target, biome));
                    biomes.set(qx, qy, qz, biome as NativeBiome);
                }
            }
        }
    }
    biomes
}

/// Overworld climate target at an absolute quart, in Java-exact arithmetic,
/// before a caller chooses its search-history policy.
pub(crate) fn overworld_target_native<N: NoiseSource>(
    noises: &N,
    quart_x: i32,
    quart_y: i32,
    quart_z: i32,
) -> [i64; 7] {
    let ctx = noise::FunctionContext::new(quart_x << 2, quart_y << 2, quart_z << 2);
    climate_target!(unastar_noise, &ctx, noises)
}

/// A native biome fill and, when every search it ran found a unique nearest
/// leaf, how to replay it. Such a fill has the same biomes from every search
/// history, and leaves the calling thread's history at its last leaf.
pub struct NativeFill {
    pub biomes: QuartBiomes,
    pub replay: Option<Replay>,
}

/// Restores the search history a history-independent fill leaves behind.
#[derive(Clone, Copy)]
pub struct Replay {
    last_leaf: u16,
    restore: fn(u16),
}

impl Replay {
    /// Leaves the calling thread's search history as running the fill would.
    pub fn apply(self) {
        (self.restore)(self.last_leaf);
    }
}

/// Records the searches a fill runs (reused identical targets run none).
struct Trace {
    last_leaf: Option<u16>,
    unique: bool,
}

impl Trace {
    fn new() -> Self {
        Self {
            last_leaf: None,
            unique: true,
        }
    }

    fn record(&mut self, leaf: u16, unique: bool) {
        self.last_leaf = Some(leaf);
        self.unique &= unique;
    }

    fn finish(self, biomes: QuartBiomes, restore: fn(u16)) -> NativeFill {
        let replay = self
            .last_leaf
            .filter(|_| self.unique)
            .map(|last_leaf| Replay { last_leaf, restore });
        NativeFill { biomes, replay }
    }
}

/// Overworld depth varies with Y; the other five vanilla climate axes do not.
/// Scalar cache extraction and those five samples happen once per quart column.
pub fn fill_overworld_native<N: NoiseSource>(
    noises: &N,
    pos: ChunkPos,
    flat: &noise::FlatCacheGrid,
    columns: &noise::ColumnContextGrid,
) -> NativeFill {
    let mut trace = Trace::new();
    let biomes = fill_overworld_native_with(noises, pos, flat, columns, |target| {
        let (biome, found) = noise::overworld_biome_source::find_traced(target);
        trace.record(found.leaf, found.unique);
        biome
    });
    trace.finish(biomes, noise::overworld_biome_source::restore)
}

/// [`fill_overworld_native`] with an edition-owned biome lookup policy.
pub(crate) fn fill_overworld_native_with<N: NoiseSource>(
    noises: &N,
    pos: ChunkPos,
    flat: &noise::FlatCacheGrid,
    columns: &noise::ColumnContextGrid,
    lookup: impl FnMut(&[i64; 7]) -> JavaBiome,
) -> QuartBiomes {
    let columns: [_; 16] =
        std::array::from_fn(|i| columns.get((i % 4) * 4, (i / 4) * 4).as_scalar());
    let climates: [[f64; 5]; 16] = std::array::from_fn(|i| {
        let ctx = noise::FunctionContext::new(
            pos.x * 16 + (i % 4) as i32 * 4,
            0,
            pos.z * 16 + (i / 4) as i32 * 4,
        );
        let col = &columns[i];
        [
            noise::compute_temperature_native(&ctx, noises, flat, col),
            noise::compute_vegetation_native(&ctx, noises, flat, col),
            noise::compute_continents_native(&ctx, noises, flat, col),
            noise::compute_erosion_native(&ctx, noises, flat, col),
            noise::compute_ridges_native(&ctx, noises, flat, col),
        ]
    });
    fill_lattice(
        JavaDimension::Overworld.block_bounds(),
        |qx, y, qz| {
            let i = qz * 4 + qx;
            let [temperature, vegetation, continents, erosion, ridges] = climates[i];
            let ctx = noise::FunctionContext::new(
                pos.x * 16 + qx as i32 * 4,
                y,
                pos.z * 16 + qz as i32 * 4,
            );
            noise::overworld_biome_source::target(
                temperature,
                vegetation,
                continents,
                erosion,
                noise::compute_depth_native(&ctx, noises, flat, &columns[i]),
                ridges,
            )
        },
        lookup,
    )
}

/// Nether's native climate has no Y dependency. Reuse arithmetic, not search
/// results: section traversal still updates the native tree at every distinct target.
pub fn fill_nether_native<N: NoiseSource>(noises: &N, pos: ChunkPos) -> NativeFill {
    let targets: [[i64; 7]; 16] = std::array::from_fn(|i| {
        let ctx = noise::FunctionContext::new(
            pos.x * 16 + (i % 4) as i32 * 4,
            0,
            pos.z * 16 + (i / 4) as i32 * 4,
        );
        climate_target!(unastar_noise::nether_compiled, &ctx, noises)
    });
    let mut trace = Trace::new();
    let biomes = fill_lattice(
        JavaDimension::Nether.block_bounds(),
        |qx, _, qz| targets[qz * 4 + qx],
        |target| {
            let (biome, found) = noise::nether_biome_source::find_traced(target);
            trace.record(found.leaf, found.unique);
            biome
        },
    );
    trace.finish(biomes, noise::nether_biome_source::restore)
}

/// Every End quart of a chunk shares its containing-chunk selector sample.
pub fn fill_end_native<N: NoiseSource>(noises: &N, pos: ChunkPos) -> QuartBiomes {
    let biome = sample_biome_native(JavaDimension::End, noises, pos.x * 4, 0, pos.z * 4);
    let mut biomes = QuartBiomes::filled(biome as NativeBiome);
    biomes.set_block_bounds(JavaDimension::End.block_bounds());
    biomes
}
