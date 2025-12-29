//! NoiseRouter - routes all density function outputs used in terrain generation.
//!
//! The NoiseRouter contains 15 density function outputs that together define
//! terrain shape, climate, aquifers, and ore veins. These functions form a
//! complex graph that is wired together during initialization.

use super::function::{DensityFunction, Visitor};
use std::sync::Arc;

/// Routes all density functions used in terrain generation.
///
/// The NoiseRouter is the central hub for all density functions used during
/// chunk generation. It contains 15 outputs that control different aspects
/// of terrain generation:
///
/// ## Aquifer Functions (4)
/// - `barrier_noise` - Controls barriers between aquifer cells
/// - `fluid_level_floodedness` - How "flooded" an area is
/// - `fluid_level_spread` - Spread of fluid levels
/// - `lava_noise` - Where lava appears in deep regions
///
/// ## Climate Functions (6)
/// - `temperature` - Temperature parameter for biomes
/// - `vegetation` - Vegetation/humidity parameter for biomes
/// - `continents` - Continentalness (ocean vs land)
/// - `erosion` - Terrain erosion level
/// - `depth` - Depth from surface
/// - `ridges` - Ridge/peak locations
///
/// ## Terrain Functions (2)
/// - `preliminary_surface_level` - Initial surface Y calculation
/// - `final_density` - The main 3D density output
///
/// ## Ore Vein Functions (3)
/// - `vein_toggle` - Enables/disables vein generation
/// - `vein_ridged` - Vein ridge pattern
/// - `vein_gap` - Gaps in veins
#[derive(Clone)]
pub struct NoiseRouter {
    // ========== Aquifer Functions ==========
    /// Noise that creates barriers between aquifer cells.
    pub barrier_noise: Arc<dyn DensityFunction>,

    /// How "flooded" an area is - higher values mean more water.
    pub fluid_level_floodedness: Arc<dyn DensityFunction>,

    /// Controls spread/variation of fluid levels between aquifers.
    pub fluid_level_spread: Arc<dyn DensityFunction>,

    /// Where lava appears instead of water in deep regions.
    pub lava_noise: Arc<dyn DensityFunction>,

    // ========== Climate Functions ==========
    /// Temperature parameter for biome selection.
    /// Range: typically -1.0 to 1.0
    pub temperature: Arc<dyn DensityFunction>,

    /// Vegetation/humidity parameter for biome selection.
    /// Range: typically -1.0 to 1.0
    pub vegetation: Arc<dyn DensityFunction>,

    /// Continentalness - controls ocean vs land.
    /// Low values = ocean, high values = inland.
    pub continents: Arc<dyn DensityFunction>,

    /// Erosion level - controls terrain smoothness.
    /// High erosion = flatter terrain.
    pub erosion: Arc<dyn DensityFunction>,

    /// Depth from surface - used in terrain shaping.
    pub depth: Arc<dyn DensityFunction>,

    /// Ridge/peak noise - controls where mountains form.
    /// Used to create ridges and peaks.
    pub ridges: Arc<dyn DensityFunction>,

    // ========== Terrain Functions ==========
    /// Preliminary surface level calculation.
    /// Used before full density evaluation.
    pub preliminary_surface_level: Arc<dyn DensityFunction>,

    /// The final 3D terrain density.
    /// Positive = solid block, negative/zero = air/fluid.
    pub final_density: Arc<dyn DensityFunction>,

    // ========== Ore Vein Functions ==========
    /// Toggle for ore vein generation.
    pub vein_toggle: Arc<dyn DensityFunction>,

    /// Ridged pattern for ore veins.
    pub vein_ridged: Arc<dyn DensityFunction>,

    /// Gap pattern in ore veins.
    pub vein_gap: Arc<dyn DensityFunction>,
}

