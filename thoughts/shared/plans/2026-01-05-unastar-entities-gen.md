# unastar_data: Unified Game Data Pipeline

## Overview

Create a unified data pipeline (`unastar_data/`) that transforms raw Minecraft Bedrock data sources into clean, typed KDL artifacts, then generates Rust ECS code. This separates **data merging** (datagen) from **code generation** (codegen), with KDL as the human-readable intermediate format.

```
┌─────────────────────────────────────────────────────────────────────┐
│                         unastar_data/                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  data/                      datagen/              output/           │
│  ├── vanilla_bp/            ├── main.rs           └── entities.kdl  │
│  │   └── entities/*.json    ├── merge.rs              (future:      │
│  ├── minecraft-data/        ├── defaults.rs            blocks.kdl   │
│  │   └── (protocol)         └── normalize.rs           items.kdl)   │
│  └── overrides/                    │                       │        │
│      ├── _defaults.kdl             │                       │        │
│      └── entities/                 ▼                       ▼        │
│          └── zombie.kdl      ┌──────────┐           ┌──────────┐    │
│                              │ datagen  │──────────▶│  output  │    │
│                              │ (binary) │           │  (KDL)   │    │
│                              └──────────┘           └──────────┘    │
│                                                           │         │
│  codegen/                    src/                         │         │
│  ├── main.rs                 └── lib.rs ◀─────────────────┘         │
│  └── entities.rs                 (generated Rust)                   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Current State Analysis

### Existing Infrastructure

**valentine_gen** (`crates/valentine_gen/`):
- Mature xtask parsing `minecraft-data` JSON
- Generates protocol types, items, blocks, entities (basic metadata only)
- Uses fingerprinting for type deduplication
- Outputs to `crates/valentine/bedrock_versions/`

**unastar ECS** (`crates/unastar/src/entity/`):
- Uses `bevy_ecs` 0.17
- Handwritten components: `Health`, `CollisionBox`, `Transform`, etc.
- Systems for physics, lifecycle, effects
- Entity registry loading valentine's `EntityData`

### Critical Insight: Bedrock Data is a Patch System

**The behavior pack JSONs are NOT complete entity definitions.** They are **diffs/patches** on top of hardcoded C++ defaults in the Bedrock engine. If a zombie JSON doesn't list `minecraft:physics`, it doesn't mean it lacks physics—it means it uses the C++ hardcoded default.

This means:
1. We cannot derive complete entities from JSON alone
2. We must layer: `_defaults.kdl` → `vanilla JSON` → `entity overrides`
3. Community can contribute missing data via PRable KDL overrides
4. The output KDL becomes the **single source of truth**

### Data Inconsistencies in Vanilla JSON

Analysis of 122 entity files revealed:

**Type Polymorphism**:
- `fuse_length`: `1.5` (fixed) vs `{range_min, range_max}` (random range)
- `damage`: `3` (fixed) vs `[1, 5]` (array range)
- `deals_damage`: `false` (bool) vs `"no"` (string)
- `seats`: `{single object}` vs `[{array}]`

**Molang Expressions in Numeric Fields**:
```json
"rotate_rider_by": "query.has_any_family(...) ? -90 : 0"
"success_chance": "query.is_baby ? 0.02 : 0.001"
```

## Desired End State

After implementation:

```
crates/unastar_data/
├── Cargo.toml                # Workspace crate with datagen/codegen binaries
├── data/
│   ├── vanilla_bp/           # Git submodule: bedrock-samples
│   │   └── entities/*.json   # 122 entity behavior files
│   └── overrides/            # COMMITTED: Community-maintained fixes
│       ├── _defaults.kdl     # Universal entity defaults
│       └── entities/
│           ├── zombie.kdl    # "zombie has attack damage 3"
│           └── player.kdl
├── datagen/
│   └── src/
│       ├── main.rs           # CLI: cargo run -p unastar_data_gen
│       ├── ingest/           # Parse vanilla JSON
│       ├── merge.rs          # Apply overrides
│       ├── normalize.rs      # Flatten polymorphic types
│       ├── validate.rs       # Assert completeness
│       └── emit.rs           # Write KDL
├── output/                   # COMMITTED: The golden artifact
│   └── entities.kdl          # Clean, complete, typed
├── codegen/
│   └── src/
│       ├── main.rs           # CLI: cargo run -p unastar_data_codegen
│       └── entities.rs       # Generate Rust from KDL
└── src/                      # COMMITTED: Generated Rust library
    ├── lib.rs
    ├── types.rs              # RangeOrVal, MolangOr, etc.
    └── entities/
        ├── mod.rs
        ├── components/       # Component DTOs
        └── definitions/      # Entity definitions
```

### Verification

- [ ] `cargo run -p unastar_data_gen` completes without errors
- [ ] `output/entities.kdl` is valid KDL and human-readable
- [ ] `cargo run -p unastar_data_codegen` generates valid Rust
- [ ] `cargo check -p unastar_data` passes
- [ ] Generated code integrates with unastar ECS

## What We're NOT Doing

1. **Protocol migration** - valentine_gen continues handling protocol (future work)
2. **Blocks/Items** - Focus on entities first, extend architecture later
3. **Runtime behavior pack loading** - No dynamic JSON parsing at runtime
4. **Full behavior AI** - Just data structures, not AI systems
5. **Event execution** - Events are enums, not an event loop
6. **Animation/rendering** - Server-side only

## Implementation Approach

### KDL Library Choice

After research, using **`kdl` v6.x** (reference implementation) for both datagen and codegen:
- Format preservation for readable output
- Bidirectional (read and write)
- Actively maintained (435 GitHub stars, v6.5.0)
- Apache-2.0 licensed

Alternative considered: `knuffel` for typed derive macros (read-only, no serialization).

### Merge Strategy

```
Layer 1: _defaults.kdl      → Universal defaults (physics, collision)
Layer 2: vanilla JSON       → Parsed and normalized to KDL structure
Layer 3: entity overrides   → Community fixes (zombie.kdl, player.kdl)
         ─────────────────
Result:  output/entities.kdl → Complete, validated, typed
```

---

## Phase 1: Crate Scaffolding

### Overview
Create the `unastar_data` workspace crate with datagen and codegen binaries.

### Changes Required

#### 1. Create Directory Structure

```
crates/unastar_data/
├── Cargo.toml
├── data/
│   └── overrides/
│       ├── _defaults.kdl
│       └── entities/
│           └── .gitkeep
├── datagen/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── codegen/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── output/
│   └── .gitkeep
└── src/
    └── lib.rs
```

#### 2. Root Cargo.toml

**File**: `crates/unastar_data/Cargo.toml`

```toml
[package]
name = "unastar_data"
version = "0.1.0"
edition = "2024"
description = "Generated game data for Minecraft Bedrock"

[lib]
path = "src/lib.rs"

[dependencies]
serde = { workspace = true }
bevy_ecs = { workspace = true }
rand = { workspace = true }

[workspace]
members = ["datagen", "codegen"]
```

#### 3. Datagen Cargo.toml

**File**: `crates/unastar_data/datagen/Cargo.toml`

```toml
[package]
name = "unastar_data_gen"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "unastar_data_gen"
path = "src/main.rs"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
kdl = "6.5"
walkdir = "2.5"
clap = { version = "4.5", features = ["derive"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
heck = "0.5"
miette = { version = "7.0", features = ["fancy"] }
thiserror = { workspace = true }
```

#### 4. Codegen Cargo.toml

**File**: `crates/unastar_data/codegen/Cargo.toml`

```toml
[package]
name = "unastar_data_codegen"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "unastar_data_codegen"
path = "src/main.rs"

[dependencies]
kdl = "6.5"
quote = "1.0"
proc-macro2 = "1.0"
syn = "2.0"
prettyplease = "0.2"
clap = { version = "4.5", features = ["derive"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
heck = "0.5"
miette = { version = "7.0", features = ["fancy"] }
thiserror = { workspace = true }
```

#### 5. Datagen Main Entry

**File**: `crates/unastar_data/datagen/src/main.rs`

```rust
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod ingest;
mod merge;
mod normalize;
mod validate;
mod emit;
mod ir;

#[derive(Parser, Debug)]
#[command(name = "unastar_data_gen")]
#[command(about = "Merge Bedrock data sources into clean KDL artifacts")]
struct Args {
    /// Path to vanilla behavior pack entities directory
    #[arg(long, default_value = "../data/vanilla_bp/entities")]
    vanilla: PathBuf,

    /// Path to overrides directory
    #[arg(long, default_value = "../data/overrides")]
    overrides: PathBuf,

    /// Output directory for KDL artifacts
    #[arg(short, long, default_value = "../output")]
    output: PathBuf,

    /// Only process specific entities (comma-separated)
    #[arg(long)]
    entities: Option<String>,

    /// List available entities and exit
    #[arg(long)]
    list: bool,

    /// Tracing filter
    #[arg(long, default_value = "info")]
    log: String,
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&args.log)
        .init();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    info!("unastar_data_gen starting...");
    info!("Vanilla BP: {}", manifest_dir.join(&args.vanilla).display());
    info!("Overrides: {}", manifest_dir.join(&args.overrides).display());
    info!("Output: {}", manifest_dir.join(&args.output).display());

    // TODO: Implement pipeline
    // 1. ingest::parse_vanilla_entities()
    // 2. ingest::parse_overrides()
    // 3. merge::merge_layers()
    // 4. normalize::normalize_types()
    // 5. validate::validate_completeness()
    // 6. emit::write_kdl()

    info!("Done!");
    Ok(())
}
```

#### 6. Codegen Main Entry

**File**: `crates/unastar_data/codegen/src/main.rs`

```rust
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod entities;
mod utils;

#[derive(Parser, Debug)]
#[command(name = "unastar_data_codegen")]
#[command(about = "Generate Rust code from KDL artifacts")]
struct Args {
    /// Path to KDL output directory
    #[arg(short, long, default_value = "../output")]
    input: PathBuf,

    /// Output directory for generated Rust code
    #[arg(short, long, default_value = "../src")]
    output: PathBuf,

    /// Tracing filter
    #[arg(long, default_value = "info")]
    log: String,
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&args.log)
        .init();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    info!("unastar_data_codegen starting...");
    info!("Input: {}", manifest_dir.join(&args.input).display());
    info!("Output: {}", manifest_dir.join(&args.output).display());

    // TODO: Implement pipeline
    // 1. Parse output/entities.kdl
    // 2. Generate src/entities/**/*.rs

    info!("Done!");
    Ok(())
}
```

#### 7. Library Root

**File**: `crates/unastar_data/src/lib.rs`

```rust
//! Generated game data for Minecraft Bedrock.
//!
//! This crate provides strongly-typed entity definitions, components,
//! and game data generated from Bedrock behavior packs.
//!
//! ## Architecture
//!
//! Data flows through two stages:
//!
//! 1. **datagen**: Merges vanilla JSON + community overrides → `output/*.kdl`
//! 2. **codegen**: Generates Rust code from KDL → `src/**/*.rs`
//!
//! The KDL output is the "golden artifact" - human-readable, versionable,
//! and consumable by non-Rust tooling.

