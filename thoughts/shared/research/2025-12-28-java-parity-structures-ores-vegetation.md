---
date: 2025-12-28T12:00:00-05:00
researcher: Claude
git_commit: 8b6fde79c9feccea820d9dc6c05978cc59702601
branch: main
repository: axolotl-stack
topic: "Java Edition vs Unastar: Structures, Ores, and Vegetation Parity Analysis"
tags: [research, world-generation, structures, ores, vegetation, java-parity]
status: complete
last_updated: 2025-12-28
last_updated_by: Claude
---

# Research: Java Edition vs Unastar - Structures, Ores, and Vegetation Parity

**Date**: 2025-12-28T12:00:00-05:00
**Researcher**: Claude
**Git Commit**: 8b6fde79c9feccea820d9dc6c05978cc59702601
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Compare java-ed-world's method of doing structures, ores, vegetation/trees to unastar's generator. The hypothesis is that the current implementation uses an old 1.8-style approach rather than modern 1.18+ methods.

## Summary

**The hypothesis is confirmed.** Unastar's current generator uses direct procedural generation reminiscent of pre-1.13 Minecraft, while Java Edition 1.18+ uses a sophisticated data-driven feature system with:

1. **ConfiguredFeature/PlacedFeature** - Separates feature logic from placement logic
2. **Jigsaw Structure System** - Template-based structures with procedural assembly
3. **PlacementModifier Chain** - Composable position transformation pipeline
4. **HeightProvider System** - Sophisticated height distributions (uniform, trapezoid, biased)
5. **OreVeinifier** - Density function-based large ore veins (1.18+)
6. **Terrain Adaptation** - Structures modify terrain density (Beardifier)

Unastar currently has:
- Hardcoded geometric structures (no templates)
- Simple noise-threshold ore placement (no vein shapes)
- Direct tree/vegetation placement during chunk gen (no feature pipeline)

## Detailed Findings

### 1. Structure Generation

#### Java Edition Modern Approach

**Architecture Overview:**
- `Structure` base class with `StructureSettings` (biomes, spawn overrides, terrain adaptation)
- `StructureSet` groups structures with `StructurePlacement` rules
- `StructureStart` contains `StructurePiece` collection
- Jigsaw system assembles structures from template pools

**Jigsaw Structure System** (java-ed-world/level/levelgen/structure/structures/JigsawStructure.java):
```java
// Configuration includes:
- startPool: StructureTemplatePool (initial templates)
- maxDepth: 0-20 (assembly recursion limit)
- startHeight: HeightProvider
- poolAliases: Template substitutions
- maxDistanceFromCenter: Size constraints
```

**JigsawPlacement Algorithm** (java-ed-world/level/levelgen/structure/pools/JigsawPlacement.java):
1. Select random template from start pool
2. Find jigsaw block attachments
3. For each jigsaw, lookup target pool
4. Try templates from pool with rotations
5. Check collisions with existing pieces
6. Create junction connections
7. Queue children for recursive placement
8. Uses priority queue for placement order

**Structure Placement Types:**
- `RandomSpreadStructurePlacement`: Grid-based with spacing/separation
- `ConcentricRingsStructurePlacement`: Ring patterns (strongholds)

**Terrain Adaptation** (java-ed-world/level/levelgen/Beardifier.java):
- Modifies terrain density around structures
- 24x24x24 pre-computed kernel for smooth blending
- TerrainAdjustment enum: NONE, BURY, BEARD_THIN, BEARD_BOX, ENCAPSULATE

**Generation Pipeline:**
1. STRUCTURE_STARTS: Calculate positions, create StructureStart
2. STRUCTURE_REFERENCES: Link neighboring chunks
3. NOISE: Beardifier modifies density
4. FEATURES: StructureStart.placeInChunk() places blocks

#### Unastar Current Approach

