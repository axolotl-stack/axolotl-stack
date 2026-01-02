# unastar_noise Crate with build.rs Code Generation

## Overview

Convert `unastar_worldgen_gen` from an xtask-style manual-run generator into a proper `unastar_noise` crate that uses `build.rs` for code generation. The generated code will go into `OUT_DIR` instead of being checked into the source tree, which:
- Keeps the codebase clean
- Allows incremental regeneration when JSON changes
- Separates compilation of generated code from the main crate
- Enables parallel compilation with other crates

## Current State Analysis

### What Exists
- **`unastar_worldgen_gen`**: A binary crate that manually runs to generate code
  - Parses JSON from `worldgen_data/` directory (3.9MB of worldgen JSON)
  - Generates ~16,500 lines of Rust into `unastar/src/world/generator/density/generated/`
  - Must be manually run with `cargo run -p unastar_worldgen_gen`
  - Output is checked into version control

### Problems
1. **Manual regeneration**: Easy to forget to regenerate after JSON changes
2. **Polluted source tree**: 1.3MB of generated code in the repo
3. **Compilation coupling**: Generated code compiles as part of unastar, blocking other compilation
4. **No dependency tracking**: Cargo doesn't know when to rebuild

### Generated Files Overview
| File | Lines | Description |
|------|-------|-------------|
| `noise_params.rs` | 209 | NoiseRef enum + NOISE_PARAMS static |
| `overworld.rs` | 6,433 | Arena-based density function builder |
| `overworld_compiled.rs` | 9,937 | AOT-compiled flat density functions |
| `mod.rs` | 11 | Re-exports |

## Desired End State

1. **New crate**: `unastar_noise` that exposes generated worldgen types
2. **build.rs**: Generates code at build time into `OUT_DIR`
3. **Dependency tracking**: Rebuilds automatically when JSON files change
4. **Clean separation**: unastar depends on unastar_noise for worldgen
5. **Parallel compilation**: unastar_noise can compile independently

### Verification
- `cargo build -p unastar_noise` generates and compiles the code
- `cargo build -p unastar` works unchanged (just depends on unastar_noise)
- JSON changes trigger automatic rebuild
- Generated files not in version control

## What We're NOT Doing

- Not changing the generation logic itself (reuse existing parser/emitter)
- Not moving the JSON files (keep in worldgen_data/)
- Not restructuring the generated code format
- Not touching the handwritten types in `unastar/src/world/generator/density/types.rs`

## Implementation Approach

**Strategy**: Create a new crate that:
1. Contains the parser/emitter code from unastar_worldgen_gen as a library
2. Has a build.rs that uses the library to generate code
3. Exposes generated types via `include!()` from OUT_DIR
4. unastar depends on this crate instead of having generated code inline

---

## Phase 1: Create unastar_noise Crate Structure

### Overview
Set up the new crate with the existing generator code reorganized as a library.

### Changes Required:

#### 1. Create crate directory
```
crates/unastar_noise/
├── Cargo.toml
├── build.rs
├── src/
│   ├── lib.rs              # Public exports
│   └── types.rs            # Core types (moved from unastar)
├── gen/                    # Generator library
│   ├── mod.rs
│   ├── parser/             # Moved from unastar_worldgen_gen
│   ├── emitter/            # Moved from unastar_worldgen_gen
│   └── analyzer/           # Moved from unastar_worldgen_gen
└── worldgen_data/          # Moved from unastar_worldgen_gen
```

#### 2. Cargo.toml
**File**: `crates/unastar_noise/Cargo.toml`

```toml
[package]
name = "unastar_noise"
version = "0.1.0"
edition.workspace = true
build = "build.rs"

[dependencies]
# Runtime dependencies (used by generated code)
# None initially - types are self-contained

[build-dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
walkdir = "2"
```

#### 3. build.rs
**File**: `crates/unastar_noise/build.rs`

