# Measured BDS facts (decision D4)

Facts BDS 1.26.51 keeps in code, measured on a loopback BDS with the sweep
harness in `tools/bds-sweep` (Endstone plugin `endstone-sweep` plus the
`bds-sweep-client` scripted player). These files are measurements of BDS
behaviour, not copies of Mojang data files; the raw per-run logs
(`results.jsonl`, server logs) stay outside the repository in
`C:/Users/jvigu/orca/bds-runtime/sweeps/1.26.51/`.

`manifest.json` lists every artefact with its SHA-256, the BDS version, the
harness revision, the sweep runs (start time, status, seconds, item count) and
the measurement method. The capture-derived tables (`recipe_legacy_identities.json`,
`builtin_recipes.json`, `potion_types.json`, `block_name_keys.json`) are not
sweep artefacts and record their source capture's SHA-256 inside the file
instead.
`enchantments.json` is not a sweep artefact either; it records its provenance in
its `source` object.
`tests/measured_manifest.rs` checks every listed hash and that no other file is
missing from the manifest. Regenerate a family with `sweep.py` (exclusive BDS
slot; resumable with `--out`) and then `summarize.py`, e.g.:

```powershell
cargo build -p bds-sweep
$py = 'C:/Users/jvigu/orca/bds-runtime/venv/Scripts/python.exe'
& $py -m pip install -e tools/bds-sweep/endstone-sweep   # once per venv
& $py tools/bds-sweep/sweep.py drops --blocks all --samples 400 --out <dir>
& $py tools/bds-sweep/summarize.py drops <dir>
```

## Files

