---
date: 2026-01-04T12:00:00-05:00
researcher: Claude
git_commit: 6b1dc6b9b4ba939e94777528f8abb0de4d0523dc
branch: main
repository: axolotl-stack
topic: "Block and Item Registry Architecture for Plugin Extensibility"
tags: [research, codebase, registry, blocks, items, plugins, extensibility]
status: complete
last_updated: 2026-01-04
last_updated_by: Claude
---

# Research: Block and Item Registry Architecture for Plugin Extensibility

**Date**: 2026-01-04T12:00:00-05:00
**Researcher**: Claude
**Git Commit**: 6b1dc6b9b4ba939e94777528f8abb0de4d0523dc
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Research into the current block and item generation code, registries, and related systems. The goal is to understand how the API can be made extensible for plugins to add custom blocks/items with custom namespaces (e.g., `newnamespace:custom_block` instead of just `minecraft:xxxx`). This requires understanding the current array-based storage and how to transition to registry-based HashMap lookups.

## Summary

The codebase has a **three-layer architecture** for blocks and items:

1. **Code Generation Layer** (`valentine_gen`) - Parses minecraft-data JSON files and generates Zero-Sized Type (ZST) structs with const trait implementations
2. **Static Definition Layer** (`valentine/bedrock_versions`) - Contains generated `BLOCKS` and `ITEMS` static slices of trait objects
3. **Runtime Registry Layer** (`unastar/registry`) - Wraps a sparse `Vec<Option<T>>` for O(1) ID lookups with O(n) name lookups

**Current Limitations for Extensibility:**
- All string IDs are hardcoded with `"minecraft:"` namespace prefix during generation
- No HashMap exists for name-based lookups (linear O(n) scan through ~1000+ entries)
- The `BLOCKS` and `ITEMS` static arrays cannot be extended at runtime
- World generation has its own separate `BLOCK_LOOKUP` HashMap that bypasses the registry
- Protocol packet generation iterates static arrays, not the registry
- No plugin API exists for registering custom blocks/items

## Detailed Findings

### 1. Code Generation Architecture

#### Valentine Generator (`crates/valentine_gen/`)

The generator processes minecraft-data JSON files to create Rust code:

**Entry Point**: [main.rs](crates/valentine_gen/src/main.rs)

**Block Generation Flow** ([blocks.rs](crates/valentine_gen/src/data_generator/blocks.rs)):
1. Parses `blocks.json` from minecraft-data (lines 71-72)
2. Parses `blockStates.json` for state properties (lines 75-76)
3. Optionally loads `legacy.json` for ID mapping (lines 87-106)
4. Generates ZST struct for each block (lines 126-178)
5. Creates static `BLOCKS` array (lines 180-188)

**Item Generation Flow** ([items.rs](crates/valentine_gen/src/data_generator/items.rs)):
1. Parses `items.json` from minecraft-data (line 97)
2. Builds `name_to_id` HashMap for repair item resolution (lines 102-105)
3. Generates ZST struct for each item (lines 132-174)
4. Generates trait impls: `DurableItem`, `RepairableItem`, `EnchantableItem`, `VariantItem` (lines 177-253)
5. Creates static `ITEMS` array (lines 257-283)

**Namespace Handling in Generation**:
```rust
// blocks.rs:150-153
writeln!(out, "    const STRING_ID: &'static str = \"minecraft:{}\";", block.name)?;

// items.rs:165-166
writeln!(out, "    const STRING_ID: &'static str = \"minecraft:{}\";", item.name)?;
```

The `"minecraft:"` prefix is **hardcoded** during generation. The source JSON files contain bare names like `"stone"`, `"air"`.

### 2. Generated Block/Item Definitions

#### Block Definitions ([blocks.rs](crates/valentine/bedrock_versions/v1_21_130/src/blocks.rs))

Each block is a Zero-Sized Type (ZST) with const trait implementation:

