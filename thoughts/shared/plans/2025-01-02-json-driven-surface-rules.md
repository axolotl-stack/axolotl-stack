# JSON-Driven Surface Rules Implementation Plan

## Overview

Refactor the surface rules system to parse and generate rules from the vanilla `noise_settings/overworld.json` file at build time, replacing the hand-coded rules in `overworld.rs`. This will enable accurate vanilla-matching surface generation including proper biome-specific blocks (badlands terracotta, frozen peaks ice, desert sand, etc.).

## Current State Analysis

### What Exists

**Surface System** (`crates/unastar/src/world/generator/surface/`):
- `condition.rs`: 11 condition types (`BiomeCheck`, `StoneDepthCheck`, `YCheck`, `WaterCheck`, `NoiseThreshold`, `VerticalGradient`, `Steep`, `Hole`, `AbovePreliminarySurface`, `Not`, `Temperature`)
- `rule.rs`: 4 rule types (`BlockRule`, `SequenceRule`, `TestRule`, `BandlandsRule`)
- `context.rs`: `SurfaceContext` with position, biome, water level, stone depth tracking
- `system.rs`: `SurfaceSystem` that applies rules during chunk generation
- `overworld.rs`: **Hand-coded** ~300 lines of simplified surface rules

**Codegen Pattern** (`crates/unastar_noise/codegen/`):
- `parser/` - JSON parsing with serde into typed Rust structures
- `emitter/` - Code generation using `quote` crate
- `build.rs` - Orchestrates parsing and emission

### What's Missing

1. **JSON Parser** for surface rules - the `surface_rule` object in `noise_settings/overworld.json` is ~2600 lines of nested conditions/rules
2. **Code Generator** to emit Rust code from parsed surface rules
3. **Proper Biome Matching** - vanilla uses `biome_is: ["minecraft:badlands", ...]` arrays
4. **Complete Vanilla Rules** - current hand-coded rules miss many biome-specific behaviors

### Key Discoveries

From `overworld.json` surface_rule analysis:
- **Structure**: Nested tree of `sequence`, `condition`, and `block` nodes
- **Condition Types Used**: `vertical_gradient`, `above_preliminary_surface`, `stone_depth`, `biome`, `y_above`, `water`, `noise_threshold`, `steep`, `hole`, `not`, `temperature`
- **Rule Types Used**: `sequence`, `condition`, `block`, `bandlands`
- **Biome Lists**: Rules use arrays like `"biome_is": ["minecraft:badlands", "minecraft:eroded_badlands", "minecraft:wooded_badlands"]`
- **Vertical Anchors**: `{"above_bottom": 5}`, `{"absolute": 97}`, etc.

## Desired End State

After implementation:
1. Surface rules are parsed from `worldgen_data/noise_settings/overworld.json` at build time
2. Generated Rust code creates the complete vanilla surface rule tree
3. All 65 biomes have correct surface blocks matching vanilla behavior
4. Badlands has proper terracotta banding with colored layers
5. The hand-coded `overworld.rs` is replaced by generated code

### Verification

- `cargo build` succeeds with generated surface rules
- Visual inspection of generated chunks shows correct biome surfaces:
  - Desert: sand on top, sandstone below
  - Badlands: terracotta banding with orange/white/red/yellow layers
  - Frozen peaks: packed ice on steep slopes, snow blocks elsewhere
  - Mangrove swamp: mud surface
  - Mushroom fields: mycelium surface

## What We're NOT Doing

- Not implementing Nether/End surface rules (overworld only for now)
- Not adding new condition/rule types beyond what vanilla uses
- Not optimizing the rule tree (keeping it structurally similar to vanilla)
- Not handling custom datapacks (only vanilla overworld.json)

## Implementation Approach

Use the existing codegen architecture: **Parse JSON → Analyze → Emit Rust Code**

The generated code will construct the rule tree at runtime using the existing `Rule` and `Condition` traits, avoiding the need for runtime JSON parsing.

---

## Phase 1: JSON Parser for Surface Rules

### Overview

Create a parser module that deserializes the `surface_rule` JSON into typed Rust structures that can be processed by the emitter.

### Changes Required

#### 1. Create Surface Rule Parser

**File**: `crates/unastar_noise/codegen/parser/surface_rule.rs`