**Location**: [terrain.rs:1609-1698](crates/unastar/src/world/generator/terrain.rs#L1609-L1698)

**Implementation:**
- Uses cubiomes-accurate positioning algorithms (structures.rs)
- Region-grid based position calculation
- Hardcoded geometric structures (no templates)

**Structure Types Implemented:**
```rust
// terrain.rs:1700-1937
- Village Well: 5x5 cobblestone platform, water pool
- Desert Pyramid: 9x9 stepped sandstone pyramid
- Swamp Hut: 5x5 spruce plank hut on stilts
- Igloo: Snow dome with hollow interior
- Jungle Temple: 7x7 cobblestone with vines
```

**Key Differences:**

| Aspect | Java Edition 1.18+ | Unastar Current |
|--------|-------------------|-----------------|
| Template System | NBT templates + Jigsaw assembly | Hardcoded geometry |
| Structure Pieces | Modular, composable pieces | Single monolithic placement |
| Terrain Adaptation | Beardifier density modification | None |
| Placement Rules | Data-driven StructurePlacement | Direct region calculation |
| Village Generation | Complex jigsaw with roads/buildings | Simple well only |
| Structure Processors | Block replacement rules | None |

### 2. Ore Generation

#### Java Edition Modern Approach

**Feature System** (java-ed-world/level/levelgen/feature/):

**OreFeature** (OreFeature.java:23-168):
- Ellipsoid vein generation
- Calculates spherical segments
- Distance-based block placement: `dx² + dy² + dz² < 1.0`
- Air exposure filtering (discardChanceOnAirExposure)

**OreConfiguration** (OreConfiguration.java):
```java
- targetStates: List<TargetBlockState> (multiple ore variants)
- size: 0-64 blocks per vein
- discardChanceOnAirExposure: 0.0-1.0
```

**ScatteredOreFeature** (ScatteredOreFeature.java):
- Individual block placement
- Triangular distribution offset
- MAX_DIST_FROM_ORIGIN = 7 blocks

**Height Distribution System:**

| HeightProvider Type | Algorithm |
|--------------------|-----------|
| UniformHeight | Equal probability across range |
| TrapezoidHeight | Linear ramps with optional plateau |
| BiasedToBottomHeight | Double random favoring bottom |
| VeryBiasedToBottomHeight | Triple random for extreme bottom bias |

**VerticalAnchor Types:**
- `absolute(y)`: Fixed world Y
- `aboveBottom(offset)`: Relative to min world height
- `belowTop(offset)`: Relative to max world height

**PlacementModifier Chain:**
```
CountPlacement (repeat N times)
    → InSquarePlacement (spread within chunk)
    → HeightRangePlacement (select Y with HeightProvider)
    → BiomeFilter (validate biome)
    → OreFeature.place()
```

**OreVeinifier** (1.18+ Large Veins) (OreVeinifier.java):
- Density function-based ore veins
- COPPER: Y 0-50, granite filler
- IRON: Y -60 to -8, tuff filler
- 2% raw ore block chance
- 70% vein solidness threshold

#### Unastar Current Approach

**Location**: [terrain.rs:939-1123](crates/unastar/src/world/generator/terrain.rs#L939-L1123)

**Implementation:**
- SIMD-optimized noise sampling (4 X positions at once)
- Three independent noise layers
- Simple threshold-based placement
- No vein shapes - individual block replacement

**Ore Distribution:**
```rust
// Height ranges and noise thresholds
Coal:     Y 5-128,   n1 > 0.75 - (y/300)
Iron:     Y -60-64,  n2 > 0.78
Copper:   Y -16-112, n1 < -0.78 && n3 > 0.3
Gold:     Y -60-32,  n3 > 0.85
Redstone: Y -60-16,  n2 < -0.78
Lapis:    Y -60-64,  n3 < -0.88 && hash > 0.7
Diamond:  Y -60-16,  n1 > 0.92 && n2 > 0.5
Emerald:  Y -16-100, n1 > 0.95 && n2 < -0.5 && n3 > 0.7 (mountains only)
```

**Key Differences:**

| Aspect | Java Edition 1.18+ | Unastar Current |
|--------|-------------------|-----------------|
| Vein Shape | Ellipsoid/scattered | Individual blocks |
| Configuration | Data-driven JSON | Hardcoded thresholds |
| Height Distribution | HeightProvider (trapezoid, biased) | Linear noise threshold |
| Placement Pipeline | PlacementModifier chain | Direct noise check |
| Large Ore Veins | OreVeinifier density function | Not implemented |
| Air Exposure | discardChanceOnAirExposure | Not implemented |
| Multi-Target | RuleTest matching | Single ore type per check |

### 3. Vegetation and Tree Generation

#### Java Edition Modern Approach

**Tree Architecture:**

**TreeConfiguration** (TreeConfiguration.java):
```java
- trunkProvider: BlockStateProvider
- trunkPlacer: TrunkPlacer (algorithm)
- foliageProvider: BlockStateProvider
- foliagePlacer: FoliagePlacer (algorithm)
- rootPlacer: Optional<RootPlacer>
- decorators: List<TreeDecorator>
- minimumSize: FeatureSize
```

**TrunkPlacer Types:**
- `StraightTrunkPlacer`: Vertical trunk, single foliage point
- `FancyTrunkPlacer`: Branching with 0.618 golden ratio shape
- `DarkOakTrunkPlacer`: 2x2 trunk with direction offset
- `BendingTrunkPlacer`: Curved trunk (cherry trees)
- `UpwardsBranchingTrunkPlacer`: Multiple branches

**FoliagePlacer Types:**
- `BlobFoliagePlacer`: Spherical with corner randomization
- `PineFoliagePlacer`: Conical spruce shape
- `MegaPineFoliagePlacer`: Large spruce
- `AcaciaFoliagePlacer`: Flat canopy
- `DarkOakFoliagePlacer`: Wide canopy
- `FancyFoliagePlacer`: Layered oak

**TreeDecorator Types:**
- `BeehiveDecorator`: Adds bee nests (probability-based)
- `AlterGroundDecorator`: Places podzol
- `LeavesVineDecorator`: Adds vines to leaves
- `TrunkVineDecorator`: Adds vines to trunk
- `CocoaDecorator`: Adds cocoa pods

**Vegetation Features:**

**RandomPatchFeature** (RandomPatchFeature.java):
```java
- tries: 128 (placement attempts)
- xzSpread: 7 (horizontal spread)
- ySpread: 3 (vertical spread)
- feature: PlacedFeature to place
```

**VegetationPatchFeature** (VegetationPatchFeature.java):
- Ground replacement + vegetation on top
- Edge probability handling
- Surface detection (up/down direction)

**Placement Pipeline:**
```
ConfiguredFeature (Feature + FeatureConfiguration)
    → PlacedFeature (ConfiguredFeature + List<PlacementModifier>)
    → BiomeGenerationSettings.features (per decoration step)
```

**GenerationStep.Decoration Order:**
1. RAW_GENERATION
2. LAKES
3. LOCAL_MODIFICATIONS
4. UNDERGROUND_STRUCTURES
5. SURFACE_STRUCTURES
6. STRONGHOLDS
7. UNDERGROUND_ORES
8. UNDERGROUND_DECORATION
9. FLUID_SPRINGS
10. **VEGETAL_DECORATION** (trees, flowers, grass)
11. TOP_LAYER_MODIFICATION

**Sapling/Bonemeal System:**
- `TreeGrower` handles sapling growth
- References ConfiguredFeatures
- 2x2 mega tree detection
- Flower proximity for bee variants

#### Unastar Current Approach

**Location**: [terrain.rs:296-831](crates/unastar/src/world/generator/terrain.rs#L296-L831)

**Tree Generation** (terrain.rs:296-368):
- Perlin noise at 0.08 frequency
- 5-block step spacing for performance
- Biome-specific density thresholds
- Direct tree placement functions

**Tree Types:**
```rust
// All use hardcoded algorithms
Oak:      Height 4-6, 4-layer leaves (radius 2→1)
Birch:    Height 5-7, same as oak with birch blocks
Spruce:   Height 6-9, conical with oscillating radius
Jungle:   Height 8, large 2-radius canopy
Dark Oak: Height 5, very wide canopy (radius 3)
Acacia:   Height 5, flat 2-tier canopy
Swamp:    Height 5, oak + hanging vines
```

**Vegetation Generation** (terrain.rs:728-831):
- Low-frequency noise (0.02) for patches
- Type noise (0.06) for variety
- Biome-specific flower/grass selection

**Vegetation Types:**
```rust
FlowerForest: 6 flower types based on noise
Meadow:       Flowers + grass mix
Plains/Forest: Mostly grass, rare flowers
DarkForest:   Mushroom clusters
Swamp:        Lily pads at water level
Taiga:        Ferns and grass
```

**Key Differences:**

| Aspect | Java Edition 1.18+ | Unastar Current |
|--------|-------------------|-----------------|
| Tree Architecture | Composable Placer components | Monolithic functions |
| Tree Decorators | Beehives, vines, ground alter | Swamp vines only |
| Configuration | Data-driven TreeConfiguration | Hardcoded algorithms |
| Fancy Trees | FancyTrunkPlacer with branches | Not implemented |
| Mega Trees | 2x2 trunk variants | Not implemented |
| Root System | MangroveRootPlacer | Not implemented |
| Feature Pipeline | ConfiguredFeature → PlacedFeature | Direct placement |
| Decoration Steps | 11 ordered steps | Single pass |
| Bonemeal | TreeGrower + ConfiguredFeature | Not implemented |

## Architecture Comparison

### Java Edition 1.18+ Pipeline

```
World Seed
    ↓
BiomeSource.getNoiseBiome() → Biome
    ↓
BiomeGenerationSettings
    ├── carvers: HolderSet<ConfiguredWorldCarver>
    └── features: List<HolderSet<PlacedFeature>> (per step)
    ↓
ChunkGenerator.applyBiomeDecoration()
    ↓
For each GenerationStep.Decoration:
    ├── Place structure pieces
    └── For each PlacedFeature:
        ├── Apply PlacementModifier chain
        └── ConfiguredFeature.place()
```

### Unastar Current Pipeline

```
World Seed
    ↓
generate_chunk()
    ├── 3D Density (NoiseChunk + Aquifer)
    ├── Surface Rules
    ├── Stone Variants (SIMD)
    ├── Ores (SIMD)
    ├── Caves/Ravines
    ├── Trees (if no structure)
    ├── Vegetation
    └── Structures (hardcoded geometry)
```

## Code References

### Java Edition Key Files

**Structures:**
- `java-ed-world/level/levelgen/structure/Structure.java` - Base class
- `java-ed-world/level/levelgen/structure/structures/JigsawStructure.java` - Template assembly
- `java-ed-world/level/levelgen/structure/pools/JigsawPlacement.java` - Assembly algorithm
- `java-ed-world/level/levelgen/Beardifier.java` - Terrain adaptation

**Ores:**
- `java-ed-world/level/levelgen/feature/OreFeature.java` - Ellipsoid veins
- `java-ed-world/level/levelgen/feature/configurations/OreConfiguration.java` - Config
- `java-ed-world/level/levelgen/heightproviders/TrapezoidHeight.java` - Height distribution
- `java-ed-world/level/levelgen/OreVeinifier.java` - Large ore veins

**Vegetation:**
- `java-ed-world/level/levelgen/feature/TreeFeature.java` - Tree generation
- `java-ed-world/level/levelgen/feature/trunkplacers/` - Trunk algorithms
- `java-ed-world/level/levelgen/feature/foliageplacers/` - Leaf algorithms
- `java-ed-world/level/levelgen/feature/RandomPatchFeature.java` - Scattered vegetation

**Feature System:**
- `java-ed-world/level/levelgen/feature/ConfiguredFeature.java` - Feature + config
- `java-ed-world/level/levelgen/placement/PlacedFeature.java` - Feature + placement
- `java-ed-world/level/levelgen/placement/PlacementModifier.java` - Position transforms

### Unastar Key Files

- `crates/unastar/src/world/generator/terrain.rs` - All generation logic
- `crates/unastar/src/world/generator/structures.rs` - Position algorithms
- `crates/unastar/src/world/generator/surface/` - Surface rules

## What Would Full Parity Require

### Structures

1. **Template Loading System**
   - NBT structure template parser
   - StructureTemplateManager with caching
   - Processor application (block replacement rules)

2. **Jigsaw Assembly Engine**
   - StructureTemplatePool registry
   - Jigsaw block detection and matching
   - Rotation/mirror transformation
   - Collision detection between pieces
   - Priority queue for placement order

3. **Placement System**
   - StructureSet registry
   - StructurePlacement implementations (RandomSpread, ConcentricRings)
   - Exclusion zone handling
   - Frequency reduction

4. **Terrain Adaptation**
   - Beardifier integration with density functions
   - Pre-computed kernel for smooth blending
   - TerrainAdjustment enum support

### Ores

1. **Feature System**
   - ConfiguredFeature/PlacedFeature records
   - Feature registry
   - PlacementModifier chain

2. **Ore Features**
   - OreFeature with ellipsoid vein generation
   - ScatteredOreFeature for scattered placement
   - Air exposure handling

3. **Height Providers**
   - UniformHeight, TrapezoidHeight
   - BiasedToBottomHeight, VeryBiasedToBottomHeight
   - VerticalAnchor (absolute, aboveBottom, belowTop)

4. **Large Ore Veins**
   - OreVeinifier density function integration
   - Copper and iron vein types
   - Filler block placement (granite, tuff)

### Vegetation

1. **Tree Components**
   - TrunkPlacer trait + implementations
   - FoliagePlacer trait + implementations
   - RootPlacer trait + implementations
   - TreeDecorator trait + implementations

2. **TreeConfiguration**
   - BlockStateProvider for blocks
   - Component composition
   - FeatureSize constraints

3. **Vegetation Features**
   - RandomPatchFeature with retry logic
   - VegetationPatchFeature with ground replacement
   - SimpleBlockFeature for single blocks

4. **Decoration Pipeline**
   - GenerationStep.Decoration ordering
   - Per-biome feature lists
   - Decoration random seeding

## Open Questions

1. **Performance Trade-offs**: How does the data-driven feature system affect generation performance compared to direct procedural generation?

2. **Data Format**: Should ore/tree configurations use JSON like Java Edition, or a Rust-native format?

3. **Structure Templates**: How to handle NBT template loading for Bedrock-compatible structures?

4. **Biome Integration**: How should the feature lists integrate with the existing biome system?

5. **Incremental Migration**: What's the priority order for implementing these systems?

## Related Research

- [thoughts/shared/plans/2025-12-28-density-function-simd.md](thoughts/shared/plans/2025-12-28-density-function-simd.md) - Density function optimization
- [thoughts/shared/plans/2025-12-28-java-parity-density-aquifer-surface.md](thoughts/shared/plans/2025-12-28-java-parity-density-aquifer-surface.md) - Related parity plan
