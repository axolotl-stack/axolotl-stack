# Block Registry Typestate Architecture Implementation Plan

## Overview

This plan redesigns the block/item registry system to provide **compile-time type safety** for block capabilities while maintaining **runtime extensibility** for plugins.

**Key design decisions:**
1. **String IDs are the source of truth.** `BlockId` (cached array index) is an optimization, not identity.
2. **Two kinds of capabilities:** StateCaps (structural, inferrable from property names) vs RoleCaps (semantic/gameplay, must be explicit).
3. **Mixed-radix state encoding:** Proper stride-based decode/encode, not bitfields.
4. **Freeze/snapshot pattern:** `RegistryBuilder` → `freeze()` → `RegistrySnapshot` for runtime ID allocation.
5. **World persistence strategy:** Store string_id → runtime_id mapping in world metadata to handle plugin order changes.
6. **Model A: Preserve vanilla runtime IDs.** Vanilla blocks keep their minecraft-data `min_state_id/max_state_id`. Plugin blocks get allocated after vanilla range.

## Current State Analysis

Based on research in [2026-01-04-block-item-registry-extensibility.md](../research/2026-01-04-block-item-registry-extensibility.md):

### Current Architecture
- **Code Generation** (`valentine_gen`): Generates ZST marker types with `BlockDef` trait
- **Static Arrays** (`valentine/bedrock_versions`): `BLOCKS` and `ITEMS` static slices
- **Runtime Registry** (`unastar/registry`): `BlockEntry` with O(n) name lookups
- **World Storage**: Chunks store raw `u32` runtime IDs
- **World Generator**: Uses string IDs from JSON (`"minecraft:grass_block"`)