pub mod types;
pub mod entities;

pub use types::*;
```

#### 8. Core Types

**File**: `crates/unastar_data/src/types.rs`

```rust
//! Core polymorphic types for handling Bedrock's inconsistent JSON.

use serde::{Deserialize, Serialize};

/// Handles fields that can be a single value OR a min/max range.
/// Examples: `fuse_length`, `damage`, `cooldown`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RangeOrVal<T> {
    Range {
        #[serde(alias = "range_min")]
        min: T,
        #[serde(alias = "range_max")]
        max: T,
    },
    Fixed(T),
}

impl<T: Copy> RangeOrVal<T> {
    pub fn fixed(&self) -> Option<T> {
        match self {
            Self::Fixed(val) => Some(*val),
            Self::Range { .. } => None,
        }
    }

    pub fn range(&self) -> Option<(T, T)> {
        match self {
            Self::Range { min, max } => Some((*min, *max)),
            Self::Fixed(_) => None,
        }
    }
}

impl<T: Copy + PartialOrd + rand::distributions::uniform::SampleUniform> RangeOrVal<T> {
    pub fn sample(&self, rng: &mut impl rand::Rng) -> T {
        match self {
            Self::Fixed(val) => *val,
            Self::Range { min, max } => rng.gen_range(*min..=*max),
        }
    }
}

impl<T: Default> Default for RangeOrVal<T> {
    fn default() -> Self {
        Self::Fixed(T::default())
    }
}

/// Handles fields that can be a literal value OR a Molang expression.
/// Examples: `rotate_rider_by`, `success_chance`, `priority`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MolangOr<T> {
    Value(T),
    Expr(String),
}

impl<T> MolangOr<T> {
    pub fn as_value(&self) -> Option<&T> {
        match self {
            Self::Value(v) => Some(v),
            Self::Expr(_) => None,
        }
    }

    pub fn as_expr(&self) -> Option<&str> {
        match self {
            Self::Expr(s) => Some(s),
            Self::Value(_) => None,
        }
    }
}

impl<T: Default> Default for MolangOr<T> {
    fn default() -> Self {
        Self::Value(T::default())
    }
}

/// Handles boolean fields that can be `true`/`false` OR string `"yes"`/`"no"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(untagged)]
pub enum BoolOrString {
    #[default]
    Bool(bool),
    String(String),
}

impl BoolOrString {
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::String(s) => {
                s.eq_ignore_ascii_case("yes")
                    || s.eq_ignore_ascii_case("true")
                    || s == "1"
            }
        }
    }
}

impl From<bool> for BoolOrString {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

/// Deserializer helper for fields that can be a single object or array.
pub mod one_or_many {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany<T> {
            One(T),
            Many(Vec<T>),
        }

        match OneOrMany::deserialize(deserializer)? {
            OneOrMany::One(val) => Ok(vec![val]),
            OneOrMany::Many(vec) => Ok(vec),
        }
    }

    pub fn serialize<S, T>(value: &Vec<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        value.serialize(serializer)
    }
}
```

#### 9. Placeholder Entities Module

**File**: `crates/unastar_data/src/entities/mod.rs`

```rust
//! Generated entity definitions from Bedrock behavior packs.
//!
//! This module is auto-generated by `unastar_data_codegen`.
//! Do not edit manually.

pub mod components;
pub mod definitions;

pub use components::*;
```

#### 10. Placeholder Components Module

**File**: `crates/unastar_data/src/entities/components/mod.rs`

```rust
//! Generated entity components.
//!
//! These are DTOs (Data Transfer Objects) for parsing behavior pack JSON.
//! They use `Option<T>` for all fields since vanilla JSON is a patch system.

// Components will be generated here
```

#### 11. Placeholder Definitions Module

**File**: `crates/unastar_data/src/entities/definitions/mod.rs`

```rust
//! Generated entity definitions.
//!
//! Each entity has a definition struct with component groups and events.

// Entity definitions will be generated here
```

#### 12. Default Overrides

**File**: `crates/unastar_data/data/overrides/_defaults.kdl`

```kdl
// Default values for ALL entities
// These fill gaps where vanilla JSON doesn't specify values
// (because Bedrock assumes C++ hardcoded defaults)

defaults type="entity" {
    physics {
        has_gravity true
        has_collision true
        gravity 0.08
        drag 0.02
    }
    pushable {
        is_pushable true
        is_pushable_by_piston true
    }
    collision_box {
        // Most mobs if not specified
        width 0.6
        height 1.8
    }
}
```

### Success Criteria

#### Automated Verification:
- [ ] Crate compiles: `cargo check -p unastar_data`
- [ ] Datagen CLI runs: `cargo run -p unastar_data_gen -- --help`
- [ ] Codegen CLI runs: `cargo run -p unastar_data_codegen -- --help`

#### Manual Verification:
- [ ] Directory structure matches specification
- [ ] Core types compile and have correct serde behavior

---

## Phase 2: Datagen - Vanilla JSON Ingestion

### Overview
Parse Bedrock behavior pack JSON files into an intermediate representation.

### Changes Required

#### 1. Intermediate Representation

**File**: `crates/unastar_data/datagen/src/ir.rs`

```rust
//! Intermediate representation for entity data.
//!
//! This IR is format-agnostic - it represents the merged, normalized data
//! before emission to KDL.

