# Proposal: picking actors as BDS does

## Why

`ActorPickRequestPacket` is `missing`. Round 2 measured one case (a creative pick of a cow gives
its spawn egg, `protocol/relay-first`). Which actors give which item, what with data changes and
how block picks compare were not measured. Nothing in `docs/solutions` or `docs/decisions` covers
picks; the block pick (`BlockPickRequestPacket`, items lane, `done`) documents that it sends no
`GuiDataPickItem` because its names were never captured. PR #71 has no pick work.

## What Changes

- `protocol_probe.py pick` (actor kinds, a full hotbar, an egg already held, block picks with and
  without data beside the player, survival), `pick-actors` (every summonable actor picked
  without and with data, and rechecks of the actors it missed) and `pick-reach` (a cow 1 to 24
  blocks away); `pick_table.py` turns a `pick-actors` run into the measured table
  `crates/unastar-data/data/measured/actor_pick_items.json`; fixtures in
  `crates/bds-extractor/captures/1.26.51/protocol/pick` and `pick-actors`.
- bds-extractor: `~` coordinates for `!pick-block`, resolved from the player's position.
- unastar-data: `actor_picks` decodes the table, actor names to `ActorType`, typed errors.
- unastar: `ActorPickRequest` is queued with the inventory packets; a creative pick of a mob in
  the table within 14 blocks selects its item like a block pick and names it with
  `GuiDataPickItem` first.

Crates: `unastar`, `unastar-data`, `bds-extractor`, `tools/bds-sweep` (Python).

## Impact

### Parity rows

`protocol`, owner `protocol`:

| Row | From | To |
|---|---|---|
| `ActorPickRequestPacket` | missing | partial (picks with data of chest vehicles ignored) |
| `GuiDataPickItemPacket` | missing | partial (actor picks only) |

`BlockPickRequestPacket` (items) keeps its status; the new measurements are in the quirks.

## Out of scope

- `GuiDataPickItem` for block picks (needs the name of every block's item) and block picks with
  data of blocks with block entities (items lane).
- Spawning boats, minecarts and end crystals (entities lane), whose picks the table covers.
