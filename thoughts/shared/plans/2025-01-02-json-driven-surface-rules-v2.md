# JSON-Driven Surface Rules v2 - Direct Codegen Implementation Plan

## Overview

Refactor the surface rules system so that rules are generated **directly from vanilla JSON at build time** and can be used immediately from `unastar_noise`. The generated code produces actual `Box<dyn Rule>` trait objects.

This requires moving surface rule types (Rule, Condition, Biome, noise types) to `unastar_noise`. Block ID resolution happens at **apply time** via a closure, enabling custom blocks and totally custom generation.

## Current State Analysis

### What Exists

**Parser (Complete)** - `crates/unastar_noise/codegen/parser/surface_rule.rs`:
- Deserializes `surface_rule` JSON into typed Rust structures
- All condition types: `Biome`, `StoneDepth`, `YAbove`, `Water`, `NoiseThreshold`, `VerticalGradient`, `Steep`, `Hole`, `AbovePreliminarySurface`, `Temperature`, `Not`
- All rule types: `Sequence`, `Condition`, `Block`, `Bandlands`
- Integrated into `NoiseSettings` struct

**Surface Types in unastar** - `crates/unastar/src/world/generator/surface/`:
- `condition.rs`: 11 condition impls using `Biome`, `DoublePerlinNoise`, `Xoroshiro128`
- `rule.rs`: 4 rule impls (`BlockRule`, `SequenceRule`, `TestRule`, `BandlandsRule`)
- `context.rs`: `SurfaceContext`, `CaveSurface`, `VerticalAnchor`
- `overworld.rs`: Hand-coded rules (to be replaced)

**Noise Types in unastar** - `crates/unastar/src/world/generator/`:
- `noise.rs`: `PerlinNoise`, `OctaveNoise`, `DoublePerlinNoise`, `BlendedNoise`, `SimplexNoise` (1400+ lines, uses SIMD)
- `xoroshiro.rs`: `Xoroshiro128`, `JavaRandom`, `PositionalRandomFactory` (300 lines)
- `constants.rs`: `Biome` enum with 65 variants

**Block IDs** - `crates/unastar/src/world/chunk.rs`:
- `blocks` module uses `LazyLock<u32>` with runtime lookup via `jolyne::valentine::blocks::BLOCKS`
- Block IDs are **runtime values**, not compile-time constants

### The Problem

Generated code in `unastar_noise` cannot reference `blocks::GRASS_BLOCK` because:
1. `unastar_noise` cannot depend on `unastar` (circular dependency)
2. Block IDs require `jolyne` which is a heavy protocol dependency

### The Solution

1. **Move types to unastar_noise**: Biome, noise types, surface rule types
2. **Closure-based block lookup**: `Rule::try_apply()` takes a `&dyn Fn(&str) -> u32` closure
3. **Direct codegen**: Generated code creates actual `Box<dyn Rule>` objects
4. **Custom block support**: Closure approach allows custom blocks for custom generation

## Desired End State

After implementation:
```rust
// In unastar_noise - generated code available immediately
use unastar_noise::build_overworld_surface_rule;

// Build rules once at world init
let rules = build_overworld_surface_rule(seed);

// In unastar - provide block lookup closure at apply time
let block_id = rules.try_apply(&ctx, &|name| {
    BLOCKS.get(name).map(|b| b.default_state_id()).unwrap_or(AIR)
});

// Or with custom blocks for modded generation
let block_id = rules.try_apply(&ctx, &|name| {
    custom_block_registry.get(name).unwrap_or(0)
});
```

### Verification

- `cargo build -p unastar_noise` succeeds
- `cargo build -p unastar` succeeds
- Generated chunks show correct biome-specific surfaces:
  - Desert: sand on top, sandstone below
  - Badlands: terracotta banding
  - Frozen peaks: ice/snow
  - Mangrove swamp: mud

## What We're NOT Doing

- Not implementing Nether/End surface rules (overworld only)
- Not moving chunk encoding (stays in unastar, depends on jolyne)
- Not changing the density function system (already in unastar_noise)

