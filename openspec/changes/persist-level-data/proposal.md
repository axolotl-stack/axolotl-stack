# Proposal: persist level data

## Why

unastar saved chunks and player records but nothing the world keeps beside them: after a restart
the clock, game rules, world spawn, scoreboard, ticking areas, `disk` structures and mob event
switches were gone, and a world BDS saved lost all of them when unastar opened it. The commands
lane's round 2 listed these as the gaps of `setworldspawn`, `scoreboard`, `tickingarea`,
`structure` and `mobevent` (`docs/parity/commands.toml`), and `world.persistence` /
`world.game_rules` in `docs/parity/systems.toml` say level data is not saved.

Prior work: no `docs/solutions` note or decision covers level data. Decision 0013 (copy BDS
exactly) and 0014 (measure built-in facts) set the approach; 0030 (26.x world clocks) is why the
clock also has a `WorldClocks` record; 0033 and the open change `persist-block-states-by-name`
cover sub-chunk palettes, which this change does not touch. The `world-storage` spec describes
chunk and player persistence only.

## What Changes

- Measure where BDS 1.26.51 keeps each value (`tools/bds-sweep/level_data.py`: change every
  value with commands, stop, decode `level.dat` and the world LevelDB) and commit the decoded
  evidence (`crates/unastar/tests/fixtures/bds_1_26_51_level_data.json`).
- Read and write `level.dat` in BDS's layout (8-byte header, little-endian NBT with keys in byte
  order), keeping every field unastar does not use; a new world gets BDS's fresh-world fields.
- Read and write the world database's level records: `scoreboard`, `mobevents`,
  `tickingarea_<uuid>`, `structuretemplate_<name>` and `WorldClocks` (LevelDB keys as BDS; a
  checked side file for BlazeDB).
- Load level data at startup, before players join; save it at shutdown after the chunks.
  Unreadable `level.dat` and single records stop the start; unreadable ticking area or structure
  records are skipped and left on disk.
- Scores of players and saved actors survive leaving, unloading and restarts (stored by the
  player's saved id and the actor's unique id, as BDS's `PlayerId`/`EntityID`); joining players
  get the displayed objectives and scores.
- New players spawn at the world spawn a command set or the level data holds; StartGame carries
  the level's tick count and clock as BDS does.
- Copied behaviour found on the way: `/tickingarea add circle` centres on its chunk's middle at
  y 0; `sendCommandFeedback` silences players' successful commands and console `/gamerule`
  changes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `world-storage`: adds level data (level.dat and level records) to what is persisted, when it
  loads and saves, and how unreadable level data is handled; the shutdown order gains the level
  data save.

## Impact

- Crates: `unastar` (`storage/level/`, `world/level_data.rs`, `world/scoreboard.rs`, `world/rules.rs`,
  command executors `scoreboard`, `tickingarea`, `structure`, `spawnpoint`, `gamerule`, server
  runtime and join), `unastar-data` (console formatting of literal messages), `bds-extractor`
  (`--uuid` for a stable offline identity).
- Tools: `tools/bds-sweep/level_data.py`, `le_nbt.py`, and `leveldb_read.py` (identical to the copy
  on the `persist-block-states-by-name` branch).
- Player records gain `unique_id` (JSON, defaults when missing).

### Parity rows

| Domain | Row | Target |
|---|---|---|
| commands | `setworldspawn` | `verified` (world spawn saved; new players spawn there) |
| commands | `scoreboard` | stays `partial`: saving done, `belowname` actor data still missing |
| commands | `tickingarea` | stays `partial`: saving done, areas still keep no chunks loaded |
| commands | `structure` | stays `partial`: `disk` saves persist; block states, entities and block entities as before |
| commands | `mobevent` | stays `partial`: switches saved, events still do nothing |
| commands | `gamerule`, `time`, `daylock`, `difficulty`, `weather` | stay `verified`; notes say what is saved |
| systems | `world.game_rules` | stays `partial`; "rules are not persisted" leaves its gaps |

`world.persistence` belongs to the storage work in `persist-block-states-by-name`; its level-data
gap is reported, not edited, here. The commands rows are under the `lane COMMANDS` claim.

### Out of scope

Player records in BDS's NBT format (a separate decision), weather countdowns and the weather
cycle, ticking areas that load chunks, structure entities and block entities, upgrading block
names from older versions, and every `level.dat` field unastar does not act on (kept as read).