use std::collections::HashMap;

/// Complete entity definition after merging all layers
#[derive(Debug, Clone)]
pub struct EntityDef {
    pub identifier: String,
    pub spawn_category: Option<String>,
    pub is_spawnable: bool,
    pub is_summonable: bool,
    pub runtime_id: Option<u32>,
    pub properties: HashMap<String, PropertyDef>,
    pub components: HashMap<String, ComponentValue>,
    pub component_groups: HashMap<String, HashMap<String, ComponentValue>>,
    pub events: HashMap<String, EventDef>,
    /// Track where each field came from for debugging
    pub attribution: Attribution,
}

#[derive(Debug, Clone, Default)]
pub struct Attribution {
    /// Which source contributed each component
    pub component_sources: HashMap<String, Source>,
}

#[derive(Debug, Clone)]
pub enum Source {
    Defaults,
    Vanilla,
    Override(String), // filename
}

/// Entity property (synced enum or value)
#[derive(Debug, Clone)]
pub struct PropertyDef {
    pub prop_type: PropertyType,
    pub default: String,
    pub client_sync: bool,
}

#[derive(Debug, Clone)]
pub enum PropertyType {
    Enum { values: Vec<String> },
    Int { range: (i32, i32) },
    Float { range: (f32, f32) },
    Bool,
}

/// Component value from JSON
#[derive(Debug, Clone)]
pub enum ComponentValue {
    /// Empty component like `"minecraft:physics": {}`
    Marker,
    /// Structured component data
    Data(serde_json::Value),
}

/// Event definition
#[derive(Debug, Clone)]
pub struct EventDef {
    pub add_groups: Vec<String>,
    pub remove_groups: Vec<String>,
    pub set_properties: HashMap<String, serde_json::Value>,
    pub trigger: Option<String>,
    pub sequence: Vec<EventDef>,
    pub randomize: Vec<RandomizeEntry>,
    pub filters: Option<serde_json::Value>,
}

impl Default for EventDef {
    fn default() -> Self {
        Self {
            add_groups: Vec::new(),
            remove_groups: Vec::new(),
            set_properties: HashMap::new(),
            trigger: None,
            sequence: Vec::new(),
            randomize: Vec::new(),
            filters: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RandomizeEntry {
    pub weight: i32,
    pub trigger: Option<String>,
    pub add_groups: Vec<String>,
    pub remove_groups: Vec<String>,
}
```

#### 2. JSON Ingestion Module

**File**: `crates/unastar_data/datagen/src/ingest/mod.rs`

```rust
//! Parse vanilla behavior pack JSON and override KDL files.

mod vanilla;
mod overrides;

pub use vanilla::parse_vanilla_entities;
pub use overrides::parse_overrides;
```

#### 3. Vanilla JSON Parser

**File**: `crates/unastar_data/datagen/src/ingest/vanilla.rs`

```rust
use crate::ir::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};
use walkdir::WalkDir;

/// Raw JSON structure matching behavior pack format
#[derive(Deserialize)]
struct RawEntityFile {
    format_version: Option<String>,
    #[serde(rename = "minecraft:entity")]
    entity: RawEntity,
}

#[derive(Deserialize)]
struct RawEntity {
    description: RawDescription,
    #[serde(default)]
    components: HashMap<String, serde_json::Value>,
    #[serde(default)]
    component_groups: HashMap<String, HashMap<String, serde_json::Value>>,
    #[serde(default)]
    events: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct RawDescription {
    identifier: String,
    #[serde(default)]
    spawn_category: Option<String>,
    #[serde(default)]
    is_spawnable: bool,
    #[serde(default)]
    is_summonable: bool,
    #[serde(default)]
    runtime_identifier: Option<String>,
    #[serde(default)]
    properties: HashMap<String, serde_json::Value>,
}

pub fn parse_vanilla_entities(
    dir: &Path,
) -> miette::Result<HashMap<String, EntityDef>> {
    let mut entities = HashMap::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "json")
        })
    {
        let path = entry.path();
        debug!("Parsing vanilla: {}", path.display());

        match parse_entity_file(path) {
            Ok(entity) => {
                let name = entity
                    .identifier
                    .strip_prefix("minecraft:")
                    .unwrap_or(&entity.identifier)
                    .to_string();
                entities.insert(name, entity);
            }
            Err(e) => {
                warn!("Failed to parse {}: {}", path.display(), e);
            }
        }
    }

    Ok(entities)
}

fn parse_entity_file(path: &Path) -> miette::Result<EntityDef> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;

    // Remove JSON comments (Bedrock allows // comments)
    let content = remove_json_comments(&content);

    let raw: RawEntityFile = serde_json::from_str(&content)
        .map_err(|e| miette::miette!("Failed to parse {}: {}", path.display(), e))?;

    let mut attribution = Attribution::default();

    // Mark all components as from vanilla
    for name in raw.entity.components.keys() {
        attribution
            .component_sources
            .insert(name.clone(), Source::Vanilla);
    }

    Ok(EntityDef {
        identifier: raw.entity.description.identifier,
        spawn_category: raw.entity.description.spawn_category,
        is_spawnable: raw.entity.description.is_spawnable,
        is_summonable: raw.entity.description.is_summonable,
        runtime_id: None, // Filled in later from minecraft-data
        properties: parse_properties(raw.entity.description.properties),
        components: parse_components(raw.entity.components),
        component_groups: raw
            .entity
            .component_groups
            .into_iter()
            .map(|(k, v)| (k, parse_components(v)))
            .collect(),
        events: parse_events(raw.entity.events),
        attribution,
    })
}

fn remove_json_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_string = false;
    let mut escape_next = false;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }

        match c {
            '\\' if in_string => {
                result.push(c);
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
                result.push(c);
            }
            '/' if !in_string => {
                if chars.peek() == Some(&'/') {
                    // Skip to end of line
                    while let Some(c) = chars.next() {
                        if c == '\n' {
                            result.push('\n');
                            break;
                        }
                    }
                } else {
                    result.push(c);
                }
            }
            _ => result.push(c),
        }
    }

    result
}

fn parse_properties(
    raw: HashMap<String, serde_json::Value>,
) -> HashMap<String, PropertyDef> {
    raw.into_iter()
        .filter_map(|(name, value)| {
            parse_property(&value).map(|p| (name, p))
        })
        .collect()
}

