---
date: 2026-10-24
author: VANILLA-ITEMS worker
target: BDS 1.26.51 (protocol 2193)
status: research (no implementation); the decisions asked for in §6 were made by the owner as D1-D5 (`thoughts/shared/plans/2026-10-24-data-driven-decisions.md`); cross-area contracts in `thoughts/shared/plans/2026-10-24-data-driven-architecture.md`
scope: item components, interactable blocks and block entities, recipes, loot tables, ItemStackRequest validation
---

# Data-driven items, containers, recipes and loot: how BDS does it and how unastar should

## How to read this

**Behaviour statements.** These describe vanilla BDS 1.26.x behaviour. Where a statement can be checked against public or measured data, that source is cited:

| Tag | Source |
|---|---|
| **(N)** | Native BDS 1.26.51 measurement tables, CC0: `crates/valentine_gen/protocolgen/data/source/inputs/1.26.51/native/` |
| **(R)** | protocolgen's captured `ItemRegistryPacket` payload: `protocolgen/generated/data/registry/items.nbt` |
| **(P40)** | protocolgen's BDS 1.26.40 export: `protocolgen/generated/1.26.40/vanilla-data/` (`recipes.json`, `crafting_data.dat`) |
| **(BP)** | The bedrock-samples behaviour pack and its metadata/JSON schemas. Counts in this document were taken on the old pin 1.21.130.3; D1 has since bumped main to 1.26.50.4 |
| **(BDSpk)** | Pack files shipped in the BDS 1.26.51 install (`behavior_packs/vanilla*/__brarchive/*.brarchive`). Read as data, never copied. |
| **(V)** | Generated protocol types: `crates/valentine/bedrock_versions/v1_26_51/src/{proto,types}.rs` |
| **[dragonfly]** | df-mc/dragonfly (MIT) |
| **[gophertunnel]** | The pinned gophertunnel fork |
| **[PMMP]** | PMMP BedrockData 1.26.0 (CC0) |
| **[capture]** | `tools/client-diff` captures and `jolyne/src/proto/testdata/bds-1.26.51-join-s2c.txt` |

Statements with no tag state vanilla behaviour without a public source. They must be confirmed by a capture or a BDS measurement before shipping; §4.6 lists the fixtures.

**Data kinds.**

| Tag | Meaning |
|---|---|
| **(a)** | Data-driven from pack JSON |
| **(b)** | Native, but measurable as a table |
| **(c)** | Hardcoded behaviour that unastar must write in Rust |

**Evidence notes.** The detailed notes are outside the repo; see the appendix.

## TL;DR

1. **Items are mostly code, not data.**
   - Decoding (R) splits the 2076 vanilla items into four groups:

     | Count | Group | Examples |
     |---|---|---|
     | 1953 | Pure code items: no JSON, empty component NBT | |
     | 44 | Component-based items whose components are attached in code | coal fuel, arrows, music discs, block planters |
     | 48 | Code items augmented by a legacy JSON (`format_version` < 1.16.100) | bread, meats, seeds, golden apples, camera |
     | 31 | Fully JSON-defined component items | apple, 17 bundles, 7 spears, wind_charge, trial keys, breeze_rod, red_shrub, shelf_mushroom |

   - "Data-driven items" therefore means three things:
     - send the component NBT BDS sends (78 items);
     - implement each item component **once**, generically;
     - write about 40 vanilla item classes in Rust (bow, bucket, armour, tools, potions, …), fed by tables.
2. **Recipes are fully data-driven (a).**
   - Every recipe type is loaded from pack JSON (BP, BDSpk), including furnace recipes, which are one-ingredient recipes tagged `furnace`/`blast_furnace`/`smoker`/`campfire`.
   - The 26.51 `CraftingDataPacket` has no furnace list (V), and (P40) shows `furnace: null`.
   - Code (c) covers only the 14 "multi" recipes, the shulker-box/dye user-data recipes, the matching algorithms and the fuel rules.
3. **Loot tables are fully data-driven (a); block drops mostly are not (c).**
   - Loot tables have 26 functions and 20 conditions.
   - Most block drops are a built-in rule set. Only 99 blocks use the `loot_tables/blocks/*.json` that ship in the 1.26.50 pack (BDSpk).
4. **Containers and workstations are built-in block behaviours (c).**
   - The block→block-entity mapping is native and measurable (b): it is the `blockEntityType` field of (N), already in `blocks.kdl` but dropped by codegen.
   - The only data-driven interaction component is `minecraft:crafting_table`, for custom blocks (BP schema).
5. **ItemStackRequest validation is a generic engine plus two static tables.**
   - The tables are a validator per `ContainerEnumName` and an allowed-container set per screen `ContainerType`.
   - Item-side checks read item data: `max_stack_size`, `stacked_by_data`, `wearable.slot`, `allow_off_hand`, fuel, `storage_item`, `item_lock`, tags.
   - The rule set is in §2.6.
6. **The pinned vanilla pack (1.21.130.3) was stale for 26.51** (resolved by D1: main reads bedrock-samples 1.26.50.4). Compared with BDSpk, the old pin had:

   | Family | Missing from the pin | Differing |
   |---|---|---|
   | Recipes | 236 | 154 |
   | Loot tables | 102 (99 of them block tables) | 6 |
   | Items | 2 | `apple` uses an older format |

7. **Four wire-format defects** were found and need owners before this is built (§3.4):
   - UUID byte order;
   - ContainerType ordinals;
   - empty ItemRegistry component NBT;
   - a CraftRecipeAuto layout check.

---

## 1. Current unastar state (items, blocks-for-interaction, recipes, loot, inventory)

### 1.1 Pipeline families

| Family | BDS data source | unastar datagen (`crates/unastar-data`) | Runtime consumer |
|---|---|---|---|
| Item identity and stack size | (N) `item_properties_table.json`, `item_tags.json`; (R) | `datagen/src/pmmp.rs:42-223` → `output/items.kdl` | `registry/item.rs` `ItemEntry{id, network_id, component_based, version, string_id, name, stack_size}` |
| Item native fields | (N) maxDamage, maxUseDuration, useAnimation, furnaceBurnIntervalModifier, furnaceXpMultiplier, tags, … | Kept in `items.kdl`; codegen squashes them into `ItemData.native_properties_json: &str` (`src/items.rs:10-34`) | **Unused.** Tags are dropped before `ItemEntry`. |
| Item component NBT (78 items) | (R) `data` compound | **Dropped** in `pmmp.rs:89-157` | `ItemRegistryPacket` sends `Nbt::default()` for every item (`registry/item.rs:199-215`) |
| BP `items/*.json` (77 in the pin; BDSpk has 79) | (BP) | **Not ingested** (`ingest/vanilla.rs` reads entities and biomes only) | — |
| Blocks for interaction | (N) `blockEntityType`, `requiresCorrectToolForDrops`, `xpDrop` | Kept in `blocks.kdl` `native_properties_json`; **dropped by `codegen/src/blocks.rs:67-84`** | — |
| Recipes | (BP) `recipes/` (1656), [PMMP], (P40) | **None** (`source-gap-audit.md:25`: "Not normalized") | `CraftingDataPacket` is never sent |
| Loot tables | (BP) `loot_tables/` (207) | **None.** `entities.kdl` keeps only table *paths* (94 `loot`, 18 `equipment`, 34 other) | Nothing. Block break drops the same-named item (`blocks.rs:514-604`). |
| Creative | PMMP `creative_*.json` | `creative.kdl` (keeps block_states, nbt) | `build_creative_content` uses only `damage`; block runtime id = first canonical state (`server/game/mod.rs:606-640`) |

