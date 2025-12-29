# Bug Fix Plan: Water Pillars and Stone Roof in Density-Based Terrain

**Created**: 2025-12-28
**Status**: Implementation Complete - Awaiting Manual Verification
**Severity**: Critical - Terrain is unplayable

## Symptoms Observed

1. **Random Water Pillars**: Tall vertical columns of water appearing on the surface
2. **Infinite Stone Roof**: A continuous roof of stone blocks ~200+ blocks above the surface

## Root Cause Analysis

### Bug #1: Stone Roof (PRIMARY CAUSE)

**Location**: [overworld.rs:348-353](crates/unastar/src/world/generator/density/overworld.rs#L348-L353)

**The Problem**: The depth function formula is MISSING the critical `noiseGradientDensity` transformation that Java uses.

**Current Rust Code**:
```rust
// In overworld.rs lines 368-372
let terrain_base = add(depth.clone(), jaggedness_spline);
let terrain_shaped = mul(factor_spline, terrain_base);

// Apply squeeze to limit extreme values
let terrain_squeezed = squeeze(terrain_shaped);
```

**Java Reference** ([NoiseRouterData.java:192-194](java-ed-world/level/levelgen/NoiseRouterData.java#L192-L194)):
```java
DensityFunction densityFunction6 = DensityFunctions.mul(densityFunction5, densityFunction.halfNegative());
DensityFunction densityFunction7 = noiseGradientDensity(densityFunction3, DensityFunctions.add(densityFunction4, densityFunction6));
bootstrapContext.register(resourceKey5, DensityFunctions.add(densityFunction7, getFunction(holderGetter, BASE_3D_NOISE_OVERWORLD)));
```

**Java's `noiseGradientDensity`** ([NoiseRouterData.java:497-499](java-ed-world/level/levelgen/NoiseRouterData.java#L497-L499)):
```java
private static DensityFunction noiseGradientDensity(DensityFunction factor, DensityFunction depth) {
    DensityFunction product = DensityFunctions.mul(depth, factor);
    return DensityFunctions.mul(DensityFunctions.constant(4.0), product.quarterNegative());
}
```

**What's Missing in Rust**:
1. The `quarterNegative()` transformation is NOT applied to the terrain product
2. The `4.0` multiplier is NOT applied
3. The jaggedness is multiplied by `halfNegative()` of a noise function, which we're not doing

**Why This Causes Stone Roof**:
- Without `quarterNegative()`, large positive density values (from factor * depth) are unbounded
- The factor spline can return values up to 6.0, and depth can be large at high Y
- Result: density stays strongly positive even at high Y, creating solid blocks

### Bug #2: Water Pillars (SECONDARY CAUSE)

**Location**: [aquifer.rs:543-564](crates/unastar/src/world/generator/aquifer.rs#L543-L564)

**The Problem**: Aquifer floodedness thresholds are creating discontinuous water levels.

**Current Rust Code**:
```rust
fn compute_fluid(&self, x: i32, y: i32, z: i32) -> FluidStatus {
    let global_fluid = self.global_fluid_picker.compute_fluid(x, y, z);
    let ctx = SinglePointContext::new(x, y, z);

    let floodedness = self.floodedness_noise.compute(&ctx).clamp(-1.0, 1.0);

    if floodedness > 0.8 {
        // Fully flooded - use global fluid level
        return global_fluid;
    } else if floodedness > 0.3 {
        // Partially flooded - randomized level
        let spread = self.spread_noise.compute(&ctx) * 10.0;
        let quantized = (spread / 3.0).round() as i32 * 3;
        let base_level = grid_y(y) * Y_SPACING + 20;  // <-- BUG: Uses aquifer center Y
        let level = base_level + quantized;
        return FluidStatus::new(level, ...);
    }

    // Not flooded - WAY_BELOW_MIN_Y marker
    FluidStatus::new(WAY_BELOW_MIN_Y, FluidType::Water)
}
```

**Issues**:
1. `base_level` is computed from the **aquifer center's Y** (`grid_y(y) * Y_SPACING + 20`), not from surface detection
2. This creates water at arbitrary levels throughout the terrain
3. When density is negative (air) but floodedness is > 0.3, water appears at these computed levels
4. Result: water pillars where aquifer cells happen to overlap with air pockets

**Java's Approach** ([Aquifer.java:548-580](java-ed-world/level/levelgen/Aquifer.java)):
Java computes surface levels by sampling the preliminary surface height and maps the water level relative to actual terrain, not arbitrary grid-based values.

## Fix Strategy

### Fix #1: Implement `noiseGradientDensity` Correctly

**File**: [overworld.rs](crates/unastar/src/world/generator/density/overworld.rs)

**Changes**:

1. Add helper function for `noiseGradientDensity`:
```rust
/// Apply noise gradient density transformation.
/// This is the core terrain shaping function from Java:
///   noiseGradientDensity(factor, depth) = 4.0 * (factor * depth).quarterNegative()
fn noise_gradient_density(
    factor: Arc<dyn DensityFunction>,
    depth: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    let product = mul(depth, factor);
    let quarter_neg = Arc::new(Mapped::new(MappedType::QuarterNegative, product));
    mul(constant(4.0), quarter_neg)
}
```

2. Update terrain construction (~line 368):
```rust
// Jaggedness gets halfNegative of a 3D noise (we can simplify for now)
// In full impl: mul(jaggedness, jagged_noise.halfNegative())
let terrain_base = add(depth.clone(), jaggedness_spline);

// Apply noiseGradientDensity instead of simple multiplication
let terrain_shaped = noise_gradient_density(factor_spline, terrain_base);

// squeeze is applied after noiseGradientDensity, not before
let terrain_squeezed = squeeze(terrain_shaped);
```

3. Add base 3D noise (missing entirely):
Java's `SLOPED_CHEESE` includes `+ getFunction(holderGetter, BASE_3D_NOISE_OVERWORLD)`.

We need to add a 3D Perlin noise that creates terrain variation.

### Fix #2: Improve Aquifer Water Level Computation

**File**: [aquifer.rs](crates/unastar/src/world/generator/aquifer.rs)

**Changes**:

1. Don't use `grid_y(y) * Y_SPACING + 20` as the base level
2. Instead, use a more sensible default:
   - For floodedness > 0.8: use sea level (63)
   - For floodedness > 0.3: compute level relative to sea level, not arbitrary grid

```rust
fn compute_fluid(&self, x: i32, y: i32, z: i32) -> FluidStatus {
    let global_fluid = self.global_fluid_picker.compute_fluid(x, y, z);
    let ctx = SinglePointContext::new(x, y, z);

    let floodedness = self.floodedness_noise.compute(&ctx).clamp(-1.0, 1.0);

    // Fully flooded = use global sea level
    if floodedness > 0.8 {
        return global_fluid;
    }

    // Partially flooded = compute level based on position but relative to sea level
    if floodedness > 0.3 {
        let spread = self.spread_noise.compute(&ctx) * 10.0;
        let quantized = (spread / 3.0).round() as i32 * 3;
        // Use sea level as base, not arbitrary grid position
        let base_level = 63; // sea level
        let variation = quantized.clamp(-20, 20); // limit variation
        let level = base_level + variation;
        return FluidStatus::new(level, self.compute_fluid_type(x, y, z, &global_fluid, level));
    }

    // Not flooded
    FluidStatus::new(WAY_BELOW_MIN_Y, FluidType::Water)
}
```

### Fix #3: Slide Function Parameters

**File**: [terrain_funcs.rs](crates/unastar/src/world/generator/density/terrain_funcs.rs)

The Slide parameters look reasonable but don't match Java exactly.

**Java** ([NoiseRouterData.java:422-423](java-ed-world/level/levelgen/NoiseRouterData.java#L422-L423)):
```java
private static DensityFunction slideOverworld(boolean bl, DensityFunction densityFunction) {
    return slide(densityFunction, -64, 384, bl ? 16 : 80, bl ? 0 : 64, -0.078125, 0, 24, bl ? 0.4 : 0.1171875);
}
```

For normal overworld (bl=false):
- min_y = -64
- height = 384 (so max_y = 320)
- top_slide_size = 80
- top_slide_offset = 64
- top_slide_target = -0.078125
- bottom_slide_offset = 0
- bottom_slide_size = 24
- bottom_slide_target = 0.1171875

**Current Rust** (in Slide::new):
- top_slide_offset = -8  (WRONG - should be 64)
- top_slide_size = 16 (WRONG - should be 80)
- bottom_slide_size = 8 (WRONG - should be 24)

These need to be corrected.

## Implementation Steps

1. **[x] Fix noiseGradientDensity** (overworld.rs)
   - Add the `noise_gradient_density` helper function
   - Replace simple `mul(factor, terrain_base)` with `noise_gradient_density`
   - Add the 4.0 multiplier and quarterNegative transformation

2. **[ ] Add base 3D noise** (overworld.rs) - *Low Priority, deferred*
   - Create BlendedNoise or similar 3D noise function
   - Add it to the sloped cheese: `add(terrain_shaped, base_3d_noise)`

3. **[x] Fix Slide parameters** (terrain_funcs.rs or overworld.rs)
   - Update default Slide settings to match Java
   - Or use `Slide::with_settings()` with correct values

4. **[x] Fix aquifer fluid level computation** (aquifer.rs)
   - Remove grid-based level calculation
   - Use sea level as base for aquifer water levels

5. **[x] Remove incorrect squeeze application** (overworld.rs)
   - Squeeze was applied before noiseGradientDensity causing density inversion
   - In Java, squeeze is only applied in postProcess with 0.64 multiplier
   - Removed squeeze from terrain pipeline for now

6. **[x] Add GLOBAL_OFFSET** (overworld.rs)
   - Added GLOBAL_OFFSET constant (-0.50375) matching Java
   - Applied to offset spline output to shift terrain down
   - Adjusted spline values to produce Y=64-80 surface heights

7. **[ ] Test** - *Manual verification required*
   - Generate terrain and verify:
     - No stone roof at high Y
     - No water pillars on surface
     - Terrain height looks reasonable (~Y=64-100 for typical land)

## Priority

1. **Critical**: Fix #1 (noiseGradientDensity) - This is the main cause of the stone roof
2. **Critical**: Fix #3 (Slide parameters) - Prevents terrain at world edges
3. **Medium**: Fix #2 (aquifer levels) - Water pillars are disruptive but secondary
4. **Low**: Add base 3D noise - Improves terrain variation but not critical

## Expected Results After Fix

- Surface should appear at reasonable heights (Y=60-120 depending on terrain)
- No solid blocks above Y=200 unless extreme mountains
- Water should only appear below sea level or in proper aquifer pockets
- Underground should have proper caves and aquifer-based water pockets