### Key Discoveries
- [bedrock_core/src/block.rs:74-112](../../crates/valentine/bedrock_core/src/block.rs#L74-L112): `BlockDef` trait with const data + associated `State` type
- [bedrock_core/src/block.rs:114-168](../../crates/valentine/bedrock_core/src/block.rs#L114-L168): `BlockDefDyn` for object-safe dynamic dispatch
- [registry/block.rs:10-25](../../crates/unastar/src/registry/block.rs#L10-L25): `BlockEntry` runtime struct
- [data_generator/block_states.rs:409-436](../../crates/valentine_gen/src/data_generator/block_states.rs#L409-L436): **Mixed-radix** state_offset() generation (correct pattern!)
- [world/chunk.rs:54-69](../../crates/unastar/src/world/chunk.rs#L54-L69): `BLOCK_LOOKUP` HashMap for world gen

### Current Limitations
1. No typed block access after reading from world
2. No capability system (can't ask "does this block have redstone?")
3. ZST blocks have no state data (state is separate)
4. No behavior dispatch (blocks can't handle their own logic)
5. Can't match on block types without runtime ID comparisons
6. Plugin blocks would need hardcoded indices (fragile)

## Capability Design: StateCaps vs RoleCaps

### The Problem with Inferring Capabilities

**Don't** infer semantic capabilities ("this block is a redstone source") from property names like `powered` or `lit`. A block having a `powered` property says nothing about:
- Whether it's a redstone source, conductor, or consumer
- Whether it responds to interactions
- Whether it schedules ticks or needs neighbor updates

### Two Bitflags: StateCaps and RoleCaps

```rust
bitflags! {
    /// Structural state capabilities - SAFE to infer from property names.
    /// These just describe the shape of the state data.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct StateCaps: u32 {
        const POWERED      = 1 << 0;  // Has powered property
        const FACING       = 1 << 1;  // Has direction/facing property
        const LIT          = 1 << 2;  // Has lit property
        const OPEN         = 1 << 3;  // Has open property
        const AXIS         = 1 << 4;  // Has pillar_axis property
        const WATERLOGGED  = 1 << 5;  // Has waterlogged property
        const AGE          = 1 << 6;  // Has age/growth property
        const LEVEL        = 1 << 7;  // Has level/liquid_depth property
    }
}

bitflags! {
    /// Semantic role capabilities - MUST be explicitly assigned.
    /// These describe gameplay behavior and cannot be inferred.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct RoleCaps: u32 {
        /// Produces redstone signal (levers, buttons, pressure plates, comparators)
        const RS_SOURCE           = 1 << 0;
        /// Transmits redstone signal (redstone wire, repeaters)
        const RS_CONDUCTOR        = 1 << 1;
        /// Reacts to redstone power (lamps, pistons, doors, dispensers)
        const RS_CONSUMER         = 1 << 2;
        /// Logic gate (repeater, comparator)
        const RS_GATE             = 1 << 3;
        /// Toggles state when right-clicked (lever, button, trapdoor)
        const RS_TOGGLE_ON_INTERACT = 1 << 4;
        /// Needs to process neighbor block updates
        const NEEDS_NEIGHBOR_UPDATES = 1 << 5;
        /// Schedules game ticks (repeaters, observers)
        const SCHEDULES_TICKS     = 1 << 6;
        /// Can be placed on/against surfaces
        const ATTACHABLE          = 1 << 7;
        /// Block has associated block entity (NBT data like sign text, chest contents)
        const HAS_BLOCK_ENTITY    = 1 << 8;
        /// Block entity has inventory slots
        const HAS_INVENTORY       = 1 << 9;
        /// Block entity has text content (signs, command blocks)
        const HAS_TEXT            = 1 << 10;
    }
}
```

### Inference Rules

**StateCaps** - Auto-inferred from property names during generation:
```rust
fn infer_state_caps(props: &HashMap<String, PropertyDef>) -> StateCaps {
    let mut caps = StateCaps::empty();
    for prop_name in props.keys() {
        match prop_name.as_str() {
            "powered" | "powered_bit" => caps |= StateCaps::POWERED,
            "facing_direction" | "cardinal_direction" | "direction" | "weirdo_direction"
                => caps |= StateCaps::FACING,
            "lit" => caps |= StateCaps::LIT,
            "open_bit" | "open" => caps |= StateCaps::OPEN,
            "pillar_axis" | "axis" => caps |= StateCaps::AXIS,
            "waterlogged" => caps |= StateCaps::WATERLOGGED,
            "age" | "growth" => caps |= StateCaps::AGE,
            "level" | "liquid_depth" | "fill_level" => caps |= StateCaps::LEVEL,
            _ => {}
        }
    }
    caps
}
```

**RoleCaps** - Must be explicitly defined (data file or hardcoded):
```rust
// In a data file or generated from curated list:
fn get_role_caps(string_id: &str) -> RoleCaps {
    match string_id {
        // Redstone
        "minecraft:lever" => RoleCaps::RS_SOURCE | RoleCaps::RS_TOGGLE_ON_INTERACT,
        "minecraft:stone_button" | "minecraft:wooden_button"
            => RoleCaps::RS_SOURCE | RoleCaps::RS_TOGGLE_ON_INTERACT | RoleCaps::SCHEDULES_TICKS,
        "minecraft:redstone_lamp" => RoleCaps::RS_CONSUMER,
        "minecraft:piston" | "minecraft:sticky_piston"
            => RoleCaps::RS_CONSUMER | RoleCaps::NEEDS_NEIGHBOR_UPDATES,
        "minecraft:redstone_wire" => RoleCaps::RS_CONDUCTOR,
        "minecraft:repeater" | "minecraft:unpowered_repeater" | "minecraft:powered_repeater"
            => RoleCaps::RS_CONDUCTOR | RoleCaps::RS_GATE | RoleCaps::SCHEDULES_TICKS,
        "minecraft:observer" => RoleCaps::RS_SOURCE | RoleCaps::SCHEDULES_TICKS | RoleCaps::NEEDS_NEIGHBOR_UPDATES,

        // Block entities - containers
        "minecraft:chest" | "minecraft:trapped_chest" | "minecraft:barrel"
            => RoleCaps::HAS_BLOCK_ENTITY | RoleCaps::HAS_INVENTORY,
        "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker"
            => RoleCaps::HAS_BLOCK_ENTITY | RoleCaps::HAS_INVENTORY,
        "minecraft:hopper" | "minecraft:dropper" | "minecraft:dispenser"
            => RoleCaps::HAS_BLOCK_ENTITY | RoleCaps::HAS_INVENTORY | RoleCaps::RS_CONSUMER,
        "minecraft:brewing_stand" => RoleCaps::HAS_BLOCK_ENTITY | RoleCaps::HAS_INVENTORY,

        // Block entities - signs
        s if s.contains("sign") => RoleCaps::HAS_BLOCK_ENTITY | RoleCaps::HAS_TEXT,

        // Block entities - banners
        s if s.contains("banner") => RoleCaps::HAS_BLOCK_ENTITY,

        // Block entities - other
        "minecraft:beacon" => RoleCaps::HAS_BLOCK_ENTITY,
        "minecraft:command_block" | "minecraft:chain_command_block" | "minecraft:repeating_command_block"
            => RoleCaps::HAS_BLOCK_ENTITY | RoleCaps::HAS_TEXT,
        "minecraft:mob_spawner" => RoleCaps::HAS_BLOCK_ENTITY,

        _ => RoleCaps::empty(),
    }
}
```

## State Encoding: Mixed-Radix, Not Bitfields

### The Problem

Previous plan used `(bit_offset, bit_width)` for state layout. This is **wrong**. Bedrock (and our current generator) uses **mixed-radix encoding**:

```
offset = prop0_value * 1
       + prop1_value * prop0_count
       + prop2_value * (prop0_count * prop1_count)
       + ...
```

### Correct: PropLayout with Stride + O(1) Indexing

```rust
/// Describes how to decode a single property from state offset.
#[derive(Clone, Debug)]
pub struct PropLayout {
    /// Which property this is (for typed access).
    pub kind: PropKind,
    /// Multiplier for this property in the offset formula.
    /// stride = product of all previous property counts.
    pub stride: u32,
    /// Number of valid values for this property.
    pub value_count: u32,
    /// How to interpret the raw value.
    pub value_map: ValueMap,
}

/// Property kind enum for typed access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropKind {
    Powered,
    Facing,
    Lit,
    Open,
    Axis,
    Waterlogged,
    Age,
    Level,
    /// Custom property by name hash.
    Custom(u32),
}

/// Number of builtin PropKind variants (excluding Custom).
pub const PROP_KIND_BUILTIN_COUNT: usize = 8;

/// How to map raw values to typed values.
#[derive(Clone, Debug)]
pub enum ValueMap {
    /// Boolean: 0 = false, 1 = true
    Bool,
    /// Integer range: raw value + min = actual value
    IntRange { min: u32, max: u32 },
    /// Enum with string values mapped to indices
    Enum { values: Vec<String> },
    /// Direction with specific domain
    Facing(FacingDomain),
}

/// Direction domain - prevents decoding 4-way cardinal as 6-way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacingDomain {
    /// 4-way cardinal (N/E/S/W) - value_count = 4
    Cardinal4,
    /// 6-way (N/E/S/W/Up/Down) - value_count = 6
    Full6,
    /// Lever-style direction (Bedrock-specific encoding)
    LeverDirection,
    /// Torch-style (includes "unknown" for floor placement)
    TorchFacing,
}

/// Complete state layout for a block with O(1) property access.
#[derive(Clone, Debug, Default)]
pub struct StateLayout {
    /// Properties in encoding order (matches blockStates.json property order).
    pub props: Vec<PropLayout>,
    /// Total state count (product of all value_counts).
    pub state_count: u32,

    // === O(1) INDEX TABLE FOR BUILTIN PROPS ===
    // Indices into `props` vec for each builtin PropKind.
    // None = block doesn't have this property.
    idx_powered: Option<u8>,
    idx_facing: Option<u8>,
    idx_lit: Option<u8>,
    idx_open: Option<u8>,
    idx_axis: Option<u8>,
    idx_waterlogged: Option<u8>,
    idx_age: Option<u8>,
    idx_level: Option<u8>,
}

impl StateLayout {
    /// Create layout for a stateless block.
    pub fn stateless() -> Self {
        Self {
            props: Vec::new(),
            state_count: 1,
            ..Default::default()
        }
    }

    /// Build from props, computing index table.
    pub fn new(props: Vec<PropLayout>) -> Self {
        let state_count = props.iter().map(|p| p.value_count).product::<u32>().max(1);

        let mut layout = Self {
            props,
            state_count,
            idx_powered: None,
            idx_facing: None,
            idx_lit: None,
            idx_open: None,
            idx_axis: None,
            idx_waterlogged: None,
            idx_age: None,
            idx_level: None,
        };

        // Build index table
        for (i, prop) in layout.props.iter().enumerate() {
            let idx = Some(i as u8);
            match prop.kind {
                PropKind::Powered => layout.idx_powered = idx,
                PropKind::Facing => layout.idx_facing = idx,
                PropKind::Lit => layout.idx_lit = idx,
                PropKind::Open => layout.idx_open = idx,
                PropKind::Axis => layout.idx_axis = idx,
                PropKind::Waterlogged => layout.idx_waterlogged = idx,
                PropKind::Age => layout.idx_age = idx,
                PropKind::Level => layout.idx_level = idx,
                PropKind::Custom(_) => {} // Not indexed
            }
        }

        layout
    }

    /// O(1) lookup for builtin prop index.
    #[inline]
    fn get_prop_index(&self, kind: PropKind) -> Option<usize> {
        match kind {
            PropKind::Powered => self.idx_powered.map(|i| i as usize),
            PropKind::Facing => self.idx_facing.map(|i| i as usize),
            PropKind::Lit => self.idx_lit.map(|i| i as usize),
            PropKind::Open => self.idx_open.map(|i| i as usize),
            PropKind::Axis => self.idx_axis.map(|i| i as usize),
            PropKind::Waterlogged => self.idx_waterlogged.map(|i| i as usize),
            PropKind::Age => self.idx_age.map(|i| i as usize),
            PropKind::Level => self.idx_level.map(|i| i as usize),
            PropKind::Custom(_) => {
                // Fallback to linear search for custom props
                self.props.iter().position(|p| p.kind == kind)
            }
        }
    }

    /// Decode a property value from state offset. O(1) for builtin props.
    #[inline]
    pub fn decode_prop(&self, offset: u32, kind: PropKind) -> Option<u32> {
        let idx = self.get_prop_index(kind)?;
        let prop = &self.props[idx];
        Some((offset / prop.stride) % prop.value_count)
    }

    /// Create a new offset with one property changed. O(1) for builtin props.
    #[inline]
    pub fn with_prop(&self, offset: u32, kind: PropKind, new_value: u32) -> u32 {
        if let Some(idx) = self.get_prop_index(kind) {
            let prop = &self.props[idx];
            let current = (offset / prop.stride) % prop.value_count;
            offset - (current * prop.stride) + (new_value * prop.stride)
        } else {
            offset
        }
    }

    // === TYPED CONVENIENCE METHODS ===

    /// Decode powered property. O(1).
    #[inline]
    pub fn decode_powered(&self, offset: u32) -> Option<bool> {
        self.idx_powered.map(|i| {
            let prop = &self.props[i as usize];
            ((offset / prop.stride) % prop.value_count) != 0
        })
    }

    /// Decode facing property with domain safety. O(1).
    #[inline]
    pub fn decode_facing(&self, offset: u32) -> Option<(u32, FacingDomain)> {
        self.idx_facing.map(|i| {
            let prop = &self.props[i as usize];
            let raw = (offset / prop.stride) % prop.value_count;
            let domain = match &prop.value_map {
                ValueMap::Facing(d) => *d,
                _ => FacingDomain::Full6, // Default fallback
            };
            (raw, domain)
        })
    }

    /// Decode lit property. O(1).
    #[inline]
    pub fn decode_lit(&self, offset: u32) -> Option<bool> {
        self.idx_lit.map(|i| {
            let prop = &self.props[i as usize];
            ((offset / prop.stride) % prop.value_count) != 0
        })
    }
}
```

### Generator Example

From `block_states.rs` (lines 409-436), the existing generator already does mixed-radix correctly:

```rust
// state_offset generation (existing code)
writeln!(out, "    fn state_offset(&self) -> u32 {{")?;
writeln!(out, "        let mut offset = 0u32;")?;
writeln!(out, "        let mut multiplier = 1u32;")?;
for (i, prop) in shape.props.iter().enumerate() {
    let field_name = to_snake_case(&prop.name);
    let range = if prop.prop_type == PropType::String {
        prop.string_values.len() as u32
    } else {
        (prop.max - prop.min + 1) as u32
    };
    writeln!(out, "        offset += (self.{} as u32) * multiplier;", field_name)?;
    if i < shape.props.len() - 1 {
        writeln!(out, "        multiplier *= {};", range)?;
    }
}
```

We just need to store the stride/value_count in `StateLayout` at registration time.

## Registry Lifecycle: Builder → Freeze → Snapshot

### The Problem

Current plan has plugins registering into the live registry. But:
1. Runtime IDs must be assigned after all blocks are known
2. `runtime_to_id` vec must be sized correctly
3. Block properties packets need to be built once

### Solution: RegistryBuilder → freeze() → RegistrySnapshot

```rust
/// Block specification for registration.
/// Contains all data needed to register a block.
pub struct BlockSpec {
    pub string_id: String,
    pub name: String,
    pub state_caps: StateCaps,
    pub role_caps: RoleCaps,
    pub state_layout: StateLayout,
    pub default_state_offset: u32,
    pub behavior: Option<Arc<dyn BlockBehavior>>,
    /// Pre-assigned runtime ID range (for vanilla blocks from minecraft-data).
    /// None = allocate dynamically during freeze() (for plugin blocks).
    pub fixed_runtime_range: Option<(u32, u32)>, // (min_state_id, max_state_id)
}

/// Mutable builder phase - accepts registrations.
pub struct RegistryBuilder {
    /// Vanilla blocks with fixed runtime IDs (registered first).
    vanilla_blocks: Vec<BlockSpec>,
    /// Plugin blocks needing dynamic allocation.
    plugin_blocks: Vec<BlockSpec>,
    /// String ID → index tracking for duplicate detection.
    by_string_id: HashMap<String, ()>,
    /// Tracks the vanilla runtime ID ceiling for plugin allocation.
    vanilla_max_runtime_id: u32,
}

impl RegistryBuilder {
    pub fn new() -> Self {
        Self {
            vanilla_blocks: Vec::new(),
            plugin_blocks: Vec::new(),
            by_string_id: HashMap::new(),
            vanilla_max_runtime_id: 0,
        }
    }

    /// Register a vanilla block with fixed runtime IDs from minecraft-data.
    /// These preserve the canonical runtime IDs that clients expect.
    pub fn register_vanilla(&mut self, spec: BlockSpec) -> Result<(), RegistryError> {
        if self.by_string_id.contains_key(&spec.string_id) {
            return Err(RegistryError::DuplicateBlock(spec.string_id));
        }

        // Vanilla blocks MUST have fixed runtime range
        let (min_id, max_id) = spec.fixed_runtime_range
            .ok_or_else(|| RegistryError::InvalidSpec(
                format!("vanilla block {} must have fixed_runtime_range", spec.string_id)
            ))?;

        // Track the maximum runtime ID for later plugin allocation
        self.vanilla_max_runtime_id = self.vanilla_max_runtime_id.max(max_id + 1);

        self.by_string_id.insert(spec.string_id.clone(), ());
        self.vanilla_blocks.push(spec);
        Ok(())
    }

    /// Register a plugin block with dynamically allocated runtime IDs.
    /// These are allocated after the vanilla range during freeze().
    pub fn register_plugin(&mut self, spec: BlockSpec) -> Result<(), RegistryError> {
        if self.by_string_id.contains_key(&spec.string_id) {
            return Err(RegistryError::DuplicateBlock(spec.string_id));
        }

        // Plugin blocks should NOT have fixed runtime range
        if spec.fixed_runtime_range.is_some() {
            return Err(RegistryError::InvalidSpec(
                format!("plugin block {} should not have fixed_runtime_range", spec.string_id)
            ));
        }

        self.by_string_id.insert(spec.string_id.clone(), ());
        self.plugin_blocks.push(spec);
        Ok(())
    }

    /// Freeze the registry: finalize runtime IDs, build lookup tables.
    /// Model A: Vanilla blocks keep their minecraft-data IDs.
    /// Plugin blocks are allocated sequentially after vanilla range.
    pub fn freeze(self) -> RegistrySnapshot {
        let total = self.vanilla_blocks.len() + self.plugin_blocks.len();
        let mut entries = Vec::with_capacity(total);
        let mut by_string_id = HashMap::with_capacity(total);
        let mut default_states = HashMap::with_capacity(total);

        // Phase 1: Process vanilla blocks (fixed runtime IDs)
        for (idx, spec) in self.vanilla_blocks.into_iter().enumerate() {
            let id = BlockId::new(idx as u32);
            let (min_state_id, max_state_id) = spec.fixed_runtime_range.unwrap();
            let default_state_id = min_state_id + spec.default_state_offset;

            by_string_id.insert(spec.string_id.clone(), id);
            default_states.insert(spec.string_id.clone(), default_state_id);

            entries.push(BlockEntry {
                id,
                string_id: spec.string_id,
                name: spec.name,
                state_caps: spec.state_caps,
                role_caps: spec.role_caps,
                state_layout: spec.state_layout,
                behavior: spec.behavior,
                min_state_id,
                max_state_id,
                default_state_id,
            });
        }

        // Phase 2: Process plugin blocks (dynamic allocation after vanilla)
        let mut next_runtime_id = self.vanilla_max_runtime_id;
        let vanilla_count = entries.len();

        for (idx, spec) in self.plugin_blocks.into_iter().enumerate() {
            let id = BlockId::new((vanilla_count + idx) as u32);
            let state_count = spec.state_layout.state_count.max(1);

            let min_state_id = next_runtime_id;
            let max_state_id = next_runtime_id + state_count - 1;
            let default_state_id = min_state_id + spec.default_state_offset.min(state_count - 1);

            next_runtime_id = max_state_id + 1;

            by_string_id.insert(spec.string_id.clone(), id);
            default_states.insert(spec.string_id.clone(), default_state_id);

            entries.push(BlockEntry {
                id,
                string_id: spec.string_id,
                name: spec.name,
                state_caps: spec.state_caps,
                role_caps: spec.role_caps,
                state_layout: spec.state_layout,
                behavior: spec.behavior,
                min_state_id,
                max_state_id,
                default_state_id,
            });
        }

        // Build runtime_to_id lookup (dense vec covering full range)
        let max_runtime_id = entries.iter()
            .map(|e| e.max_state_id)
            .max()
            .unwrap_or(0);

        let mut runtime_to_id = vec![None; (max_runtime_id + 1) as usize];
        for entry in &entries {
            for runtime_id in entry.min_state_id..=entry.max_state_id {
                runtime_to_id[runtime_id as usize] = Some(entry.id);
            }
        }

        RegistrySnapshot {
            entries,
            by_string_id,
            default_states,
            runtime_to_id,
        }
    }
}

/// Immutable snapshot - used during gameplay.
pub struct RegistrySnapshot {
    entries: Vec<BlockEntry>,
    by_string_id: HashMap<String, BlockId>,
    default_states: HashMap<String, u32>,
    /// Runtime ID → BlockId. None for invalid/unmapped IDs.
    runtime_to_id: Vec<Option<BlockId>>,
}

impl RegistrySnapshot {
    /// Resolve runtime ID to BlockDyn. Returns None for invalid IDs.
    #[inline]
    pub fn resolve(&self, runtime_id: u32) -> Option<BlockDyn<'_>> {
        let id = *self.runtime_to_id.get(runtime_id as usize)?.as_ref()?;
        let entry = self.entries.get(id.0 as usize)?;
        Some(BlockDyn::new(id, runtime_id, entry))
    }

    // ... other lookup methods
}
```

## World Persistence: Handling Plugin Order Changes

### The Problem

If a world is saved with PluginA's blocks, then loaded without PluginA:
- Runtime IDs have shifted
- Chunks contain invalid runtime IDs
- Blocks become corrupted

### Solution: String ID Mapping in World Metadata

```rust
/// Stored in world metadata (level.dat or similar).
#[derive(Serialize, Deserialize)]
pub struct BlockPalette {
    /// Mapping from string_id → runtime_id as of last save.
    /// When loading, we compare against current registry to detect mismatches.
    entries: Vec<PaletteEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct PaletteEntry {
    pub string_id: String,
    pub min_state_id: u32,
    pub max_state_id: u32,
}

impl BlockPalette {
    /// Create palette from current registry snapshot.
    pub fn from_registry(registry: &RegistrySnapshot) -> Self {
        Self {
            entries: registry.iter().map(|e| PaletteEntry {
                string_id: e.string_id.clone(),
                min_state_id: e.min_state_id,
                max_state_id: e.max_state_id,
            }).collect(),
        }
    }

    /// Create remapping table from saved palette to current registry.
    /// Returns None for runtime IDs that can't be mapped (missing plugins).
    pub fn create_remap(&self, registry: &RegistrySnapshot) -> RuntimeIdRemap {
        let max_old_id = self.entries.iter()
            .map(|e| e.max_state_id)
            .max()
            .unwrap_or(0);

        let mut remap = vec![None; (max_old_id + 1) as usize];

        for old_entry in &self.entries {
            // Find this block in current registry
            if let Some(new_entry) = registry.get_by_string(&old_entry.string_id) {
                // Check state counts match
                let old_count = old_entry.max_state_id - old_entry.min_state_id + 1;
                let new_count = new_entry.max_state_id - new_entry.min_state_id + 1;

                if old_count == new_count {
                    // Direct remap: old_state_id → new_state_id
                    for offset in 0..old_count {
                        let old_id = old_entry.min_state_id + offset;
                        let new_id = new_entry.min_state_id + offset;
                        remap[old_id as usize] = Some(new_id);
                    }
                } else {
                    // State count mismatch - remap to default state
                    for offset in 0..old_count {
                        let old_id = old_entry.min_state_id + offset;
                        remap[old_id as usize] = Some(new_entry.default_state_id);
                    }
                }
            }
            // If block doesn't exist in current registry, remap stays None
            // (will become air or trigger error handling)
        }

        RuntimeIdRemap { table: remap }
    }
}

/// Runtime ID remapping table for world loading.
pub struct RuntimeIdRemap {
    /// old_runtime_id → new_runtime_id (or None if block is missing)
    table: Vec<Option<u32>>,
}

impl RuntimeIdRemap {
    /// Remap a runtime ID from saved world to current registry.
    /// Returns fallback (air) if block is missing.
    pub fn remap(&self, old_id: u32, fallback: u32) -> u32 {
        self.table.get(old_id as usize)
            .and_then(|opt| *opt)
            .unwrap_or(fallback)
    }
}
```

### Loading Flow

```rust
// When loading a world:
fn load_world(world_dir: &Path, registry: &RegistrySnapshot) -> World {
    // 1. Load saved palette from world metadata
    let saved_palette: BlockPalette = load_palette(world_dir)?;

    // 2. Create remap table
    let remap = saved_palette.create_remap(registry);
    let air_id = registry.get_default_state("minecraft:air").unwrap();

    // 3. Load chunks with remapping
    for chunk_pos in world_chunks(world_dir) {
        let mut chunk = load_chunk_raw(world_dir, chunk_pos)?;

        // Remap all block IDs
        for section in &mut chunk.sections {
            for block in &mut section.blocks {
                *block = remap.remap(*block, air_id);
            }
        }
    }

    // 4. Save updated palette for next time
    save_palette(world_dir, &BlockPalette::from_registry(registry))?;
}
```

## Desired End State

```rust
// World returns opaque BlockDyn
let block = world.get_block(pos);

// Cast to specific block type - full compile-time safety
if let Some(lever) = block.cast::<Lever>() {
    lever.powered   // bool, not Option - Lever always has this
    lever.facing    // Direction, not Option

    let toggled = lever.with_powered(!lever.powered);
    world.set_block(pos, toggled);
}

// Check state capabilities (structural - "does it have a powered property?")
if block.has_state::<Powered>() {
    let powered = block.state_cap::<Powered>().unwrap().value;
}

// Check role capabilities (semantic - "is it a redstone source?")
if block.has_role(RoleCaps::RS_SOURCE) {
    // This block produces redstone signal
}

// Match on string ID (authoritative)
match block.string_id() {
    "minecraft:lever" => { /* handle lever */ }
    "minecraft:redstone_lamp" => { /* handle lamp */ }
    "myplugin:custom_block" => { /* plugin block! */ }
    _ => {}
}

// Or use cached BlockId for hot paths (after resolving once)
let lever_id = registry.get_id("minecraft:lever").unwrap();
if block.id() == lever_id {
    // fast comparison
}

// World generator uses string IDs directly
let runtime_id = registry.get_default_state("minecraft:grass_block")?;
chunk.set_block(x, y, z, runtime_id);

// Behavior dispatch - block handles its own logic
let new_state = block.entry().behavior.on_interact(block.runtime_id());
world.set_block_raw(pos, new_state);

// Generic over capability
fn toggle_powered<B: Block + HasPowered>(block: B) -> B {
    block.with_powered(!block.powered())
}
```

### Verification Criteria
- [ ] `block.cast::<Lever>()` returns `Option<Lever>` where `Lever` has direct field access
- [ ] `block.state_cap::<Powered>()` returns `Option<PoweredView>` for blocks with powered property
- [ ] `block.has_role(RoleCaps::RS_SOURCE)` returns true for levers, false for lamps
- [ ] Calling `.powered()` on `Stone` is a compile error (Stone doesn't impl `HasPowered`)
- [ ] `registry.get_default_state("minecraft:stone")` returns correct runtime ID
- [ ] `registry.get_id("minecraft:lever")` returns cached `BlockId`
- [ ] State decode/encode uses mixed-radix (not bitfields)
- [ ] Behavior uses `Arc<dyn BlockBehavior>` not `&'static`
- [ ] `RegistryBuilder::freeze()` assigns runtime IDs
- [ ] World palette remapping works for plugin changes
- [ ] All existing tests pass
- [ ] `cargo check` passes with no warnings

## What We're NOT Doing

- **Not changing chunk storage format**: Chunks still store `u32` runtime IDs
- **Not breaking protocol compatibility**: Same runtime IDs sent to clients
- **Not removing existing `BlockDef`/`BlockDefDyn`**: Keeping for compatibility during transition
- **Not implementing all behaviors**: Just the infrastructure; specific behaviors come later
- **Not breaking world gen**: String ID lookups continue to work

---

## Phase 1: Core Type System

### Overview
Introduce the foundational types: `BlockId`, `BlockDyn`, `Block` trait, StateCaps/RoleCaps, and capability traits.

### Changes Required

#### 1. New Core Types Module
**File**: `crates/valentine/bedrock_core/src/block_v2.rs` (new file)

```rust
//! Next-generation block type system with typestate capabilities.
//!
//! Design principles:
//! 1. **String IDs are the source of truth.** BlockId is a cached array index.
//! 2. **StateCaps** describe structural state (powered, facing, lit) - inferrable.
//! 3. **RoleCaps** describe semantic roles (RS_SOURCE, RS_CONSUMER) - explicit.
//! 4. **Mixed-radix encoding** for state properties, not bitfields.

use std::fmt;
use std::sync::Arc;

/// Cached block identity - an index into the registry's entries array.
///
/// **Important**: This is NOT the source of truth for block identity.
/// String IDs (e.g., "minecraft:lever") are authoritative.
/// `BlockId` is an optimization for O(1) lookups after resolution.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct BlockId(pub u32);

impl BlockId {
    /// Sentinel value for invalid/unassigned block ID.
    pub const INVALID: BlockId = BlockId(u32::MAX);

    /// Create a new block ID from a raw index.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw index value.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Check if this is a valid block ID.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_valid() {
            write!(f, "BlockId({})", self.0)
        } else {
            write!(f, "BlockId(INVALID)")
        }
    }
}

impl Default for BlockId {
    fn default() -> Self {
        Self::INVALID
    }
}

// ===== CAPABILITY FLAGS =====

bitflags::bitflags! {
    /// Structural state capabilities - SAFE to infer from property names.
    /// These describe the shape of block state data.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct StateCaps: u32 {
        /// Block has powered/power property.
        const POWERED      = 1 << 0;
        /// Block has facing/direction property.
        const FACING       = 1 << 1;
        /// Block has lit property.
        const LIT          = 1 << 2;
        /// Block has open property.
        const OPEN         = 1 << 3;
        /// Block has axis property.
        const AXIS         = 1 << 4;
        /// Block has waterlogged property.
        const WATERLOGGED  = 1 << 5;
        /// Block has age/growth property.
        const AGE          = 1 << 6;
        /// Block has level property.
        const LEVEL        = 1 << 7;
    }
}

bitflags::bitflags! {
    /// Semantic role capabilities - MUST be explicitly assigned.
    /// These describe gameplay behavior and cannot be inferred from properties.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct RoleCaps: u32 {
        /// Produces redstone signal (levers, buttons, pressure plates).
        const RS_SOURCE             = 1 << 0;
        /// Transmits redstone signal (redstone wire, repeaters).
        const RS_CONDUCTOR          = 1 << 1;
        /// Reacts to redstone power (lamps, pistons, doors).
        const RS_CONSUMER           = 1 << 2;
        /// Logic gate (repeater, comparator).
        const RS_GATE               = 1 << 3;
        /// Toggles state on right-click (lever, button, trapdoor).
        const RS_TOGGLE_ON_INTERACT = 1 << 4;
        /// Needs to process neighbor block updates.
        const NEEDS_NEIGHBOR_UPDATES = 1 << 5;
        /// Schedules game ticks (repeaters, observers).
        const SCHEDULES_TICKS       = 1 << 6;
        /// Can be placed on/against surfaces.
        const ATTACHABLE            = 1 << 7;
    }
}

// ===== STATE LAYOUT (MIXED-RADIX) =====

/// Describes how to decode a single property from state offset.
#[derive(Clone, Debug)]
pub struct PropLayout {
    /// Which property this is.
    pub kind: PropKind,
    /// Multiplier for this property (product of all previous value_counts).
    pub stride: u32,
    /// Number of valid values for this property.
    pub value_count: u32,
    /// How to interpret the raw value.
    pub value_map: ValueMap,
}

/// Property kind enum for typed access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropKind {
    Powered,
    Facing,
    Lit,
    Open,
    Axis,
    Waterlogged,
    Age,
    Level,
    /// Custom property (hash of name for lookup).
    Custom(u32),
}

/// How to map raw values to typed values.
#[derive(Clone, Debug)]
pub enum ValueMap {
    /// Boolean: 0 = false, 1 = true
    Bool,
    /// Integer range: raw value + min = actual value
    IntRange { min: u32, max: u32 },
    /// Enum with string values mapped to indices
    Enum { values: Vec<String> },
}

/// Complete state layout for a block.
#[derive(Clone, Debug, Default)]
pub struct StateLayout {
    /// Properties in encoding order.
    pub props: Vec<PropLayout>,
    /// Total state count (product of all value_counts).
    pub state_count: u32,
}

impl StateLayout {
    /// Create layout for a stateless block.
    pub fn stateless() -> Self {
        Self {
            props: Vec::new(),
            state_count: 1,
        }
    }

    /// Decode a property value from state offset.
    pub fn decode_prop(&self, offset: u32, kind: PropKind) -> Option<u32> {
        self.props.iter().find(|p| p.kind == kind).map(|p| {
            (offset / p.stride) % p.value_count
        })
    }

    /// Encode property values to state offset.
    pub fn encode(&self, values: &[(PropKind, u32)]) -> u32 {
        let mut offset = 0;
        for (kind, value) in values {
            if let Some(prop) = self.props.iter().find(|p| &p.kind == kind) {
                offset += value * prop.stride;
            }
        }
        offset
    }

    /// Create a new offset with one property changed.
    pub fn with_prop(&self, offset: u32, kind: PropKind, new_value: u32) -> u32 {
        if let Some(prop) = self.props.iter().find(|p| p.kind == kind) {
            let current = (offset / prop.stride) % prop.value_count;
            offset - (current * prop.stride) + (new_value * prop.stride)
        } else {
            offset
        }
    }
}

// ===== BLOCK ENTRY =====

/// Block entry in the registry with capabilities and behavior.
pub struct BlockEntry {
    /// Cached ID (array index). NOT the source of truth.
    pub id: BlockId,
    /// Authoritative string ID (e.g., "minecraft:lever").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Structural state capabilities (inferrable).
    pub state_caps: StateCaps,
    /// Semantic role capabilities (explicit).
    pub role_caps: RoleCaps,
    /// State layout for decoding properties (mixed-radix).
    pub state_layout: StateLayout,
    /// Behavior handlers (optional).
    pub behavior: Option<Arc<dyn BlockBehavior>>,
    /// Minimum runtime state ID.
    pub min_state_id: u32,
    /// Maximum runtime state ID.
    pub max_state_id: u32,
    /// Default runtime state ID.
    pub default_state_id: u32,
}

impl fmt::Debug for BlockEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockEntry")
            .field("id", &self.id)
            .field("string_id", &self.string_id)
            .field("name", &self.name)
            .field("state_caps", &self.state_caps)
            .field("role_caps", &self.role_caps)
            .field("min_state_id", &self.min_state_id)
            .field("max_state_id", &self.max_state_id)
            .field("default_state_id", &self.default_state_id)
            .field("has_behavior", &self.behavior.is_some())
            .finish()
    }
}

// ===== BLOCKDYN =====

/// Runtime opaque block returned by world.get_block().
///
/// This is the "I don't know what block this is" type.
/// Use `.cast::<T>()` to narrow to a specific block type,
/// or `.state_cap::<C>()` / `.has_role()` to access capabilities.
#[derive(Clone, Debug)]
pub struct BlockDyn<'r> {
    /// Cached block ID (array index).
    id: BlockId,
    /// Encoded block + state as runtime ID.
    runtime_id: u32,
    /// State offset within this block's range.
    state_offset: u32,
    /// Reference to block entry in registry.
    entry: &'r BlockEntry,
}

impl<'r> BlockDyn<'r> {
    /// Create a new BlockDyn from registry lookup.
    #[inline]
    pub fn new(id: BlockId, runtime_id: u32, entry: &'r BlockEntry) -> Self {
        let state_offset = runtime_id.saturating_sub(entry.min_state_id);
        Self { id, runtime_id, state_offset, entry }
    }

    /// Get the cached block ID.
    #[inline]
    pub fn id(&self) -> BlockId {
        self.id
    }

    /// Get the authoritative string ID.
    #[inline]
    pub fn string_id(&self) -> &str {
        &self.entry.string_id
    }

    /// Get the raw runtime ID.
    #[inline]
    pub fn runtime_id(&self) -> u32 {
        self.runtime_id
    }

    /// Get the state offset within this block's range.
    #[inline]
    pub fn state_offset(&self) -> u32 {
        self.state_offset
    }

    /// Get the block entry.
    #[inline]
    pub fn entry(&self) -> &'r BlockEntry {
        self.entry
    }

    /// Check if this is a specific block by string ID.
    #[inline]
    pub fn is_string(&self, string_id: &str) -> bool {
        self.entry.string_id == string_id
    }

    /// Check if this is a specific block by cached ID.
    #[inline]
    pub fn is_id(&self, id: BlockId) -> bool {
        self.id == id
    }

    /// Check if this is a specific typed block.
    #[inline]
    pub fn is<B: Block>(&self) -> bool {
        self.entry.string_id == B::STRING_ID
    }

    /// Cast to a specific block type.
    #[inline]
    pub fn cast<B: Block>(&self) -> Option<B> {
        if self.entry.string_id == B::STRING_ID {
            B::decode(self.state_offset, self.entry)
        } else {
            None
        }
    }

    /// Check if this block has a state capability.
    #[inline]
    pub fn has_state_cap(&self, cap: StateCaps) -> bool {
        self.entry.state_caps.contains(cap)
    }

    /// Check if this block has a role capability.
    #[inline]
    pub fn has_role(&self, role: RoleCaps) -> bool {
        self.entry.role_caps.contains(role)
    }

    /// Decode a property value by kind.
    #[inline]
    pub fn get_prop(&self, kind: PropKind) -> Option<u32> {
        self.entry.state_layout.decode_prop(self.state_offset, kind)
    }

    /// Check if block is powered (if it has powered property).
    #[inline]
    pub fn is_powered(&self) -> Option<bool> {
        self.get_prop(PropKind::Powered).map(|v| v != 0)
    }

    /// Check if block is lit (if it has lit property).
    #[inline]
    pub fn is_lit(&self) -> Option<bool> {
        self.get_prop(PropKind::Lit).map(|v| v != 0)
    }

    /// Check if block is open (if it has open property).
    #[inline]
    pub fn is_open(&self) -> Option<bool> {
        self.get_prop(PropKind::Open).map(|v| v != 0)
    }
}

// ===== BLOCK TRAIT =====

/// Trait for typed block structs.
pub trait Block: Sized + Clone + 'static {
    /// Authoritative string ID.
    const STRING_ID: &'static str;
    /// Display name.
    const NAME: &'static str;

    /// Decode from state offset to typed block.
    fn decode(state_offset: u32, entry: &BlockEntry) -> Option<Self>;
    /// Encode to state offset.
    fn encode(&self, entry: &BlockEntry) -> u32;
    /// Get the default state.
    fn default_state() -> Self;
}

// ===== CAPABILITY TRAITS =====

/// Blocks with powered property.
pub trait HasPowered: Block {
    fn powered(&self) -> bool;
    fn with_powered(self, val: bool) -> Self;
}

/// Blocks with facing direction.
pub trait HasFacing: Block {
    fn facing(&self) -> Direction;
    fn with_facing(self, dir: Direction) -> Self;
}

/// Blocks with lit state.
pub trait HasLit: Block {
    fn lit(&self) -> bool;
    fn with_lit(self, val: bool) -> Self;
}

/// Blocks with open state.
pub trait HasOpen: Block {
    fn open(&self) -> bool;
    fn with_open(self, val: bool) -> Self;
}

/// Blocks with axis.
pub trait HasAxis: Block {
    fn axis(&self) -> Axis;
    fn with_axis(self, axis: Axis) -> Self;
}

/// Blocks with waterlogged.
pub trait HasWaterlogged: Block {
    fn waterlogged(&self) -> bool;
    fn with_waterlogged(self, val: bool) -> Self;
}

// ===== COMMON ENUMS =====

/// Direction for directional blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Direction {
    #[default]
    North = 0,
    South = 1,
    East = 2,
    West = 3,
    Up = 4,
    Down = 5,
}

impl Direction {
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::North),
            1 => Some(Self::South),
            2 => Some(Self::East),
            3 => Some(Self::West),
            4 => Some(Self::Up),
            5 => Some(Self::Down),
            _ => None,
        }
    }
}

/// Axis for pillar blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Axis {
    X = 0,
    #[default]
    Y = 1,
    Z = 2,
}

impl Axis {
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::X),
            1 => Some(Self::Y),
            2 => Some(Self::Z),
            _ => None,
        }
    }
}

// ===== BEHAVIOR TRAIT =====

/// Block behavior handlers.
pub trait BlockBehavior: Send + Sync + 'static {
    /// Called when block receives redstone power change.
    fn on_redstone(&self, runtime_id: u32, power: u8, entry: &BlockEntry) -> u32 {
        runtime_id
    }

    /// Called when player interacts with block.
    fn on_interact(&self, runtime_id: u32, entry: &BlockEntry) -> (u32, InteractResult) {
        (runtime_id, InteractResult::None)
    }

    /// Called when neighboring block changes.
    fn on_neighbor_update(&self, runtime_id: u32, entry: &BlockEntry) -> u32 {
        runtime_id
    }
}

/// Result of block interaction.
#[derive(Clone, Debug)]
pub enum InteractResult {
    None,
    RedstoneUpdate { power: u8 },
    PlaySound { sound: String },
    /// Open block entity UI (signs, chests, furnaces, etc.)
    OpenBlockEntityUI,
    /// Drop items (e.g., breaking container)
    DropItems { items: Vec<ItemStack> },
}
```

#### 2. Export from bedrock_core
**File**: `crates/valentine/bedrock_core/src/lib.rs`
**Changes**: Add module export

```rust
pub mod block_v2;
```

#### 3. Add bitflags dependency
**File**: `crates/valentine/bedrock_core/Cargo.toml`
**Changes**: Add bitflags

```toml
[dependencies]
bitflags = "2"
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo check -p valentine_bedrock_core` passes
- [ ] `cargo test -p valentine_bedrock_core` passes
- [ ] No warnings in new code

#### Manual Verification:
- [ ] Review type definitions match discussed design
- [ ] Verify `BlockId::INVALID` sentinel is used, not `BlockId(0)`
- [ ] Verify StateCaps vs RoleCaps separation
- [ ] Verify `StateLayout` uses mixed-radix, not bitfields

---

## Phase 2: Registry with Builder/Freeze Pattern

### Overview
Create the registry with `RegistryBuilder` → `freeze()` → `RegistrySnapshot` lifecycle.

### Changes Required

#### 1. Registry Module
**File**: `crates/unastar/src/registry/block_v2.rs` (new file)

```rust
//! Block registry with builder/freeze pattern.

use std::collections::HashMap;
use std::sync::Arc;
use valentine_bedrock_core::block_v2::*;

/// Error during block registration.
#[derive(Debug, Clone)]
pub enum RegistryError {
    DuplicateBlock(String),
    InvalidSpec(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateBlock(id) => write!(f, "duplicate block: {}", id),
            Self::InvalidSpec(msg) => write!(f, "invalid spec: {}", msg),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Block specification for registration.
/// Contains all data needed to register a block.
pub struct BlockSpec {
    pub string_id: String,
    pub name: String,
    pub state_caps: StateCaps,
    pub role_caps: RoleCaps,
    pub state_layout: StateLayout,
    pub default_state_offset: u32,
    pub behavior: Option<Arc<dyn BlockBehavior>>,
    /// Pre-assigned runtime ID range (for vanilla blocks from minecraft-data).
    /// None = allocate dynamically during freeze() (for plugin blocks).
    pub fixed_runtime_range: Option<(u32, u32)>, // (min_state_id, max_state_id)
}

/// Mutable builder phase - accepts registrations.
///
/// Model A: Vanilla blocks keep their minecraft-data runtime IDs.
/// Plugin blocks are allocated sequentially after vanilla range.
pub struct RegistryBuilder {
    /// Vanilla blocks with fixed runtime IDs (registered first).
    vanilla_blocks: Vec<BlockSpec>,
    /// Plugin blocks needing dynamic allocation.
    plugin_blocks: Vec<BlockSpec>,
    /// String ID → tracking for duplicate detection.
    by_string_id: HashMap<String, ()>,
    /// Tracks the vanilla runtime ID ceiling for plugin allocation.
    vanilla_max_runtime_id: u32,
}

impl RegistryBuilder {
    pub fn new() -> Self {
        Self {
            vanilla_blocks: Vec::new(),
            plugin_blocks: Vec::new(),
            by_string_id: HashMap::new(),
            vanilla_max_runtime_id: 0,
        }
    }

    /// Register a vanilla block with fixed runtime IDs from minecraft-data.
    pub fn register_vanilla(&mut self, spec: BlockSpec) -> Result<(), RegistryError> {
        if self.by_string_id.contains_key(&spec.string_id) {
            return Err(RegistryError::DuplicateBlock(spec.string_id));
        }
        let (_, max_id) = spec.fixed_runtime_range
            .ok_or_else(|| RegistryError::InvalidSpec(
                format!("vanilla block {} must have fixed_runtime_range", spec.string_id)
            ))?;
        self.vanilla_max_runtime_id = self.vanilla_max_runtime_id.max(max_id + 1);
        self.by_string_id.insert(spec.string_id.clone(), ());
        self.vanilla_blocks.push(spec);
        Ok(())
    }

    /// Register a plugin block with dynamically allocated runtime IDs.
    pub fn register_plugin(&mut self, spec: BlockSpec) -> Result<(), RegistryError> {
        if self.by_string_id.contains_key(&spec.string_id) {
            return Err(RegistryError::DuplicateBlock(spec.string_id));
        }
        if spec.fixed_runtime_range.is_some() {
            return Err(RegistryError::InvalidSpec(
                format!("plugin block {} should not have fixed_runtime_range", spec.string_id)
            ));
        }
        self.by_string_id.insert(spec.string_id.clone(), ());
        self.plugin_blocks.push(spec);
        Ok(())
    }

    /// Number of total pending registrations.
    pub fn len(&self) -> usize {
        self.vanilla_blocks.len() + self.plugin_blocks.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.vanilla_blocks.is_empty() && self.plugin_blocks.is_empty()
    }

    /// Freeze the registry: finalize runtime IDs, build lookup tables.
    pub fn freeze(self) -> RegistrySnapshot {
        let total = self.vanilla_blocks.len() + self.plugin_blocks.len();
        let mut entries = Vec::with_capacity(total);
        let mut by_string_id = HashMap::with_capacity(total);
        let mut default_states = HashMap::with_capacity(total);

        // Phase 1: Vanilla blocks (fixed runtime IDs)
        for (idx, spec) in self.vanilla_blocks.into_iter().enumerate() {
            let id = BlockId::new(idx as u32);
            let (min_state_id, max_state_id) = spec.fixed_runtime_range.unwrap();
            let default_state_id = min_state_id + spec.default_state_offset;

            by_string_id.insert(spec.string_id.clone(), id);
            default_states.insert(spec.string_id.clone(), default_state_id);

            entries.push(BlockEntry {
                id,
                string_id: spec.string_id,
                name: spec.name,
                state_caps: spec.state_caps,
                role_caps: spec.role_caps,
                state_layout: spec.state_layout,
                behavior: spec.behavior,
                min_state_id,
                max_state_id,
                default_state_id,
            });
        }

        // Phase 2: Plugin blocks (dynamic allocation after vanilla)
        let mut next_runtime_id = self.vanilla_max_runtime_id;
        let vanilla_count = entries.len();

        for (idx, spec) in self.plugin_blocks.into_iter().enumerate() {
            let id = BlockId::new((vanilla_count + idx) as u32);
            let state_count = spec.state_layout.state_count.max(1);

            let min_state_id = next_runtime_id;
            let max_state_id = next_runtime_id + state_count - 1;
            let default_state_id = min_state_id + spec.default_state_offset.min(state_count - 1);

            next_runtime_id = max_state_id + 1;

            by_string_id.insert(spec.string_id.clone(), id);
            default_states.insert(spec.string_id.clone(), default_state_id);

            entries.push(BlockEntry {
                id,
                string_id: spec.string_id,
                name: spec.name,
                state_caps: spec.state_caps,
                role_caps: spec.role_caps,
                state_layout: spec.state_layout,
                behavior: spec.behavior,
                min_state_id,
                max_state_id,
                default_state_id,
            });
        }

        // Build runtime_to_id lookup (dense vec covering full range)
        let max_runtime_id = entries.iter()
            .map(|e| e.max_state_id)
            .max()
            .unwrap_or(0);

        let mut runtime_to_id = vec![None; (max_runtime_id + 1) as usize];
        for entry in &entries {
            for runtime_id in entry.min_state_id..=entry.max_state_id {
                runtime_to_id[runtime_id as usize] = Some(entry.id);
            }
        }

        RegistrySnapshot {
            entries,
            by_string_id,
            default_states,
            runtime_to_id,
        }
    }
}

impl Default for RegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable snapshot - used during gameplay.
pub struct RegistrySnapshot {
    entries: Vec<BlockEntry>,
    by_string_id: HashMap<String, BlockId>,
    default_states: HashMap<String, u32>,
    runtime_to_id: Vec<Option<BlockId>>,
}

impl RegistrySnapshot {
    /// Get entry by BlockId. O(1).
    #[inline]
    pub fn get(&self, id: BlockId) -> Option<&BlockEntry> {
        if !id.is_valid() {
            return None;
        }
        self.entries.get(id.0 as usize)
    }

    /// Get entry by string ID. O(1).
    #[inline]
    pub fn get_by_string(&self, string_id: &str) -> Option<&BlockEntry> {
        let id = self.by_string_id.get(string_id)?;
        self.get(*id)
    }

    /// Resolve string ID to BlockId. O(1).
    #[inline]
    pub fn get_id(&self, string_id: &str) -> Option<BlockId> {
        self.by_string_id.get(string_id).copied()
    }

    /// Get string ID from BlockId. O(1).
    #[inline]
    pub fn get_string_id(&self, id: BlockId) -> Option<&str> {
        self.get(id).map(|e| e.string_id.as_str())
    }

    /// Get default runtime ID for world gen. O(1).
    #[inline]
    pub fn get_default_state(&self, string_id: &str) -> Option<u32> {
        self.default_states.get(string_id).copied()
    }

    /// Resolve runtime ID to BlockDyn. O(1).
    #[inline]
    pub fn resolve(&self, runtime_id: u32) -> Option<BlockDyn<'_>> {
        let id = *self.runtime_to_id.get(runtime_id as usize)?.as_ref()?;
        let entry = self.entries.get(id.0 as usize)?;
        Some(BlockDyn::new(id, runtime_id, entry))
    }

    /// Iterator over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &BlockEntry> {
        self.entries.iter()
    }

    /// Number of registered blocks.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total runtime ID count.
    pub fn runtime_id_count(&self) -> usize {
        self.runtime_to_id.len()
    }
}
```

#### 2. Export from registry module
**File**: `crates/unastar/src/registry/mod.rs`
**Changes**: Add module export

```rust
pub mod block_v2;
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo check -p unastar` passes
- [ ] Vanilla blocks preserve minecraft-data runtime IDs (`fixed_runtime_range`)
- [ ] Plugin blocks allocated after vanilla range
- [ ] `runtime_to_id` uses `Option<BlockId>`, not defaulting to `BlockId(0)`
- [ ] `resolve()` returns `None` for invalid runtime IDs
- [ ] `register_vanilla()` requires `fixed_runtime_range`
- [ ] `register_plugin()` rejects `fixed_runtime_range`

---

## Phase 3: World Persistence Layer

### Overview
Implement block palette storage for handling plugin changes between saves.

### Changes Required

#### 1. Block Palette Module
**File**: `crates/unastar/src/world/block_palette.rs` (new file)

```rust
//! Block palette for world persistence.

use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use serde::{Deserialize, Serialize};
use crate::registry::block_v2::RegistrySnapshot;
use valentine_bedrock_core::block_v2::{BlockId, StateLayout, PropKind};

/// Stored in world metadata.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockPalette {
    pub entries: Vec<PaletteEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PaletteEntry {
    pub string_id: String,
    pub min_state_id: u32,
    pub max_state_id: u32,
    /// Hash of the state layout structure (property order, types, ranges).
    /// Used to detect if state meaning changed even if count stayed same.
    pub layout_hash: u64,
}

/// Compute a stable hash of a StateLayout for compatibility checking.
fn compute_layout_hash(layout: &StateLayout) -> u64 {
    let mut hasher = DefaultHasher::new();

    // Hash property count
    layout.props.len().hash(&mut hasher);

    // Hash each property in order (order matters for mixed-radix!)
    for prop in &layout.props {
        // Hash kind discriminant
        std::mem::discriminant(&prop.kind).hash(&mut hasher);

        // Hash the actual kind value for Custom props
        if let PropKind::Custom(hash) = prop.kind {
            hash.hash(&mut hasher);
        }

        // Hash stride and value_count
        prop.stride.hash(&mut hasher);
        prop.value_count.hash(&mut hasher);

        // Hash value_map discriminant (Bool, IntRange, Enum, Facing)
        std::mem::discriminant(&prop.value_map).hash(&mut hasher);
    }

    hasher.finish()
}

impl BlockPalette {
    /// Create palette from current registry.
    pub fn from_registry(registry: &RegistrySnapshot) -> Self {
        Self {
            entries: registry.iter().map(|e| PaletteEntry {
                string_id: e.string_id.clone(),
                min_state_id: e.min_state_id,
                max_state_id: e.max_state_id,
                layout_hash: compute_layout_hash(&e.state_layout),
            }).collect(),
        }
    }

    /// Create remapping table from saved palette to current registry.
    pub fn create_remap(&self, registry: &RegistrySnapshot) -> RuntimeIdRemap {
        let max_old_id = self.entries.iter()
            .map(|e| e.max_state_id)
            .max()
            .unwrap_or(0);

        let mut remap = vec![None; (max_old_id + 1) as usize];

        for old_entry in &self.entries {
            if let Some(new_entry) = registry.get_by_string(&old_entry.string_id) {
                let old_count = old_entry.max_state_id - old_entry.min_state_id + 1;
                let new_count = new_entry.max_state_id - new_entry.min_state_id + 1;
                let new_layout_hash = compute_layout_hash(&new_entry.state_layout);

                // Check BOTH state count AND layout hash for safe direct remap
                if old_count == new_count && old_entry.layout_hash == new_layout_hash {
                    // Safe direct remap - state structure identical
                    for offset in 0..old_count {
                        let old_id = old_entry.min_state_id + offset;
                        let new_id = new_entry.min_state_id + offset;
                        remap[old_id as usize] = Some(new_id);
                    }
                } else {
                    // State structure changed - use default state for safety
                    // This handles: count mismatch, property reordering, type changes
                    for offset in 0..old_count {
                        let old_id = old_entry.min_state_id + offset;
                        remap[old_id as usize] = Some(new_entry.default_state_id);
                    }
                }
            }
        }

        RuntimeIdRemap::new(remap)
    }
}

/// Runtime ID remapping table.
pub struct RuntimeIdRemap {
    table: Vec<Option<u32>>,
    /// True if any old_id maps to a different new_id (not identity mapping).
    has_changes: bool,
}

impl RuntimeIdRemap {
    /// Create a new remap table, computing whether changes exist.
    fn new(table: Vec<Option<u32>>) -> Self {
        // Check if any mapping is non-identity: old_id != new_id
        let has_changes = table.iter().enumerate().any(|(old_id, opt)| {
            match opt {
                Some(new_id) => *new_id != old_id as u32,
                None => true, // Missing block = needs remap (to air)
            }
        });
        Self { table, has_changes }
    }

    /// Remap a runtime ID. Returns fallback if missing.
    pub fn remap(&self, old_id: u32, fallback: u32) -> u32 {
        self.table.get(old_id as usize)
            .and_then(|opt| *opt)
            .unwrap_or(fallback)
    }

    /// Check if any remapping is needed (IDs changed or blocks missing).
    /// If false, the world can skip the remap pass entirely.
    pub fn needs_remap(&self) -> bool {
        self.has_changes
    }

    /// Check if a specific block is missing (no mapping exists).
    pub fn is_missing(&self, old_id: u32) -> bool {
        self.table.get(old_id as usize)
            .map(|opt| opt.is_none())
            .unwrap_or(true)
    }
}
```

#### 2. Export from world module
**File**: `crates/unastar/src/world/mod.rs`
**Changes**: Add module export

```rust
pub mod block_palette;
```

### Success Criteria

#### Automated Verification:
- [ ] Palette serializes/deserializes correctly
- [ ] Remap table handles missing blocks (returns fallback)
- [ ] Remap table handles state count changes (uses default)
- [ ] `needs_remap()` returns false when palette matches registry (identity mapping)
- [ ] `needs_remap()` returns true when IDs shifted or blocks missing
- [ ] `is_missing()` detects blocks not in current registry
- [ ] `layout_hash` stored in PaletteEntry
- [ ] Direct remap only when BOTH count AND layout_hash match
- [ ] Property reordering detected via layout_hash mismatch

---

## Phase 4: Generator Updates

### Overview
Update valentine_gen to produce typed block structs with mixed-radix encoding and proper capability assignment.

### Changes Required

#### 1. Update Block States Generator
**File**: `crates/valentine_gen/src/data_generator/block_states.rs`
**Changes**:
- Generate `StateLayout` with `PropLayout` entries
- Generate StateCaps from property names
- Emit RoleCaps from curated data file

#### 2. Role Caps Data File
**File**: `crates/valentine_gen/data/role_caps.json` (new file)

```json
{
  "minecraft:lever": ["RS_SOURCE", "RS_TOGGLE_ON_INTERACT"],
  "minecraft:stone_button": ["RS_SOURCE", "RS_TOGGLE_ON_INTERACT", "SCHEDULES_TICKS"],
  "minecraft:wooden_button": ["RS_SOURCE", "RS_TOGGLE_ON_INTERACT", "SCHEDULES_TICKS"],
  "minecraft:redstone_lamp": ["RS_CONSUMER"],
  "minecraft:piston": ["RS_CONSUMER", "NEEDS_NEIGHBOR_UPDATES"],
  "minecraft:sticky_piston": ["RS_CONSUMER", "NEEDS_NEIGHBOR_UPDATES"],
  "minecraft:redstone_wire": ["RS_CONDUCTOR"],
  "minecraft:unpowered_repeater": ["RS_CONDUCTOR", "RS_GATE", "SCHEDULES_TICKS"],
  "minecraft:powered_repeater": ["RS_CONDUCTOR", "RS_GATE", "SCHEDULES_TICKS"],
  "minecraft:observer": ["RS_SOURCE", "SCHEDULES_TICKS", "NEEDS_NEIGHBOR_UPDATES"]
}
```

### Success Criteria

#### Automated Verification:
- [ ] Generated code compiles
- [ ] StateLayout uses stride/value_count, not bit_offset/bit_width
- [ ] StateCaps are inferred from property names
- [ ] RoleCaps are loaded from data file

---

## Phase 5: Behavior Dispatch

### Overview
Implement behavior dispatch with `Arc<dyn BlockBehavior>`.

### Changes Required

#### 1. Behavior Module
**File**: `crates/unastar/src/server/game/behaviors/mod.rs` (new file)

```rust
//! Block behavior implementations.
//!
//! Uses static LazyLock to avoid Arc allocation on every lookup.

use std::sync::{Arc, LazyLock};
use valentine_bedrock_core::block_v2::{BlockBehavior, BlockEntry, InteractResult, PropKind};

mod lever;
mod redstone_lamp;

pub use lever::LeverBehavior;
pub use redstone_lamp::RedstoneLampBehavior;

// Static behavior singletons - allocated once, reused forever.
static LEVER_BEHAVIOR: LazyLock<Arc<dyn BlockBehavior>> =
    LazyLock::new(|| Arc::new(LeverBehavior));
static REDSTONE_LAMP_BEHAVIOR: LazyLock<Arc<dyn BlockBehavior>> =
    LazyLock::new(|| Arc::new(RedstoneLampBehavior));

/// Get behavior for a block by string ID.
/// Returns a clone of the static Arc (cheap reference count bump).
pub fn get_behavior(string_id: &str) -> Option<Arc<dyn BlockBehavior>> {
    match string_id {
        "minecraft:lever" => Some(Arc::clone(&*LEVER_BEHAVIOR)),
        "minecraft:redstone_lamp" => Some(Arc::clone(&*REDSTONE_LAMP_BEHAVIOR)),
        _ => None,
    }
}
```

#### 2. Lever Behavior
**File**: `crates/unastar/src/server/game/behaviors/lever.rs` (new file)

```rust
use valentine_bedrock_core::block_v2::*;

pub struct LeverBehavior;

impl BlockBehavior for LeverBehavior {
    fn on_interact(&self, runtime_id: u32, entry: &BlockEntry) -> (u32, InteractResult) {
        let offset = runtime_id - entry.min_state_id;

        // Toggle powered using mixed-radix
        let current_powered = entry.state_layout.decode_prop(offset, PropKind::Powered).unwrap_or(0);
        let new_powered = 1 - current_powered;
        let new_offset = entry.state_layout.with_prop(offset, PropKind::Powered, new_powered);
        let new_runtime_id = entry.min_state_id + new_offset;

        let power = if new_powered != 0 { 15 } else { 0 };
        (new_runtime_id, InteractResult::RedstoneUpdate { power })
    }
}
```

#### 3. Redstone Lamp Behavior
**File**: `crates/unastar/src/server/game/behaviors/redstone_lamp.rs` (new file)

```rust
use valentine_bedrock_core::block_v2::*;

pub struct RedstoneLampBehavior;

impl BlockBehavior for RedstoneLampBehavior {
    fn on_redstone(&self, runtime_id: u32, power: u8, entry: &BlockEntry) -> u32 {
        let offset = runtime_id - entry.min_state_id;

        let currently_lit = entry.state_layout.decode_prop(offset, PropKind::Lit).unwrap_or(0) != 0;
        let should_be_lit = power > 0;

        if currently_lit != should_be_lit {
            let new_offset = entry.state_layout.with_prop(offset, PropKind::Lit, if should_be_lit { 1 } else { 0 });
            entry.min_state_id + new_offset
        } else {
            runtime_id
        }
    }
}
```

### Success Criteria

#### Automated Verification:
- [ ] Behaviors use `Arc<dyn BlockBehavior>`
- [ ] Behaviors use mixed-radix decode/encode via StateLayout
- [ ] Static `LazyLock` singletons avoid repeated Arc allocations
- [ ] `get_behavior()` returns `Arc::clone()` not `Arc::new()`
- [ ] Unit tests pass

---

## Phase 6: Block Entities

### Overview

Block entities store persistent NBT data that is **NOT** part of the runtime ID. This is fundamentally different from block states:

| Aspect | Block State | Block Entity |
|--------|-------------|--------------|
| **Storage** | Runtime ID (u32) | NBT compound |
| **Affects** | Visual rendering, protocol | Internal data only |
| **Examples** | `facing_direction`, `powered_bit` | Sign text, chest contents, banner patterns |
| **Lifecycle** | Immutable per state | Mutable during gameplay |

### Examples: State vs Entity Data

**Banner** (from Dragonfly analysis):
- State properties (runtime ID): `facing_direction`, `ground_sign_direction`
- Entity data (NBT): `Patterns`, `Type` (illager), `Base` (color)

**Sign**:
- State properties: `facing_direction`, `ground_sign_direction`, `hanging`
- Entity data: Text lines, glow state, text color, locked state

**Chest**:
- State properties: `facing_direction`
- Entity data: Inventory contents (27 item slots), custom name, lock

**Command Block**:
- State properties: `facing_direction`, `conditional_bit`
- Entity data: Command text, output, track output, auto execute, tick delay

### Which Blocks Have Entities?

```rust
bitflags::bitflags! {
    /// Extended role capabilities for block entities.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct RoleCaps: u32 {
        // ... existing flags ...

        /// Block has associated block entity (NBT data).
        const HAS_BLOCK_ENTITY = 1 << 8;
        /// Block entity has inventory.
        const HAS_INVENTORY    = 1 << 9;
        /// Block entity has text content.
        const HAS_TEXT         = 1 << 10;
    }
}
```

Blocks with entities (non-exhaustive):
- **Containers**: Chest, Trapped Chest, Barrel, Shulker Box, Hopper, Dropper, Dispenser, Furnace variants, Brewing Stand
- **Signs**: All sign types, hanging signs
- **Banners**: All banner types
- **Heads**: All mob head types
- **Redstone**: Command Block, Structure Block, Jigsaw Block, Comparator (when reading container)
- **Functional**: Beacon, Enchanting Table, Lectern, Bell, Campfire, End Gateway, Spawner, Sculk Sensor/Shrieker/Catalyst
- **Beds**: All bed colors (store color in NBT on Bedrock)

### Block Entity Type System

```rust
/// Trait for block entity types.
pub trait BlockEntityDef: Send + Sync + 'static {
    /// Block entity type identifier (e.g., "minecraft:chest").
    const TYPE_ID: &'static str;

    /// Associated block(s) that use this entity type.
    /// Multiple blocks can share an entity type (e.g., all sign variants → "minecraft:sign").
    fn associated_blocks() -> &'static [&'static str];
}

/// Runtime block entity data.
pub struct BlockEntity {
    /// Position in world.
    pub pos: BlockPos,
    /// Type identifier.
    pub type_id: &'static str,
    /// NBT data (mutable during gameplay).
    pub nbt: NbtCompound,
}

/// Block entity storage per chunk (or chunk section).
/// Uses HashMap since entities are sparse - most blocks don't have them.
pub struct ChunkBlockEntities {
    /// Position within chunk → Block entity.
    entities: HashMap<LocalBlockPos, BlockEntity>,
}

impl ChunkBlockEntities {
    pub fn get(&self, pos: LocalBlockPos) -> Option<&BlockEntity> {
        self.entities.get(&pos)
    }

    pub fn get_mut(&mut self, pos: LocalBlockPos) -> Option<&mut BlockEntity> {
        self.entities.get_mut(&pos)
    }

    pub fn insert(&mut self, pos: LocalBlockPos, entity: BlockEntity) {
        self.entities.insert(pos, entity);
    }

    pub fn remove(&mut self, pos: LocalBlockPos) -> Option<BlockEntity> {
        self.entities.remove(&pos)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&LocalBlockPos, &BlockEntity)> {
        self.entities.iter()
    }
}
```

### Block Entity Behavior Integration

Block entities integrate with the behavior system for specific operations:

```rust
/// Extended behavior trait for blocks with entities.
pub trait BlockEntityBehavior: BlockBehavior {
    /// Create default entity data when block is placed.
    fn create_entity(&self, entry: &BlockEntry, state_id: u32) -> Option<NbtCompound>;

    /// Called when entity data is modified.
    fn on_entity_update(&self, entry: &BlockEntry, entity: &mut BlockEntity);

    /// Serialize entity for network/save.
    fn serialize_entity(&self, entity: &BlockEntity) -> NbtCompound {
        entity.nbt.clone()
    }

    /// Deserialize entity from network/load.
    fn deserialize_entity(&self, nbt: NbtCompound) -> Option<BlockEntity>;
}

/// Example: Sign behavior with entity support.
pub struct SignBehavior;

impl BlockBehavior for SignBehavior {
    fn on_interact(&self, runtime_id: u32, entry: &BlockEntry) -> (u32, InteractResult) {
        // Signs open text edit UI on interact
        (runtime_id, InteractResult::OpenBlockEntityUI)
    }
}

impl BlockEntityBehavior for SignBehavior {
    fn create_entity(&self, entry: &BlockEntry, _state_id: u32) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        nbt.insert("Text", NbtString::new(""));
        nbt.insert("TextOwner", NbtString::new(""));
        nbt.insert("IgnoreLighting", NbtByte::new(0));
        nbt.insert("SignTextColor", NbtInt::new(0)); // Black
        Some(nbt)
    }

    fn on_entity_update(&self, _entry: &BlockEntry, entity: &mut BlockEntity) {
        // Validate text length, filter profanity, etc.
    }
}
```

### World Integration

```rust
impl World {
    /// Get block at position (state only).
    pub fn get_block(&self, pos: BlockPos) -> Option<BlockDyn<'_>> {
        let chunk = self.get_chunk(pos.chunk())?;
        let runtime_id = chunk.get_block(pos.local());
        self.registry.resolve(runtime_id)
    }

    /// Get block entity at position (NBT data).
    pub fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity> {
        let chunk = self.get_chunk(pos.chunk())?;
        chunk.block_entities.get(pos.local())
    }

    /// Get mutable block entity.
    pub fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity> {
        let chunk = self.get_chunk_mut(pos.chunk())?;
        chunk.block_entities.get_mut(pos.local())
    }

    /// Set block with automatic entity handling.
    pub fn set_block(&mut self, pos: BlockPos, runtime_id: u32) {
        let chunk = self.get_chunk_mut(pos.chunk()).unwrap();
        let old_id = chunk.get_block(pos.local());

        // Remove old entity if block changed
        if let Some(old_block) = self.registry.resolve(old_id) {
            if old_block.has_role(RoleCaps::HAS_BLOCK_ENTITY) {
                chunk.block_entities.remove(pos.local());
            }
        }

        // Set new block state
        chunk.set_block(pos.local(), runtime_id);

        // Create new entity if needed
        if let Some(new_block) = self.registry.resolve(runtime_id) {
            if new_block.has_role(RoleCaps::HAS_BLOCK_ENTITY) {
                if let Some(behavior) = new_block.entry().behavior.as_ref() {
                    if let Some(entity_behavior) = behavior.as_any().downcast_ref::<dyn BlockEntityBehavior>() {
                        if let Some(nbt) = entity_behavior.create_entity(new_block.entry(), runtime_id) {
                            chunk.block_entities.insert(pos.local(), BlockEntity {
                                pos,
                                type_id: get_entity_type_for_block(new_block.string_id()),
                                nbt,
                            });
                        }
                    }
                }
            }
        }
    }
}
```

### Protocol: Block Entity Packets

Bedrock uses specific packets for block entity updates:

```rust
/// PacketBlockEntityData - sent when entity data changes.
pub struct PacketBlockEntityData {
    pub pos: BlockPos,
    pub nbt: NbtCompound,
}

/// Sent after chunk to sync entities.
/// The client expects BlockEntityData packets for all entities in view.
fn send_chunk_block_entities(player: &Player, chunk: &Chunk) {
    for (local_pos, entity) in chunk.block_entities.iter() {
        let world_pos = chunk.pos.to_world_pos(local_pos);
        player.send(PacketBlockEntityData {
            pos: world_pos,
            nbt: entity.nbt.clone(),
        });
    }
}
```

### Persistence: Entity Serialization

Block entities are saved alongside chunk data:

```rust
/// Chunk save format includes block entities.
#[derive(Serialize, Deserialize)]
pub struct ChunkSaveData {
    pub sections: Vec<ChunkSection>,
    pub block_entities: Vec<SavedBlockEntity>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedBlockEntity {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub type_id: String,
    pub nbt: NbtCompound,
}
```

### ECS Integration (Future)

Block entities map naturally to ECS for complex behaviors:

```rust
/// Sparse component for blocks with entities.
/// Stored as an ECS component, keyed by BlockPos.
#[derive(Component)]
pub struct HasBlockEntity {
    pub entity_id: Entity, // ECS entity holding the data
}

/// Individual entity data components.
#[derive(Component)]
pub struct SignData {
    pub lines: [String; 4],
    pub color: TextColor,
    pub glowing: bool,
}

#[derive(Component)]
pub struct ChestInventory {
    pub slots: [Option<ItemStack>; 27],
    pub custom_name: Option<String>,
}

#[derive(Component)]
pub struct BannerData {
    pub base_color: DyeColor,
    pub patterns: Vec<BannerPattern>,
}
```

### Success Criteria

#### Automated Verification:
- [ ] `RoleCaps::HAS_BLOCK_ENTITY` flag exists
- [ ] `BlockEntity` struct with pos, type_id, nbt
- [ ] `ChunkBlockEntities` sparse storage
- [ ] `BlockEntityBehavior` trait extends `BlockBehavior`
- [ ] Entity creation/removal on block place/break

#### Manual Verification:
- [ ] Place sign, add text, verify it persists
- [ ] Break chest, verify contents drop
- [ ] Save/load world with entities intact

---

## Testing Strategy

### Unit Tests
- `BlockId::INVALID` sentinel behavior
- Mixed-radix encode/decode round-trip
- StateCaps inference from property names
- RoleCaps loading from data file
- `RegistryBuilder::freeze()` assigns sequential IDs
- `runtime_to_id` returns `None` for invalid IDs
- Block palette remap handles missing/changed blocks

### Integration Tests
- Full registration → freeze → resolve flow
- World generator string lookups
- Behavior dispatch
- Palette save/load/remap

### Manual Testing
1. Place lever, interact, verify toggle
2. Place redstone lamp near lever, verify it lights
3. Add plugin block, save world, remove plugin, load world
4. Verify world gen works with new registry

## Performance Considerations

- String ID → BlockId: O(1) HashMap lookup
- BlockId → BlockEntry: O(1) Vec index
- Runtime ID → BlockDyn: O(1) Vec index (with Option check)
- State decode: O(n) prop search, but n is small (typically <5)
- `BlockDyn` carries reference, not owned data
- `Arc<dyn BlockBehavior>` adds one indirection vs `&'static`
- No allocations in hot paths

## Compile Time Considerations

With ~1000 blocks, each generating a struct + trait impls:
- Consider feature flags to split into groups
- Use incremental compilation
- Profile build times after implementation
- Could lazy-generate less common blocks

## Migration Notes

- V2 types coexist with V1 during transition
- Gradually move systems to use `BlockDyn`
- World generator continues to work unchanged
- Once fully migrated, deprecate V1 types
- World saves require palette for compatibility

## References

- Original research: [2026-01-04-block-item-registry-extensibility.md](../research/2026-01-04-block-item-registry-extensibility.md)
- Current block traits: [bedrock_core/src/block.rs](../../crates/valentine/bedrock_core/src/block.rs)
- Current registry: [unastar/src/registry/block.rs](../../crates/unastar/src/registry/block.rs)
- State generator: [valentine_gen/src/data_generator/block_states.rs](../../crates/valentine_gen/src/data_generator/block_states.rs)
- World gen block lookup: [world/chunk.rs:54-69](../../crates/unastar/src/world/chunk.rs#L54-L69)