### 1.2 Runtime item and inventory model today

**`ItemStack`** (`item/stack.rs:19-46`):
- Fields: `{item_id: String, runtime_id, count: u8, damage, nbt: Option<Vec<u8>>, max_stack_size_override}`.
- Without a registry override, max stack size falls back to **name heuristics** (`stack.rs:124-176`: `_sword`→1, `bucket`→16, …).
- Armour slots are validated by **name suffix** (`entity/components/inventory.rs:388-406`).

Both heuristics contradict the user's own rules:
- "do not invent fallbacks" (`source-gap-audit.md:37`);
- "don't infer semantic capabilities from names" (`plans/2026-01-04-block-registry-typestate-architecture.md:45`).

**ItemStackRequest** (`server/game/inventory.rs:68-147`):
- Only `[CraftCreative, Place]` into an empty main slot is accepted, and only in creative.
- Every other action answers Error and resyncs.
- The legacy `NormalTransaction` path is count-conserving and atomic (`:333-436`).

**Containers and block entities:**
- `OpenContainer`/`ContainerType` (`inventory.rs:533-583`) are unused.
- There are no block entities: `world/chunk.rs` has no storage for them, `storage/keys.rs` key 0x31 is never written, and `leveldb_world.rs:86,139` are TODOs.
- Join inventory packets carry no stack network ids (`join.rs:299-327`, `net_id_variant: None`).

### 1.3 The user's standing design decisions that constrain this

| # | Decision | Source |
|---|---|---|
| P1 | Two offline stages: datagen writes the golden KDL (committed, provenance-hashed), codegen turns it into Rust | `plans/2026-01-05-unastar-entities-gen.md:5,56` |
| P2 | "No dynamic JSON parsing at runtime". **Superseded by D2** (schemas generated, data embedded and decoded at startup) | `…entities-gen.md:124`; `unastar-data/README.md:9` |
| P3 | BP JSON is a *patch* on code defaults; layer defaults and overrides in KDL. **Superseded**: BDS replaces whole files by identifier and unmeasured defaults are not invented (D3, D4; entities research §1.1) | `…entities-gen.md:50,143-150` |
| P6/P7 | Source priority: native → BP → BDS capture → protocolgen → PMMP → overrides. Fail on missing data; every fact carries provenance. | `README.md:13-22`, `source-gap-audit.md:34-37` |
| P9/P11 | String ids are identity. Registries go Builder → freeze, with plugin ids after vanilla. | `…typestate-architecture.md:7,10-12` |
| P10 | No semantic roles inferred from names | `…typestate-architecture.md:45` |

---

## 2. How BDS does it

### 2.1 Items

**Registration and load order.**
1. Built-in item classes are registered first.
2. Item JSON is then loaded from the whole behaviour-pack stack: the base `vanilla` pack plus the versioned `vanilla_1.x` override packs (BDSpk). The newest override at or below the world's base game version applies.
3. Built-in item tags are added last.
4. Only vanilla packs may define `minecraft:` items.

**`format_version` selects the model.** The legacy-vs-data-driven split is visible in (R) as `version` 0 vs 1.
- **< 1.16.100 (legacy):** the JSON augments an existing built-in item using legacy components (`minecraft:food` with named saturation, `minecraft:seed`, `minecraft:use_duration`, `minecraft:camera`).
- **≥ 1.16.100:** the JSON creates a component-based item. Each component has its own minimum and deprecation version (BP JSON schemas `metadata/json_schemas/server/item*`).

**Behaviour dispatch.**
- A component-based item's behaviour is the sum of its components: use, use-on, use-time-depleted, release, hurt-actor, mining, durability.
- Built-in item classes override the same hooks with their own logic. For example:
  - bucket stack size depends on the variant;
  - trident durability and crossbow use duration are not the table values.

**What the client receives.**
- `ItemRegistryPacket` entries are `{name, id, component_based, version, nbt}` (V).
- The NBT is non-empty for exactly 78 items: 31 data-driven and 47 legacy (R).
- All other items send `{}`.
- Data-driven NBT is shaped `{components:{item_properties:{…}, item_tags:[…], minecraft:<c>:{…}}}`. Legacy NBT carries the legacy components, with saturation names already converted to floats (poor…supernatural → 0.1…1.2) (R).

**Generic, data-driven components (a).** Implement each one once.

| Component | Fields (BP schema / (R)) | Vanilla users (BP, BDSpk, (R)) |
|---|---|---|
| `minecraft:food` (new) | nutrition, saturation_modifier, can_always_eat, using_converts_to | apple |
| `minecraft:food` (legacy) | + effects[], remove_effects, named saturation, on_use_action, cooldown_* | 39 legacy foods |
| `minecraft:use_modifiers` / legacy `use_duration` | use_duration, movement_modifier | apple, spears / 40 legacy items |
| `minecraft:max_stack_size` | value | bundles, spears, stews, honey_bottle |
| `minecraft:durability` | max_durability, damage_chance | spears |
| `minecraft:repairable` | repair_items[{items, repair_amount: **Molang**}] | spears |
| `minecraft:enchantable`, `damage`, `cooldown`, `swing_duration`, `kinetic_weapon`, `piercing_weapon`, `hand_equipped` | — | spears |
| `minecraft:wearable` | slot, protection | none in vanilla JSON (armour is built in) |
| `minecraft:allow_off_hand` | value | none in JSON; (N) `inventory_capability_tags.json` lists the vanilla offhand set: shield, arrow, firework_rocket, totem, filled_map, nautilus_shell, sparkler |
| `minecraft:fuel` | duration (seconds) | wooden_spear (JSON); coal/charcoal have a built-in 80 s fuel component |
| `minecraft:throwable` + `projectile` | launch power, draw, entity | wind_charge |
| `minecraft:block_placer` / legacy `seed` | block, use_on / crop_result, plant_at | red_shrub, shelf_mushroom / 11 seeds |
| `minecraft:storage_item`, `storage_weight_limit`, `storage_weight_modifier`, `bundle_interaction` | allowed/banned items, weights | 17 bundles |
| `minecraft:stacked_by_data`, `glint`/`foil`, `tags`, `display_name`, `icon`, `compostable`, `fire_resistant` | flags | various |

