## ADDED Requirements

### Requirement: Item cooldowns are kept by the server
The server SHALL keep each player's item cooldowns by category, from the held item's `minecraft:cooldown` component, on its own tick count, and never trust the client's reported cooldown state. While an attack cooldown runs, a hit or jab with an item of that category MUST change nothing. Starting one MUST send `PlayerStartItemCooldown` with the category and its ticks. Other items MUST NOT be held back.

#### Scenario: A hit inside the cooldown
- **WHEN** a survival player hits a pig with an iron spear and hits another pig with it 18 inputs later
- **THEN** the second pig is unhurt, and a hit 19 inputs after the first lands

#### Scenario: Another item during the cooldown
- **WHEN** a player hits with a diamond spear and then hits with an iron sword 2 inputs later
- **THEN** the sword's hit lands

#### Scenario: Several hits in one input
- **WHEN** a player sends three spear hits on three pigs in one input
- **THEN** only the first pig is hurt and one cooldown starts

Evidence: crates/unastar/src/item/cooldown.rs (`cooldown`, `cooling`, `start`, test `spear_cooldowns_come_from_the_item_documents`), crates/unastar/src/entity/mob/combat/hit.rs (`player_attack`), crates/unastar/src/server/game/item_use/mod.rs (`attack_air`), crates/unastar/src/entity/mob/combat/hit/spear_tests.rs (`spear_hits_jabs_and_swings_answer_like_bds`, replaying `testdata/bds-spears.jsonl`), [docs/quirks/bds.md](../../../../docs/quirks/bds.md) (Items / spears)