fn parse_property(value: &serde_json::Value) -> Option<PropertyDef> {
    let obj = value.as_object()?;
    let prop_type = match obj.get("type")?.as_str()? {
        "enum" => {
            let values = obj
                .get("values")?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            PropertyType::Enum { values }
        }
        "int" => {
            let range = obj.get("range").and_then(|r| {
                let arr = r.as_array()?;
                Some((
                    arr.get(0)?.as_i64()? as i32,
                    arr.get(1)?.as_i64()? as i32,
                ))
            }).unwrap_or((0, 100));
            PropertyType::Int { range }
        }
        "float" => {
            let range = obj.get("range").and_then(|r| {
                let arr = r.as_array()?;
                Some((
                    arr.get(0)?.as_f64()? as f32,
                    arr.get(1)?.as_f64()? as f32,
                ))
            }).unwrap_or((0.0, 1.0));
            PropertyType::Float { range }
        }
        "bool" => PropertyType::Bool,
        _ => return None,
    };

    Some(PropertyDef {
        prop_type,
        default: obj
            .get("default")
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default(),
        client_sync: obj
            .get("client_sync")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

fn parse_components(
    raw: HashMap<String, serde_json::Value>,
) -> HashMap<String, ComponentValue> {
    raw.into_iter()
        .map(|(name, value)| {
            let comp = if value.as_object().map_or(false, |o| o.is_empty()) {
                ComponentValue::Marker
            } else {
                ComponentValue::Data(value)
            };
            (name, comp)
        })
        .collect()
}

fn parse_events(
    raw: HashMap<String, serde_json::Value>,
) -> HashMap<String, EventDef> {
    raw.into_iter()
        .filter_map(|(name, value)| {
            parse_event(&value).map(|e| (name, e))
        })
        .collect()
}

fn parse_event(value: &serde_json::Value) -> Option<EventDef> {
    let obj = value.as_object()?;

    Some(EventDef {
        add_groups: extract_component_groups(obj.get("add")),
        remove_groups: extract_component_groups(obj.get("remove")),
        set_properties: obj
            .get("set_property")
            .and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        trigger: obj.get("trigger").and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| {
                    v.as_object()
                        .and_then(|o| o.get("event")?.as_str().map(String::from))
                })
        }),
        sequence: obj
            .get("sequence")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(parse_event).collect())
            .unwrap_or_default(),
        randomize: obj
            .get("randomize")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|entry| {
                        let o = entry.as_object()?;
                        Some(RandomizeEntry {
                            weight: o.get("weight")?.as_i64()? as i32,
                            trigger: o
                                .get("trigger")
                                .and_then(|v| v.as_str().map(String::from)),
                            add_groups: extract_component_groups(o.get("add")),
                            remove_groups: extract_component_groups(o.get("remove")),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        filters: obj.get("filters").cloned(),
    })
}

fn extract_component_groups(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.get("component_groups"))
        .and_then(|g| g.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
```

#### 4. Override Parser (KDL)

**File**: `crates/unastar_data/datagen/src/ingest/overrides.rs`

```rust
use crate::ir::*;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};
use walkdir::WalkDir;

/// Parsed override data to apply on top of vanilla
#[derive(Debug, Default)]
pub struct Overrides {
    pub defaults: DefaultsOverride,
    pub entities: HashMap<String, EntityOverride>,
}

#[derive(Debug, Default)]
pub struct DefaultsOverride {
    pub components: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Default)]
pub struct EntityOverride {
    pub components: HashMap<String, serde_json::Value>,
    pub meta: OverrideMeta,
}

#[derive(Debug, Default)]
pub struct OverrideMeta {
    pub verified_version: Option<String>,
    pub verified_by: Option<String>,
    pub notes: Option<String>,
}

pub fn parse_overrides(dir: &Path) -> miette::Result<Overrides> {
    let mut overrides = Overrides::default();

    // Parse _defaults.kdl
    let defaults_path = dir.join("_defaults.kdl");
    if defaults_path.exists() {
        debug!("Parsing defaults: {}", defaults_path.display());
        match parse_defaults_kdl(&defaults_path) {
            Ok(defaults) => overrides.defaults = defaults,
            Err(e) => warn!("Failed to parse defaults: {}", e),
        }
    }

    // Parse entity overrides
    let entities_dir = dir.join("entities");
    if entities_dir.exists() {
        for entry in WalkDir::new(&entities_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "kdl")
            })
        {
            let path = entry.path();
            debug!("Parsing override: {}", path.display());

            match parse_entity_override_kdl(path) {
                Ok((name, entity_override)) => {
                    overrides.entities.insert(name, entity_override);
                }
                Err(e) => {
                    warn!("Failed to parse {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(overrides)
}

fn parse_defaults_kdl(path: &Path) -> miette::Result<DefaultsOverride> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| miette::miette!("Failed to read: {}", e))?;

    let doc: kdl::KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {}", e))?;

    let mut defaults = DefaultsOverride::default();

    for node in doc.nodes() {
        if node.name().value() == "defaults" {
            // Parse children as component defaults
            if let Some(children) = node.children() {
                for child in children.nodes() {
                    let comp_name = format!("minecraft:{}", child.name().value());
                    let value = kdl_node_to_json(child);
                    defaults.components.insert(comp_name, value);
                }
            }
        }
    }

    Ok(defaults)
}

fn parse_entity_override_kdl(
    path: &Path,
) -> miette::Result<(String, EntityOverride)> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| miette::miette!("Failed to read: {}", e))?;

    let doc: kdl::KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {}", e))?;

    let mut entity_override = EntityOverride::default();
    let mut entity_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    for node in doc.nodes() {
        if node.name().value() == "entity" {
            // Get entity identifier from first argument
            if let Some(arg) = node.entries().first() {
                if let Some(s) = arg.value().as_string() {
                    entity_name = s
                        .strip_prefix("minecraft:")
                        .unwrap_or(s)
                        .to_string();
                }
            }

            // Parse children
            if let Some(children) = node.children() {
                for child in children.nodes() {
                    let name = child.name().value();

                    if name == "_meta" {
                        // Parse metadata
                        if let Some(meta_children) = child.children() {
                            for meta_node in meta_children.nodes() {
                                match meta_node.name().value() {
                                    "verified_version" => {
                                        entity_override.meta.verified_version =
                                            meta_node
                                                .entries()
                                                .first()
                                                .and_then(|e| {
                                                    e.value().as_string().map(String::from)
                                                });
                                    }
                                    "verified_by" => {
                                        entity_override.meta.verified_by =
                                            meta_node
                                                .entries()
                                                .first()
                                                .and_then(|e| {
                                                    e.value().as_string().map(String::from)
                                                });
                                    }
                                    "notes" => {
                                        entity_override.meta.notes =
                                            meta_node
                                                .entries()
                                                .first()
                                                .and_then(|e| {
                                                    e.value().as_string().map(String::from)
                                                });
                                    }
                                    _ => {}
                                }
                            }
                        }
                    } else {
                        // Component override
                        let comp_name = if name.starts_with("minecraft:") {
                            name.to_string()
                        } else {
                            format!("minecraft:{}", name)
                        };
                        let value = kdl_node_to_json(child);
                        entity_override.components.insert(comp_name, value);
                    }
                }
            }
        }
    }

    Ok((entity_name, entity_override))
}

/// Convert a KDL node to a JSON value
fn kdl_node_to_json(node: &kdl::KdlNode) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    // Add properties from entries
    for entry in node.entries() {
        if let Some(name) = entry.name() {
            obj.insert(name.value().to_string(), kdl_value_to_json(entry.value()));
        }
    }

    // If node has children, recurse
    if let Some(children) = node.children() {
        for child in children.nodes() {
            let child_value = kdl_node_to_json(child);
            obj.insert(child.name().value().to_string(), child_value);
        }
    }

    // If no properties and no children, return the first argument as value
    if obj.is_empty() {
        if let Some(arg) = node.entries().first() {
            if arg.name().is_none() {
                return kdl_value_to_json(arg.value());
            }
        }
        return serde_json::Value::Object(serde_json::Map::new());
    }

    serde_json::Value::Object(obj)
}

fn kdl_value_to_json(value: &kdl::KdlValue) -> serde_json::Value {
    match value {
        kdl::KdlValue::String(s) => serde_json::Value::String(s.clone()),
        kdl::KdlValue::Base10(n) => {
            serde_json::Value::Number(serde_json::Number::from(*n))
        }
        kdl::KdlValue::Base10Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        kdl::KdlValue::Bool(b) => serde_json::Value::Bool(*b),
        kdl::KdlValue::Null => serde_json::Value::Null,
        _ => serde_json::Value::Null,
    }
}
```

### Success Criteria

#### Automated Verification:
- [ ] Datagen compiles: `cargo check -p unastar_data_gen`
- [ ] Can list entities: `cargo run -p unastar_data_gen -- --list`
- [ ] Parses sample entity JSON without error

#### Manual Verification:
- [ ] JSON comment stripping works correctly
- [ ] Properties, components, events are parsed
- [ ] Override KDL is parsed and converted to JSON values

**Implementation Note**: User needs to set up the vanilla_bp submodule before testing.

---

## Phase 3: Datagen - Merge Layer

### Overview
Merge defaults, vanilla JSON, and overrides into unified entity definitions.

### Changes Required

#### 1. Merge Module

**File**: `crates/unastar_data/datagen/src/merge.rs`

```rust
use crate::ir::*;
use crate::ingest::overrides::Overrides;
use std::collections::HashMap;
use tracing::debug;