**Native table semantics (b).** The burn and XP values below agree with (N) and with vanilla furnace timings.
- `furnaceBurnIntervalModifier` is the fuel burn time in "items smelted":
  - non-block items: `burn = cook_interval × modifier`. In a furnace, lava_bucket 100 gives 20 000 ticks, blaze_rod 12 gives 2400, boats 6 give 1200.
  - with a `minecraft:fuel` component: `max(cook_interval × duration_s / 10, 1)`. Coal at 80 s gives 1600 ticks.
  - block fuels are 0 in the table and follow a built-in rule (c): wood material ×1.5, coal_block ×80, dried_kelp_block ×20, a few ×0.25, other burnables ×0.5. Nether woods, chorus, mushrooms and beehives are excluded.
- `furnaceXpMultiplier` is the XP per smelted *output* item. Coal and charcoal read 0 because a built-in override applies.
- The native fields are the **base values before class overrides**:
  - buckets read 64;
  - spears read `maxDamage` 0, because their durability lives in a component;
  - trident and crossbow values are overridden.

**Built-in item classes unastar must write (c),** each parameterized by data:

| Class | Data it needs |
|---|---|
| weapons/diggers | an item tier table (not in any local data) |
| armour | material and slot (not in any local data) |
| bow, crossbow, trident | — |
| buckets | fill type |
| throwables | ender pearl/eye, snowball, egg, potions, XP bottle, fire charge |
| shield, shears, flint and steel, fishing rod, spyglass, goat horn, brush, mace | — |
| block/actor placers | spawn eggs, boats, minecarts, signs, doors, beds, … |
| other | maps, books, fireworks, compasses, bone meal |

### 2.2 Recipes

**Loading.**
1. Pack JSON from every pack's `recipes/` folder (BP, BDSpk). The type is the root key; `format_version` comes from the file.
2. **The first loaded identifier wins**, and shaped and shapeless share one identifier namespace.
3. Then the built-in user-data recipes: shulker boxes and dyed items. These are the 1084 user-data rows in [PMMP].
4. Then the 14 multi recipes ([PMMP] `special_hardcoded.json` lists their UUIDs).

**Per-recipe rules:**
- `tags` is required. Each tag yields its own recipe entry with its own net id: [PMMP] smelting lists 263 (recipe, tag) rows.
- `smithing_table` is valid only on smithing types.
- `priority` is read.
- `unlock` is required from format 1.20 onward, otherwise the recipe is rejected. Older formats default to "always unlocked".
- `assume_symmetry` defaults to true.

| JSON type | Sent in the 26.51 `CraftingDataPacket` (V) | Server use |
|---|---|---|
| `recipe_shaped` | `shaped_recipes` | crafting grid |
| `recipe_shapeless` (also stonecutter, cartography_table tags) | `shapeless_recipes` | crafting grid, stonecutter, cartography |
| `recipe_furnace` | no furnace list exists (V); (P40) `furnace: null` | furnace family, by tag |
| `recipe_brewing_mix` / `_container` | `potion_mixes` / `container_mixes` | brewing stand |
| `recipe_smithing_transform` / `_trim` | `smithing_*_recipes` | smithing table |
| built-in multi (14) | `multi_recipes` (uuid + net id) | built-in match/assemble (c) |
| built-in user-data shapeless | `user_data_shapeless_recipes` | crafting (c) |

**Net ids.**
- Recipe net ids are sequential from 1 in load order. Clients only need them to agree with the packet, so **unastar does not have to reproduce BDS numbering**.
- [gophertunnel] and [dragonfly] number their own recipes freely, and the client accepts that.

**Packet.**
- `clear_recipes = true`.
- Every recipe of every tag is included.
- Join order per [capture] (`bds-1.26.51-join-s2c.txt:46-54`): `CreativeContent → TrimData → InventoryContent ×4 → PlayerHotbar → CraftingData → AvailableCommands`.

**Ingredient wire format** (V) `CerealizerRecipeIngredientSerializedData`: a key/value list plus aux and count. The keys follow [gophertunnel]; **confirm them with a capture**:

| Descriptor | Key/value | Aux |
|---|---|---|
| empty | `{}` | — |
| item | `{"name": "minecraft:stick"}` | 32767 = any |
| Molang | `{"tags": expr}` | — |
| item tag | `{"item_tag": "minecraft:planks"}` | — |

**Crafting validation.**
- Look up the recipe by net id.
- A recipe needs the 3×3 grid when it is shaped with w or h ≥ 3, or shapeless with > 4 ingredients. This is the only 2×2/3×3 restriction; the tag is not checked against the open screen.
- **CraftRecipe:**
  - The grid is read from UI slots 28–31 (2×2) or 32–40 (3×3).
  - Every non-empty slot must hold ≥ numCrafts items.
  - The result (× numCrafts) goes into created-output slot 50; later Take/Place actions pull from it.
- **CraftRecipeAuto:** the recipe's ingredients are consumed from anywhere allowed.

**Matching:**
- **Shaped:** try every offset in the 3×3 grid, plus the mirrored layout when `assume_symmetry`. Counts are not compared.
- **Shapeless:** for each non-empty slot, remove the first matching ingredient (greedy, no backtracking). Succeed iff no ingredients remain.
- [dragonfly] skips the shape check entirely, so it is not a valid reference for matching.

**Furnace family:**
- Recipe lookup is by the furnace's tag.
- Cook interval: 200 ticks for the furnace, 100 for the blast furnace and smoker.
- Progress decays by 2 per tick when not burning.
- Special case: a wet sponge with an empty bucket in the fuel slot.
- XP is stored in the block entity and paid when the output is taken.
- `ContainerSetData` ids are sent only when a value changes:

  | Id | Value |
  |---|---|
  | 0 | cook time |
  | 1 | lit time |
  | 2 | lit duration |
  | 3 | stored XP |

  [gophertunnel] names the same ids.

**Brewing.** The mixes are data (a). The fuel is built in (c): blaze powder, 20 uses.

**Unlocking (c).** A per-player unlock system sends `UnlockedRecipesPacket`. Unastar can start by unlocking everything.

