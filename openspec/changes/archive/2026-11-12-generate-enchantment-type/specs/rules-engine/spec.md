## ADDED Requirements

### Requirement: One generated enchantment type
`unastar_rules::ids::Enchantment` SHALL be the only enchantment type in the workspace, generated at build time: ids and names from the BDS 1.26.51 AvailableCommands capture (`Enchant`, id = index), rules from `unastar-data/data/measured/enchantments.json`. The build MUST fail when the capture disagrees with the protocol docs' `Enchant__Type`, the vanilla `mojang-enchantments.json` module or `en_US.lang`, or when the data file misses or adds an enchantment.

#### Scenario: Ids and names match the capture
- **WHEN** every `Enchantment` is converted to its id, name and identifier and back
- **THEN** each id is its index in the captured `Enchant` enum, each name is the captured value, and every conversion round-trips

#### Scenario: Rules come from the data file
- **WHEN** each enchantment's max level, lang key and weight are read
- **THEN** they equal its row in `enchantments.json`

Evidence: crates/unastar-rules/build/enchantments.rs (`write`), crates/unastar-rules/src/ids.rs (`Enchantment`; tests `ids_and_names_match_the_bds_capture`, `rules_come_from_the_data_file`)

### Requirement: Enchantment conversions are checked
`TryFrom<i16>` and `FromStr` for `Enchantment` SHALL return the typed `UnknownEnchantment` error for an unknown id, an unknown name or a namespace other than `minecraft:`, never a panic. `FromStr` MUST accept the bare name, the `minecraft:` identifier and the vanilla aliases.

#### Scenario: Unknown ids and names are typed errors
- **WHEN** id 42, id -1, `speed` or `foo:sharpness` is converted
- **THEN** the result is `UnknownEnchantment::Id` or `UnknownEnchantment::Name`

#### Scenario: Aliases
- **WHEN** `bow_infinity` or `minecraft:bow_infinity` is parsed
- **THEN** the result is `Enchantment::Infinity`

Evidence: crates/unastar-rules/src/ids.rs (`UnknownEnchantment`; tests `unknown_ids_and_names_are_typed_errors`, `vanilla_aliases_parse`, `serde_uses_the_identifier`)

### Requirement: Loot enchantment names are typed at load
`loot::decode_table` SHALL decode the enchantment names in `match_tool`, `table_bonus`, `apply_bonus` and `specific_enchants` to `Enchantment` values. An unknown name MUST be a `ParseError` whose path starts with the table name, so a bad name in the embedded vanilla tables fails the `unastar-data` build.

#### Scenario: Unknown enchantment rejected at load
- **WHEN** a table's `apply_bonus` names `speed`, its `specific_enchants` names `foo:fortune`, or its `match_tool` names `speed`
- **THEN** decoding fails with a `ParseError` naming the table and the unknown enchantment

#### Scenario: Known names decode
- **WHEN** a table's `apply_bonus` names `minecraft:fortune` and its `specific_enchants` lists `swift_sneak` at `[1, 3]` and `protection` at 4
- **THEN** the decoded function holds `Enchantment::Fortune`, and the enchants hold `SwiftSneak` 1..=3 and `Protection` 4..=4

Evidence: crates/unastar-rules/src/loot/decode.rs (`enchantment`, `specific_enchant`), crates/unastar-rules/src/loot/tests.rs (`enchantment_names_are_typed_at_decode`, `an_unknown_enchantment_rejects_the_table_naming_it`), crates/unastar-data/build/loot.rs

## MODIFIED Requirements

### Requirement: Random enchanting without an item registry
`enchanting` SHALL select and add enchantments from the caller's `ItemEnchanting` (slot mask and enchantability) and the caller's RNG, with no item registry. `select_enchantments` MUST return nothing for an item without slots or enchantability, else only discoverable enchantments that fit the item (treasure only when allowed), at levels the rolled cost reaches, never two that conflict. `add_enchantment` MUST refuse an enchantment that fits no slot or conflicts with the list.

#### Scenario: Selection stays valid
- **WHEN** enchantments are selected for many items and positive levels
- **THEN** every result is non-empty, fits the item, has a level within the enchantment's range and has no conflicting pair

#### Scenario: Adding to an existing list
- **WHEN** enchantments are added to a sword
- **THEN** compatible ones are added, a repeat raises the level, and an exclusive-group conflict or wrong slot is refused

Evidence: crates/unastar-rules/src/enchanting.rs (`select_enchantments`, `add_enchantment`, `ItemEnchanting`, `EnchantmentInstance`; tests `selected_enchantments_fit_the_item_and_never_conflict`, `levels_follow_the_minimum_costs`, `unenchantable_items_stay_plain`, `adding_respects_slots_lists_and_exclusive_groups`, `generated_rows_keep_the_measured_table`), crates/unastar-rules/src/ids.rs (`Enchantment::max_level`)
Verified by: parity row `crafting.enchantments` in [systems.toml](../../../../../../docs/parity/systems.toml)