```rust
/// Stone
pub struct Stone;

impl BlockDef for Stone {
    const ID: u32 = 1;
    const STRING_ID: &'static str = "minecraft:stone";
    const NAME: &'static str = "Stone";
    const HARDNESS: f32 = 1.5_f32;
    const RESISTANCE: f32 = 6.0_f32;
    const IS_TRANSPARENT: bool = false;
    const EMIT_LIGHT: u8 = 0;
    const FILTER_LIGHT: u8 = 15;
    const MIN_STATE_ID: u32 = 2532;
    const MAX_STATE_ID: u32 = 2532;
    type State = ();
    fn default_state() -> Self::State { Default::default() }
}
```

**Static Registry Array** (line 23785):
```rust
pub static BLOCKS: &[&'static dyn BlockDefDyn] = &[
    &Air,
    &Stone,
    &Granite,
    // ... 1321 blocks total
];
```

#### Item Definitions ([items.rs](crates/valentine/bedrock_versions/v1_21_130/src/items.rs))

Similar ZST pattern with optional trait extensions:

```rust
pub struct DiamondSword;

impl ItemDef for DiamondSword {
    const ID: u32 = 316;
    const STRING_ID: &'static str = "minecraft:diamond_sword";
    const NAME: &'static str = "Diamond Sword";
    const STACK_SIZE: u8 = 1;
}

impl DurableItem for DiamondSword {
    const MAX_DURABILITY: u16 = 1561;
}

impl EnchantableItem for DiamondSword {
    fn enchant_categories() -> &'static [EnchantmentCategory] {
        &[EnchantmentCategory::Weapon, EnchantmentCategory::Sword, ...]
    }
}
```

**Static Registry Array** (line 20270):
```rust
pub static ITEMS: &[&'static dyn ItemDefDyn] = &[
    &Air,
    &Stone,
    // ... 1888 items total
];
```

### 3. Runtime Registry System

#### Generic Registry Infrastructure ([mod.rs](crates/unastar/src/registry/mod.rs))

```rust
pub trait RegistryEntry: Clone + Debug {
    fn id(&self) -> u32;
    fn string_id(&self) -> &str;
}

pub struct Registry<T: RegistryEntry> {
    entries: Vec<Option<T>>,  // Sparse vector indexed by ID
    count: usize,
}
```

**Lookup Methods**:
- `get(id: u32)` - O(1) direct array indexing (line 78-82)
- `get_by_name(name: &str)` - **O(n) linear scan** (line 138-140)

```rust
pub fn get_by_name(&self, name: &str) -> Option<&T> {
    self.iter().find(|e| e.string_id() == name)
}
```

#### Block Registry ([block.rs](crates/unastar/src/registry/block.rs))

```rust
pub struct BlockEntry {
    pub id: u32,
    pub string_id: String,
    pub name: String,
    pub state_count: u32,
    pub min_state_id: u32,
    pub max_state_id: u32,
    pub default_state_id: u32,
}

pub type BlockRegistry = Registry<BlockEntry>;
```

**Loading** (lines 44-59):
```rust
pub fn load_vanilla(&mut self) {
    use jolyne::valentine::blocks::BLOCKS;
    for block in BLOCKS.iter() {
        let entry = BlockEntry {
            id: block.id(),
            string_id: block.string_id().to_string(),  // Copies to owned String
            // ...
        };
        let _ = self.register(entry);
    }
}
```

**Runtime ID Lookup** (lines 62-68):
```rust
pub fn get_by_runtime_id(&self, runtime_id: u32) -> Option<&BlockEntry> {
    // O(n) linear scan - comment notes "could use interval tree"
    self.iter().find(|entry| {
        runtime_id >= entry.min_state_id && runtime_id <= entry.max_state_id
    })
}
```

#### Item Registry ([item.rs](crates/unastar/src/registry/item.rs))

```rust
pub struct ItemEntry {
    pub id: u32,
    pub string_id: String,
    pub name: String,
    pub stack_size: u8,
}

pub type ItemRegistry = Registry<ItemEntry>;
```

### 4. World Generation Block Lookups

#### Separate HashMap System ([chunk.rs](crates/unastar/src/world/chunk.rs))

World generation has its **own lookup system** that bypasses the registry:

```rust
// Pre-built HashMap for O(1) lookups (lines 54-60)
static BLOCK_LOOKUP: LazyLock<HashMap<String, u32>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for block in BLOCKS.iter() {
        map.insert(block.string_id().to_string(), block.default_state_id());
    }
    map
});

// Fast lookup function (lines 67-69)
pub fn get_block_id(name: &str) -> u32 {
    BLOCK_LOOKUP.get(name).copied().unwrap_or(*AIR)
}
```