```rust
use serde::Deserialize;

/// Vertical anchor for Y coordinate resolution
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VerticalAnchor {
    AboveBottom { above_bottom: i32 },
    BelowTop { below_top: i32 },
    Absolute { absolute: i32 },
}

/// Block state from JSON
#[derive(Debug, Clone, Deserialize)]
pub struct BlockState {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Properties", default)]
    pub properties: Option<std::collections::HashMap<String, String>>,
}

/// Surface rule condition source
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ConditionSource {
    #[serde(rename = "minecraft:biome")]
    Biome { biome_is: Vec<String> },

    #[serde(rename = "minecraft:stone_depth")]
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: String, // "floor" or "ceiling"
    },

    #[serde(rename = "minecraft:y_above")]
    YAbove {
        anchor: VerticalAnchor,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },

    #[serde(rename = "minecraft:water")]
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },

    #[serde(rename = "minecraft:noise_threshold")]
    NoiseThreshold {
        noise: String,
        min_threshold: f64,
        max_threshold: f64,
    },

    #[serde(rename = "minecraft:vertical_gradient")]
    VerticalGradient {
        random_name: String,
        true_at_and_below: VerticalAnchor,
        false_at_and_above: VerticalAnchor,
    },

    #[serde(rename = "minecraft:steep")]
    Steep,

    #[serde(rename = "minecraft:hole")]
    Hole,

    #[serde(rename = "minecraft:above_preliminary_surface")]
    AbovePreliminarySurface,

    #[serde(rename = "minecraft:temperature")]
    Temperature,

    #[serde(rename = "minecraft:not")]
    Not { invert: Box<ConditionSource> },
}

/// Surface rule source
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum RuleSource {
    #[serde(rename = "minecraft:sequence")]
    Sequence { sequence: Vec<RuleSource> },

    #[serde(rename = "minecraft:condition")]
    Condition {
        if_true: ConditionSource,
        then_run: Box<RuleSource>,
    },

    #[serde(rename = "minecraft:block")]
    Block { result_state: BlockState },

    #[serde(rename = "minecraft:bandlands")]
    Bandlands,
}
```

#### 2. Update Parser Module

**File**: `crates/unastar_noise/codegen/parser/mod.rs`

Add:
```rust
pub mod surface_rule;
```

#### 3. Extract Surface Rule from NoiseSettings

**File**: `crates/unastar_noise/codegen/parser/noise_settings.rs`

Add `surface_rule` field to `NoiseSettings` struct:
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct NoiseSettings {
    // ... existing fields ...
    pub surface_rule: surface_rule::RuleSource,
}
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] Parser correctly deserializes overworld.json surface_rule (add test)
- [ ] All condition types are parsed without errors
- [ ] All rule types are parsed without errors

#### Manual Verification:
- [ ] Print parsed rule tree and verify structure matches JSON

---

## Phase 2: Surface Rule Code Emitter

### Overview

Create an emitter that generates Rust code to construct the surface rule tree at runtime using the existing `Rule` and `Condition` traits.

### Changes Required

#### 1. Create Surface Rule Emitter

**File**: `crates/unastar_noise/codegen/emitter/surface_rules.rs`

```rust
use crate::codegen::parser::surface_rule::{ConditionSource, RuleSource, VerticalAnchor, BlockState};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;

pub struct SurfaceRuleEmitter {
    /// Noise names used by noise_threshold conditions
    noises_used: HashSet<String>,
    /// Counter for generating unique variable names
    var_counter: usize,
}

impl SurfaceRuleEmitter {
    pub fn new() -> Self {
        Self {
            noises_used: HashSet::new(),
            var_counter: 0,
        }
    }

    /// Generate the complete surface rules module
    pub fn emit_module(&mut self, rule: &RuleSource) -> TokenStream {
        // Collect all noises used
        self.collect_noises(rule);

        // Generate the rule tree
        let rule_code = self.emit_rule(rule);

        // Generate noise initialization
        let noise_init = self.emit_noise_init();

        quote! {
            //! Generated surface rules from overworld.json
            //! DO NOT EDIT - This file is generated by build.rs

            use crate::world::generator::surface::*;
            use crate::world::generator::surface::condition::*;
            use crate::world::generator::surface::rule::*;
            use crate::world::generator::noise::DoublePerlinNoise;
            use crate::world::generator::xoroshiro::Xoroshiro128;
            use crate::world::generator::constants::Biome;
            use crate::world::chunk::blocks;

            /// Build the overworld surface rule from parsed JSON.
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
                let block = self.emit_block_state(result_state);
                quote! {
                    Box::new(BlockRule::new(#block))
                }
            }
            RuleSource::Bandlands => {
                quote! {
                    Box::new(build_bandlands_rule(seed))
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
            // ... other conditions ...
        }
    }

    fn emit_block_state(&self, state: &BlockState) -> TokenStream {
        let block_name = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
        let block_const = to_block_const(block_name);
        quote! { *blocks::#block_const }
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

fn to_block_const(name: &str) -> proc_macro2::Ident {
    // Convert "grass_block" to "GRASS_BLOCK"
    let upper = name.to_uppercase();
    proc_macro2::Ident::new(&upper, proc_macro2::Span::call_site())
}
```

#### 2. Add Bandlands Rule Builder

