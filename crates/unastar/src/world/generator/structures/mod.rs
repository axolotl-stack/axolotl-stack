//! Dimension-native structure starts and decoration context.
//!
//! Base columns come from the generator's dedicated no-structure native column
//! sampler. Structure placement never feeds back into these queries: Java uses
//! a one-cell-wide `NoiseChunk`, an empty beardifier and no blender here, not a
//! completed chunk or point samples of the final density root.

use std::collections::HashMap;
use std::sync::Mutex;

use unastar_noise::features::java_blocks::states;
use unastar_noise::features::{Heightmap, JavaState};
use unastar_noise::structures::context::StructureContext;
use unastar_noise::structures::manager::StructureManager;

use super::JavaDimension;
use super::features::{DimensionDecoration, dimension_decoration};

/// Native generator primitives that must not query structure placement.
///
/// Implementations retain the native biome source's query history. Columns use
/// `NoiseBasedChunkGenerator.iterateNoiseColumn`: native cell interpolation and
/// aquifer material rules, with one horizontal noise cell and no beardifier.
/// This is not the bulk `fillFromNoise` traversal or a surface/carver column.
pub trait NativeStructureSource: Sync {
    /// Unzoomed source biome at a quart position, in compiled biome index space.
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u8;
    /// Fill canonical Java states starting at the dimension's storage minimum.
    /// `out` may have any length up to the logical storage height. Positions
    /// outside the clamped noise settings (above 128 in Nether/End) are air.
    fn base_column(&self, x: i32, z: i32, out: &mut [JavaState]);
}

const HEIGHTMAPS: [Heightmap; 6] = [
    Heightmap::WorldSurfaceWg,
    Heightmap::WorldSurface,
    Heightmap::OceanFloorWg,
    Heightmap::OceanFloor,
    Heightmap::MotionBlocking,
    Heightmap::MotionBlockingNoLeaves,
];

type ColumnTop = [i32; HEIGHTMAPS.len()];

const MAX_COLUMN_HEIGHT: usize = {
    use unastar_noise::{end_settings, nether_settings, overworld_settings};
    let mut height = overworld_settings::WORLD_HEIGHT;
    if nether_settings::WORLD_HEIGHT > height {
        height = nether_settings::WORLD_HEIGHT;
    }
    if end_settings::WORLD_HEIGHT > height {
        height = end_settings::WORLD_HEIGHT;
    }
    height as usize
};

/// Bound the per-world cache without sharing results between dimensions.
const COLUMN_CACHE_LIMIT: usize = 1 << 18;

/// Structure generation state of one Java dimension.
pub struct JavaStructures {
    pub manager: StructureManager,
    dimension: JavaDimension,
    columns: Mutex<HashMap<(i32, i32), ColumnTop>>,
}

impl JavaStructures {
    pub fn new(dimension: JavaDimension) -> Self {
        Self {
            manager: StructureManager::new(dimension_decoration(dimension).biomes),
            dimension,
            columns: Mutex::new(HashMap::new()),
        }
    }

    /// A statically dispatched context over the generator's native primitives.
    pub fn context<'a, S: NativeStructureSource>(
        &'a self,
        seed: i64,
        source: &'a S,
    ) -> JavaStructureContext<'a, S> {
        JavaStructureContext {
            seed,
            source,
            settings: dimension_decoration(self.dimension),
            dimension: self.dimension,
            columns: &self.columns,
        }
    }
}

/// Structure context with native source dispatch outside the voxel loop.
pub struct JavaStructureContext<'a, S: NativeStructureSource> {
    seed: i64,
    source: &'a S,
    settings: DimensionDecoration,
    dimension: JavaDimension,
    columns: &'a Mutex<HashMap<(i32, i32), ColumnTop>>,
}

impl<S: NativeStructureSource> JavaStructureContext<'_, S> {
    fn column_top(&self, x: i32, z: i32) -> ColumnTop {
        if let Some(top) = self.columns.lock().expect("column cache").get(&(x, z)) {
            return *top;
        }
        // Native getBaseHeight has the same descending column traversal up to
        // its first predicate match. Scan the native column with the actual
        // per-type predicates; floating solids and aquifer fluids both count.
        let bounds = self.dimension.block_bounds();
        let mut states = [states::AIR; MAX_COLUMN_HEIGHT];
        let column = &mut states[..(bounds.end - bounds.start) as usize];
        self.source.base_column(x, z, column);
        let top = HEIGHTMAPS.map(|kind| {
            column
                .iter()
                .rposition(|state| kind.is_opaque(*state))
                .map_or(bounds.start, |index| bounds.start + index as i32 + 1)
        });
        let mut cache = self.columns.lock().expect("column cache");
        if cache.len() >= COLUMN_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert((x, z), top);
        top
    }
}