---

## Phase 1: Move Noise Types to unastar_noise

### Overview

Move `Xoroshiro128`, `PerlinNoise`, `OctaveNoise`, `DoublePerlinNoise`, `BlendedNoise`, `SimplexNoise` from unastar to unastar_noise. These are self-contained and only depend on `std::simd`.

### Changes Required

#### 1. Create Noise Module in unastar_noise

**File**: `crates/unastar_noise/src/noise.rs`

Copy entire contents from `crates/unastar/src/world/generator/noise.rs`

#### 2. Create Xoroshiro Module in unastar_noise

**File**: `crates/unastar_noise/src/xoroshiro.rs`

Copy entire contents from `crates/unastar/src/world/generator/xoroshiro.rs`

#### 3. Export from lib.rs

**File**: `crates/unastar_noise/src/lib.rs`

Add:
```rust
pub mod noise;
pub mod xoroshiro;

pub use noise::{PerlinNoise, OctaveNoise, DoublePerlinNoise, BlendedNoise, SimplexNoise};
pub use xoroshiro::{Xoroshiro128, JavaRandom, PositionalRandomFactory, get_seed};
```

#### 4. Update unastar to Re-export

**File**: `crates/unastar/src/world/generator/noise.rs`

Replace entire file with:
```rust
//! Re-export noise types from unastar_noise.
pub use unastar_noise::noise::*;
```

**File**: `crates/unastar/src/world/generator/xoroshiro.rs`

Replace entire file with:
```rust
//! Re-export RNG types from unastar_noise.
pub use unastar_noise::xoroshiro::*;
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] `cargo build -p unastar` succeeds
- [ ] `cargo test -p unastar` passes (existing tests still work)

---

## Phase 2: Move Biome Enum to unastar_noise

### Overview

Move the `Biome` enum to unastar_noise. It has no external dependencies.

### Changes Required

#### 1. Create Biome Module

**File**: `crates/unastar_noise/src/biome.rs`

Copy `Biome` enum and impls from `crates/unastar/src/world/generator/constants.rs`.
(Keep `BIOME_PARAMS` in unastar for now - it's only used by climate code)

#### 2. Export from lib.rs

Add:
```rust
pub mod biome;
pub use biome::Biome;
```

#### 3. Update unastar to Re-export

**File**: `crates/unastar/src/world/generator/constants.rs`

Change:
```rust
pub use unastar_noise::Biome;
// Keep BIOME_PARAMS here
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] `cargo build -p unastar` succeeds
- [ ] All existing biome references still compile

---

## Phase 3: Move Surface Rule Types to unastar_noise

### Overview

Move `SurfaceContext`, `Condition` trait and impls, `Rule` trait and impls to unastar_noise. Update `Rule::try_apply()` to take a closure for block lookup.

### Changes Required

#### 1. Create Surface Module Structure

```
crates/unastar_noise/src/surface/
├── mod.rs
├── context.rs
├── condition.rs
└── rule.rs
```

#### 2. Move Context Types

**File**: `crates/unastar_noise/src/surface/context.rs`

Copy from `crates/unastar/src/world/generator/surface/context.rs`.
Update imports:
```rust
use crate::Biome;  // Now from unastar_noise
```

#### 3. Move Condition Types

**File**: `crates/unastar_noise/src/surface/condition.rs`

Copy from `crates/unastar/src/world/generator/surface/condition.rs`.
Update imports:
```rust
use super::context::{CaveSurface, SurfaceContext, VerticalAnchor};
use crate::Biome;
use crate::noise::DoublePerlinNoise;
use crate::xoroshiro::Xoroshiro128;
```

#### 4. Move Rule Types with Closure-based Block Lookup

**File**: `crates/unastar_noise/src/surface/rule.rs`

Copy from `crates/unastar/src/world/generator/surface/rule.rs`.

