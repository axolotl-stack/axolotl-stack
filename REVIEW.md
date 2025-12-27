# Axolotl Stack - Comprehensive Codebase Review

**Date**: December 2025
**Scope**: Full architecture review with focus on code generation strategy and data sources

---

## Executive Summary

The Axolotl Stack is a well-architected Minecraft Bedrock server implementation in Rust. However, there's a growing tension between the code generation approach and the limitations of the underlying data source (minecraft-data). This review examines the current state, identifies pain points, and proposes a path forward.

**Key Finding**: The minecraft-data source is excellent for **protocol definitions** but increasingly insufficient for **game data** (blocks, items, entities, biomes). A hybrid approach is recommended.

---

## 1. Current Architecture Overview

### Crate Structure

| Crate | Purpose | Code Type |
|-------|---------|-----------|
| **valentine** | Protocol abstraction layer | Generated |
| **valentine_gen** | Code generator | Manual |
| **jolyne** | High-level Bedrock protocol | Manual |
| **tokio-raknet** | RakNet transport | Manual |
| **tokio-nethernet** | NetherNet/WebRTC transport | Manual |
| **unastar** | Server implementation (ECS) | Manual |
| **axelerator** | Xbox friend broadcast | Manual |
| **axolotl-xbl** | Xbox Live auth | Manual |

### What Gets Generated

From `minecraft-data/data/bedrock/VERSION/`:

1. **protocol.json** → `packets/*.rs`, `types/*.rs` (~50,000+ LOC)
2. **items.json** → `items.rs` (800+ ZST items)
3. **blocks.json** → `blocks.rs` (600+ ZST blocks)
4. **blockStates.json** → `block_states.rs` (state enums & structs)
5. **entities.json** → `entities.rs`
6. **biomes.json** → `biomes.rs`

---

## 2. The Generation Problem

### What Works Well

**Protocol generation is solid**:
- minecraft-data protocol.json is well-maintained by PrismarineJS
- Packet structures are accurate and update with new versions
- Type system (containers, arrays, switches, enums) maps cleanly to Rust
- Feature flags for version-specific compilation work well

### What's Breaking Down

**Game data generation is problematic**:

| Data Type | minecraft-data Quality | Impact |
|-----------|----------------------|--------|
| Protocol packets | Excellent | None - works great |
| Block IDs/names | Good | Minor issues |
| Block states | Incomplete | **State mapping bugs** |
| Block properties | Missing many | **Hardness, resistance not always accurate** |
| Block entities | Almost none | **Cannot load/save tile entities** |
| Item components | Minimal | **Creative content packet broken** |
| Entity metadata | Incomplete | **Entity sync unreliable** |
| Biome tags | None | **Custom biomes impossible** |
| Recipes | None | **Crafting system impossible** |

### Known Bugs Currently Present

1. **Block Entity Item Data** (`unastar/src/storage/leveldb_world.rs:87-138`)
   ```rust
   // TODO: Load block entities
   // TODO: Block entities, entities
   ```
   - LevelDB stores block entities but we can't parse them
   - No NBT schema for chests, furnaces, signs, etc.

2. **Creative Content Items** (`unastar/src/server/game/packets.rs:227`)
   ```rust
   // TODO: Fix item format in creative content packet
   ```
   - Items sent with placeholder data
   - Enchantments, durability, NBT components missing

3. **Stack Size Logic** (`unastar/src/item/stack.rs:104`)
   - Hardcoded to 64 instead of reading from item registry
   - Eggs (16), tools (1), etc. don't work correctly

---

## 3. Comparison: How Others Handle This

### Dragonfly (Go)

Dragonfly takes the **opposite approach**:
- **No code generation** - everything manually written
- **Embedded data files** - hardcoded block/item registries
- **Full NBT support** - block entities work completely
- **Trade-off**: More maintenance burden, but complete control

Their block registration looks like:
```go
// Hardcoded block definitions
world.RegisterBlock(stone.Stone{})
world.RegisterBlock(planks.Planks{Wood: wood.Oak()})
```