**Data sources.**
- **(BDSpk) `.brarchive`** is a simple deterministic container:
  - a 16-byte header `{u64 magic 0x267052A0B125277D, u32 count, u32 version}`;
  - 256-byte entries `{u8 len, name[247], u32 offset, u32 length}`;
  - then the JSON payloads.

  Overlaying the 34 recipe archives gives 1881 effective recipes:

  | Type | Count |
  |---|---|
  | shaped | 1058 |
  | shapeless | 586 |
  | furnace | 152 |
  | brewing_mix | 70 |
  | brewing_container | 2 |
  | smithing_transform | 12 |
  | trim | 1 |

- **[PMMP]** (CC0) is the only free source for the 1084 user-data rows.
- **(P40)** has a captured 1.26.40 packet: 1118 shaped, 1431 shapeless, 14 multi, 210 potion mixes, 12+1 smithing. There is no 1.26.51 export yet.

### 2.3 Loot tables and block drops

**Loading and evaluation (a).**
- Tables load lazily from the pack stack, keyed by lowercased path, and are cached. A missing table means no loot.
- Entries are `item`, `empty`, `loot_table`, or nested pools.
- Rolls are `floor(min) + nextInt(range)`, and **one bonus-roll float is drawn even without `bonus_rolls`**.
- Weight is `floor(weight + quality × luck)`.
- `tiers` exist.
- Conditions short-circuit.
- A table cannot recursively evaluate itself.

**Functions.** There are 26:
- The `minecraft:` prefix is stripped.
- Unknown functions are dropped with a log message.
- `set_data` and `random_aux_value` share an implementation.
- `trader_material_type` does **not** exist.
- `carry_over_block_entity_data` is experiment-gated.
- 17 of the 26 are used by (BP); `set_count` is the most common, with 595 uses.

**Conditions.** There are 20:
- There is no prefix stripping.
- Unknown conditions are silently skipped.
- `entity_properties` is **not a condition**. All 15 (BP) uses sit on `furnace_smelt`, whose on-fire check is built into the function.

The full name/parameter/usage tables are in the evidence notes. The parameter names agree with the public Script API metadata in (BP) `metadata/`.

**Invocation sites:**
- entity death (`minecraft:loot`);
- equipment;
- interact (shear, brush, milk);
- barter;
- the `spawn_loot` event;
- AI goals (eat_mob, drop_item_for, sneeze, dig);
- fishing (luck matters);
- chests, barrels and minecart chests, via NBT `LootTable` + `LootTableSeed`, unpacked on open or break;
- trial spawner and vault;
- `fill_container`;
- `/loot`.

**Block drops** are resolved in priority order:
1. A built-in rule set (c). Its primitives are drop self/other, silk touch, fortune/ore bonus, and special rules for leaves, crops, doors, double plants, mushroom blocks, redstone ore, campfire, flower beds, nether vines, TNT, beehive and banner names.
2. A block loot table (a): the 99 `vanilla_1.26.50/loot_tables/blocks` files (BDSpk).
3. For custom blocks: silk touch → the block itself; otherwise its loot table; otherwise the block itself.

(N) has only `xpDrop` and `requiresCorrectToolForDrops`. The exact source for the rule set is a BDS measurement sweep with `/loot spawn … mine <pos> <tool>`.

**RNG.**
- The loot random is MT19937 with `nextInt(n) = u32 % n` and `nextFloat = u32·2⁻³²`.
- Death and block loot use unseeded or level randoms, so they can only match **statistically**.
- Seeded containers can match **exactly** if the draw order and slot scatter are reproduced.

### 2.4 Interactable blocks and block entities

**Use path:**
1. The data-driven `minecraft:crafting_table` block component (custom blocks; BP schema `block_components`: `crafting_tags`, `table_name`; shaped/shapeless only per `doc_modules/blocks.json`) (a).
2. The held item's use-on behaviour.
3. Built-in per-block behaviour for chests, barrels, furnaces, crafting tables and so on (c).

Most other workstations open a container type chosen in code by block identifier (c).

**Block-entity type is native data (b).**
- (N) `block_properties_table.json` `blockEntityType`: chest 2, furnace 1, barrel 42, ender_chest 23, blast_furnace 38, crafting_table 0.
- These values select the block entity created on placement or load.
- The only data-driven block-entity component holds script properties, not a container.

**Containers (c):**

| Block | Slots | Notes |
|---|---|---|
| Chest | 27, or 54 when paired | Pairs only with a chest of the same type, same Y and same facing. NBT `pairx`/`pairz`/`pairlead`/`forceunpair`. Lid: `BlockEvent(pos, 1, 1)` when the first viewer opens, `(pos, 1, 0)` when the last leaves. |
| Barrel | 27 | Toggles `open_bit` instead of a lid event. |
| Ender chest | 27 | The player's ender inventory. |
| Shulker box | 27 | Rejects shulker items. |
| Furnace, blast furnace, smoker | 3 | input, fuel, output |
| Hopper | 5 | |

[dragonfly] `server/block/{chest,barrel,furnace,…}.go` uses the same sizes and NBT ids.

**Persistence.** A chunk's block-entity NBT compounds are written concatenated:
- every block entity: `id`, `x`, `y`, `z`, `CustomName`, `LootTable`, `LootTableSeed`;
- containers: `Items` [ {`Slot`, `Name`, `Count`, `Damage`, `WasPickedUp`, `Block`, `tag`} ]. Each chest half saves only its own 27 slots;
- furnace family: `BurnTime`, `CookTime`, `BurnDuration` (Short), `StoredXPInt` (Int). [dragonfly] uses the same names.

Storage and network placement:
- The LevelDB key is chunk key + 0x31, matching unastar `keys.rs`.
- On the network, the same compounds follow the border byte in `LevelChunk`, or are attached per sub-chunk in sub-chunk mode.
- The chest's `BlockActorDataPacket` update omits `Items`.

**Windows:**
- Window ids are a per-player counter that wraps 99 → 1. The player inventory uses it too; [dragonfly] uses 0.
- Wire container types:

  | Value | Container type |
  |---|---|
  | 0 | CONTAINER |
  | 1 | WORKBENCH |
  | 2 | FURNACE |
  | 8 | HOPPER |
  | −1 | inventory |
  | −9 | NONE |

  Blast furnace and smoker are expected to be 27 and 28; confirm with a capture.
- Opening a chest sends `ContainerOpen{id, type, pos, actor −1}`, then `InventoryContent` with the window id. The container name is `LevelEntity` (7), `ShulkerBox` (30) or `Barrel` (58) (V `EnumsContainerEnumName`).
- The crafting table sends only `ContainerOpen`.
- The client's `ContainerClose` is answered with `{current id, type −9, server_initiated = false}`, after all queued item requests are processed.
- **BDS does not return cursor or crafting-grid leftovers on close.** The client moves them back itself with ordinary requests.