/// Merge layers: defaults → vanilla → overrides
pub fn merge_layers(
    mut vanilla: HashMap<String, EntityDef>,
    overrides: &Overrides,
) -> HashMap<String, EntityDef> {
    for (name, entity) in vanilla.iter_mut() {
        debug!("Merging entity: {}", name);

        // Layer 1: Apply defaults to missing components
        apply_defaults(entity, &overrides.defaults.components);

        // Layer 2: Apply entity-specific overrides
        if let Some(entity_override) = overrides.entities.get(name) {
            apply_entity_override(entity, entity_override, name);
        }
    }

    vanilla
}

fn apply_defaults(
    entity: &mut EntityDef,
    defaults: &HashMap<String, serde_json::Value>,
) {
    for (comp_name, default_value) in defaults {
        // Only apply if entity doesn't have this component
        if !entity.components.contains_key(comp_name) {
            entity.components.insert(
                comp_name.clone(),
                ComponentValue::Data(default_value.clone()),
            );
            entity
                .attribution
                .component_sources
                .insert(comp_name.clone(), Source::Defaults);
        }
    }
}

fn apply_entity_override(
    entity: &mut EntityDef,
    override_data: &crate::ingest::overrides::EntityOverride,
    entity_name: &str,
) {
    for (comp_name, override_value) in &override_data.components {
        match entity.components.get_mut(comp_name) {
            Some(ComponentValue::Data(existing)) => {
                // Merge override into existing
                merge_json_objects(existing, override_value);
            }
            Some(ComponentValue::Marker) => {
                // Upgrade marker to data
                entity.components.insert(
                    comp_name.clone(),
                    ComponentValue::Data(override_value.clone()),
                );
            }
            None => {
                // Add new component
                entity.components.insert(
                    comp_name.clone(),
                    ComponentValue::Data(override_value.clone()),
                );
            }
        }

        entity.attribution.component_sources.insert(
            comp_name.clone(),
            Source::Override(format!("{}.kdl", entity_name)),
        );
    }
}