The `BandlandsRule` needs the 192-element terracotta array. Add a builder function:

**File**: `crates/unastar/src/world/generator/surface/rule.rs`

Add function to generate the terracotta bands array matching Java's algorithm.

#### 3. Update Emitter Module

**File**: `crates/unastar_noise/codegen/emitter/mod.rs`

Add:
```rust
pub mod surface_rules;
```

Update `emit_all` to call surface rule emitter.

### Success Criteria

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] Generated code compiles without errors
- [ ] Generated `build_overworld_surface_rule` function exists

#### Manual Verification:
- [ ] Inspect generated code for correctness
- [ ] Verify biome list matching looks correct

---

## Phase 3: Integration and Cleanup

### Overview

Wire up the generated surface rules into the terrain generator, remove hand-coded rules, and verify everything works.

### Changes Required

#### 1. Update build.rs

**File**: `crates/unastar_noise/build.rs`

Already parses `noise_settings`, just need to call the surface rule emitter with the parsed `surface_rule` field.

#### 2. Export Generated Surface Rules

**File**: `crates/unastar_noise/src/lib.rs`

Add:
```rust
// Re-export generated surface rules
pub use generated::surface_rules::build_overworld_surface_rule;
```

#### 3. Update Terrain Generator

**File**: `crates/unastar/src/world/generator/terrain.rs`

Change from:
```rust
use crate::world::generator::surface::build_overworld_surface_rule;
```

To:
```rust
use unastar_noise::build_overworld_surface_rule;
```

#### 4. Remove Hand-Coded Rules

**File**: `crates/unastar/src/world/generator/surface/overworld.rs`

Delete this file entirely - it's replaced by generated code.

**File**: `crates/unastar/src/world/generator/surface/mod.rs`

Remove:
```rust
mod overworld;
pub use overworld::build_overworld_surface_rule;
```

#### 5. Add Missing Blocks

Review generated code and add any missing block constants to `crates/unastar/src/world/chunk/blocks.rs`:
- `COARSE_DIRT`
- `MUD`
- `MYCELIUM`
- `PODZOL`
- `POWDER_SNOW`
- `PACKED_ICE`
- `ICE`
- `CALCITE`
- `RED_SAND`
- `RED_SANDSTONE`
- All terracotta variants (orange, white, yellow, brown, red, light_gray, etc.)

#### 6. Add Missing Biome Variants

Review generated code and add any missing biomes to `crates/unastar/src/world/generator/constants.rs`.

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` succeeds for entire workspace
- [ ] `cargo test -p unastar` passes
- [ ] No compiler warnings about unused code

#### Manual Verification:
- [ ] Generate world and visually inspect biome surfaces
- [ ] Badlands shows terracotta banding
- [ ] Desert shows sand/sandstone
- [ ] Frozen peaks shows ice/snow
- [ ] Mangrove swamp shows mud
- [ ] Compare screenshots to vanilla Minecraft

---

## Testing Strategy

### Unit Tests

1. **Parser Tests** (`codegen/parser/surface_rule.rs`):
   - Parse minimal surface rule JSON
   - Parse all condition types individually
   - Parse nested sequence/condition structures
   - Verify biome list parsing

2. **Emitter Tests** (`codegen/emitter/surface_rules.rs`):
   - Generate code for simple block rule
   - Generate code for condition with biome check
   - Generate code for sequence of rules

### Integration Tests

1. **Build Test**: Ensure `cargo build` succeeds with generated code
2. **Rule Application Test**: Create test chunks and verify surface blocks match expected biome behavior

### Manual Testing Steps

1. Generate a world with seed 12345
2. Teleport to known biome locations:
   - Desert (x=1000, z=0)
   - Badlands (x=2000, z=0)
   - Frozen Peaks (x=0, z=1000)
   - Mangrove Swamp (x=-1000, z=0)
3. Verify surface blocks match vanilla behavior
4. Take screenshots for comparison

## Performance Considerations

- Generated code creates the rule tree once at world initialization
- No runtime JSON parsing overhead
- Rule evaluation is identical to current hand-coded approach
- No additional memory overhead beyond the rule tree itself

## Migration Notes

This is a drop-in replacement - the `build_overworld_surface_rule(seed: i64) -> Box<dyn Rule>` signature is preserved. The only change is the source of the rules (generated vs hand-coded).

## References

- Vanilla surface rules JSON: `crates/unastar_noise/worldgen_data/noise_settings/overworld.json`
- Java SurfaceRules implementation: `java-ed-world/level/levelgen/SurfaceRules.java`
- Java SurfaceSystem implementation: `java-ed-world/level/levelgen/SurfaceSystem.java`
- Existing codegen pattern: `crates/unastar_noise/codegen/`
- Existing surface system: `crates/unastar/src/world/generator/surface/`