**Breaking a container:**
- **Chest/barrel:** unpack the loot table, then drop each slot in random 10–30 item chunks.
- **Furnace:** spawn its XP orbs, then drop its items unsplit.
- **Shulker box:** keeps its contents in the dropped item.

### 2.5 ItemStackRequest processing (the engine)

**Queueing:**
- Requests are processed synchronously and in order.
- **One `ItemStackResponsePacket` per incoming packet**, with responses in request order.
- Requests whose id is not negative-odd get **no response**. The vanilla client always uses negative odd ids ([gophertunnel] `nextRequestID`).
- A request with zero actions gets status 1.

**All-or-nothing:**
- Actions mutate a scratch view, which is committed once at the end.
- Any non-zero status empties `containers`.
- **No InventoryContent resync** follows an error.
- [dragonfly] behaves the same (`reject` sends status only and reverts).

**Action gate** (otherwise status 3):
- Swap only before a craft action.
- Consume/Create only after one.
- At most one craft action per request.

**Response layout:**
- Containers appear in first-touched order, merged by `FullContainerName`.
- Slots appear in first-touched order, merged by requested slot; a later touch overwrites in place.
- A transfer adds the source slot before the destination slot.
- `slot == requested_slot` always.
- `custom_name` comes from `tag.display.Name`.
- `durability_correction` is the damage value.
- [dragonfly] iterates maps, so its order is random. It is not a reference here.

**Stack net ids:**
- One global counter starting at 1.
- Merges keep the destination id; full moves and swaps keep the ids.
- A split destination and crafted output get fresh ids.
- Ids are not assigned at send time.
- Validation:
  - a positive client id must equal the slot's current id;
  - a negative odd id refers to an earlier successful request's result, remembered for 5 s;
  - a mismatch fails with 49 (source) or 50 (destination).

**Transfer checks**, in order:
1. Destination is CreatedOutput → 47.
2. Source and destination are the same slot (including the Hotbar/Inventory ↔ CombinedHotbarAndInventory aliases) → 48.
3. Source slot invalid → 49.
4. Amount 0, or more than the source count in a craft request → 51. Otherwise the amount is clamped.
5. Destination slot invalid → 50.

**Moving the items:**
- **Merge path:** if the stacks are stackable, merge `min(count, room)`. Per-slot item rules are **not** consulted on a merge.
- **Empty-destination path:** requires the set count, the per-slot item rules and the move-to-container rule.

**Validation outcomes** map to statuses:

| Outcome | Status |
|---|---|
| OK | 0 |
| no-op / invalid | 1 Error |
| cannot remove | 65 |
| cannot place | 55 |
| cannot swap | 54 |
| cannot destroy | 60 |
| cannot drop | 59 |

The status numbers match [gophertunnel] `ItemStackResponseStatus*`.

**Drop and Destroy:**
- **Drop** spawns the item on commit, at camera height − 0.5, with pickup delay 10 and throw time 40.
- **Destroy** is creative-only, on the inventory or workbench screen; otherwise status 60.

### 2.6 What drives validation from data (the two tables to port)

**Item-side inputs (a/b):**
- `max_stack_size`: set cap and merge room; an item with < 2 does not stack.
- `stacked_by_data`: aux must be equal for items to stack.
- User-data equality.
- `wearable.slot`, or a skull/carved_pumpkin block, decides the armour slot (set count 1, no merging).
- `allow_off_hand`.
- `binding_curse`: cannot be removed outside creative.
- `item_lock` user data.
- Fuel: a burn duration > 0, or a bucket/lava bucket, which is capped at 1.
- `storage_item`/`storage_weight_*`: bundles.
- Smithing item tags: `trimmable_armors`, `trim_materials`, … (N).

**Container-side: a static table keyed by `ContainerEnumName`** (V):

| Container(s) | Rule |
|---|---|
| Preview containers | never valid endpoints |
| CreatedOutput (60) | take-only, slot 50 |
| CraftingInput (13) | offset 28, size 4 on the inventory screen; offset 32, size 9 on a workbench |
| Cursor (59) | slot 0 |
| Armor (6) | rules above |
| Offhand (34) | `allow_off_hand` |
| FurnaceFuel / Ingredient / Result (24 / 25 / 26) | **Result accepts Place on the server** (permissive) |
| BeaconPayment | five built-in ingots/gems |
| EnchantingMaterial | lapis |
| Cartography inputs | built-in names + glass_pane |
| Shulker box | rejects shulkers |
| Dynamic (63) | bundles |

**Screen-side: a static table keyed by the open screen's `ContainerType`.** Each screen allows a set of `ContainerEnumName`s; std = {Combined 12, Hotbar 28, Inventory 29, Cursor 59}:

| Screen | Allowed containers |
|---|---|
| Inventory / workbench | {CraftingInput, CreatedOutput, Armor, Offhand} + std |
| Chest | {LevelEntity} + std |
| Furnace | {Ingredient, Fuel, Result} + std |
| … | … |

Addressing a container outside the set fails with status 1.

The whole rule system is a closed engine: **two tables, one transfer algorithm, item data**. [dragonfly] has no per-screen sets, no `item_lock`, and no merge/empty asymmetry.

---

## 3. Gaps and risks

### 3.1 Data that no pack or native table contains

These must be authored, measured or captured.

| Fact | Proposed source |
|---|---|
| Armour slot and material per armour item; toughness, knockback resistance | BDS sweep (D4): `/replaceitem … slot.armor.*` accept/reject plus damage-reduction measurements, committed as measured data |
| Tool tier, dig speed, attack damage per tool/weapon | BDS break-time and damage sweeps (D4, shared with ENTITIES E9 and SURVIVAL F3) |
| Block fuel values and exclusions | BDS furnace-timing sweep (D4) over every burnable block |
| Bucket per-variant stack size; trident/crossbow overrides; coal fuel and XP | Per-class constants in Rust, each measured from BDS (D4) |
| Block drop rules (~1380 blocks) | `block_drops` from a BDS `/loot … mine` sweep (statistical fixtures; D4, same sweep as ENTITIES E9) |
| 14 multi-recipe semantics; shulker/dye recipes | Rust impls; UUIDs and user-data rows from [PMMP] (CC0) |
| Brewing fuel, beacon payment items, lapis, cartography inputs, skull ids, shulker ids | Small Rust constant tables, each confirmed by a BDS capture or sweep |
| Blast furnace/smoker cook intervals and container types | Confirm once with a BDS capture (ContainerOpen type plus timing) |

### 3.2 Staleness of the pinned pack

The pin is bedrock-samples 1.21.130.3; the target is BDS 1.26.51.
- **Recipes:** 236 missing and 154 differing (furnace priority/unlock, mushroom stew tags, …), plus 11 duplicate ids.
- **Loot:** 102 missing (99 block tables + 3 camp tables) and 6 differing; there is a new condition, `biome_has_tag`.
- **Items:** red_shrub and shelf_mushroom are missing, and `apple` is at format 1.20.50 instead of 1.26.30.