```rust
//! Build script that generates density function code from worldgen JSON.

mod gen;

use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    let json_root = PathBuf::from(&manifest_dir).join("worldgen_data");
    let output_dir = PathBuf::from(&out_dir);

    // Emit rerun-if-changed for all JSON files
    println!("cargo:rerun-if-changed=worldgen_data");
    for entry in walkdir::WalkDir::new(&json_root) {
        if let Ok(e) = entry {
            if e.path().extension().map_or(false, |ext| ext == "json") {
                println!("cargo:rerun-if-changed={}", e.path().display());
            }
        }
    }

    // Parse all JSON
    let noises = gen::parser::noise::parse_all(&json_root.join("noise"))
        .expect("Failed to parse noise definitions");
    let density_functions = gen::parser::density_function::parse_all(&json_root.join("density_function"))
        .expect("Failed to parse density functions");
    let noise_settings = gen::parser::noise_settings::parse_all(&json_root.join("noise_settings"))
        .expect("Failed to parse noise settings");

    // Generate Rust code
    gen::emitter::emit_all(&output_dir, &noises, &density_functions, &noise_settings)
        .expect("Failed to emit generated code");

    println!("cargo:warning=Generated worldgen code in {:?}", output_dir);
}
```

### Success Criteria:

#### Automated Verification:
- [ ] Crate directory structure exists
- [ ] `cargo build -p unastar_noise` completes (even if empty lib)
- [ ] build.rs runs without error

---

## Phase 2: Move Generator Code

### Overview
Move parser/emitter/analyzer from unastar_worldgen_gen into unastar_noise/gen/.

### Changes Required:

#### 1. Move source files
- Move `unastar_worldgen_gen/src/parser/` → `unastar_noise/gen/parser/`
- Move `unastar_worldgen_gen/src/emitter/` → `unastar_noise/gen/emitter/`
- Move `unastar_worldgen_gen/src/analyzer/` → `unastar_noise/gen/analyzer/`
- Move `unastar_worldgen_gen/worldgen_data/` → `unastar_noise/worldgen_data/`

#### 2. Update module paths
**File**: `crates/unastar_noise/gen/mod.rs`

```rust
pub mod analyzer;
pub mod emitter;
pub mod parser;
```

#### 3. Fix imports in moved files
Update `crate::` references to work from the gen/ directory context.

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] build.rs generates files in OUT_DIR
- [ ] Generated files match previous output (diff check)

---

## Phase 3: Create Public Library Interface

### Overview
Create the public API that unastar will depend on.

### Changes Required:

#### 1. src/lib.rs
**File**: `crates/unastar_noise/src/lib.rs`

```rust
//! Worldgen noise and density function library.
//!
//! This crate provides generated density functions and noise parameters
//! for Minecraft worldgen. Code is generated at build time from JSON files.

// Include generated code from OUT_DIR
include!(concat!(env!("OUT_DIR"), "/mod.rs"));

// Re-export core types
mod types;
pub use types::*;
```

#### 2. Move core types from unastar
Move from `unastar/src/world/generator/density/types.rs`:
- `FunctionContext`, `FunctionContext4`
- `NoiseRegistry`
- `DensityArena`, `DensityIdx`, `DensityFunction` (if used)
- `Spline`, `SplineIdx`, `SplinePoint`, `SplineValue`
- `RarityType`

These types are needed by the generated code but should live with it.

#### 3. Update generated code output
Modify emitter to generate code compatible with `include!()`:
- Proper module structure in OUT_DIR
- No `use super::` - use `crate::` instead

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build -p unastar_noise` succeeds
- [ ] Public types are accessible
- [ ] Generated code compiles with include!()

---

## Phase 4: Update unastar to Use unastar_noise

### Overview
Replace inline generated code with dependency on unastar_noise.

### Changes Required:

#### 1. Add dependency
**File**: `crates/unastar/Cargo.toml`

Add:
```toml
unastar_noise.workspace = true
```

And in workspace Cargo.toml:
```toml
unastar_noise = { path = "crates/unastar_noise" }
```

#### 2. Update density module
**File**: `crates/unastar/src/world/generator/density/mod.rs`

Replace:
```rust
mod generated;
pub use generated::*;
```

With:
```rust
// Re-export from unastar_noise
pub use unastar_noise::{
    NoiseParamsData, NoiseRef, NOISE_PARAMS,
    build_overworld_router,
    FlatCacheGrid,
    compute_barrier, compute_continents, compute_depth, compute_erosion,
    compute_final_density, compute_final_density_4,
    compute_fluid_level_floodedness, compute_fluid_level_spread,
    compute_initial_density_without_jaggedness, compute_lava,
    compute_ridges, compute_temperature, compute_vegetation,
    compute_vein_gap, compute_vein_ridged, compute_vein_toggle,
};
```

#### 3. Delete generated directory
Remove `crates/unastar/src/world/generator/density/generated/`

#### 4. Move/reconcile types
Either:
- Move types to unastar_noise (preferred, keeps types with generated code)
- Or keep in unastar and have unastar_noise depend on unastar (creates cycle, avoid)

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build -p unastar` succeeds
- [ ] `cargo test -p unastar` passes
- [ ] No generated code in unastar source tree