| File | Content | Method |
|---|---|---|
| `block_drops/<block>.json` | `{"block", "cases": [...]}`, one compact case per line: `states` (list of requested block states sharing this case), `placed_states` (what BDS stored, aligned with `states`; only present where it differs), `tools` (tool keys sharing this outcome, see below), `samples`, `observed` (histogram of whole outcomes: per dropped stack `name@data` + `x` + count, `;`-separated, `""` = nothing), `fit`, `loot_table` | `/loot spawn <p> mine <block> mainhand` run as the sweep player holding the tool; 400 samples per tool for each block's first palette state, 64 for every other state |
| `block_updates/<family>.json` | `{family, method, row, rows: [...]}`, one row per line, in the shape `row` describes: the states BDS stored after each block scenario (stair corners, fence/pane/bars/wall connections, wall heights and posts, fence gate `in_wall_bit`, which neighbours hold each block up, which blocks hold attached blocks, plant soils, wall junctions, two-block halves); `connection_faces.json` is derived from `connections.json`: the blocks whose face fences, panes, bars and walls treat differently from their collision shape | `sweep.py scenarios`: blocks set through the Endstone block API with or without the block update a placement triggers, in waves of side-by-side cells on the superflat world, read back after the wait each file's `method` states |
| `block_updates/` (round 2) | `stairs_blocks.json` (every stair block), `connections_blocks.json` (every fence, pane, bars and wall with a curated set of neighbours; `connection_faces.json` is now derived from both connection files and lists single-state exceptions such as the full snow layer), `wall_heights_blocks.json` (every wall under a set of blocks), `multiface_support.json` (vine, glow lichen, sculk vein and resin clump faces as their holders go), `rails.json` (rail shapes from neighbouring rails), `soils_more.json` (lily pads, wither roses, red shrubs, mushrooms in daylight, cactus flowers and frog spawn, liquids included; `plant_soils.json` is derived from both soil files and lists single soil states such as a water source) | `sweep.py scenarios --family stairs_blocks` and the round-2 families in `scenarios.py` |
| `block_actor_defaults.json` | `{method, actors: {block: hex}}`: the block actor (network NBT) BDS announces right after a player places each block, signs left out | derived from the placement captures (`crates/unastar/src/server/game/testdata/bds-placements*.json`) by the ignored test `write_block_actor_defaults`; `block_actor_defaults_match_the_placement_captures` checks it is current, so it is not in the manifest |
| `armor_slots.json` | `{item: {head, chest, legs, feet, offhand}}`, true when the slot holds the item after the command | `/replaceitem entity <player> slot.armor.<slot>\|slot.weapon.offhand 0 <item>` |
| `item_blocks.json` | `{items: {item: {block, states} or null}, not_given: [...]}`: the block state BDS describes a stack of each item with (`null`: the item is no block item, e.g. doors, beds, signs, hoppers); `not_given` lists the items `/replaceitem` refuses (internal block items such as double slabs and wall signs) | `sweep.py item-blocks`: `/replaceitem entity <player> slot.hotbar 0 <item> 1` for every registry item, slot 0 of the next main-inventory `InventoryContent` the scripted client received |
| `fuel_burn_ticks.json` | `{item: ticks}` for every item that lights a furnace | furnace with one cobblestone input and the item as fuel; server ticks from lit to unlit (±1) |
| `armor_values.json` | `{item: {slot, defense, toughness_range, per_hit}}`: health lost per hit for each damage amount; `defense` exact (from a 1000-point hit); `toughness_range` = interval per damage amount where the 1/hits read-out resolves it, else null (formula in `summarize.py`) | wearing only that (unbreakable) item, `/damage <player> D entity_attack` repeated past the hurt cooldown, max health 2000, natural regeneration off |
| `armor_wear.json` | `{item: {slot, wear: {damage amount: durability lost}}}` for one hit of each amount | wearing only that (breakable) item, one `/damage <player> D entity_attack` per D, the piece read back before and after each hit |
| `attack_damage.json` | `{tool key: {tool_item, damage: [per trial], consistent, gamemode, wear}}`; `wear`: durability damage each trial added to the held stack, the ignored warm-up attack first | the scripted player (survival) attacks a fresh pig (InventoryTransaction attack); Endstone `ActorDamageEvent` value; the held stack read back after every trial |
| `mining_ticks.json` | `{block state: {variant: {ticks: [trials], median, tool_item, commands, on_ground, in_water, wear}}}`; variants: tool key plus `+efficiencyN`, `+hasteN`, `+mining_fatigueN`, `+underwater`, and held items by name; `wear`: durability damage each break added to the held stack. Airborne is not measured (levitation lifts the player out of reach); the `+underwater` cells are unreliable (the water ran off the player's head between trials) | the scripted player keeps mining (Start/ContinueDestroyBlock); ticks from the click event to the break event (±1 tick); the held stack read back after every break |
| `recipe_legacy_identities.json` | canonical input/output item and aux identities for legacy recipe spellings | pinned 1.26.51 CraftingData capture; `../../tools/extract_recipe_identities.py` |
| `builtin_recipes.json` | native shaped/shapeless/user-data recipes absent from pack JSON, stable UUID groups/multi identities, result NBT and canonical block states (`null` explicitly means no block state) | same pinned capture; transient network ids/random UUID values discarded; source SHA-256 included |
| `enchantments.json` | `{source, enchantments: {name: {lang_key, max_level, weight, primary_slots, secondary_slots, min_cost, exclusive_group, excludes, list, treasure, discoverable}}}`: the rules of every enchantment, keyed by the bare names of the BDS 1.26.51 `Enchant` command enum; slots are `0x` bit masks (`source.slot_bits` names them) | moved unchanged from the random-enchanting table in `unastar-rules` and the `/enchant` lang keys (provenance in `source`); not a sweep artefact. `unastar-rules/build.rs` generates the `Enchantment` type from it (decision 0052) |
| `potion_types.json` | lowercase pack potion type identifier to measured aux | unique labelled-graph solution against all captured brewing edges; source SHA-256 included |
| `block_name_keys.json` | block identifier to the translation key command feedback names it by (`minecraft:dirt` → `tile.dirt.default.name`); blocks the `Block` command enum refuses are absent | `capture.py --commands` session of `/testforblock 8 199 8 <block>` for every palette block (default state) against stone, `--command-wait 150`, answers paired in order; `commands.testforblock.failed.tile` names the expected block; source SHA-256 of the session's `command-outputs.json` included |

`loot_table` uses the behaviour-pack loot-table schema (`pools`, `rolls`,
`entries` of `type` `item`/`empty`, `weight`, `functions` `set_count` and
`set_data`). `fit: "exact"` means every sample produced the same outcome and
the table reproduces it exactly. `fit: "empirical"` means the outcome varied:
there is one pool per dropped item, and each observed count of that item is an
entry weighted by how often it was seen. Items are treated as independent of
each other, and the weights are sample frequencies (see `samples`), not
derived probabilities. Consumers that need the exact random rule (fortune
formulas, flint chance and similar) derive it from `observed` and record the
derivation; the measured histogram stays the evidence. `fit: "unstable_block"`
(buttons, frames, wall banners, tripwire hooks, … without support) means the
block popped off during sampling, so `observed` is not a drop table and
`loot_table` is null; `fit: "not_placeable"` means BDS stores another block
for that state (flowing liquids, bubble columns, soul fire); `fit:
"server_crash"` means setting that state crashed Endstone (access violation in
`Block.set_data`) and nothing was measured. Exact cases that differ only in the
block state are folded into one case listing every state.

Tool keys: `hand`; `<tier>_<kind>` for `minecraft:<tier>_<kind>` (tiers wooden,
stone, copper, iron, golden, diamond, netherite; kinds pickaxe, axe, shovel, hoe,
sword); `shears`; enchantments appended as `+<enchantment><level>`
(`diamond_pickaxe+fortune3`, `diamond_shovel+silk_touch1`). Per block the
drops sweep uses: hand, every tier of each correct tool (protocolgen native
mining classes), one wrong tool (diamond), shears, silk touch and fortune
I-III on the diamond correct tool (pickaxe when none). Mining variants add
`+efficiencyN`, `+haste1/2`, `+mining_fatigue1`, `+underwater` (diamond correct
tool), `hand+haste1/2/3`, `hand+mining_fatigue1/2`, and held items keyed by name
(`apple`, `iron_chestplate`, `wooden_spear`, ...).

Coverage: drops for every canonical block state of the protocolgen 1.26.51
palette (22,091 states, 287,231 (state, tool) cells); 5 states crashed Endstone
(43 cells: one yellow glazed terracotta, one prismarine double slab and three
hanging-sign states); mining for the 10-block slice of `sweep.py`, Haste I-III
and Mining Fatigue I-II by hand, every non-tool item the parity catalog gives a
`digger` facet (110) on dirt, netherrack and oak leaves, and every tool's speed
and wear on dirt and netherrack (and on wool, vine, glow lichen and short grass
for shears and the weapons); armour slots and fuels for every registry item;
armour values and wear per hit for every `minecraft:is_armor` item; attack damage and wear for
every tool tier, trident, mace, spear and sharpness I-V. Not measured yet:
airborne and underwater mining, enchantability, mining for other blocks.

Notes from the sweeps:

- `/replaceitem` puts any item into `slot.armor.head` and `slot.weapon.offhand`;
  chest, legs and feet only take items that fit them. This is the command path;
  inventory transactions can be stricter.
- `/loot spawn … mine` drops each item of a stack as a separate item actor (two
  diamonds from fortune are two actors with count 1); `observed` keeps them
  separate.
- The first attack after the held item changes deals no damage; the attack job
  makes one warm-up attack before its three measured trials.