### node-minecraft-protocol (JS)

- Uses minecraft-data **only for protocol**
- Handles game data separately with manual definitions
- Acknowledges minecraft-data limitations

### Nukkit/Cloudburst (Java)

- **Extracts data directly from Bedrock client**
- Uses actual game data files (`.dat`, `.json` from game)
- Has complete block entity, recipe, and entity data
- **This is probably the best source of truth**

---

## 4. The Core Decision

### Option A: Abandon Generation Entirely

**Pros**:
- Full control over all data
- Can implement features immediately
- No upstream dependency issues

**Cons**:
- Massive manual maintenance burden
- Easy to make mistakes/typos
- Version updates become tedious
- Loses reproducibility benefits

**Verdict**: Not recommended for protocol. Too much to maintain manually.

### Option B: Hybrid Approach (Recommended)

**Keep generating**:
- Protocol packets (minecraft-data is excellent here)
- Basic block/item IDs and names (the "index")

**Stop generating, manually maintain**:
- Block entity NBT schemas
- Item component data
- Recipes
- Entity metadata structures
- Biome definitions

**New data source ideas**:
- Extract from Bedrock client directly (like Nukkit does)
- Use Cloudburst's extracted data files
- Create our own extraction tool

### Option C: Better Data Source

Find/create a more complete data source:

| Source | Protocol | Blocks | Items | Entities | Block Entities | Recipes |
|--------|----------|--------|-------|----------|----------------|---------|
| minecraft-data | Excellent | Okay | Okay | Partial | None | None |
| Bedrock client extraction | N/A | Complete | Complete | Complete | Complete | Complete |
| Cloudburst data | N/A | Good | Good | Good | Good | Good |

**Recommendation**: Extract from Bedrock client, generate Rust code from that.

---

## 5. Proposed Architecture Changes

### Phase 1: Decouple Data from Protocol

```
valentine/
├── protocol/          # KEEP GENERATING from minecraft-data
│   └── bedrock_1_21_130/
│       └── packets/
└── data/              # STOP GENERATING - use new source
    └── bedrock_1_21_130/
        ├── blocks.rs          # Manual or from better source
        ├── items.rs
        ├── block_entities.rs  # NEW: NBT schemas
        ├── entities.rs
        ├── biomes.rs
        └── recipes.rs         # NEW
```

### Phase 2: Block Entity NBT Schemas

Instead of generating, define NBT structures manually:

```rust
// block_entities.rs
#[derive(Debug, NbtSerialize, NbtDeserialize)]
pub struct ChestBlockEntity {
    pub items: Vec<ItemSlot>,
    pub custom_name: Option<String>,
    pub lock: Option<String>,
}

#[derive(Debug, NbtSerialize, NbtDeserialize)]
pub struct FurnaceBlockEntity {
    pub burn_time: i16,
    pub cook_time: i16,
    pub cook_time_total: i16,
    pub items: [ItemSlot; 3],
}

// Registry
pub fn get_block_entity_parser(id: &str) -> Option<fn(&[u8]) -> BlockEntity> {
    match id {
        "Chest" => Some(parse_chest),
        "Furnace" => Some(parse_furnace),
        // ...
    }
}
```

### Phase 3: Item Component System

```rust
// item_components.rs - NOT generated
pub struct ItemComponents {
    pub durability: Option<DurabilityComponent>,
    pub enchantments: Option<EnchantmentsComponent>,
    pub custom_data: Option<NbtCompound>,
    pub food: Option<FoodComponent>,
    pub tool: Option<ToolComponent>,
}

#[derive(Debug)]
pub struct DurabilityComponent {
    pub max_damage: i32,
    pub damage: i32,
}
```

### Phase 4: New Data Extraction Tool

Create a separate tool that:
1. Reads from Bedrock client resource packs
2. Extracts block definitions, items, recipes
3. Outputs Rust code or structured data files
4. Can be run when new versions release

---