/// Deep merge two JSON objects
fn merge_json_objects(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    if let (serde_json::Value::Object(base_obj), serde_json::Value::Object(overlay_obj)) =
        (base, overlay)
    {
        for (key, value) in overlay_obj {
            match base_obj.get_mut(key) {
                Some(base_value) if base_value.is_object() && value.is_object() => {
                    merge_json_objects(base_value, value);
                }
                _ => {
                    base_obj.insert(key.clone(), value.clone());
                }
            }
        }
    } else {
        *base = overlay.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_json_objects() {
        let mut base = serde_json::json!({
            "value": 10,
            "nested": { "a": 1 }
        });
        let overlay = serde_json::json!({
            "value": 20,
            "nested": { "b": 2 },
            "new_field": true
        });

        merge_json_objects(&mut base, &overlay);

        assert_eq!(base["value"], 20);
        assert_eq!(base["nested"]["a"], 1);
        assert_eq!(base["nested"]["b"], 2);
        assert_eq!(base["new_field"], true);
    }
}
```

### Success Criteria

#### Automated Verification:
- [ ] Merge module compiles
- [ ] Unit test passes: `cargo test -p unastar_data_gen`

#### Manual Verification:
- [ ] Defaults are applied to entities missing components
- [ ] Overrides correctly update existing component values
- [ ] Attribution tracking shows correct sources

---

## Phase 4: Datagen - KDL Emission

### Overview
Write merged entity data to `output/entities.kdl`.

### Changes Required

#### 1. Emit Module

**File**: `crates/unastar_data/datagen/src/emit.rs`

```rust
use crate::ir::*;
use kdl::{KdlDocument, KdlNode, KdlValue, KdlEntry};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

pub fn write_entities_kdl(
    entities: &HashMap<String, EntityDef>,
    output_dir: &Path,
) -> miette::Result<()> {
    let mut doc = KdlDocument::new();

    // Add header comment
    doc.set_leading(format!(
        "// AUTO-GENERATED by unastar_data_gen\n\
         // Do not edit manually - changes will be overwritten\n\
         // Entity count: {}\n\n",
        entities.len()
    ));

    // Sort entities by name for deterministic output
    let mut sorted_names: Vec<_> = entities.keys().collect();
    sorted_names.sort();

    for name in sorted_names {
        let entity = &entities[name];
        let node = entity_to_kdl_node(entity);
        doc.nodes_mut().push(node);
    }

    // Write to file
    let output_path = output_dir.join("entities.kdl");
    std::fs::create_dir_all(output_dir)
        .map_err(|e| miette::miette!("Failed to create output dir: {}", e))?;

    std::fs::write(&output_path, doc.to_string())
        .map_err(|e| miette::miette!("Failed to write KDL: {}", e))?;

    info!("Wrote {} entities to {}", entities.len(), output_path.display());

    Ok(())
}

fn entity_to_kdl_node(entity: &EntityDef) -> KdlNode {
    let mut node = KdlNode::new("entity");

    // Add identifier as first argument
    node.push(KdlEntry::new(KdlValue::String(entity.identifier.clone())));

    // Create children document
    let mut children = KdlDocument::new();

    // Add basic info
    if let Some(spawn_cat) = &entity.spawn_category {
        let mut spawn_node = KdlNode::new("spawn_category");
        spawn_node.push(KdlEntry::new(KdlValue::String(spawn_cat.clone())));
        children.nodes_mut().push(spawn_node);
    }

    let mut spawnable_node = KdlNode::new("is_spawnable");
    spawnable_node.push(KdlEntry::new(KdlValue::Bool(entity.is_spawnable)));
    children.nodes_mut().push(spawnable_node);

    let mut summonable_node = KdlNode::new("is_summonable");
    summonable_node.push(KdlEntry::new(KdlValue::Bool(entity.is_summonable)));
    children.nodes_mut().push(summonable_node);

    // Add runtime_id if present
    if let Some(id) = entity.runtime_id {
        let mut id_node = KdlNode::new("runtime_id");
        id_node.push(KdlEntry::new(KdlValue::Base10(id as i64)));
        children.nodes_mut().push(id_node);
    }

    // Add properties
    if !entity.properties.is_empty() {
        let props_node = properties_to_kdl(&entity.properties);
        children.nodes_mut().push(props_node);
    }

    // Add components
    if !entity.components.is_empty() {
        let comps_node = components_to_kdl(&entity.components, &entity.attribution);
        children.nodes_mut().push(comps_node);
    }

    // Add component groups
    if !entity.component_groups.is_empty() {
        let groups_node = component_groups_to_kdl(&entity.component_groups);
        children.nodes_mut().push(groups_node);
    }

    // Add events
    if !entity.events.is_empty() {
        let events_node = events_to_kdl(&entity.events);
        children.nodes_mut().push(events_node);
    }

    node.set_children(children);
    node
}

fn properties_to_kdl(properties: &HashMap<String, PropertyDef>) -> KdlNode {
    let mut node = KdlNode::new("properties");
    let mut children = KdlDocument::new();

    for (name, prop) in properties {
        let mut prop_node = KdlNode::new(name.as_str());

        match &prop.prop_type {
            PropertyType::Enum { values } => {
                prop_node.push(KdlEntry::new_prop("type", KdlValue::String("enum".into())));
                // Add values as children
                let mut values_doc = KdlDocument::new();
                let mut values_node = KdlNode::new("values");
                for v in values {
                    values_node.push(KdlEntry::new(KdlValue::String(v.clone())));
                }
                values_doc.nodes_mut().push(values_node);

                let mut default_node = KdlNode::new("default");
                default_node.push(KdlEntry::new(KdlValue::String(prop.default.clone())));
                values_doc.nodes_mut().push(default_node);

                prop_node.set_children(values_doc);
            }
            PropertyType::Int { range } => {
                prop_node.push(KdlEntry::new_prop("type", KdlValue::String("int".into())));
                prop_node.push(KdlEntry::new_prop("min", KdlValue::Base10(range.0 as i64)));
                prop_node.push(KdlEntry::new_prop("max", KdlValue::Base10(range.1 as i64)));
                prop_node.push(KdlEntry::new_prop(
                    "default",
                    KdlValue::String(prop.default.clone()),
                ));
            }
            PropertyType::Float { range } => {
                prop_node.push(KdlEntry::new_prop("type", KdlValue::String("float".into())));
                prop_node.push(KdlEntry::new_prop(
                    "min",
                    KdlValue::Base10Float(range.0 as f64),
                ));
                prop_node.push(KdlEntry::new_prop(
                    "max",
                    KdlValue::Base10Float(range.1 as f64),
                ));
                prop_node.push(KdlEntry::new_prop(
                    "default",
                    KdlValue::String(prop.default.clone()),
                ));
            }
            PropertyType::Bool => {
                prop_node.push(KdlEntry::new_prop("type", KdlValue::String("bool".into())));
                prop_node.push(KdlEntry::new_prop(
                    "default",
                    KdlValue::Bool(prop.default == "true"),
                ));
            }
        }

        prop_node.push(KdlEntry::new_prop("client_sync", KdlValue::Bool(prop.client_sync)));

        children.nodes_mut().push(prop_node);
    }

    node.set_children(children);
    node
}

fn components_to_kdl(
    components: &HashMap<String, ComponentValue>,
    attribution: &Attribution,
) -> KdlNode {
    let mut node = KdlNode::new("components");
    let mut children = KdlDocument::new();

    // Sort for deterministic output
    let mut sorted_names: Vec<_> = components.keys().collect();
    sorted_names.sort();

    for name in sorted_names {
        let value = &components[name];
        let source = attribution.component_sources.get(name);

        let mut comp_node = component_value_to_kdl(name, value);

        // Add source comment
        if let Some(src) = source {
            let comment = match src {
                Source::Defaults => " // from: _defaults.kdl",
                Source::Vanilla => " // from: vanilla",
                Source::Override(f) => return {
                    comp_node.set_trailing(format!(" // from: {}", f));
                    children.nodes_mut().push(comp_node);
                    continue;
                },
            };
            comp_node.set_trailing(comment.to_string());
        }

        children.nodes_mut().push(comp_node);
    }

    node.set_children(children);
    node
}

fn component_value_to_kdl(name: &str, value: &ComponentValue) -> KdlNode {
    // Strip minecraft: prefix for cleaner output
    let clean_name = name.strip_prefix("minecraft:").unwrap_or(name);
    let mut node = KdlNode::new(clean_name);

    match value {
        ComponentValue::Marker => {
            // Empty node
        }
        ComponentValue::Data(json) => {
            if let serde_json::Value::Object(obj) = json {
                if obj.len() <= 4 && obj.values().all(|v| v.is_number() || v.is_boolean()) {
                    // Inline simple values as properties
                    for (k, v) in obj {
                        node.push(KdlEntry::new_prop(
                            k.as_str(),
                            json_to_kdl_value(v),
                        ));
                    }
                } else {
                    // Complex object - use children
                    let children = json_to_kdl_children(json);
                    node.set_children(children);
                }
            } else {
                // Single value
                node.push(KdlEntry::new(json_to_kdl_value(json)));
            }
        }
    }

    node
}

fn component_groups_to_kdl(groups: &HashMap<String, HashMap<String, ComponentValue>>) -> KdlNode {
    let mut node = KdlNode::new("component_groups");
    let mut children = KdlDocument::new();

    let mut sorted_names: Vec<_> = groups.keys().collect();
    sorted_names.sort();

    for name in sorted_names {
        let components = &groups[name];
        let clean_name = name.strip_prefix("minecraft:").unwrap_or(name);

        let mut group_node = KdlNode::new("group");
        group_node.push(KdlEntry::new(KdlValue::String(clean_name.to_string())));

        let mut group_children = KdlDocument::new();
        for (comp_name, comp_value) in components {
            let comp_node = component_value_to_kdl(comp_name, comp_value);
            group_children.nodes_mut().push(comp_node);
        }
        group_node.set_children(group_children);

        children.nodes_mut().push(group_node);
    }

    node.set_children(children);
    node
}

fn events_to_kdl(events: &HashMap<String, EventDef>) -> KdlNode {
    let mut node = KdlNode::new("events");
    let mut children = KdlDocument::new();

    let mut sorted_names: Vec<_> = events.keys().collect();
    sorted_names.sort();

    for name in sorted_names {
        let event = &events[name];
        let clean_name = name.strip_prefix("minecraft:").unwrap_or(name);

        let mut event_node = KdlNode::new("event");
        event_node.push(KdlEntry::new(KdlValue::String(clean_name.to_string())));

        let mut event_children = KdlDocument::new();

        // Add groups
        if !event.add_groups.is_empty() {
            let mut add_node = KdlNode::new("add");
            for group in &event.add_groups {
                add_node.push(KdlEntry::new(KdlValue::String(group.clone())));
            }
            event_children.nodes_mut().push(add_node);
        }

        if !event.remove_groups.is_empty() {
            let mut remove_node = KdlNode::new("remove");
            for group in &event.remove_groups {
                remove_node.push(KdlEntry::new(KdlValue::String(group.clone())));
            }
            event_children.nodes_mut().push(remove_node);
        }

        // Add trigger
        if let Some(trigger) = &event.trigger {
            let mut trigger_node = KdlNode::new("trigger");
            trigger_node.push(KdlEntry::new(KdlValue::String(trigger.clone())));
            event_children.nodes_mut().push(trigger_node);
        }

        // Add randomize
        if !event.randomize.is_empty() {
            let mut randomize_node = KdlNode::new("randomize");
            let mut rand_children = KdlDocument::new();

            for entry in &event.randomize {
                let mut option_node = KdlNode::new("option");
                option_node.push(KdlEntry::new_prop(
                    "weight",
                    KdlValue::Base10(entry.weight as i64),
                ));
                if let Some(trigger) = &entry.trigger {
                    option_node.push(KdlEntry::new_prop(
                        "trigger",
                        KdlValue::String(trigger.clone()),
                    ));
                }
                rand_children.nodes_mut().push(option_node);
            }

            randomize_node.set_children(rand_children);
            event_children.nodes_mut().push(randomize_node);
        }

        if !event_children.nodes().is_empty() {
            event_node.set_children(event_children);
        }

        children.nodes_mut().push(event_node);
    }

    node.set_children(children);
    node
}

fn json_to_kdl_value(json: &serde_json::Value) -> KdlValue {
    match json {
        serde_json::Value::Null => KdlValue::Null,
        serde_json::Value::Bool(b) => KdlValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                KdlValue::Base10(i)
            } else if let Some(f) = n.as_f64() {
                KdlValue::Base10Float(f)
            } else {
                KdlValue::Null
            }
        }
        serde_json::Value::String(s) => KdlValue::String(s.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            // Complex values become strings for now
            KdlValue::String(json.to_string())
        }
    }
}

fn json_to_kdl_children(json: &serde_json::Value) -> KdlDocument {
    let mut doc = KdlDocument::new();

    if let serde_json::Value::Object(obj) = json {
        for (key, value) in obj {
            let mut node = KdlNode::new(key.as_str());

            match value {
                serde_json::Value::Object(_) => {
                    node.set_children(json_to_kdl_children(value));
                }
                serde_json::Value::Array(arr) => {
                    for item in arr {
                        node.push(KdlEntry::new(json_to_kdl_value(item)));
                    }
                }
                _ => {
                    node.push(KdlEntry::new(json_to_kdl_value(value)));
                }
            }

            doc.nodes_mut().push(node);
        }
    }

    doc
}
```

#### 2. Update Main to Use Full Pipeline

**File**: `crates/unastar_data/datagen/src/main.rs` (update)

Add to the main function after the TODO comments:

```rust
// After the info! statements, replace the TODO section with:

let vanilla_path = manifest_dir.join(&args.vanilla);
let overrides_path = manifest_dir.join(&args.overrides);
let output_path = manifest_dir.join(&args.output);

// Step 1: Parse vanilla entities
info!("Parsing vanilla entities...");
let vanilla = ingest::parse_vanilla_entities(&vanilla_path)?;
info!("Parsed {} vanilla entities", vanilla.len());

if args.list {
    let mut names: Vec<_> = vanilla.keys().collect();
    names.sort();
    for name in names {
        println!("{}", name);
    }
    return Ok(());
}