The pin is shared with entities and biomes (ENTITIES, WORLD). Resolved by D1: main now reads bedrock-samples 1.26.50.4; the brarchive diff check (slice 3) lists what still differs from the BDS 1.26.51 layer stack.

### 3.3 Molang

- Items need only `repairable.repair_amount` (vanilla spears use Molang strings) and Molang ingredient descriptors (unused by vanilla recipes).
- Loot uses none.
- The Molang subset ENTITIES is designing covers this. Items should consume that evaluator, not define their own.

### 3.4 Wire-format defects found (need owners outside ITEMS)

1. **UUID byte order.**
   - `bedrock_core/src/bedrock/codec.rs:651` writes `Uuid::as_bytes()` (big-endian).
   - [gophertunnel] `protocol/writer.go:172-178` writes each 8-byte half little-endian.
   - This affects every UUID field: multi-recipe UUIDs, recipe uuids, PlayerList/AddPlayer.
   - Round-trip conformance cannot catch it, because the codec is symmetric. It needs a fixed-vector test against gophertunnel bytes (valentine owner).
2. **ContainerType ordinals.**
   - `valentine_gen/overrides/enum-ordinals.json:1296+` numbers `NONE, INVENTORY, CONTAINER, …` from 0.
   - The wire values are CONTAINER=0, WORKBENCH=1, FURNACE=2, inventory −1, NONE −9 (§2.4; [gophertunnel] `protocol/container.go`).
   - Fix this before anything uses the generated enum.
3. **ItemRegistry component NBT.**
   - unastar sends `{}` for the 78 items that (R) shows BDS sends components for (`registry/item.rs:210`).
   - Likely effect, unchecked in-game: apple, bundles, spears, legacy foods and seeds lose client-side eating, use duration, cooldown and placement prediction.
   - Cheap fix: ingest (R) `data` verbatim (§5, slice 2).
4. **CraftRecipeAuto layout.**
   - (V) decodes `{recipe_net_id, numberofrequestedcrafts u8, ingredients Vec(varuint)}`.
   - The research suggests a `timesCrafted` byte and a u8 ingredient count.
   - Settle this with a captured client packet before implementing auto-craft.

### 3.5 Codegen at build vs runtime loading; addons

- BDS loads items, recipes, loot and custom blocks from the pack stack at startup, so custom packs work through the same loaders.
- P2 (no runtime JSON) fitted vanilla but ruled out addon packs. Resolved by D2 and D5: schemas are generated, vanilla data is embedded and decoded at startup, and the same decoder later accepts addon packs before the registries freeze (§4.5).
- Static Rust tables are slow to compile:
  - `unastar-data/src/items.rs` is already 1.8 MB, and `blocks.rs` 0.9 MB;
  - loot (207+ tables) and recipes (~1900 + 1084) would add more.

### 3.6 Exactness vs strictness

- BDS is sometimes *more permissive* than dragonfly:
  - Place into the furnace result slot is allowed;
  - merges skip per-slot rules.
- It is sometimes *stricter*:
  - invalid request ids get no response;
  - per-screen container sets;
  - `item_lock`.
- "BDS is the truth" means copying both.

### 3.7 Performance

- **Items:** items are values inside stacks, not entities. Item components should be plain Rust data on the frozen registry entry, not bevy components.
- **Recipes:** the client sends the recipe net id, so matching is one lookup plus O(9) work. Index the furnace lookup by `(tag, input)`.
- **Loot:** evaluation is cheap and per event.
- **Block entities:** there are few per chunk. Make them bevy entities with a per-chunk `BlockPos → Entity` index.

---

## 4. Proposed unastar design

### 4.1 Principles

1. **Mirror BDS's split.**
   - Pack data feeds registries at startup.
   - Behaviour is generic component implementations, written once, plus explicit item and block *classes* for the built-in behaviour (c). The classes are parameterized by tables.
   - No per-item `match` on identifiers outside the class tables.