**Pre-cached Block Constants** (lines 72-95):
```rust
pub static AIR: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:air"));
pub static STONE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:stone"));
pub static DIRT: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:dirt"));
// ... many more
```

### 5. Protocol Packet Generation

#### Block Properties Packet ([block.rs:71-94](crates/unastar/src/registry/block.rs))

```rust
pub fn to_block_properties(&self) -> Vec<BlockPropertiesItem> {
    // Iterates static BLOCKS array, NOT the registry!
    for block in BLOCKS.iter() {
        items.push(BlockPropertiesItem {
            name: block.string_id().to_string(),
            state: Nbt::default(),  // Empty NBT (TODO noted in comments)
        });
    }
}
```

#### Item Registry Packet ([item.rs:49-66](crates/unastar/src/registry/item.rs))

```rust
pub fn to_packet(&self) -> ItemRegistryPacket {
    let itemstates = self.iter().map(|item| ItemstatesItem {
        name: item.string_id.clone(),
        runtime_id: item.id as i16,
        component_based: false,
        // ...
    }).collect();
}
```

### 6. Plugin System

#### Current Plugin API ([lib.rs](crates/unastar-api/src/lib.rs))

The plugin API provides:
- Event subscription (17 event types)
- Player actions (message, teleport, kick, give_item)
- World queries (`get_block(x, y, z)` returns opaque `BlockId(u32)`)

**No registry APIs are exposed to plugins.** From [components.rs:6](crates/unastar-api/src/native/components.rs):
> "No component re-exports - plugins access via World.get::<T>() where T comes from unastar"

#### Plugin Item Resolution ([plugins.rs:104-127](crates/unastar/src/server/game/plugins.rs))

When plugins give items, name-based lookups are used:
```rust
let network_id = item_registry.0
    .get_by_name(&item_id)  // O(n) scan
    .map(|entry| entry.id as i32)
    .unwrap_or(1);  // fallback to dirt

let block_runtime_id = block_registry.0
    .get_by_name(&item_id)  // Another O(n) scan
    .map(|entry| entry.min_state_id as i32)
    .unwrap_or(0);
```

### 7. Name-Based Lookup Usage Patterns

| Location | Method | Complexity | Use Case |
|----------|--------|------------|----------|
| `registry/mod.rs:138` | `get_by_name()` | O(n) | General registry lookups |
| `world/chunk.rs:54` | `BLOCK_LOOKUP` HashMap | O(1) | World generation |
| `world/chunk.rs:42` | `lookup()` linear scan | O(n) | Static constant initialization |
| `server/game/blocks.rs:706` | `get_by_name()` | O(n) | Item-to-block mapping |
| `server/game/plugins.rs:106` | `get_by_name()` | O(n) | Plugin item giving |

## Architecture Documentation

### Current Data Flow

```
minecraft-data JSON files
        │
        ▼
┌─────────────────────────────┐
│   valentine_gen (build)     │
│   - Parses JSON             │
│   - Generates ZST structs   │
│   - Hardcodes "minecraft:"  │
└─────────────────────────────┘
        │
        ▼
┌─────────────────────────────┐
│   valentine/bedrock_versions│
│   - Static BLOCKS array     │
│   - Static ITEMS array      │
│   - ~1300 blocks, ~1900 items│
└─────────────────────────────┘
        │
        ▼
┌─────────────────────────────┐
│   unastar/registry          │
│   - BlockRegistry           │
│   - ItemRegistry            │
│   - Vec<Option<T>> storage  │
│   - O(n) name lookups       │
└─────────────────────────────┘
        │
        ├──────────────────────┐
        ▼                      ▼
┌───────────────┐    ┌─────────────────┐
│ World Gen     │    │ Protocol Packets│
│ - Own HashMap │    │ - Uses static   │
│ - Bypasses    │    │   BLOCKS/ITEMS  │
│   registry    │    │   arrays        │
└───────────────┘    └─────────────────┘
```

### Key Types