// Step 2: Parse overrides
info!("Parsing overrides...");
let overrides = ingest::parse_overrides(&overrides_path)?;
info!(
    "Loaded {} default components, {} entity overrides",
    overrides.defaults.components.len(),
    overrides.entities.len()
);

// Step 3: Merge layers
info!("Merging layers...");
let merged = merge::merge_layers(vanilla, &overrides);

// Step 4: Emit KDL
info!("Writing KDL output...");
emit::write_entities_kdl(&merged, &output_path)?;

info!("Done!");
```

### Success Criteria

#### Automated Verification:
- [ ] Datagen runs end-to-end: `cargo run -p unastar_data_gen`
- [ ] `output/entities.kdl` is created and valid

#### Manual Verification:
- [ ] KDL output is human-readable
- [ ] Source attribution comments are present
- [ ] Components, properties, events are all present

---

## Phase 5: Codegen - Rust Generation

### Overview
Parse `output/entities.kdl` and generate Rust code.

### Changes Required

#### 1. Entity Codegen Module

**File**: `crates/unastar_data/codegen/src/entities.rs`

```rust
use heck::{ToPascalCase, ToSnakeCase};
use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::info;

/// Parsed entity from KDL for code generation
struct ParsedEntity {
    identifier: String,
    name: String, // without minecraft: prefix
    is_spawnable: bool,
    is_summonable: bool,
    runtime_id: Option<u32>,
    components: Vec<String>, // component names
    component_groups: Vec<String>,
    events: Vec<String>,
    properties: Vec<ParsedProperty>,
}

struct ParsedProperty {
    name: String,
    prop_type: String,
    values: Vec<String>, // for enums
    default: String,
    client_sync: bool,
}

pub fn generate_entities(
    input_dir: &Path,
    output_dir: &Path,
) -> miette::Result<()> {
    let kdl_path = input_dir.join("entities.kdl");
    let content = std::fs::read_to_string(&kdl_path)
        .map_err(|e| miette::miette!("Failed to read entities.kdl: {}", e))?;

    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {}", e))?;

    // Parse entities
    let mut entities = Vec::new();
    let mut all_components = HashSet::new();

    for node in doc.nodes() {
        if node.name().value() == "entity" {
            let entity = parse_entity_node(node)?;
            for comp in &entity.components {
                all_components.insert(comp.clone());
            }
            entities.push(entity);
        }
    }

    info!("Parsed {} entities, {} unique components", entities.len(), all_components.len());

    // Generate code
    let entities_dir = output_dir.join("entities");
    std::fs::create_dir_all(&entities_dir)?;
    std::fs::create_dir_all(entities_dir.join("components"))?;
    std::fs::create_dir_all(entities_dir.join("definitions"))?;

    // Generate components module
    generate_components_module(&all_components, &entities_dir)?;

    // Generate entity definitions
    for entity in &entities {
        generate_entity_definition(entity, &entities_dir)?;
    }

    // Generate mod.rs files
    generate_entities_mod(&entities, &entities_dir)?;

    Ok(())
}