impl<S: NativeStructureSource> StructureContext for JavaStructureContext<'_, S> {
    fn seed(&self) -> i64 {
        self.seed
    }
    fn min_y(&self) -> i32 {
        self.settings.min_y.max(self.dimension.block_bounds().start)
    }
    fn height(&self) -> i32 {
        let bounds = self.dimension.block_bounds();
        self.settings.gen_depth.min(bounds.end - bounds.start)
    }
    fn level_min_y(&self) -> i32 {
        self.dimension.block_bounds().start
    }
    fn level_height(&self) -> i32 {
        let bounds = self.dimension.block_bounds();
        bounds.end - bounds.start
    }
    fn sea_level(&self) -> i32 {
        self.settings.sea_level
    }
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u8 {
        self.source.noise_biome(quart_x, quart_y, quart_z)
    }
    fn base_height(&self, x: i32, z: i32, kind: Heightmap) -> i32 {
        let index = match kind {
            Heightmap::WorldSurfaceWg => 0,
            Heightmap::WorldSurface => 1,
            Heightmap::OceanFloorWg => 2,
            Heightmap::OceanFloor => 3,
            Heightmap::MotionBlocking => 4,
            Heightmap::MotionBlockingNoLeaves => 5,
        };
        self.column_top(x, z)[index]
    }
    fn base_column(&self, x: i32, z: i32, out: &mut [JavaState]) {
        self.source.base_column(x, z, out);
    }
}

#[cfg(test)]
mod native_column_tests {
    use super::*;
    use crate::world::generator::{JavaGenerationOptions, JavaGenerator};

    /// Actual public getBaseColumn/getBaseHeight captures, seed 0, x=z=0:
    /// native-column-corpus-20261027/columns-{overworld,nether,end}-seed0.json.
    #[test]
    fn base_queries_preserve_native_columns_and_dimension_bounds() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                type ColumnCase<'a> = (JavaDimension, &'a [(JavaState, usize)], ColumnTop);
                let cases: [ColumnCase<'_>; 3] = [
                    (
                        JavaDimension::Overworld,
                        &[(states::STONE, 120), (states::WATER, 7), (states::AIR, 257)],
                        [63, 63, 56, 56, 63, 63],
                    ),
                    (
                        JavaDimension::Nether,
                        &[
                            (states::NETHERRACK, 76),
                            (states::AIR, 19),
                            (states::NETHERRACK, 33),
                            (states::AIR, 128),
                        ],
                        [128; 6],
                    ),
                    (
                        JavaDimension::End,
                        &[
                            (states::AIR, 11),
                            (states::END_STONE, 52),
                            (states::AIR, 193),
                        ],
                        [63; 6],
                    ),
                ];
                for (dimension, runs, expected_heights) in cases {
                    let generator = JavaGenerator::with_options(
                        0,
                        JavaGenerationOptions {
                            dimension,
                            ..JavaGenerationOptions::default()
                        },
                    );
                    let structures = JavaStructures::new(dimension);
                    let context = structures.context(0, &generator);
                    let bounds = dimension.block_bounds();
                    let mut column = vec![states::AIR; (bounds.end - bounds.start) as usize];
                    context.base_column(0, 0, &mut column);
                    let mut offset = 0;
                    for &(state, length) in runs {
                        assert!(
                            column[offset..offset + length]
                                .iter()
                                .all(|&actual| actual == state),
                            "{dimension:?}: wrong native states in {offset}..{}",
                            offset + length
                        );
                        offset += length;
                    }
                    assert_eq!(offset, column.len());
                    for (kind, expected) in HEIGHTMAPS.into_iter().zip(expected_heights) {
                        assert_eq!(
                            context.base_height(0, 0, kind),
                            expected,
                            "{dimension:?}/{kind:?}"
                        );
                    }
                    assert_eq!(context.min_y(), bounds.start);
                    assert_eq!(
                        context.height(),
                        if dimension == JavaDimension::Overworld {
                            384
                        } else {
                            128
                        }
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