## 6. Block State Handling - A Better Way

### Current Approach (Generated)

```rust
// Generated from blockStates.json - incomplete
pub struct DoorState {
    pub facing: CardinalDirection,
    pub half: DoorHalf,
    pub open: bool,
}
```

### Dragonfly's Approach

```go
// Hardcoded but complete
type Door struct {
    Wood  wood.Wood
    Facing cube.Direction
    Open  bool
    Top   bool
    Right bool
}

func (d Door) EncodeBlock() (string, map[string]any) {
    return "minecraft:" + d.Wood.String() + "_door", map[string]any{
        "direction":     int32(horizontalDirection(d.Facing)),
        "door_hinge_bit": d.Right,
        "open_bit":      d.Open,
        "upper_block_bit": d.Top,
    }
}
```

### Proposed Approach

Use a data-driven approach with validation:

```rust
// blocks/doors.rs - manually maintained
#[derive(BlockState)]
#[block_state(id = "minecraft:oak_door")]
pub struct OakDoor {
    #[state(key = "direction", range = 0..=3)]
    pub direction: u8,
    #[state(key = "door_hinge_bit")]
    pub hinge_right: bool,
    #[state(key = "open_bit")]
    pub open: bool,
    #[state(key = "upper_block_bit")]
    pub upper: bool,
}

impl OakDoor {
    pub fn facing(&self) -> Direction {
        Direction::from_bedrock_horizontal(self.direction)
    }
}
```

---

## 7. Immediate Action Items

### High Priority

1. **Fix Block Entity Loading** - Critical for world persistence
   - Define NBT schemas for common block entities
   - Implement LevelDB block entity parsing
   - Location: `unastar/src/storage/leveldb_world.rs`

2. **Fix Creative Content Items** - Critical for gameplay
   - Implement proper item component serialization
   - Location: `unastar/src/server/game/packets.rs:227`

3. **Separate Protocol from Data** - Architectural
   - Keep protocol generation
   - Move to manual/extracted data for game data

### Medium Priority

4. **Item Stack Sizes** - Use registry data
5. **Recipe System** - New data source needed
6. **Entity Metadata** - Proper type definitions

### Low Priority

7. **Biome Tags** - When needed for worldgen
8. **Custom Dimensions** - Long-term feature

---

## 8. Technical Debt Summary

| Area | Debt Level | Fix Complexity | Impact |
|------|------------|----------------|--------|
| Block entity NBT | High | Medium | Worlds don't persist properly |
| Item components | High | Medium | Creative mode broken |
| minecraft-data dependency | Medium | High | Long-term maintenance |
| Stack size logic | Low | Low | Minor gameplay bug |
| Entity metadata | Medium | Medium | Entity sync issues |

---

## 9. Recommendation Summary

### Keep
- Protocol packet generation from minecraft-data
- Basic block/item ID registry generation
- Feature flag system for version support
- ZST pattern for zero-cost abstractions

### Change
- Stop generating game data (block entities, item components)
- Create new data extraction from Bedrock client
- Manually maintain NBT schemas
- Use derive macros for block state encoding

### Add
- Block entity NBT schema definitions
- Item component system
- Recipe data and crafting system
- Data extraction tool for new versions

### Consider
- Cloudburst/Nukkit data as reference
- Dragonfly patterns for block state handling
- Hybrid generation (generate index, manual details)

---

## 10. Conclusion

The minecraft-data source has served well for protocol implementation but is fundamentally limited for game data. The path forward is clear:

1. **Protocol**: Keep generating - it works
2. **Game Data**: New approach needed - extract from client or manual maintenance
3. **Immediate Fix**: Block entities and item components are breaking core functionality

The generator isn't the problem - the data source is. Either find a better source (Bedrock client extraction) or accept manual maintenance for game data while keeping protocol generation.

---

## Appendix A: Files Requiring Immediate Attention