fn parse_entity_node(node: &KdlNode) -> miette::Result<ParsedEntity> {
    let identifier = node
        .entries()
        .first()
        .and_then(|e| e.value().as_string())
        .unwrap_or("unknown")
        .to_string();

    let name = identifier
        .strip_prefix("minecraft:")
        .unwrap_or(&identifier)
        .to_string();

    let mut entity = ParsedEntity {
        identifier,
        name,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        components: Vec::new(),
        component_groups: Vec::new(),
        events: Vec::new(),
        properties: Vec::new(),
    };

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "is_spawnable" => {
                    entity.is_spawnable = child
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_bool())
                        .unwrap_or(false);
                }
                "is_summonable" => {
                    entity.is_summonable = child
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_bool())
                        .unwrap_or(false);
                }
                "runtime_id" => {
                    entity.runtime_id = child
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_i64())
                        .map(|i| i as u32);
                }
                "components" => {
                    if let Some(comp_children) = child.children() {
                        for comp in comp_children.nodes() {
                            entity.components.push(comp.name().value().to_string());
                        }
                    }
                }
                "component_groups" => {
                    if let Some(group_children) = child.children() {
                        for group in group_children.nodes() {
                            if group.name().value() == "group" {
                                if let Some(name) = group
                                    .entries()
                                    .first()
                                    .and_then(|e| e.value().as_string())
                                {
                                    entity.component_groups.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                "events" => {
                    if let Some(event_children) = child.children() {
                        for event in event_children.nodes() {
                            if event.name().value() == "event" {
                                if let Some(name) = event
                                    .entries()
                                    .first()
                                    .and_then(|e| e.value().as_string())
                                {
                                    entity.events.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                "properties" => {
                    if let Some(prop_children) = child.children() {
                        for prop in prop_children.nodes() {
                            if let Some(parsed) = parse_property_node(prop) {
                                entity.properties.push(parsed);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(entity)
}

fn parse_property_node(node: &KdlNode) -> Option<ParsedProperty> {
    let name = node.name().value().to_string();
    let mut prop = ParsedProperty {
        name,
        prop_type: "unknown".to_string(),
        values: Vec::new(),
        default: String::new(),
        client_sync: false,
    };

    for entry in node.entries() {
        if let Some(name) = entry.name() {
            match name.value() {
                "type" => {
                    prop.prop_type = entry.value().as_string().unwrap_or("unknown").to_string();
                }
                "default" => {
                    prop.default = match entry.value() {
                        kdl::KdlValue::String(s) => s.clone(),
                        kdl::KdlValue::Bool(b) => b.to_string(),
                        v => v.to_string(),
                    };
                }
                "client_sync" => {
                    prop.client_sync = entry.value().as_bool().unwrap_or(false);
                }
                _ => {}
            }
        }
    }

    // Get enum values from children
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "values" {
                for entry in child.entries() {
                    if let Some(s) = entry.value().as_string() {
                        prop.values.push(s.to_string());
                    }
                }
            }
        }
    }

    Some(prop)
}

fn generate_components_module(
    components: &HashSet<String>,
    output_dir: &Path,
) -> miette::Result<()> {
    let components_dir = output_dir.join("components");

    let mut mod_items = Vec::new();

    for name in components {
        let module_name = name.to_snake_case();
        let struct_name = name.to_pascal_case();

        // Generate component file
        let code = generate_component_struct(name, &struct_name);
        let formatted = format_code(code)?;

        std::fs::write(
            components_dir.join(format!("{}.rs", module_name)),
            formatted,
        )?;

        mod_items.push((module_name, struct_name));
    }

    // Generate mod.rs
    let mods: Vec<TokenStream> = mod_items
        .iter()
        .map(|(m, _)| {
            let mod_ident = format_ident!("{}", m);
            quote! { pub mod #mod_ident; }
        })
        .collect();

    let uses: Vec<TokenStream> = mod_items
        .iter()
        .map(|(m, s)| {
            let mod_ident = format_ident!("{}", m);
            let struct_ident = format_ident!("{}", s);
            quote! { pub use #mod_ident::#struct_ident; }
        })
        .collect();

    let code = quote! {
        //! Generated component DTOs from Bedrock behavior packs.

        #(#mods)*

        #(#uses)*
    };

    std::fs::write(components_dir.join("mod.rs"), format_code(code)?)?;

    Ok(())
}

fn generate_component_struct(name: &str, struct_name: &str) -> TokenStream {
    let struct_ident = format_ident!("{}", struct_name);
    let doc = format!(" Component DTO for `minecraft:{}`", name);

    quote! {
        use bevy_ecs::prelude::*;

        #[doc = #doc]
        #[derive(Component, Debug, Clone, Default, PartialEq)]
        pub struct #struct_ident {
            /// Raw JSON data - will be typed in future iteration
            pub data: Option<serde_json::Value>,
        }
    }
}

fn generate_entity_definition(
    entity: &ParsedEntity,
    output_dir: &Path,
) -> miette::Result<()> {
    let definitions_dir = output_dir.join("definitions");
    let module_name = entity.name.to_snake_case();
    let struct_name = entity.name.to_pascal_case();

    let struct_ident = format_ident!("{}", struct_name);
    let identifier = &entity.identifier;
    let is_spawnable = entity.is_spawnable;
    let is_summonable = entity.is_summonable;

    // Component group enum
    let group_enum = if !entity.component_groups.is_empty() {
        let enum_name = format_ident!("{}ComponentGroup", struct_name);
        let variants: Vec<TokenStream> = entity
            .component_groups
            .iter()
            .map(|g| {
                let variant = format_ident!("{}", g.to_pascal_case());
                quote! { #variant }
            })
            .collect();

        quote! {
            /// Component groups for this entity
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum #enum_name {
                #(#variants),*
            }
        }
    } else {
        quote! {}
    };

    // Event enum
    let event_enum = if !entity.events.is_empty() {
        let enum_name = format_ident!("{}Event", struct_name);
        let variants: Vec<TokenStream> = entity
            .events
            .iter()
            .map(|e| {
                let variant = format_ident!("{}", e.to_pascal_case());
                quote! { #variant }
            })
            .collect();

        quote! {
            /// Events for this entity
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum #enum_name {
                #(#variants),*
            }
        }
    } else {
        quote! {}
    };

    // Property enums
    let property_enums: Vec<TokenStream> = entity
        .properties
        .iter()
        .filter(|p| p.prop_type == "enum" && !p.values.is_empty())
        .map(|p| {
            let enum_name = format_ident!(
                "{}{}",
                struct_name,
                p.name
                    .strip_prefix("minecraft:")
                    .unwrap_or(&p.name)
                    .to_pascal_case()
            );
            let variants: Vec<TokenStream> = p
                .values
                .iter()
                .map(|v| {
                    let variant = format_ident!("{}", v.to_pascal_case());
                    quote! { #variant }
                })
                .collect();

            let default_variant = format_ident!("{}", p.default.to_pascal_case());

            quote! {
                /// Synced property enum
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
                pub enum #enum_name {
                    #[default]
                    #default_variant,
                    #(#variants),*
                }
            }
        })
        .collect();

    let doc = format!(" Entity definition for `{}`", identifier);

    let code = quote! {
        //! Generated definition for entity: #identifier

        #[doc = #doc]
        pub struct #struct_ident;

        impl #struct_ident {
            /// The entity identifier
            pub const IDENTIFIER: &'static str = #identifier;

            /// Whether this entity can spawn naturally
            pub const IS_SPAWNABLE: bool = #is_spawnable;

            /// Whether this entity can be summoned via commands
            pub const IS_SUMMONABLE: bool = #is_summonable;
        }

        #group_enum

        #event_enum

        #(#property_enums)*
    };

    std::fs::write(
        definitions_dir.join(format!("{}.rs", module_name)),
        format_code(code)?,
    )?;

    Ok(())
}

fn generate_entities_mod(
    entities: &[ParsedEntity],
    output_dir: &Path,
) -> miette::Result<()> {
    // definitions/mod.rs
    let def_mods: Vec<TokenStream> = entities
        .iter()
        .map(|e| {
            let mod_ident = format_ident!("{}", e.name.to_snake_case());
            quote! { pub mod #mod_ident; }
        })
        .collect();

    let def_code = quote! {
        //! Generated entity definitions.

        #(#def_mods)*
    };

    std::fs::write(
        output_dir.join("definitions/mod.rs"),
        format_code(def_code)?,
    )?;

    // entities/mod.rs
    let entities_code = quote! {
        //! Generated entity definitions from Bedrock behavior packs.
        //!
        //! This module is auto-generated by `unastar_data_codegen`.
        //! Do not edit manually.

        pub mod components;
        pub mod definitions;

        pub use components::*;
    };

    std::fs::write(output_dir.join("mod.rs"), format_code(entities_code)?)?;

    Ok(())
}

fn format_code(code: TokenStream) -> miette::Result<String> {
    let file = syn::parse2(code)
        .map_err(|e| miette::miette!("Failed to parse generated code: {}", e))?;
    Ok(prettyplease::unparse(&file))
}
```

#### 2. Utils Module

**File**: `crates/unastar_data/codegen/src/utils.rs`

```rust
use heck::{ToPascalCase, ToSnakeCase};

pub fn component_to_module_name(name: &str) -> String {
    name.strip_prefix("minecraft:")
        .unwrap_or(name)
        .replace('.', "_")
        .to_snake_case()
}

pub fn component_to_struct_name(name: &str) -> String {
    name.strip_prefix("minecraft:")
        .unwrap_or(name)
        .replace('.', "_")
        .to_pascal_case()
}
```

#### 3. Update Codegen Main

**File**: `crates/unastar_data/codegen/src/main.rs` (update)

Replace the TODO section:

```rust
let input_path = manifest_dir.join(&args.input);
let output_path = manifest_dir.join(&args.output);

// Generate entities
info!("Generating entity code...");
entities::generate_entities(&input_path, &output_path)?;

info!("Done!");
```

### Success Criteria

#### Automated Verification:
- [ ] Codegen compiles: `cargo check -p unastar_data_codegen`
- [ ] Codegen runs: `cargo run -p unastar_data_codegen`
- [ ] Generated Rust compiles: `cargo check -p unastar_data`

#### Manual Verification:
- [ ] Generated component files are readable
- [ ] Entity definitions have correct enums
- [ ] No duplicate definitions

---

## Phase 6: Integration & Testing

### Overview
Wire everything together and verify the full pipeline.

### Changes Required

#### 1. Add to Workspace

**File**: `Cargo.toml` (workspace root)

Add to members if not auto-detected:

```toml
[workspace]
members = [
    # ... existing members
    "crates/unastar_data",
    "crates/unastar_data/datagen",
    "crates/unastar_data/codegen",
]
```

#### 2. Setup Git Submodule for vanilla_bp

```bash
cd crates/unastar_data/data
git submodule add https://github.com/Mojang/bedrock-samples.git vanilla_bp
```

#### 3. Add to .gitignore

**File**: `crates/unastar_data/.gitignore`

```gitignore
# Submodules (but not overrides)
/data/vanilla_bp/

# Keep output committed
!/output/
```

#### 4. CLI Convenience Scripts

Add to root `Makefile` or `justfile`:

```makefile
# Generate entity data pipeline
.PHONY: datagen codegen

datagen:
	cargo run -p unastar_data_gen

codegen:
	cargo run -p unastar_data_codegen

# Full pipeline
entities: datagen codegen
	cargo check -p unastar_data
```

### Success Criteria

#### Automated Verification:
- [ ] Full pipeline works: `make entities`
- [ ] Workspace compiles: `cargo check --workspace`
- [ ] Tests pass: `cargo test -p unastar_data_gen`

#### Manual Verification:
- [ ] `output/entities.kdl` is clean and readable
- [ ] Generated Rust code is idiomatic
- [ ] Components can be used with bevy_ecs

---

## Testing Strategy

### Unit Tests
- JSON comment stripping
- KDL parsing and emission roundtrip
- Merge layer ordering
- Name conversion utilities

### Integration Tests
- Parse all 122 vanilla entity JSONs
- Full pipeline produces valid output
- Generated code compiles and is usable

### Manual Testing Steps
1. `cargo run -p unastar_data_gen -- --list` - verify parsing
2. Inspect `output/entities.kdl` for readability
3. `cargo run -p unastar_data_codegen` - verify codegen
4. Try using generated component in a test system
5. Check `cargo doc -p unastar_data` includes generated docs

---

## Performance Considerations

- Datagen and codegen are offline tools - not runtime critical
- KDL output is deterministic for reproducible builds
- Generated code uses zero-copy where possible
- Component DTOs use `Option<T>` for lazy evaluation

---

## Migration Notes

- Generated code lives in `crates/unastar_data/src/entities/`
- Existing handwritten components in `unastar/src/entity/` are unaffected
- Gradual migration: use both generated DTOs and handwritten components
- Protocol stays in valentine_gen for now (future work)

---

## References

- valentine_gen: `crates/valentine_gen/` for codegen patterns
- KDL spec: https://kdl.dev/
- kdl crate: https://docs.rs/kdl/6.5.0/kdl/
- bevy_ecs: Component derive requirements
- Bedrock behavior packs: Mojang/bedrock-samples repository