**Key Change** - Update `Rule` trait and `BlockRule`:
```rust
/// Returns a constant block (looked up by name at apply time).
#[derive(Debug, Clone)]
pub struct BlockRule {
    /// The block name (e.g., "minecraft:grass_block").
    pub block_name: String,
}

impl BlockRule {
    pub fn new(block_name: impl Into<String>) -> Self {
        Self { block_name: block_name.into() }
    }
}

/// Surface rule trait. Block lookup happens via closure at apply time.
/// This allows custom blocks for modded generation.
pub trait Rule: Send + Sync {
    fn try_apply(&self, ctx: &SurfaceContext, get_block: &dyn Fn(&str) -> u32) -> Option<u32>;
}

impl Rule for BlockRule {
    fn try_apply(&self, _ctx: &SurfaceContext, get_block: &dyn Fn(&str) -> u32) -> Option<u32> {
        Some(get_block(&self.block_name))
    }
}
```

Update `SequenceRule`, `TestRule`, `BandlandsRule` to pass `get_block` through.

#### 5. Create Surface mod.rs

**File**: `crates/unastar_noise/src/surface/mod.rs`

```rust
//! Surface rule system.

pub mod context;
pub mod condition;
pub mod rule;

pub use context::{CaveSurface, SurfaceContext, VerticalAnchor};
pub use condition::*;
pub use rule::*;
```

#### 6. Export from lib.rs

Add:
```rust
pub mod surface;
pub use surface::{
    CaveSurface, SurfaceContext, VerticalAnchor,
    Condition, Rule, BlockRule, SequenceRule, TestRule, BandlandsRule,
    // ... all condition types
};
```

#### 7. Update unastar Surface Module

**File**: `crates/unastar/src/world/generator/surface/mod.rs`

Replace with:
```rust
//! Re-export surface types from unastar_noise.
pub use unastar_noise::surface::*;

mod overworld;  // Keep for now, will be removed in Phase 6
pub use overworld::build_overworld_surface_rule;
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] `cargo build -p unastar` succeeds
- [ ] Surface rule tests pass

---

## Phase 4: Create Surface Rule Emitter

### Overview

Create an emitter that generates Rust code from parsed surface rules. The generated code constructs actual `Box<dyn Rule>` trait objects.

### Changes Required

#### 1. Create Surface Rule Emitter

**File**: `crates/unastar_noise/codegen/emitter/surface_rules.rs`

```rust
use crate::codegen::parser::surface_rule::{ConditionSource, RuleSource, VerticalAnchor, BlockState};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;

pub struct SurfaceRuleEmitter {
    /// Noise names used by conditions (need initialization)
    noises_used: HashSet<String>,
}

impl SurfaceRuleEmitter {
    pub fn new() -> Self {
        Self { noises_used: HashSet::new() }
    }

    /// Generate the complete surface rules module.
    pub fn emit_module(&mut self, rule: &RuleSource) -> TokenStream {
        // First pass: collect all noises used
        self.collect_noises(rule);

        // Generate noise initialization code
        let noise_init = self.emit_noise_init();

        // Generate the rule tree
        let rule_code = self.emit_rule(rule);

        quote! {
            //! Generated surface rules from overworld.json
            //! DO NOT EDIT - This file is generated by build.rs

            use crate::surface::*;
            use crate::surface::condition::*;
            use crate::surface::rule::*;
            use crate::noise::DoublePerlinNoise;
            use crate::xoroshiro::Xoroshiro128;
            use crate::Biome;

            /// Build the overworld surface rule from parsed JSON.
            ///
            /// Block lookup happens at apply time via closure, enabling custom blocks.
            pub fn build_overworld_surface_rule(seed: i64) -> Box<dyn Rule> {
                #noise_init
                #rule_code
            }
        }
    }

