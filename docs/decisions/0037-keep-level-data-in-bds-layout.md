---
status: proposed
date: 2026-11-10
decision-makers: [project owner]
---

# Keep level data in BDS's layout and never replace unreadable level data

## Context and Problem Statement

unastar saved chunks and players but no level data: the clock, game rules, world spawn,
scoreboard, ticking areas, `disk` structures and mob event switches were lost on restart, and a
world BDS saved lost them in unastar. Two questions needed answers: in which format to keep level
data, and what to do when stored level data cannot be read. Measured layout and the work:
OpenSpec change `persist-level-data` (archived 2026-11-10), fixture
`crates/unastar/tests/fixtures/bds_1_26_51_level_data.json`.

## Decision Drivers

- Worlds move between BDS and unastar (the storage layer's LevelDB goal, decision 0033).
- Copy BDS exactly (0013), measured rather than assumed (0014).
- Stored data is never silently lost (`crates/unastar/AGENTS.md`: corrupt records are preserved).

## Considered Options

1. BDS's layout: `level.dat` kept whole (fields unastar does not use written back as read) and the
   `scoreboard`, `mobevents`, `WorldClocks`, `tickingarea_<uuid>` and `structuretemplate_<name>`
   records in the world database, compounds written with keys in byte order as BDS writes them.
2. An unastar file (JSON or TOML) beside the database, with only the values unastar uses.

For unreadable data:

- A. Stop the start for an unreadable `level.dat` or single record; skip and keep an unreadable
  ticking area or structure record.
- B. Start with defaults and overwrite at the next save.
- C. Start with defaults and save no level data that session.

## Decision Outcome

Proposed: option 1 with A. BDS's layout is the only one that lets a BDS world keep its settings in
unastar and BDS open unastar's world; keeping the whole compound means no BDS setting is lost by
a save. B loses the record at the first save; C runs a world whose changes vanish without a word.
Per-area and per-structure records are independent, so one bad record should not keep the server
down, and unastar only writes or deletes records it loaded or made.

Scores stay with their holders by stored identity (a player's saved id, an actor's unique id)
while the player's wire id stays the session's; making it persistent belongs to the player-data
decision (0038).

### Consequences

- Good, because a BDS-saved world loads, answers like BDS and saves back byte-identical except
  `LastPlayed`, `worldStartCount` and `Difficulty`, and BDS reads what unastar writes (tests in
  `crates/unastar/src/world/level_data/tests.rs`).
- Good, because damaged level data is never overwritten.
- Bad, because a world with one unreadable singleton record does not start until it is fixed.
- Bad, because strict decoders refuse BDS fields they have not seen in a record, rather than
  guessing.
- Bad, because unastar needs its own small ordered NBT module beside `zuri_nbt`.

### Confirmation

The level-data tests compare against the BDS capture byte for byte; the world-storage spec states
the load, save and failure rules.
