# Design

## Context

See proposal.md - Why. The world directory is `worlds/default`; its `db/` is the LevelDB (or
BlazeDB) the chunk provider owns, and LevelDB allows one handle per process, so level records go
through the provider that is already open. The ECS resources the level data feeds already exist
(`WorldTime`, `TickCounter`, `GameRules`, `Difficulty`, `WorldSpawn`, `Weather`/`WeatherLevels`,
`Scoreboard`, `MobEvents`, `TickingAreas`, `Structures`); only the scoreboard identified holders
by live ECS entities.

What BDS 1.26.51 stores and where was measured, not assumed (`tools/bds-sweep/level_data.py`,
fixture `crates/unastar/tests/fixtures/bds_1_26_51_level_data.json`).

## Goals / Non-Goals

**Goals:**

- Byte-for-byte BDS layout for what unastar writes, so the fixture comparisons are exact.
- A world BDS saved loads, runs and saves again without losing a field unastar does not know.
- No path where unreadable stored data is replaced by defaults.

**Non-Goals:**

- Chunk palettes (change `persist-block-states-by-name`), player records in BDS's format
  (decision record for Goal 2 of the commands round), autosave.

## Decisions

### 1. BDS's layout, not an unastar format

Options: (a) `level.dat` and LevelDB keys as BDS writes them; (b) an unastar file (JSON or TOML)
beside the database. Chosen (a): BDS worlds move to unastar and back, and the measured layout is
small. (b) would need a converter for every BDS world.

### 2. Keep the whole `level.dat` compound

Options: (a) keep the decoded compound and set only the fields unastar owns; (b) a typed struct
of known fields. Chosen (a): `level.dat` has about 130 fields, most of which unastar does not act
on (`BiomeOverride`, `experiments`, editor flags); (b) would drop them on every save. A new world
starts from the measured fresh-world compound.

### 3. A small ordered NBT module

Options: (a) a little-endian NBT tree whose compounds are sorted maps; (b) the workspace's
`zuri_nbt`, whose compounds are hash maps. BDS writes every compound with its keys in byte order
(measured on every record), so (a) reproduces BDS's bytes and makes the fixture comparisons exact;
(b) writes keys in a random order. The reader is bounded (depth, lengths against the bytes left).

### 4. Unreadable data stops the start or is left alone

Options: (a) stop the start for unreadable `level.dat`, `scoreboard`, `mobevents`, `WorldClocks`;
skip and keep an unreadable ticking area or structure record; (b) start with defaults and
overwrite at the next save; (c) start with defaults and never save level data that session.
Chosen (a): a single record would be overwritten by the next save under (b), and (c) runs a
world whose changes silently vanish. Per-area and per-structure records are independent, so one
bad record should not keep the server down; unastar deletes only records it loaded or wrote.

### 5. Copy what BDS does on load

Measured and copied: the difficulty comes from the configuration at every start (BDS takes
`server.properties`' and ignores `level.dat`'s); the clock comes from `level.dat` `Time`, not
`WorldClocks`; the tick count (`currentTick`) restarts where it stopped (`/time query gametime`);
`worldStartCount` counts down by one per start; holders without a score are not saved but
`LastUniqueID` keeps their ids used; ticking areas come back in record-key order. Not copied:
BDS prefixes `BiomeOverride` with `minecraft:` on every load (logged as a quirk not copied).

### 6. Scores keep stored identities, wire ids stay

Options: (a) holders that are away become stored identities (`OfflinePlayer(id)`,
`UnloadedActor(id)`) and are bound back to entities when they return; the player's id lives in
its record (`unique_id`); (b) make the player's actor unique id on the wire the persistent id, as
BDS does. Chosen (a) now: it fixes the lost scores without changing every packet that names a
player. (b) is part of the player-data decision.

### 7. Save at shutdown, through the open provider

Level data is captured synchronously from the ECS and written after the chunks (mob unique ids
exist by then). LevelDB writes the records in one batch; BlazeDB keeps them in a CRC-checked side
file replaced atomically. No autosave, like chunks.

## Risks / Trade-offs

- [A crash loses level data changed since start] → same as chunks today; an autosave is a
  separate change.
- [Strict record decoding refuses a BDS world with a field unastar has not seen] → the error names
  the record and field; the data is left as it was.
- [Weather levels BDS fades while no player is online] → not modelled; levels are saved and
  restored as they are.
- [Scores of actors that die are kept under their id] → not measured; kept rather than dropped.
- [Decision record numbers may collide with other open branches] → renumber at merge.

## Migration Plan

Worlds without `level.dat` get a fresh one at the first save. Player records without `unique_id`
get one at their next join. Nothing is migrated in place.