2. **Keep P1's provenance.**
   - Datagen normalizes every family into a golden artifact with provenance and hashes (whether artifacts derived from EULA packs are committed is the architecture doc's open decision O6).
   - Built-in facts are measured from BDS (D4) and stored as data with a `source=` note naming the sweep or capture, never inline Rust literals. Authored values exist only as labelled placeholders that tests flag until the measurement lands.
3. **Change how artifacts reach the runtime (D2).**
   - Codegen generates *schemas*: typed component structs, loot function and condition enums, recipe kinds, and small hot tables.
   - Large *instance* sets (recipes, loot tables, item component values) are embedded as a compact binary build of the KDL and decoded once at startup into frozen registries.
   - The same decoder can later accept addon packs.
4. **Engine tables are code, not data.** The `ContainerEnumName` validators and screen allowed-sets (§2.6) are fixed engine semantics. They live in one Rust module as static tables.

### 4.2 Artifact families

| Artifact | Sources (priority) | Generated (codegen) | Loaded and interpreted at runtime |
|---|---|---|---|
| `items.kdl` (extend) | (N) scalars, typed; (R) `data` compound verbatim (base64); BP/BDSpk JSON parsed into typed components; `inventory_capability_tags.json` | `ItemData` with typed native fields + `tags: &[&str]` + `network_nbt: &[u8]` | `ItemRegistry` entries carry `ItemComponents` (a struct of options: food, use_modifiers, max_stack, durability, repairable, enchantable, cooldown, wearable, allow_off_hand, fuel, throwable, projectile, block_placer/seed, storage, stacked_by_data, …) and an `ItemClass` |
| `overrides/items/*.kdl` (new, measured) | BDS sweeps and captures (D4) | `ItemClass` per identifier (`bucket fill=water`, `armor slot=head material=diamond`, `digger kind=pickaxe tier=diamond`, `bow`, …) + `tiers.kdl`, `armor_materials.kdl`, `fuel_blocks.kdl` | Class dispatch (§4.3) |
| `recipes.kdl` (new) | BP/BDSpk JSON with the BDS load rules (tags required, unlock rule, first wins, one row per tag); [PMMP] user-data rows; multi UUIDs | `RecipeKind`, `ItemDescriptor` enums | `Recipes` resource: net ids assigned 1..N at load, indexed by tag and by `(furnace tag, input)`; `CraftingDataPacket` built once and cached (`Arc`) |
| `loot_tables.kdl` (new) | BP/BDSpk loot JSON. Drop exactly what BDS drops (unknown functions/conditions, `entity_properties`, entry `count`); validate every item and table reference (fails today on the missing fox/piglin tables) | `LootFunction` (26 variants), `LootCondition` (20) | `LootTables` resource keyed by lowercased path; evaluator + MT19937 random |
| `block_drops.kdl` (new, measured) | BDS `/loot mine` sweep, as a rule DSL (self, other, silk touch, ore fortune, leaves, crops, doors, …) | Rule enum | Three-path drop priority (§2.3) |
| `blocks.kdl` (extend) | (N) `blockEntityType`, `requiresCorrectToolForDrops`, `xpDrop`, typed | Fields on `BlockData` | Block-entity factory, drops |

### 4.3 Runtime model (ECS mapping)

**Items.**
- `ItemStack` gains `net_id: i32` (0 = unassigned), following the BDS rules in §2.5:
  - assigned when an item is set into an empty slot, for crafted output, and for a split destination;
  - kept on merge, full move and swap;
  - drawn from one global counter.
- `max_stack_size()` reads the registry only; the name heuristics are deleted.
- Item behaviour runs as a pipeline:

  ```
  use / use_on / use_time_depleted / release_using / hurt_actor / mine_block
    -> ItemClass hook (when the class overrides it)
    -> generic component hooks (food, throwable, block_placer, cooldown, durability, …)
  ```

  It runs inside the inventory/interaction systems. The results are effects queued to the owning domain:
  - eat → SURVIVAL `eat_food`;
  - projectile → ENTITIES;
  - place → blocks.

**Inventory engine** (`server/game/inventory/`):
- `ContainerScreen { container_type, window_id, pos, allowed: &'static [ContainerEnumName] }`.
- `validator(ContainerEnumName) -> &'static ContainerValidation`, with `offset`, `size`, `is_item_allowed_in_slot`, `available_set_count`, `allowed_add_count`, `allowed_to_remove`, `can_move_to_container` and `can_destroy`. Validators read `ItemComponents`.
- A per-request `Staging` overlay covers every addressed container:
  - main, armour, offhand and cursor components;
  - UI grid slots 28–40 and 50;
  - the open block entity's `ContainerSlots`.
- A response builder that keeps first-touch order.
- Transfer, swap, remove and craft follow §2.5.
- A 5 s request-id slot cache.
- `spawn_item_drop(world, pos, stack, Some(player))` on commit (agreed with ENTITIES).

**Block entities:**
- Bevy entities with:
  - `BlockActor { kind, pos }`
  - `ContainerSlots(Inventory)`
  - `ChestPair`
  - `Viewers`
  - `FurnaceState { lit_time, lit_duration, cook_time, stored_xp }`
  - `LootSeed`
- Created from `blockEntityType` on place or chunk load.
- Saved as concatenated NBT at key 0x31, keeping unknown keys for round-tripping.
- Serialized into the LevelChunk/SubChunk payloads; updates use the `Items`-less form.

**Interaction dispatch:**
1. Data `crafting_table` component.
2. Per-block-entity-type handler (chest, barrel, ender chest, shulker, furnace family, hopper).
3. Static `workstation(block identifier) -> ContainerType` table.
4. `OpenContainer` event → screen open: window id 1..99, `ContainerOpen` + `InventoryContent`.

The furnace tick runs in `EntityLogicSet` and diffs `ContainerSetData` ids 0–3 to viewers.

**Recipes and crafting:**
- Matchers are written once: shaped offsets + mirror, shapeless greedy.
- The 14 multi recipes are Rust impls keyed by UUID.
- Furnace lookup is by `(tag, input)`.
- `CraftingDataPacket` is sent after `PlayerHotbar`. The `join.rs` hook is coordinated with SURVIVAL.
- Everything is unlocked at join until unlocking is implemented.

**Loot:**
- `LootContext { luck, killer, killer_player, pet, tool, explosion_radius, this, damage_source, difficulty, dimension, visited }`.
- The evaluator copies the BDS draw order and uses the MT19937 random for seeded containers.
- Entry points are wired by their owners: entity death and equipment by ENTITIES, block break by ENTITIES/blocks, containers by ITEMS.

**Plugins:**
- Fire only the existing WIT hooks (`on-player-item-use`, `on-player-interact-block`).
- No WIT growth without the coordinator.
- The unused `item-stack` record could carry the used item, once approved.

### 4.4 Shared contracts with other workers

| With | Contract |
|---|---|
| ENTITIES | `spawn_item_drop(world, DVec3, ItemStack, Option<Entity>) -> Option<Entity>` (agreed). One loot engine (architecture doc, "Loot engine"): the parked ENTITIES evaluator is converted to D2 and moved to the shared rules crate (entities E0); ITEMS completes it (slice 11: all 26 functions and 20 conditions, BDS draw order, MT19937) and owns chest/container entry points; ENTITIES calls it on death, equipment and interact. ENTITIES provides the Molang evaluator for `repair_amount` and the projectiles for throwable/shooter items. |
| SURVIVAL | ITEMS' consume flow calls `eat_food(world, entity, item_id)`. **Resolved (architecture doc):** gameplay food rows come from the pack's `minecraft:food` components (current since D1; the loader parses legacy and new formats the way BDS does), while `ItemRegistryPacket` sends (R) `data` verbatim; a test checks that both agree for every vanilla food. The CraftingData hook goes in `join.rs`, which SURVIVAL owns. |
| WORLD | Block-entity NBT in the chunk payload and LevelDB key 0x31 touches chunk/storage code. |
| valentine owner | §3.4 defects 1, 2 and 4. |

### 4.5 Custom packs (later)

- Move datagen's normalizers into a library crate, `unastar-data-core`: pure functions from JSON to a typed IR to validated rows.
- Datagen calls it offline to produce the golden KDL.
- unastar can call the same code at startup on `packs/*/`, appending custom items, recipes, loot and blocks after vanilla and before the registry freeze (P11).
- This avoids two parsers and keeps P2's intent: no JSON in hot paths, and one validated normalizer.

### 4.6 Keeping "BDS is the truth" testable

| Family | Fixture | Exactness |
|---|---|---|
| Item registry | unastar's `ItemRegistryPacket` payload must equal (R) for all 2076 entries | byte-exact |
| Recipes | A dev-only `brarchive` reader diffs the vendored JSON against the local BDS install (skipped when absent). One captured `CraftingDataPacket` is decoded with valentine and compared as sets keyed by `recipe_id` + tag, ignoring net ids and order. | exact content |
| Loot | Seeded chest fill compared with BDS for a fixed `LootTableSeed` (captured via client-diff `/setblock` + NBT); `/loot spawn … loot\|kill` histograms | exact / statistical |
| Block drops | `/loot … mine` sweep fixtures | statistical |
| Inventory | client-diff `inventory-moves` scenario (parked WIP `db31cda`). BDS captures become ECS replay tests: same initial inventory, same c2s requests, compare responses ignoring absolute net ids. The comparison still checks id reuse vs fresh, statuses and container/slot order. | structural-exact |
| Containers | client-diff open/close/furnace captures. The bot's block click currently gets a resync from BDS (`tools/client-diff/README.md`), so fix the bot first. | structural-exact |

---

## 5. Implementation plan

Sizes: S ≤ 1 day, M 2–4 days, L ≥ 1 week, for one worker.

| # | Slice | Size | Depends on | Test (fails before, passes after) |
|---|---|---|---|---|
| 1 | Wire defects (§3.4): UUID byte order (valentine), ContainerType ordinals (valentine_gen), CraftRecipeAuto layout check | S | valentine owner | Fixed-vector UUID test against gophertunnel bytes; ContainerType values 0/1/2/−1/−9; a captured CraftRecipeAuto decodes |
| 2 | Items data: ingest (R) `data` verbatim, typed native fields and tags into `ItemData`/`ItemEntry`; send component NBT; delete the `ItemStack` name heuristics; take the offhand set from capability tags | M | — | `ItemRegistryPacket` payload equals items.nbt for every entry; bucket and armour stack sizes come from the registry |
| 3 | Pin bump (done on main by D1: bedrock-samples 1.26.50.4) plus a dev-only brarchive diff tool, reusing the brarchive reader WG-BEDROCK wrote in `unastar_bedrock_codegen` | S | — | Regenerated artifacts are deterministic (run twice); brarchive diff is clean or its differences are listed |
| 4 | Inventory engine: validators, screens, staging, net ids, response order, drop/destroy/consume gates | L | 2 | Per-validator unit tests; per-status tests (3, 47–51, 54, 55, 59, 60, 65); BDS replay of `inventory-moves` |
| 5 | Armour and offhand: `wearable`/class slot, binding curse, skull/pumpkin head; MobArmorEquipment to viewers | S | 2, 4, armour overrides | Accept/reject matrix against a BDS sweep |
| 6 | Recipes family: `recipes.kdl`, codegen, `CraftingDataPacket` at join, `UnlockedRecipes` | M | 3 (or the current pin with a known gap) | Packet content equals the BDS capture as sets; brarchive diff |
| 7 | Crafting execution: CraftRecipe/Auto/Consume/Create/ResultsDeprecated, matchers, the 14 multi recipes | M/L | 4, 6 | Shaped offset/mirror matrices; shapeless greedy order; BDS capture replay of `craft` |
| 8 | Block entities: factory from `blockEntityType`, NBT load/save (LevelDB/BlazeDB), network payload | L | blocks.kdl field; WORLD for chunk code | Round-trip a BDS-saved chunk with chest/furnace NBT; payload bytes match a BDS SubChunk capture |
| 9 | Container windows: chest/double chest/barrel/ender/crafting table open and close, lid BlockEvent, window ids, the −9 `ContainerClose` reply | M | 4, 8 | client-diff capture, after fixing the bot's click |
| 10 | Furnace family: tick, fuel (component/native/authored blocks), XP storage and payout, lit swap, `ContainerSetData` | M | 6 (furnace rows), 8, 9 | Tick simulation against the formulas; BDS `ContainerSetData` sequence capture |
| 11 | Loot family, evaluator and MT19937 | M/L | 3 | Seeded chest exact; statistical histograms |
| 12 | Block drops: `block_drops.kdl`, the sweep harness, the three-path drop priority | L | 11; ENTITIES (blocks.rs) | Sweep fixtures (statistical) |
| 13 | Generic item components: food/use_modifiers/cooldown (with SURVIVAL), durability/repair (Molang from ENTITIES), throwable/projectile (ENTITIES), block_placer/seed | M each | 2 | Per-component ECS tests against measured BDS numbers |
| 14 | Item classes (bucket, bow, crossbow, trident, potions, tools/tiers, shield, …) | L, ongoing | 13, authored tables | Per-class tests with measured fixtures |
| 15 | BlockPickRequest and MobEquipment rebroadcast | S | 4, 8 | ECS tests |
| 16 | Brewing stand: mixes from recipes, blaze-powder fuel with 20 uses | M | 6, 8, 9 | Mix tests against data |

Critical path: **1 → 2 → 4 → (6 → 7) ∥ (8 → 9 → 10)**. Loot (slices 11–12) can start after slice 3, in parallel.

---

## 6. Decisions (resolved by the owner)

The owner answered these as D1-D5 in `thoughts/shared/plans/2026-10-24-data-driven-decisions.md`.

| # | Decision | Options | Resolution |
|---|---|---|---|
| D1 | **Vanilla pack pin** (shared by entities, biomes, items, recipes, loot) | (a) bump bedrock-samples to the 1.26.50/51 tag; (b) keep 1.21.130 and patch the gaps with overrides; (c) read BDS `.brarchive` at datagen time from a local install (not redistributable, so CI needs BDS) | (a), done on main (1.26.50.4), plus the dev-only brarchive diff check |
| D2 | **Build-time codegen vs startup loading** for large instance sets | (a) keep P1/P2: everything as generated Rust statics; (b) KDL → embedded binary artifact decoded at startup, with codegen only for schemas; (c) as (b), plus addon packs loaded at runtime through a shared normalizer library | (b) now; (c) is the D5 extension path |
| D3 | **Exactness policy** where BDS is permissive (Place into the furnace result, merges skip slot rules) or quirky (no response to bad request ids) | (a) copy BDS exactly; (b) be stricter where that is safe | (a), each case logged in `bds-quirks.md` |
| D4 | **Source for facts BDS builds in** (armour, tiers, block fuels, block drops) | (a) authored KDL from wiki/dragonfly values; (b) measured BDS sweeps (`/loot mine`, slot accept/reject, furnace timing) committed as fixtures | (b) |
| D5 | **Custom content scope** | (a) vanilla only; (b) addon packs (custom items/blocks/recipes/loot) through the shared normalizer; (c) also plugin registration APIs in WIT | (a) now, with registries keyed by identifier and open until freeze so (b) and (c) bolt on later; WIT growth still needs approval |

---

## Appendix: evidence notes and WIP

- **Evidence notes.** The fully cited research notes are kept outside the repo, in the coordinator folder `C:/Users/jvigu/orca/stab-coord/vanilla-items-evidence/`. This document's behaviour statements are summarized from them.
- **Parked WIP commit** on this branch: `db31cda wip(client-diff): inventory-moves scenario, cursor/swap/stale stack steps and capture mode`.
  - It has not yet been run against BDS.
  - The shared runtime's Endstone venv lacks the `endstone_client_diff` plugin, so `run.ps1 -Mode capture` times out waiting for `ready.json`.
