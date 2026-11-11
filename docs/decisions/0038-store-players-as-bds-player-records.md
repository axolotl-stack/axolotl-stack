---
status: accepted
date: 2026-11-10
decision-makers: [project owner]
---

# Store players as BDS's player records, starting with reading them

## Context and Problem Statement

unastar keeps each player as JSON (`PlayerData`) in its own LevelDB under `players.data_dir`,
keyed by the 16 UUID bytes. BDS 1.26.51 keeps players in the world database, so a BDS world
opened in unastar loses every player's position, inventory, experience, effects, tags and spawn
point, and BDS loses unastar's players. Should unastar switch to BDS's records?

Measured with `tools/bds-sweep/level_data.py` (an offline operator player sets its state, leaves,
BDS stops; the records are in `crates/unastar/tests/fixtures/bds_1_26_51_level_data.json`, key
`saved.records`):

- `player_<self-signed id>` holds `{SelfSignedId, ServerId: "player_server_<uuid>"}`; the second
  key holds the player: one rooted little-endian NBT compound, keys in byte order, about 9.6 KB and
  110 top-level fields. The same self-signed id finds the same record in a later session
  (scores, tags and inventory came back).
- What unastar keeps maps onto it: `Pos` (1.62001 above the feet), `Rotation`, `DimensionId`,
  `PlayerGameMode`, `Inventory` (36 entries with `Slot`, empty slots as `Count` 0 and `Name` ""),
  `Armor` (5, the fifth the body slot), `Offhand`, `EnderChestInventory` (27), `PlayerUIItems`,
  `SelectedInventorySlot`, `PlayerLevel`/`PlayerLevelProgress`, health, hunger, saturation and
  exhaustion as `Attributes` (`Base`, `Current`, ranges), `SpawnX/Y/Z` and `SpawnDimension`
  (`i32::MIN` and 3 when unset), `Tags` in the order added, `recipe_unlocking.unlocked_recipes`,
  `UniqueID` (the id the scoreboard stores players under).
- Items are `{Count, Damage, Name, WasPickedUp}` plus `tag` (durability as an Int `Damage`,
  enchantments, names) and, for block items, `Block {name, states, version}`.
- It also holds what unastar does not model yet: `ActiveEffects` (with per-difficulty durations
  and an internal factor record), abilities, permission levels, `EnchantmentSeed`, air, fire,
  UI layouts and about forty generic actor flags.

## Decision Drivers

- Worlds move between BDS and unastar, as level data now does (0037).
- Saved players are never lost or half restored (fail closed, records kept).
- Other lanes are changing the same state: the shared mob/player effect system, the player-state
  follow-up (actor data, flags, movement).

## Considered Options

1. Keep the JSON store.
2. Switch to BDS's records in the world database: keep each player's compound whole, set the
   fields unastar owns, read old JSON records as a legacy source.
3. Read BDS records only for players unastar has no record of (a BDS world keeps its players), and
   keep writing JSON.
4. Write both formats.

## Decision Outcome

Chosen: option 2, reached through option 3 first (accepted by the owner, 2026-11-11). Not
implemented in the commands round 3,
because it is not low-risk yet:

- The key for signed-in (Xbox) players is not measured: offline BDS on loopback only shows the
  self-signed form; a capture with an authenticated client is needed before unastar writes keys
  BDS must find.
- Items need a full conversion to and from BDS's item NBT (block states, `tag` contents), which
  belongs with the item work and fails closed for anything unknown.
- Effects, attributes and abilities are being reworked by other lanes; writing them now would
  freeze a format around state that is about to change.
- The player's `UniqueID` is BDS's actor unique id on the wire; unastar's players use session ids,
  and changing that touches every packet that names a player.

Option 1 keeps BDS worlds without players. Option 4 doubles every save and lets the two copies
disagree.

### Consequences

- Good, because a BDS world would keep its players and BDS would read unastar's.
- Good, because option 3 can ship alone: it only reads, so a wrong conversion cannot damage a
  record.
- Bad, because the JSON store and its configuration (`players.leveldb_enabled`, `data_dir`) would
  be replaced, with a migration of existing records.
- Bad, because unknown item or effect fields would stop a player from joining until supported.

### Confirmation

When this is built, the fixture's player record is its first test: unastar restores what BDS's commands set (17
diamonds, an iron sword, an iron helmet, five torches in the offhand, level 30 and 0.0625
progress, spawn point 10 -60 12, tags red and blue, `minecraft:stick` unlocked, speed and
regeneration) and saves it back unchanged.

## More Information

Player scores already survive restarts through `PlayerData::unique_id` (0037); with option 2 that
id is the record's `UniqueID`.