impl NoiseRouter {
    /// Create a new NoiseRouter with all density functions.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        barrier_noise: Arc<dyn DensityFunction>,
        fluid_level_floodedness: Arc<dyn DensityFunction>,
        fluid_level_spread: Arc<dyn DensityFunction>,
        lava_noise: Arc<dyn DensityFunction>,
        temperature: Arc<dyn DensityFunction>,
        vegetation: Arc<dyn DensityFunction>,
        continents: Arc<dyn DensityFunction>,
        erosion: Arc<dyn DensityFunction>,
        depth: Arc<dyn DensityFunction>,
        ridges: Arc<dyn DensityFunction>,
        preliminary_surface_level: Arc<dyn DensityFunction>,
        final_density: Arc<dyn DensityFunction>,
        vein_toggle: Arc<dyn DensityFunction>,
        vein_ridged: Arc<dyn DensityFunction>,
        vein_gap: Arc<dyn DensityFunction>,
    ) -> Self {
        Self {
            barrier_noise,
            fluid_level_floodedness,
            fluid_level_spread,
            lava_noise,
            temperature,
            vegetation,
            continents,
            erosion,
            depth,
            ridges,
            preliminary_surface_level,
            final_density,
            vein_toggle,
            vein_ridged,
            vein_gap,
        }
    }

    /// Apply a visitor to all density functions.
    ///
    /// This is used during the wiring phase to replace marker functions
    /// with actual cache implementations.
    pub fn map_all(&self, visitor: &dyn Visitor) -> NoiseRouter {
        NoiseRouter {
            barrier_noise: self.barrier_noise.map_all(visitor),
            fluid_level_floodedness: self.fluid_level_floodedness.map_all(visitor),
            fluid_level_spread: self.fluid_level_spread.map_all(visitor),
            lava_noise: self.lava_noise.map_all(visitor),
            temperature: self.temperature.map_all(visitor),
            vegetation: self.vegetation.map_all(visitor),
            continents: self.continents.map_all(visitor),
            erosion: self.erosion.map_all(visitor),
            depth: self.depth.map_all(visitor),
            ridges: self.ridges.map_all(visitor),
            preliminary_surface_level: self.preliminary_surface_level.map_all(visitor),
            final_density: self.final_density.map_all(visitor),
            vein_toggle: self.vein_toggle.map_all(visitor),
            vein_ridged: self.vein_ridged.map_all(visitor),
            vein_gap: self.vein_gap.map_all(visitor),
        }
    }

    /// Get the final terrain density function.
    pub fn final_density(&self) -> &Arc<dyn DensityFunction> {
        &self.final_density
    }

    /// Get the temperature climate function.
    pub fn temperature(&self) -> &Arc<dyn DensityFunction> {
        &self.temperature
    }

    /// Get the vegetation climate function.
    pub fn vegetation(&self) -> &Arc<dyn DensityFunction> {
        &self.vegetation
    }

    /// Get the continents climate function.
    pub fn continents(&self) -> &Arc<dyn DensityFunction> {
        &self.continents
    }

    /// Get the erosion climate function.
    pub fn erosion(&self) -> &Arc<dyn DensityFunction> {
        &self.erosion
    }

    /// Get the ridges climate function.
    pub fn ridges(&self) -> &Arc<dyn DensityFunction> {
        &self.ridges
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::math::Constant;

    fn create_test_router() -> NoiseRouter {
        let constant = Arc::new(Constant::new(0.0));
        NoiseRouter::new(
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
            constant.clone(),
        )
    }

    #[test]
    fn test_router_creation() {
        let router = create_test_router();
        // Just verify it creates successfully
        assert!(Arc::strong_count(&router.final_density) >= 1);
    }

    #[test]
    fn test_router_map_all() {
        use crate::world::generator::density::function::IdentityVisitor;

        let router = create_test_router();
        let visitor = IdentityVisitor;
        let mapped = router.map_all(&visitor);

        // After mapping with identity, functions should still work
        use crate::world::generator::density::context::SinglePointContext;
        let ctx = SinglePointContext::new(0, 0, 0);
        assert_eq!(mapped.final_density.compute(&ctx), 0.0);
    }
}