    fn emit_rule(&mut self, rule: &RuleSource) -> TokenStream {
        match rule {
            RuleSource::Sequence { sequence } => {
                let rules: Vec<_> = sequence.iter().map(|r| self.emit_rule(r)).collect();
                quote! {
                    Box::new(SequenceRule::new(vec![#(#rules),*]))
                }
            }
            RuleSource::Condition { if_true, then_run } => {
                let condition = self.emit_condition(if_true);
                let then_rule = self.emit_rule(then_run);
                quote! {
                    Box::new(TestRule::new(#condition, #then_rule))
                }
            }
            RuleSource::Block { result_state } => {
                let block_name = &result_state.name;
                quote! {
                    Box::new(BlockRule::new(#block_name))
                }
            }
            RuleSource::Bandlands {} => {
                quote! {
                    Box::new(BandlandsRule::new(seed))
                }
            }
        }
    }

    fn emit_condition(&mut self, cond: &ConditionSource) -> TokenStream {
        match cond {
            ConditionSource::Biome { biome_is } => {
                let biomes: Vec<_> = biome_is.iter().map(|b| {
                    let biome_name = b.strip_prefix("minecraft:").unwrap_or(b);
                    let ident = to_biome_ident(biome_name);
                    quote! { Biome::#ident }
                }).collect();
                quote! {
                    Box::new(BiomeCheck::multiple(vec![#(#biomes),*]))
                }
            }
            ConditionSource::StoneDepth { offset, add_surface_depth, secondary_depth_range, surface_type } => {
                let surface = if surface_type == "ceiling" {
                    quote! { CaveSurface::Ceiling }
                } else {
                    quote! { CaveSurface::Floor }
                };
                quote! {
                    Box::new(StoneDepthCheck {
                        offset: #offset,
                        add_surface_depth: #add_surface_depth,
                        secondary_depth_range: #secondary_depth_range,
                        surface_type: #surface,
                    })
                }
            }
            ConditionSource::YAbove { anchor, surface_depth_multiplier, add_stone_depth } => {
                let anchor_code = self.emit_vertical_anchor(anchor);
                quote! {
                    Box::new(YCheck {
                        anchor: #anchor_code,
                        surface_depth_multiplier: #surface_depth_multiplier,
                        add_stone_depth: #add_stone_depth,
                    })
                }
            }
            ConditionSource::Water { offset, surface_depth_multiplier, add_stone_depth } => {
                quote! {
                    Box::new(WaterCheck {
                        offset: #offset,
                        surface_depth_multiplier: #surface_depth_multiplier,
                        add_stone_depth: #add_stone_depth,
                    })
                }
            }
            ConditionSource::NoiseThreshold { noise, min_threshold, max_threshold } => {
                self.noises_used.insert(noise.clone());
                let noise_var = to_noise_var(noise);
                quote! {
                    Box::new(NoiseThreshold {
                        noise: #noise_var.clone(),
                        min_threshold: #min_threshold,
                        max_threshold: #max_threshold,
                    })
                }
            }
            ConditionSource::VerticalGradient { random_name, true_at_and_below, false_at_and_above } => {
                let true_anchor = self.emit_vertical_anchor_resolved(true_at_and_below);
                let false_anchor = self.emit_vertical_anchor_resolved(false_at_and_above);
                // Generate unique seed from random_name
                let name_hash = hash_string(random_name);
                quote! {
                    Box::new(VerticalGradient::new(#true_anchor, #false_anchor, seed ^ #name_hash))
                }
            }
            ConditionSource::Steep {} => quote! { Box::new(Steep) },
            ConditionSource::Hole {} => quote! { Box::new(Hole) },
            ConditionSource::AbovePreliminarySurface {} => quote! { Box::new(AbovePreliminarySurface) },
            ConditionSource::Temperature {} => {
                // Temperature condition needs the temperature noise
                self.noises_used.insert("minecraft:temperature".to_string());
                quote! {
                    Box::new(Temperature::new(temperature_noise.clone(), -0.45, 0.45))
                }
            }
            ConditionSource::Not { invert } => {
                let inner = self.emit_condition(invert);
                quote! { Box::new(Not::new(#inner)) }
            }
        }
    }

    fn emit_vertical_anchor(&self, anchor: &VerticalAnchor) -> TokenStream {
        match anchor {
            VerticalAnchor::AboveBottom { above_bottom } => {
                quote! { VerticalAnchor::AboveBottom(#above_bottom) }
            }
            VerticalAnchor::BelowTop { below_top } => {
                quote! { VerticalAnchor::BelowTop(#below_top) }
            }
            VerticalAnchor::Absolute { absolute } => {
                quote! { VerticalAnchor::Absolute(#absolute) }
            }
        }
    }

    fn emit_vertical_anchor_resolved(&self, anchor: &VerticalAnchor) -> TokenStream {
        // For VerticalGradient, we resolve to absolute Y at code-gen time
        // using overworld bounds (-64 to 320)
        let min_y = -64i32;
        let max_y = 320i32;
        let y = match anchor {
            VerticalAnchor::AboveBottom { above_bottom } => min_y + above_bottom,
            VerticalAnchor::BelowTop { below_top } => max_y - below_top,
            VerticalAnchor::Absolute { absolute } => *absolute,
        };
        quote! { #y }
    }

    fn emit_noise_init(&self) -> TokenStream {
        let mut inits = Vec::new();

        for noise_name in &self.noises_used {
            let var_name = to_noise_var(noise_name);
            let params = get_noise_params(noise_name);

            inits.push(quote! {
                let mut rng = Xoroshiro128::from_seed(seed ^ #params.seed_offset);
                let #var_name = DoublePerlinNoise::new(&mut rng, &#params.amplitudes, #params.first_octave);
            });
        }

        quote! { #(#inits)* }
    }

    fn collect_noises(&mut self, rule: &RuleSource) {
        match rule {
            RuleSource::Sequence { sequence } => {
                for r in sequence {
                    self.collect_noises(r);
                }
            }
            RuleSource::Condition { if_true, then_run } => {
                self.collect_noises_from_condition(if_true);
                self.collect_noises(then_run);
            }
            _ => {}
        }
    }

    fn collect_noises_from_condition(&mut self, cond: &ConditionSource) {
        match cond {
            ConditionSource::NoiseThreshold { noise, .. } => {
                self.noises_used.insert(noise.clone());
            }
            ConditionSource::Temperature {} => {
                self.noises_used.insert("minecraft:temperature".to_string());
            }
            ConditionSource::Not { invert } => {
                self.collect_noises_from_condition(invert);
            }
            _ => {}
        }
    }
}

fn to_biome_ident(name: &str) -> proc_macro2::Ident {
    // Convert "frozen_peaks" to "FrozenPeaks"
    let pascal = name.split('_')
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(c).collect(),
            }
        })
        .collect::<String>();
    proc_macro2::Ident::new(&pascal, proc_macro2::Span::call_site())
}

fn to_noise_var(name: &str) -> proc_macro2::Ident {
    // Convert "minecraft:surface" to "surface_noise"
    let base = name.strip_prefix("minecraft:").unwrap_or(name);
    let var_name = format!("{}_noise", base.replace('/', "_"));
    proc_macro2::Ident::new(&var_name, proc_macro2::Span::call_site())
}

fn hash_string(s: &str) -> i64 {
    // Simple hash for deterministic seeding
    let mut hash: i64 = 0;
    for b in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(b as i64);
    }
    hash
}

// Noise parameters lookup (from vanilla data)
struct NoiseParams {
    amplitudes: Vec<f64>,
    first_octave: i32,
    seed_offset: i64,
}

fn get_noise_params(name: &str) -> NoiseParams {
    // These should be looked up from worldgen_data/noise/*.json
    // For now, hardcode common ones
    match name {
        "minecraft:surface" => NoiseParams {
            amplitudes: vec![1.0, 1.0, 1.0],
            first_octave: -6,
            seed_offset: 0,
        },
        "minecraft:surface_swamp" => NoiseParams {
            amplitudes: vec![1.0],
            first_octave: -2,
            seed_offset: 1,
        },
        "minecraft:clay_bands_offset" => NoiseParams {
            amplitudes: vec![1.0],
            first_octave: -8,
            seed_offset: 2,
        },
        "minecraft:temperature" => NoiseParams {
            amplitudes: vec![1.5, 0.0, 1.0, 0.0, 0.0, 0.0],
            first_octave: -10,
            seed_offset: 3,
        },
        _ => NoiseParams {
            amplitudes: vec![1.0],
            first_octave: -4,
            seed_offset: hash_string(name),
        },
    }
}
```

#### 2. Update Emitter Module

**File**: `crates/unastar_noise/codegen/emitter/mod.rs`

Add:
```rust
pub mod surface_rules;
```

#### 3. Update build.rs

**File**: `crates/unastar_noise/build.rs`

Add call to surface rule emitter for overworld noise settings.

### Success Criteria

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] Generated surface_rules.rs compiles
- [ ] `build_overworld_surface_rule` function exists

---

## Phase 5: Integration and Cleanup

### Overview

Wire up the generated surface rules, create block lookup closure in unastar, remove hand-coded rules.

### Changes Required

#### 1. Create Block Lookup Helper

**File**: `crates/unastar/src/world/chunk.rs` (in existing `blocks` module)

Add a helper function:
```rust
use jolyne::valentine::blocks::BLOCKS;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Pre-built block name -> ID lookup map for surface rules.
pub static BLOCK_LOOKUP: LazyLock<HashMap<String, u32>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for block in BLOCKS.iter() {
        map.insert(block.string_id().to_string(), block.default_state_id());
    }
    map
});

/// Get block ID by name. Returns AIR if not found.
pub fn get_block_id(name: &str) -> u32 {
    BLOCK_LOOKUP.get(name).copied().unwrap_or(*AIR)
}
```

#### 2. Update Terrain Generator

**File**: `crates/unastar/src/world/generator/terrain.rs`

Change surface rule creation:
```rust
use unastar_noise::build_overworld_surface_rule;
use crate::world::chunk::blocks::get_block_id;

// In terrain generator initialization:
let surface_rule = build_overworld_surface_rule(seed);

// When applying rules - pass closure for block lookup:
if let Some(block_id) = surface_rule.try_apply(&ctx, &get_block_id) {
    chunk.set_block(x, y, z, block_id);
}

// Or inline for custom blocks:
if let Some(block_id) = surface_rule.try_apply(&ctx, &|name| {
    blocks::get_block_id(name)
}) {
    chunk.set_block(x, y, z, block_id);
}
```

#### 3. Remove Hand-Coded Rules

Delete:
- `crates/unastar/src/world/generator/surface/overworld.rs`

Update `crates/unastar/src/world/generator/surface/mod.rs`:
```rust
//! Re-export surface types from unastar_noise.
pub use unastar_noise::surface::*;
```

#### 4. Add Missing Biome Variants

Review generated code and add any missing biomes to `crates/unastar_noise/src/biome.rs`:
- `CherryGrove`
- `PaleGarden` (if 1.21+)

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` succeeds for entire workspace
- [ ] `cargo test` passes
- [ ] No compiler warnings about unused code

#### Manual Verification:
- [ ] Generate world and visually inspect biome surfaces
- [ ] Badlands shows terracotta banding
- [ ] Desert shows sand/sandstone
- [ ] Frozen peaks shows ice/snow
- [ ] Compare to vanilla Minecraft

---

## Testing Strategy

### Unit Tests

1. **Generated Rule Tests**:
   - `build_overworld_surface_rule` returns valid rule tree
   - Rule tree responds correctly to test contexts with mock block lookup

### Integration Tests

1. **Build Test**: Ensure workspace compiles
2. **Surface Application Test**: Apply generated rules to test chunks

### Manual Testing Steps

1. Generate world with seed 12345
2. Teleport to known biome locations
3. Verify surface blocks match vanilla behavior
4. Take screenshots for comparison

## Performance Considerations

- Block name → ID lookup is O(1) via HashMap (cached in `BLOCK_LOOKUP`)
- Closure passed at apply time - no trait dispatch overhead
- Rule tree is constructed once at world init
- No runtime JSON parsing

## References

- Vanilla surface rules JSON: `crates/unastar_noise/worldgen_data/noise_settings/overworld.json`
- Existing codegen pattern: `crates/unastar_noise/codegen/`
- Existing surface system: `crates/unastar/src/world/generator/surface/`
