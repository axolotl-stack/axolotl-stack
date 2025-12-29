//! Density function system for 3D terrain generation.
//!
//! This module implements Java Edition's density function system, which enables
//! composable 3D terrain density evaluation. A density value > 0 indicates solid
//! blocks, while density <= 0 indicates air or fluid.
//!
//! The system supports:
//! - Mathematical operations (add, multiply, min, max, clamp)
//! - Unary transformations (abs, square, cube, squeeze, etc.)
//! - Noise sampling with domain warping
//! - Cubic spline interpolation
//! - Multi-level caching for performance
//! - Blend functions for terrain transitions
//! - Special terrain functions (slide, beardifier, end islands)
//!
//! ## Density Function Types
//!
//! ### Math Operations (`math.rs`)
//! - `Constant` - Fixed value
//! - `TwoArg` - Binary operations (Add, Mul, Min, Max)
//! - `Mapped` - Unary transformations (Abs, Square, Cube, etc.)
//! - `Clamp` - Value clamping
//! - `YClampedGradient` - Y-based linear gradient
//! - `RangeChoice` - Conditional based on input range
//! - `MulOrAdd` - Optimized multiply-add
//!
//! ### Noise Functions (`noise_funcs.rs`)
//! - `Noise` - Basic noise sampling
//! - `ShiftedNoise` - Domain-warped noise
//! - `ShiftA`, `ShiftB`, `Shift` - Shift generators
//! - `WeirdScaledSampler` - Rarity-scaled sampling
//!
//! ### Splines (`spline.rs`)
//! - `Spline` - Cubic Hermite spline interpolation
//!
//! ### Blend Functions (`blend.rs`)
//! - `BlendAlpha` - Blend factor
//! - `BlendOffset` - Y offset for blending
//! - `BlendDensity` - Density blending
//!
//! ### Terrain Functions (`terrain_funcs.rs`)
//! - `Slide` - Vertical edge falloff
//! - `Beardifier` - Structure terrain integration
//! - `EndIslands` - End dimension terrain
//! - `OldBlendedNoise` - Legacy terrain compatibility
//!
//! ### Caching (`cache.rs`, `markers.rs`)
//! - `NoiseInterpolator` - Trilinear interpolation cache
//! - `FlatCache` - 2D XZ grid cache
//! - `Cache2D` - Single XZ position cache
//! - `CacheOnce` - Per-step cache
//! - `CacheAllInCell` - Cell precomputation
//!
//! ### NoiseChunk (`chunk.rs`)
//! - `NoiseChunk` - Cell-based interpolation engine
//! - `WrapVisitor` - Visitor for wiring cache implementations

mod blend;
mod cache;
mod chunk;
mod context;
mod function;
mod markers;
mod math;
mod noise_funcs;
mod overworld;
mod router;
mod spline;
mod terrain_funcs;
mod simd;

// Re-export core traits
pub use context::{ContextProvider, FunctionContext, SinglePointContext};
pub use function::{DensityFunction, IdentityVisitor, Visitor};

// Re-export math functions
pub use math::{
    lerp, lerp3, Clamp, Constant, Mapped, MappedType, MulOrAdd, RangeChoice, TwoArg, TwoArgType,
    YClampedGradient,
};

// Re-export cache implementations
pub use cache::{Cache2D, CacheAllInCell, CacheOnce, FlatCache, NoiseInterpolator};

// Re-export cache markers
pub use markers::{
    Cache2DMarker, CacheAllInCellMarker, CacheOnceMarker, FlatCacheMarker, Interpolated,
};

// Re-export blend functions
pub use blend::{BlendAlpha, BlendDensity, BlendOffset};

// Re-export terrain functions
pub use terrain_funcs::{Beardifier, EndIslands, OldBlendedNoise, Slide};

// Re-export noise functions
pub use noise_funcs::{
    Noise, NoiseHolder, NoiseParams, RarityType, Shift, ShiftA, ShiftB, ShiftedNoise,
    WeirdScaledSampler,
};

// Re-export splines
pub use spline::{Spline, SplineBuilder, SplinePoint, SplineValue};

// Re-export NoiseChunk
pub use chunk::{NoiseChunk, WrapVisitor};

// Re-export router and overworld builder
pub use router::NoiseRouter;
pub use overworld::{build_overworld_router, build_test_router};