```
unastar/src/storage/leveldb_world.rs:87    # TODO: Load block entities
unastar/src/storage/leveldb_world.rs:138   # TODO: Block entities, entities
unastar/src/server/game/packets.rs:227     # TODO: Fix item format
unastar/src/item/stack.rs:104              # TODO: Stack size lookup
```

## Appendix B: SerenityJS/bedrock-data Analysis

### Overview

[SerenityJS/bedrock-data](https://github.com/SerenityJS/bedrock-data) is a data extraction tool that pulls game data directly from the Bedrock Dedicated Server. This makes it a **significantly better data source** than minecraft-data for game data.

### What bedrock-data Extracts (from BDS)

| File | Size | Contents |
|------|------|----------|
| `block_types.json` | 326 KB | Block definitions with components, tags, solid/liquid/air flags |
| `block_states.json` | 23 KB | 128 state property definitions with types and ranges |
| `block_permutations.json` | 3.3 MB | Complete state permutation mappings |
| `item_types.json` | 200 KB | Items with tags, stackable flag, maxAmount |
| `entity_types.json` | 44 KB | Entities with component lists |

### What SerenityJS Adds (in their server package)

SerenityJS maintains **additional data** beyond bedrock-data:

| File | Size | Contents | **minecraft-data has this?** |
|------|------|----------|------------------------------|
| `block_metadata.json` | 138 KB | Hardness, friction, mapColor | Partial |
| `item_metadata.json` | 369 KB | Network IDs, properties (durability, damage, enchantability) as NBT | **NO** |
| `block_drops.json` | 2.3 KB | Drop tables with min/max/chance | **NO** |
| `creative_content.json` | 255 KB | Creative inventory with encoded NBT instances | **NO** |
| `creative_groups.json` | 12 KB | Creative tab groupings | **NO** |
| `shaped.json` | 535 KB | Shaped crafting recipes | **NO** |
| `shapeless.json` | 483 KB | Shapeless crafting recipes | **NO** |
| `tool_types.json` | 1.3 KB | Tool category mappings | **NO** |

### Data Structure Comparison

**bedrock-data block_types.json** (better than minecraft-data):
```json
{
  "identifier": "minecraft:chest",
  "components": ["minecraft:inventory"],
  "tags": ["minecraft:is_axe_item_destructible", "not_feature_replaceable"],
  "loggable": true,
  "air": false,
  "liquid": false,
  "solid": false,
  "states": ["minecraft:cardinal_direction"]
}
```

**minecraft-data blocks.json** (current):
```json
{
  "id": 54,
  "name": "chest",
  "displayName": "Chest",
  "hardness": 2.5,
  "resistance": 2.5,
  "transparent": true,
  "emitLight": 0,
  "filterLight": 0
}
```

**bedrock-data item_types.json**:
```json
{
  "identifier": "minecraft:diamond_pickaxe",
  "tags": ["minecraft:digger", "minecraft:is_pickaxe", "minecraft:is_tool"],
  "stackable": false,
  "maxAmount": 1
}
```

**SerenityJS item_metadata.json** (extra data):
```json
{
  "identifier": "minecraft:diamond_pickaxe",
  "networkId": 318,
  "isComponentBased": true,
  "itemVersion": 1,
  "properties": "<base64 NBT with durability, enchantability, damage, repair materials>"
}
```

### Block State Handling

**bedrock-data block_states.json** structure:
```json
{
  "identifier": "direction",
  "type": "int",
  "values": [0, 1, 2, 3]
}
```

```json
{
  "identifier": "color",
  "type": "string",
  "values": ["white", "orange", "magenta", ...]
}
```

This is **cleaner** than minecraft-data's blockStates.json which requires aggregation.

### Verdict: Is bedrock-data Enough?

| Requirement | bedrock-data | Needs SerenityJS extras? |
|-------------|--------------|--------------------------|
| Block IDs & names | ✅ Complete | No |
| Block states/permutations | ✅ Complete (3.3MB) | No |
| Block components/tags | ✅ Complete | No |
| Block hardness/friction | ❌ No | Yes (block_metadata.json) |
| Item IDs & stack sizes | ✅ Complete | No |
| Item tags | ✅ Complete | No |
| Item durability/enchantability | ❌ No | Yes (item_metadata.json) |
| Entity types & components | ✅ Complete | No |
| Block drops | ❌ No | Yes (block_drops.json) |
| Recipes | ❌ No | Yes (shaped/shapeless.json) |
| Creative inventory | ❌ No | Yes (creative_content.json) |

**Answer**: bedrock-data alone is **not quite enough** - you'd also need the metadata files that SerenityJS maintains separately.

### Recommendation

**Hybrid approach using bedrock-data + SerenityJS data**:

1. **Use bedrock-data for**:
   - Block types, states, permutations (complete)
   - Item types with tags (complete)
   - Entity types (complete)

2. **Use SerenityJS data package for**:
   - block_metadata.json (hardness, friction)
   - item_metadata.json (durability, damage, enchantability NBT)
   - creative_content.json (fixes our creative inventory bug!)
   - Recipes (shaped.json, shapeless.json)
   - block_drops.json

3. **Generator changes needed**:
   - Parse bedrock-data JSON format (different from minecraft-data)
   - Merge metadata from SerenityJS files
   - Keep protocol generation from minecraft-data (it's still good there)

### Is Your Block State Approach Correct?

**Yes, but with caveats.**

Your current approach in `block_states.rs`:
- Clusters blocks by state shape
- Generates shared state structs (DoorState, StairState, etc.)
- Implements `BlockState` trait with `state_offset()` / `from_offset()`

This is **architecturally sound**. The issue is the **data quality**, not the approach.

**SerenityJS approach** (for comparison):
- `BlockPermutation` class stores state as key-value dictionary
- `matches()` method compares states
- Serializes to NBT for persistence
- Uses Molang queries for block queries

**Your approach is actually more efficient** for Rust because:
1. ZST blocks = no runtime overhead
2. State offset calculation = fast palette lookups
3. Typed state structs = compile-time safety

**What to fix**:
- Switch data source to bedrock-data (complete state permutations)
- Add missing state patterns in `derive_state_name()` as they come up
- Consider generating a runtime state dictionary for NBT serialization

## Appendix C: Migration Path

### Phase 1: Add bedrock-data as Additional Source

```
crates/valentine_gen/
├── minecraft-data/          # Keep for protocol
└── bedrock-data/            # Add for game data
    └── data/latest/
        ├── block_types.json
        ├── block_states.json
        ├── block_permutations.json
        ├── item_types.json
        └── entity_types.json
```

### Phase 2: Add SerenityJS Metadata

Either:
- Clone their data package as submodule
- Or manually maintain the metadata files

```
crates/valentine_gen/
└── serenity-data/           # Metadata files
    ├── block_metadata.json
    ├── item_metadata.json
    ├── creative_content.json
    └── recipes/
```

### Phase 3: Update Generators

1. **blocks.rs generator**: Read from bedrock-data block_types.json
2. **block_states.rs generator**: Read from bedrock-data block_states.json + block_permutations.json
3. **items.rs generator**: Read from bedrock-data item_types.json + SerenityJS item_metadata.json
4. **NEW creative.rs generator**: Read from SerenityJS creative_content.json

### Phase 4: Protocol Stays on minecraft-data

No changes needed for protocol generation - it works well.

## Appendix D: External Resources

- [SerenityJS/bedrock-data](https://github.com/SerenityJS/bedrock-data) - **Recommended data source**
- [SerenityJS/serenity](https://github.com/SerenityJS/serenity) - Reference for metadata files
- [Dragonfly Source](https://github.com/df-mc/dragonfly) - Reference for block/item implementation
- [Cloudburst Protocol](https://github.com/CloudburstMC/Protocol) - Java Bedrock protocol
- [minecraft-data](https://github.com/PrismarineJS/minecraft-data) - Current data source (keep for protocol)
- [Bedrock Wiki](https://wiki.bedrock.dev/) - Documentation on formats
