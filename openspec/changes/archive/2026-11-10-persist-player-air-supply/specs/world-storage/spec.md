## MODIFIED Requirements

### Requirement: Player data is restored before spawn
When `players.leveldb_enabled` is true, player records SHALL be stored in a LevelDB database at `<players.data_dir>/db`, keyed by the 16 UUID bytes, as JSON `PlayerData`. A record MUST be loaded and validated during login and applied when the player entity spawns, before the initial inventory is sent: position, rotation, dimension, game mode, main inventory, armour, offhand, cursor, held slot, ender chest, health, hunger, experience and air. Fields added later MUST default when missing.

#### Scenario: Disconnect and reconnect
- **WHEN** a player's position, rotation, adventure mode, inventory slot 8 (17 diamonds), damaged helmet, offhand torches, held slot, health 13/24, hunger and experience are saved and the player joins again
- **THEN** every value is restored and the first main-inventory `InventoryContent` shows 17 items in slot 8

#### Scenario: First join
- **WHEN** no record exists for the UUID
- **THEN** `load_player_data` returns `None` and first-join defaults apply

#### Scenario: Air supply
- **WHEN** a player saved with 39 ticks of air joins on land
- **THEN** its join `SetActorData` says 300 and the first update after it carries `BREATHING` and 43

Evidence: crates/unastar/src/storage/leveldb_player.rs, crates/unastar/src/storage/provider.rs (`PlayerData`, `StoredItemStack::restore`), crates/unastar/src/server/connect.rs (`load_player_data`), crates/unastar/src/server/game/mod.rs (`spawn_player`, `restore_items`, `player_record`), crates/unastar/tests/persistence.rs (`disconnect_reconnect_restores_inventory_and_player_state`), crates/unastar/src/entity/player_life/tests/actor_data.rs (`a_rejoining_player_keeps_its_air`, `the_saved_record_keeps_the_air`)

### Requirement: Invalid player records are kept, not deleted
A player record that fails to parse or fails `PlayerData::validate` (wrong UUID, non-finite position or rotation, unknown game mode, more than 36 inventory slots, held slot above 8, health outside `0..=max`, food outside `0..=20`, negative experience) SHALL be logged and ignored for that login, and MUST NOT be deleted by the load. A record whose items do not resolve against the item registry MUST make the join fail with a disconnect message instead of spawning the player with lost items.

#### Scenario: Corrupt JSON
- **WHEN** the stored value for a UUID is `{not valid json`
- **THEN** `load_player_data` returns `None` and the stored bytes are unchanged afterwards

#### Scenario: Air out of range
- **WHEN** a record's air supply is outside `-19..=300` (-20 never stays: the drowning hit resets it to 0)
- **THEN** `PlayerData::validate` fails

Evidence: crates/unastar/src/storage/provider.rs (`PlayerData::validate`, `StoredItemStack::restore`, test `air_outside_the_supply_is_invalid`), crates/unastar/src/server/connect.rs (`load_player_data`), crates/unastar/src/server/runtime.rs (`Rejecting player with invalid saved inventory`), crates/unastar/tests/persistence.rs (`corrupt_player_record_falls_back_without_deleting_data`)