| Type | Location | Purpose |
|------|----------|---------|
| `BlockDef` | `bedrock_core/src/block.rs:78` | Compile-time block trait |
| `BlockDefDyn` | `bedrock_core/src/block.rs:114` | Runtime block trait object |
| `BlockEntry` | `registry/block.rs:10` | Runtime registry entry |
| `BlockRegistry` | `registry/block.rs:38` | Type alias for `Registry<BlockEntry>` |
| `ItemDef` | `bedrock_core/src/item.rs:13` | Compile-time item trait |
| `ItemDefDyn` | `bedrock_core/src/item.rs:121` | Runtime item trait object |
| `ItemEntry` | `registry/item.rs:7` | Runtime registry entry |
| `ItemRegistry` | `registry/item.rs:29` | Type alias for `Registry<ItemEntry>` |

### Namespace Format

All identifiers use the format `namespace:identifier`:
- Vanilla: `"minecraft:stone"`, `"minecraft:diamond_sword"`
- Custom (future): `"myplugin:custom_block"`, `"othermod:special_item"`

## Code References

### Block System
- [bedrock_core/src/block.rs:78-112](crates/valentine/bedrock_core/src/block.rs#L78-L112) - BlockDef trait definition
- [v1_21_130/src/blocks.rs:23785](crates/valentine/bedrock_versions/v1_21_130/src/blocks.rs#L23785) - Static BLOCKS array
- [registry/block.rs:10-25](crates/unastar/src/registry/block.rs#L10-L25) - BlockEntry struct
- [registry/block.rs:44-59](crates/unastar/src/registry/block.rs#L44-L59) - load_vanilla()
- [world/chunk.rs:54-69](crates/unastar/src/world/chunk.rs#L54-L69) - BLOCK_LOOKUP HashMap

### Item System
- [bedrock_core/src/item.rs:13-23](crates/valentine/bedrock_core/src/item.rs#L13-L23) - ItemDef trait definition
- [v1_21_130/src/items.rs:20270](crates/valentine/bedrock_versions/v1_21_130/src/items.rs#L20270) - Static ITEMS array
- [registry/item.rs:7-16](crates/unastar/src/registry/item.rs#L7-L16) - ItemEntry struct
- [registry/item.rs:33-46](crates/unastar/src/registry/item.rs#L33-L46) - load_vanilla()

### Registry Infrastructure
- [registry/mod.rs:40-46](crates/unastar/src/registry/mod.rs#L40-L46) - RegistryEntry trait
- [registry/mod.rs:48-53](crates/unastar/src/registry/mod.rs#L48-L53) - Registry<T> struct
- [registry/mod.rs:138-140](crates/unastar/src/registry/mod.rs#L138-L140) - get_by_name() O(n) lookup

### Code Generation
- [data_generator/blocks.rs:65-192](crates/valentine_gen/src/data_generator/blocks.rs#L65-L192) - Block generation
- [data_generator/items.rs:92-286](crates/valentine_gen/src/data_generator/items.rs#L92-L286) - Item generation

### Plugin System
- [unastar-api/src/lib.rs:283-300](crates/unastar-api/src/lib.rs#L283-L300) - PluginAction enum
- [server/game/plugins.rs:104-127](crates/unastar/src/server/game/plugins.rs#L104-L127) - Plugin item resolution

## Open Questions

1. **ID Allocation Strategy**: How should custom block/item IDs be allocated to avoid conflicts with vanilla IDs? Options:
   - Reserved ID ranges per namespace
   - Dynamic ID assignment at registration time
   - Hash-based ID generation from namespace:name

2. **Protocol Compatibility**: Bedrock clients need to know about custom blocks/items. This requires:
   - Sending custom entries in `StartGame` packet's `block_properties`
   - Sending custom entries in `ItemRegistryPacket`
   - Understanding how clients handle unknown block/item IDs

3. **State ID Management**: Custom blocks with states need runtime state ID ranges that don't overlap with vanilla blocks.

4. **World Generation Integration**: The separate `BLOCK_LOOKUP` HashMap in `chunk.rs` would need to include custom blocks for them to be usable in surface rules.

5. **Persistence**: How should custom blocks be stored in world saves? If a plugin is removed, what happens to its blocks?

6. **Hot Reloading**: Should custom blocks/items be registerable after server start, or only during initialization?
