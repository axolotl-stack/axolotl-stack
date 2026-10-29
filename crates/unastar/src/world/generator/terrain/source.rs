//! Typed native material source. Structure influence is collected once per chunk;
//! all ordinary noise methods remain statically dispatched to native arithmetic.

use std::simd::f64x4;

use unastar_noise::structures::beardifier::Beardifier;
use unastar_noise::{NoiseRef, NoiseSource};

use super::JavaGenerator;
use crate::world::ChunkPos;
use crate::world::generator::density::JavaNoiseSource;

pub(super) struct NativeTerrainSource<'a> {
    noises: JavaNoiseSource<'a>,
    beardifier: Beardifier,
}

impl JavaGenerator {
    pub(super) fn terrain_source(&self, pos: ChunkPos) -> NativeTerrainSource<'_> {
        let beardifier = if self.options.generate_structures {
            let context = self.structures.context(self.seed, self);
            let references = self.structures.manager.references(&context, pos.x, pos.z);
            Beardifier::for_chunk(
                references.iter().map(|(starts, index)| &starts[*index]),
                pos.x,
                pos.z,
            )
        } else {
            Beardifier::default()
        };
        NativeTerrainSource {
            noises: self.native_noises().java_source(),
            beardifier,
        }
    }
}

impl NoiseSource for NativeTerrainSource<'_> {
    const NATIVE_JAVA: bool = true;

    #[inline]
    fn sample(&self, noise: NoiseRef, x: f64, y: f64, z: f64) -> f64 {
        self.noises.sample(noise, x, y, z)
    }

    #[inline]
    fn sample_4(&self, noise: NoiseRef, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        self.noises.sample_4(noise, x, y, z)
    }

    #[inline]
    fn sample_end_islands(&self, x: i32, z: i32) -> f64 {
        self.noises.sample_end_islands(x, z)
    }

    #[inline]
    fn sample_beardifier(&self, x: i32, y: i32, z: i32) -> f64 {
        self.beardifier.sample(x, y, z)
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn sample_blended_noise(
        &self,
        x: f64,
        y: f64,
        z: f64,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> f64 {
        self.noises.sample_blended_noise(
            x,
            y,
            z,
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        )
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn sample_blended_noise_4(
        &self,
        x: f64,
        y: f64x4,
        z: f64,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> f64x4 {
        self.noises.sample_blended_noise_4(
            x,
            y,
            z,
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        )
    }
}