---

## Phase 5: Clean Up unastar_worldgen_gen

### Overview
Remove the now-obsolete generator binary.

### Changes Required:

#### 1. Remove from workspace
**File**: `Cargo.toml`

Remove `crates/unastar_worldgen_gen` from members list.

#### 2. Delete crate directory
Remove `crates/unastar_worldgen_gen/` entirely.

### Success Criteria:

#### Automated Verification:
- [ ] Workspace builds successfully
- [ ] No references to unastar_worldgen_gen remain

---

## Phase 6: Compilation Speed Improvements

### Overview
Investigate and implement compilation speed improvements.

### Changes Required:

#### 1. Analyze current build times
```bash
cargo build -p unastar --timings
```

Look for:
- Time spent in unastar_noise build.rs
- Time spent compiling generated code
- Parallel compilation opportunities

#### 2. Potential optimizations

**a. Split generated code into multiple files**
Instead of one 10k line file, generate multiple smaller modules:
- `overworld_terrain.rs` (~3k lines)
- `overworld_caves.rs` (~3k lines)
- `overworld_veins.rs` (~3k lines)

This enables parallel compilation within the crate.

**b. Reduce codegen-units for generated code**
In Cargo.toml:
```toml
[profile.dev]
codegen-units = 16  # Default, good for parallelism

[profile.dev.package.unastar_noise]
codegen-units = 1  # Better optimization for hot code
```

**c. Consider splitting SIMD variants**
The `_4` SIMD variants add ~3k lines. Could be:
- Feature-gated: `features = ["simd"]`
- Separate module: only compiled when used

**d. Precompile spline data**
Currently splines are inline code. Could be:
```rust
static OFFSET_SPLINE: &[SplineSegment] = &[
    SplineSegment::new(-1.1, -0.85, 0.044, 0.0, ...),
    // ...
];
```
This reduces parsing work and code size.

#### 3. Measure impact
Before/after comparison of:
- Clean build time
- Incremental build time
- Binary size

### Success Criteria:

#### Automated Verification:
- [ ] Build completes successfully
- [ ] No performance regressions

#### Manual Verification:
- [ ] Document build time improvements
- [ ] Verify generated code still produces correct terrain

---

## Testing Strategy

### Unit Tests:
- Test generated NoiseRef enum has all variants
- Test noise params have correct values
- Test FlatCacheGrid initialization

### Integration Tests:
- Test density function computation matches known values
- Test SIMD and scalar paths produce same results

### Manual Testing:
1. Clean build: `cargo build -p unastar`
2. Modify worldgen JSON, verify rebuild triggers
3. Run world generation, verify terrain unchanged

## Performance Considerations

**Build-time:**
- build.rs runs once per clean build
- JSON parsing is fast (<100ms for 3.9MB)
- Code emission is fast (<50ms)
- Compilation of generated code is the bottleneck (~10s for 16k lines)

**Runtime:**
- No performance change expected (same generated code)
- Potential for optimization via module splitting

## Migration Path

1. Create unastar_noise crate alongside existing setup
2. Build both in parallel, verify output matches
3. Switch unastar to depend on unastar_noise
4. Delete old generated code and unastar_worldgen_gen
5. Update CI/documentation

## References

- Current generator: `crates/unastar_worldgen_gen/`
- Generated output: `crates/unastar/src/world/generator/density/generated/`
- Worldgen JSON: `crates/unastar_worldgen_gen/worldgen_data/`
- Previous plan: `thoughts/shared/plans/2025-12-29-worldgen-json-build-rs-codegen.md`
