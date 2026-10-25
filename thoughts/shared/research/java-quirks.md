# Java Edition worldgen quirks and approximations (Java-style generator)

Behaviour of Java Edition 1.21.11 world generation that unastar copies on
purpose because it looks odd, and the places where unastar knowingly
approximates Java. Sources: the public 1.21.11 worldgen data
(`crates/unastar_noise/worldgen_data`), behaviour observed in Java reference
worlds, and open-source projects (cubiomes, MIT). Bedrock quirks live in
`bds-quirks.md`.

## Copied quirks

| Area | Java behaviour | Where |
|---|---|---|
| Decoration random | `WorldgenRandom` over Xoroshiro draws through `next(bits)` = top bits of the 64-bit output, so `nextInt`/`nextFloat` use the `java.util.Random` formulas, not the Xoroshiro-native ones. | `features/random.rs` |
| Feature seeds | Each placed feature is reseeded with `decorationSeed + index + 10000 * step`, where `index` is the feature's position in the dimension-wide step list built by a topological sort over every biome's lists (`FeatureSorter`), not its position in the chunk's biome. | `features/decorate.rs`, `codegen/emitter/feature.rs` |
| Structure seeds | Structures of a step are reseeded with the same formula using the structure's index among the structures of that step (registry order), before that step's features. | `structures/manager.rs` |
| `Mth.getSeed` | `x * 3129871` is multiplied in 32-bit `int` arithmetic and wraps before being widened; `z` is multiplied as a long. | `structures/util.rs` |
| Default frequency reduction | The default reducer calls `setLargeFeatureWithSalt(seed, salt, x, z)`, so the salt sits in the region-x slot and `z` in the salt slot. | `structures/placement.rs` |
| Count on every layer | All positions are collected (re-sampling the count on every loop iteration) before any is placed. | `codegen/emitter/feature.rs` |
| Template palettes | `getRandomPalette` always draws `nextInt(paletteCount)`, even for one palette; with a shared placement random (fossils) that draw shifts every later draw. | `structures/template.rs` |
| Template order | Template blocks are processed full-cube blocks first, then other blocks, then blocks with block entities, each sorted by (y, x, z). Processors that draw randomness see that order. | `codegen/templates.rs` |
| Spring fluid | `spring_feature.state` is a fluid state; the placed block is its legacy block (source = level 0). | `codegen/emitter/feature.rs` |

## Approximations

| Area | Java | unastar | Effect |
|---|---|---|---|
| Light during decoration | Light is not computed until after features; Java reads the (missing) light data. | Mushroom survival treats positions at or above `WORLD_SURFACE` as sky-lit (15), others as dark; snow/ice block light is 0. | Mushroom placement next to overhangs may differ. |
| `isSolidRender` | Occlusion shapes per state. | Full-cube collision minus see-through families (leaves, glass, ice, ...). | Chest orientation, tree `place_on_ground`, mushroom support. |
| Solid flag | `BlockBehaviour` collision bounds rule plus per-block force flags. | Collision bounds rule from collision shapes; signs, banners and pressure plates forced solid. | Other force flags are not modelled. |
| Neighbour shape updates | Template structures without a known shape update connection states (fences, panes, walls) at their edges. | Not reproduced; jigsaw pieces use a known shape in Java too. | Edge connection states of legacy template structures. |
| Cross-chunk order | A chunk's decoration sees neighbours decorated earlier, in load order. | Each origin reads the undecorated 3x3 protos plus its own writes; writes are merged in a fixed order. | Overlapping features at chunk borders. |
| Overlapping starts | Starts of one structure referencing a chunk are iterated in hash-set order. | Iterated by source chunk (x, then z). | Only when two starts of the same structure overlap one chunk. |
| Block entities and entities | Loaded or spawned during generation. | Block entity data (loot tables + seeds, spawner mobs, template NBT) is handed to the runtime; entities are not spawned yet. RNG draws are kept. | Chest contents and mobs depend on the runtime. |
