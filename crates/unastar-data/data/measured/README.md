# Measured BDS facts (decision D4)

Facts BDS 1.26.51 keeps in code, measured on a loopback BDS with the sweep
harness in `tools/bds-sweep` (Endstone plugin `endstone-sweep` plus the
`bds-sweep-client` scripted player). These files are measurements of BDS
behaviour, not copies of Mojang data files; the raw per-run logs
(`results.jsonl`, server logs) stay outside the repository in
`C:/Users/jvigu/orca/bds-runtime/sweeps/1.26.51/`.

`manifest.json` lists every artefact with its SHA-256, the BDS version, the
harness revision, the sweep runs (start time, status, seconds, item count) and
the measurement method. Regenerate a family with `sweep.py` (exclusive BDS
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
| `armor_slots.json` | `{item: {head, chest, legs, feet, offhand}}`, true when the slot holds the item after the command | `/replaceitem entity <player> slot.armor.<slot>\|slot.weapon.offhand 0 <item>` |
| `fuel_burn_ticks.json` | `{item: ticks}` for every item that lights a furnace | furnace with one cobblestone input and the item as fuel; server ticks from lit to unlit (±1) |
| `armor_values.json` | `{item: {slot, defense, toughness_range, per_hit}}`: health lost per hit for each damage amount; `defense` exact (from a 1000-point hit); `toughness_range` = interval per damage amount where the 1/hits read-out resolves it, else null (formula in `summarize.py`) | wearing only that (unbreakable) item, `/damage <player> D entity_attack` repeated past the hurt cooldown, max health 2000, natural regeneration off |
| `attack_damage.json` | `{tool key: {tool_item, damage: [per trial], consistent}}` | the scripted player attacks a fresh pig (InventoryTransaction attack); Endstone `ActorDamageEvent` value |
| `mining_ticks.json` | `{block state: {variant: {ticks: [3 trials], median, tool_item, commands, on_ground, in_water}}}`; variants: tool key plus `+efficiencyN`, `+haste1`, `+haste2`, `+mining_fatigue1`, `+underwater`. Airborne is not measured (levitation lifts the player out of reach) | the scripted player keeps mining (Start/ContinueDestroyBlock); ticks from the click event to the break event (±1 tick) |

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
`+efficiencyN`, `+haste1/2`, `+mining_fatigue1`, `+underwater`.

Coverage: drops for every canonical block state of the protocolgen 1.26.51
palette (22,091 states, 287,231 (state, tool) cells); 5 states crashed Endstone
(43 cells: one yellow glazed terracotta, one prismarine double slab and three
hanging-sign states); mining for the 10-block slice of `sweep.py`; armour
slots and fuels for every registry item; armour values for every
`minecraft:is_armor` item; attack damage for every tool tier, trident, mace and
sharpness I-V. Not measured yet: airborne mining, enchantability, mining for
blocks outside the slice.

Notes from the sweeps:

- `/replaceitem` puts any item into `slot.armor.head` and `slot.weapon.offhand`;
  chest, legs and feet only take items that fit them. This is the command path;
  inventory transactions can be stricter.
- `/loot spawn … mine` drops each item of a stack as a separate item actor (two
  diamonds from fortune are two actors with count 1); `observed` keeps them
  separate.
- The first attack after the held item changes deals no damage; the attack job
  makes one warm-up attack before its three measured trials.
