# Density Function Graph SIMD Vectorization Plan

## Overview
We will implement "Vertical Vectorization" for the Unastar density function graph. This involves processing 4 density points simultaneously using AVX2 intrinsics throughout the evaluation graph. This transforms the evaluation from a scalar depth-first traversal into a packet-based traversal, targeting a >3x performance improvement.

## Current State Analysis
- **Graph Evaluation**: Scalar `f64` recursive calls via `DensityFunction::compute`.
- **Noise**: `DoublePerlinNoise` has `sample_4` but it's isolated.
- **Performance**: ~1.6µs per point. ~2.0ms per chunk (noise only).
- **Complexity**: `Spline` nodes support recursive nesting (`SplineValue::Nested`), making pure SIMD control flow impossible without massive refactoring (flattening).

## Desired End State
- **SIMD Trait**: A `SimdDensityFunction` trait operating on `__m256d` (AVX2 256-bit registers).
- **Vectorized Math**: `Add`, `Mul`, `Min`, `Max`, `Abs`, `Square`, `Cube`, `Clamp` implemented with intrinsics.
- **Leaf Optimization**: `Noise` nodes call `DoublePerlinNoise::sample_4` efficiently.
- **Spline Strategy**: Hybrid approach. "Scalarize" (unpack/compute/pack) for complex nested splines to ensure correctness, with future potential for linear-spline optimization.

### Key Discoveries
- **Intrinsic Mapping**:
    - `Add` -> `_mm256_add_pd`
    - `Mul` -> `_mm256_mul_pd`
    - `Min` -> `_mm256_min_pd`
    - `Max` -> `_mm256_max_pd`
    - `Abs` -> `_mm256_andnot_pd` (mask sign bit)
    - `Clamp` -> `_mm256_max_pd` (min) then `_mm256_min_pd` (max)
- **Spline Complexity**: `SplineValue::Nested` means control flow diverges per-lane. We *must* use scalarization for the general case.

## Implementation Approach

### 1. The SIMD Context
We will define a `SimdContext` that carries 4x X, Y, Z coordinates.

```rust
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

pub struct SimdContext {
    pub x: __m256d,
    pub y: __m256d,
    pub z: __m256d,
}
```

### 2. The Trait
The trait will be feature-gated or use `cfg` attributes.

```rust
pub trait SimdDensityFunction: DensityFunction {
    /// Compute 4 values at once using AVX2.
    /// Safety: Caller must ensure AVX2 is available.
    unsafe fn compute_simd(&self, ctx: &SimdContext) -> __m256d;
}
```

### 3. Math Node Implementation (AVX2)
We will implement `SimdDensityFunction` for all math nodes.

*   **TwoArg (Add/Mul/Min/Max)**:
    *   Recursively call `arg1.compute_simd(ctx)` and `arg2.compute_simd(ctx)`.
    *   Apply intrinsic.
*   **Mapped**:
    *   `Square`: `_mm256_mul_pd(x, x)`
    *   `Cube`: `_mm256_mul_pd(x, _mm256_mul_pd(x, x))`
    *   `Abs`: Create mask `0x7FFFF...` and `_mm256_and_pd`.
    *   `HalfNegative`: Compare `_mm256_cmp_pd(x, zero, _CMP_GT_OQ)`, then `blendv`.

### 4. Noise Integration
*   The `Noise` struct currently holds `NoiseHolder`.
*   We will add `sample_simd` to `NoiseHolder` which bridges to `DoublePerlinNoise::sample_4`.
*   **Data Layout**: `sample_4` takes `[f64; 4]`. We can cast `__m256d` to `[f64; 4]` pointer (safe since layout is identical) or use `_mm256_storeu_pd`.
*   Ideally, we update `DoublePerlinNoise::sample_4` to accept `__m256d` directly to avoid store/load roundtrip, but for Phase 1, store/load is acceptable boundary overhead.

### 5. Spline Scalarization (The Fallback)
For `Spline` nodes:
1.  Store `__m256d` inputs to `[f64; 4]` array.
2.  Loop 4 times: compute scalar `evaluate_at`.
3.  Load results back to `__m256d`.
This ensures correctness for nested splines immediately.

## Phase 1: Infrastructure & Math Nodes

### Overview
Establish the trait and generic math implementations.

### Changes Required:
#### 1. `simd.rs`: Core Definitions
**File**: `crates/unastar/src/world/generator/density/simd.rs`
**Changes**:
- Define `SimdContext`.
- Define `SimdDensityFunction` trait.
- Implement `SimdDensityFunction` for `Constant` (`_mm256_set1_pd`).

#### 2. `math.rs`: Intrinsic Implementations
**File**: `crates/unastar/src/world/generator/density/math.rs`
**Changes**:
- Implement `SimdDensityFunction` for `TwoArg`.
- Implement `SimdDensityFunction` for `Mapped` (implementing `Abs`, `Square`, `Cube`, `HalfNegative`, `Squeeze`).
- Implement `SimdDensityFunction` for `Clamp`.
- Implement `SimdDensityFunction` for `MulOrAdd`.

### Success Criteria:
- `cargo build` passes.
- Unit tests verify SIMD math matches scalar math.

## Phase 2: Noise & Spline Connection

### Overview
Connect the heavy processing leaves (Noise) and complex nodes (Spline).

### Changes Required:
#### 1. `noise_funcs.rs`: Noise Vectorization
**File**: `crates/unastar/src/world/generator/density/noise_funcs.rs`
**Changes**:
- Implement `SimdDensityFunction` for `Noise`.
- Extract `SimdContext` to `[f64; 4]` arrays (or update `DoublePerlinNoise`).
- Apply `xz_scale` / `y_scale` using SIMD mul.
- Call `sample_4`.

#### 2. `spline.rs`: Scalarization Fallback
**File**: `crates/unastar/src/world/generator/density/spline.rs`
**Changes**:
- Implement `SimdDensityFunction` for `Spline`.
- Use `_mm256_storeu_pd` to unpack input coordinate.
- Loop 4x scalar evaluation.
- Use `_mm256_loadu_pd` to repack result.

#### 3. `shifted_noise.rs`: Domain Warping
**File**: `crates/unastar/src/world/generator/density/noise_funcs.rs`
**Changes**:
- Implement for `ShiftedNoise`.
- Compute x/y/z shifts in SIMD.
- Apply to context coords.
- Call `noise.sample_simd`.

## Phase 3: Integration & Vertical Loop

### Overview
Modify `NoiseChunk` to drive the SIMD graph.

### Changes Required:
#### 1. `chunk.rs`: Vertical Slice Filling
**File**: `crates/unastar/src/world/generator/density/chunk.rs`
**Changes**:
- In `NoiseInterpolator::fill_slice`, change the inner Y loop.
- Step by 4.
- Create `SimdContext` with:
    - `x`: `set1(block_x)`
    - `z`: `set1(block_z)`
    - `y`: `set_pd(block_y+3, block_y+2, block_y+1, block_y)`
- Call `wrapped.compute_simd`.
- Store result to slice.

## Testing Strategy
- **Correctness**: New test suite `tests/simd_parity.rs` generating random router graphs and asserting `scalar_result ≈ simd_result`.
- **Performance**: Use existing `density_benchmark.rs`. Expect >300% throughput on `router_compute_heavy`.

## Migration Notes
- Purely additive trait.
- `NoiseChunk` logic change is internal; external API remains unchanged.
