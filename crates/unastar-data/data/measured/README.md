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
| `block_drops/<block>.json` | `{block, cases: [...]}`; one case per (block state, tool): `states`, `tool` key (`hand`, `<tier>_<kind>`, `+silk_touch1`, `+fortune3`, …), `tool_item` (item and enchantments), `samples`, `observed` (histogram of whole outcomes, `name@data` + `x` + count per dropped stack, `;`-separated; `""` = nothing), `fit` and `loot_table` | `/loot spawn <p> mine <block> mainhand` run as the sweep player holding the tool |
| `armor_slots.json` | `{item: {head, chest, legs, feet, offhand}}`, true when the slot holds the item after the command | `/replaceitem entity <player> slot.armor.<slot>\|slot.weapon.offhand 0 <item>` |
| `fuel_burn_ticks.json` | `{item: ticks}` for every item that lights a furnace | furnace with one cobblestone input and the item as fuel; server ticks from lit to unlit (±1) |
| `armor_values.json` | `{item: {slot, defense, toughness, per_hit}}`: health lost per hit for each damage amount, and defence/toughness derived from it (formula in `summarize.py`; toughness null when no measured hit isolates it) | wearing only that item, `/damage <player> D entity_attack` repeated past the hurt cooldown, max health 2000 |
| `attack_damage.json` | `{tool key: {tool_item, damage: [per trial], consistent}}` | the scripted player attacks a fresh pig (InventoryTransaction attack); Endstone `ActorDamageEvent` value |
| `mining_ticks.json` | `{block state: {variant: {ticks: [3 trials], median, tool_item, commands, on_ground, in_water}}}`; variants: tool key plus `+haste1`, `+mining_fatigue1`, `+underwater`, `+levitation256` | the scripted player keeps mining (Start/ContinueDestroyBlock); ticks from the click event to the break event (±1 tick) |

`loot_table` uses the behaviour-pack loot-table schema (`pools`, `rolls`,
`entries` of `type` `item`/`empty`, `weight`, `functions` `set_count` and
`set_data`). `fit: "exact"` means every sample produced the same outcome and
the table reproduces it exactly. `fit: "empirical"` means the outcome varied:
there is one pool per dropped item, and each observed count of that item is an
entry weighted by how often it was seen. Items are treated as independent of
each other, and the weights are sample frequencies (see `samples`), not
derived probabilities. Consumers that need the exact random rule (fortune
formulas, flint chance and similar) derive it from `observed` and record the
derivation; the measured histogram stays the evidence.

Notes from the sweeps:

- `/replaceitem` puts any item into `slot.armor.head` and `slot.weapon.offhand`;
  chest, legs and feet only take items that fit them. This is the command path;
  inventory transactions can be stricter.
- BDS drops each item of a stack as a separate item actor (two diamonds from
  fortune are two actors with count 1); `observed` keeps them separate.
