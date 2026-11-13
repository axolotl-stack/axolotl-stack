# rules-engine Specification

## Purpose
`crates/unastar-rules` holds the bevy-free rule engines that Bedrock pack data needs: filters, actor event trees, the Molang parser and evaluator, loot tables and random enchanting. The runtime (`unastar`), the pack loader (`pack-loader`, also run by build scripts), `unastar-data` and the Bedrock-style worldgen codegen all use these one implementations. Callers supply facts through context traits, so the crate never sees ECS, items or worlds. Which vanilla behaviours each rule reproduces is tracked in `docs/parity/` and `docs/quirks/`, not here. Why: [decision 0021](../../../docs/decisions/0021-shared-rules-engines-in-bevy-free-crate.md).

## Requirements

### Requirement: One bevy-free rules crate
`unastar-rules` SHALL NOT depend on bevy, an ECS, an item registry or a world type; its runtime dependencies MUST stay `serde`, `serde_json`, `thiserror` and `rand`. Filter, event, Molang and loot parsing MUST exist only here, and the worldgen codegen MUST compile Molang from this crate's syntax tree instead of a second parser.

#### Scenario: Dependency boundary
- **WHEN** `crates/unastar-rules/Cargo.toml` is inspected
- **THEN** the only normal dependencies are `serde`, `serde_json`, `thiserror` and `rand`

#### Scenario: Worldgen codegen reuses the parser
- **WHEN** `unastar_bedrock_codegen` compiles a feature-rule Molang string
- **THEN** it imports the syntax tree from `unastar_rules::molang` and only adds its own Rust emitter

Evidence: crates/unastar-rules/Cargo.toml, crates/unastar-rules/src/lib.rs, crates/unastar_bedrock_codegen/src/molang.rs, crates/unastar-packs/Cargo.toml

### Requirement: Parse findings are diagnostics or errors, never silent
Each parser SHALL report a tolerated oddity as a `Diagnostic` (JSON-pointer-like path plus message) and a structural fault as a `ParseError` that rejects the document. Data that is understood but not implemented MUST be kept so it fails when used, not dropped while parsing.

#### Scenario: Tolerated oddity
- **WHEN** an event node has `"remove": {"minecraft:silverfish_calm": {}}`
- **THEN** it parses with no groups removed and one diagnostic at `e/remove/minecraft:silverfish_calm`, matching BDS ([quirk](../../../docs/quirks/bds.md))

#### Scenario: Structural fault
- **WHEN** a filter `{"test": "is_family"}` has no value
- **THEN** `Filter::parse` returns a `ParseError` whose message contains `requires a value`

Evidence: crates/unastar-rules/src/lib.rs (`Diagnostic`, `ParseError`, `Diagnostics`), crates/unastar-rules/src/event.rs (test `silverfish_remove_without_component_groups_is_tolerated`), crates/unastar-rules/src/filter.rs (test `omitted_fields_take_defaults`)

### Requirement: Filter spellings normalise to one form
`Filter::parse` SHALL accept the readable and compact filter spellings BDS accepts and `Filter::to_json` MUST write one canonical form: groups `all_of`, `any_of`, `none_of`; operators `==`, `!=`, `<`, `<=`, `>`, `>=`; subjects by name; every field explicit. Compact integer subjects MUST map through the enum generated at build time from the pinned Mojang `FilterSubject.json`, not a hand-written table. Normalising a canonical filter MUST change nothing.

#### Scenario: Aliases
- **WHEN** groups `all`/`AND`, `any`/`OR`, `NOT` and operators `equals`/`=`, `not`/`<>` are parsed
- **THEN** they are written as `all_of`, `any_of`, `none_of`, `==` and `!=`

#### Scenario: Compact and readable forms agree
- **WHEN** a filter using `AND`/`OR` with `subject: 1` and `operator: 0`/`1` is normalised next to its readable spelling
- **THEN** both give the same JSON, and normalising that JSON again gives it unchanged

#### Scenario: Compact subjects and defaults
- **WHEN** integer subjects 2 to 9 are normalised
- **THEN** they become `player`, `target`, `parent`, `baby`, `block`, `damager`, `holder` and `item`
- **AND** `{"test": "in_contact_with_water"}` becomes subject `self`, operator `==`, value `true`, and `[]` or `{}` becomes an empty `all_of`

Evidence: crates/unastar-rules/src/filter.rs (`Filter::parse`, `Filter::to_json`, `Subject`, `Operator`; tests `group_and_operator_aliases`, `compact_and_readable_spellings_normalise_identically`, `compact_subjects_normalise_using_canonical_enum_ids`, `omitted_fields_take_defaults`, `compact_difficulty_integer_is_named`), crates/unastar-rules/build.rs

### Requirement: Filter names resolve before evaluation
`FilterRegistry` SHALL map each test name to a `TestId` while parsing, with the built-in tests always present in a fixed order. `register` MUST add an addon test before data using it is parsed and MUST fail for a name already taken. An unknown test name MUST be kept with a diagnostic and MUST evaluate to `false`; an unknown group key MUST be ignored with a diagnostic.

#### Scenario: Built-in table
- **WHEN** the built-in tests are listed
- **THEN** their order matches `BuiltinTest` and no name repeats

#### Scenario: Unknown test
- **WHEN** `{"test": "made_up", "value": 3}` is parsed and evaluated with a context that answers every question with a match
- **THEN** there is one diagnostic, `to_json` keeps `made_up`, and the filter is `false`

#### Scenario: Taken name
- **WHEN** `addon_glowing` is registered twice with different value kinds, or a built-in name is registered
- **THEN** the second call fails with "filter test addon_glowing is already registered" and the first definition stays

Evidence: crates/unastar-rules/src/filter.rs (`FilterRegistry::vanilla`, `FilterRegistry::register`, `FilterRegistry::lookup`, `TestId`; tests `builtin_table_matches_enum_order`, `unknown_test_is_kept_logged_and_false`, `registering_a_taken_test_name_fails_and_keeps_the_first`), crates/unastar-packs/tests/vanilla.rs (`every_vanilla_filter_test_is_implemented`)

### Requirement: Filter evaluation over caller facts
`Filter::evaluate` SHALL be pure: it asks the caller's `FilterContext::fact` for each test and applies groups, subjects and operators. A `None` fact (subject absent) MUST make the test `false` whatever the operator. Matching tests MUST support only `==` and `!=`. An empty `all_of` MUST pass and an empty `any_of` MUST fail.

#### Scenario: Missing subject
- **WHEN** `other` is absent and `{"test": "is_family", "subject": "other", "operator": "!=", "value": "player"}` is evaluated
- **THEN** it is `false`
- **AND** `none_of` around the same test with `==` is `true`

#### Scenario: Operators
- **WHEN** health is 8 and the actor's families are `zombie` and `monster`
- **THEN** `actor_health > 4` and `<= 8` pass, `actor_health == 4` fails, and `is_family < zombie` fails

Evidence: crates/unastar-rules/src/filter.rs (`Filter::evaluate`, `FilterContext`, `Fact`; test `evaluation_applies_groups_operators_and_missing_subjects`)

### Requirement: Event tree parsing
`EventNode::parse` SHALL accept a node with optional `filters`, leaf actions and at most one of `sequence`, `randomize` or `first_valid`, and `to_json` MUST write one canonical form. `trigger` MUST accept an event name or an object with `event` and optional `target` (default `self`) and `filters`. Every `randomize` child MUST have a `weight`. Two child lists, a non-array `component_groups` or a trigger without an event MUST be rejected.

#### Scenario: Trigger spellings
- **WHEN** `"trigger": "minecraft:grow"` and `"trigger": {"event": "minecraft:grow", "target": "self"}` are parsed
- **THEN** both write `{"event": "minecraft:grow", "target": "self"}` and reparsing that gives an equal node

#### Scenario: Structural errors
- **WHEN** a `randomize` child has no weight, a node has both `sequence` and `first_valid`, `add.component_groups` is a string, or a trigger has only `target`
- **THEN** each is rejected

Evidence: crates/unastar-rules/src/event.rs (`EventNode::parse`, `EventNode::to_json`, `Trigger`; tests `trigger_spellings_normalise`, `rejects_structural_errors`)

### Requirement: Event evaluation order
A node whose filters fail SHALL produce nothing. A passing node MUST emit its removes, then adds, then other actions in key order, then its children: `sequence` runs every passing child, `randomize` picks one passing child by weight from the caller's random value in `[0, 1)`, and `first_valid` runs the first passing child only. Triggers MUST be collected and returned for the caller to run after the tree. Startup-bound actor trees MUST use the same traversal (`evaluate_tree`).

#### Scenario: Order and node kinds
- **WHEN** a node removes `r`, adds `a`, triggers `t`, and has a `sequence` of a passing child adding `z`, a failing child, and a `first_valid` whose second child adds `fv_any` and plays a sound
- **THEN** effects are remove `r`, add `a`, add `z`, add `fv_any`, the sound action, and one trigger is returned

#### Scenario: Weighted pick among passing children
- **WHEN** `randomize` has weights 95 (`adult`), 5 (`baby`) and a failing child of weight 1000
- **THEN** random values 0.0 and 0.94 pick `adult` and 0.96 and 0.999999 pick `baby`

#### Scenario: Failing filters
- **WHEN** a node's filter fails
- **THEN** it returns no effects and no triggers

Evidence: crates/unastar-rules/src/event.rs (`EventNode::evaluate`, `EventTree`, `evaluate_tree`, `Effect`, `Outcome`; tests `evaluation_order_and_node_kinds`, `randomize_picks_by_weight_among_passing_children`, `failing_filters_skip_the_node`), crates/unastar/src/entity/definition/events.rs

### Requirement: Unknown event actions disable their node
An unknown event action key SHALL be kept verbatim in the normalised form and reported as a diagnostic, and the node holding it MUST execute nothing: no known sibling actions and no triggers. This copies BDS ([quirk](../../../docs/quirks/bds.md), entities/events row).

#### Scenario: Unknown action beside known ones
- **WHEN** a node has `addon:action`, `trigger`, `set_property` and `queue_command`
- **THEN** parsing reports a diagnostic, `to_json` keeps `addon:action` unchanged, and evaluation returns no effects and no triggers

Evidence: crates/unastar-rules/src/event.rs (test `unknown_actions_survive_normalisation_without_executing`), crates/unastar-packs/tests/fixtures/bds-1.26.51-fidelity.json

### Requirement: One Molang parser
`molang::parse` SHALL be the only Molang lexer and parser. Names MUST be lower-cased at parse time while string literals keep their case, `q.`/`v.`/`t.`/`c.` MUST parse to the same node as `query.`/`variable.`/`temp.`/`context.`, and operators MUST follow the documented precedence (loosest `=`, tightest `->`). Resource-pack-only namespaces, unknown `math.` functions, wrong arities and assignments other than to `variable.` or `temp.` MUST be parse errors.

#### Scenario: Case and aliases
- **WHEN** `Math.Random(1, 3)`, `Query.IS_BABY`, `v.x`, `t.x` and `c.other` are parsed
- **THEN** they equal `math.random(1,3)`, `q.is_baby`, `variable.x`, `temp.x` and `context.other`, and `'Abc'` stays `Abc`

#### Scenario: Precedence
- **WHEN** `1 + 2 * 3`, `10 - 4 - 3` and `1 ? 2 : 0 ? 3 : 4` are parsed
- **THEN** the root is the addition, subtraction associates left, and the ternary nests in its else branch

#### Scenario: Rejected input
- **WHEN** `1 +`, `math.max(1)`, `math.nope(1)`, `geometry.x`, `foo`, `'open`, `1 2`, `query.`, `c.other = 1` or `1 = 2` is parsed
- **THEN** each is an error

Evidence: crates/unastar-rules/src/molang/mod.rs (`parse`, `parse_at`, `Expr`; tests `names_are_case_insensitive_and_aliases_resolve`, `precedence_and_associativity`, `complex_expressions`, `rejects_malformed_input`, `every_vanilla_string_parses`)

### Requirement: Molang queries bind before evaluation
Query names SHALL be resolved by `Expr::bind` against a `QueryRegistry` before evaluation, so evaluation does no name lookups. Binding MUST fail for an unregistered query and for a query unavailable in the chosen `QueryContext`: `had_component_group` only in `PropertyDefault`, live actor queries only in `Runtime`. Evaluating an unbound query MUST be an error, and a `loop` count above 1024 MUST be an error. `QueryRegistry::register` MUST reject a taken name.

#### Scenario: Context availability
- **WHEN** `Q.Had_Component_Group('has_nectar') ? 1 : 0` binds in `PropertyDefault` and `query.is_baby` binds in `Runtime`
- **THEN** both bind and evaluate from the caller's context
- **AND** each fails to bind in the other context, and `query.made_up` fails to bind

#### Scenario: Unbound evaluation
- **WHEN** `query.is_baby` is evaluated without binding
- **THEN** evaluation returns `EvalError::Unbound`

#### Scenario: Loop limit
- **WHEN** `loop(1024, ...)` and `loop(1025, ...)` are evaluated
- **THEN** the first runs 1024 times and the second returns `EvalError::LoopLimit(1025)` before any iteration runs
- **AND** zero or negative counts run nothing and fractional counts truncate, so `loop(1024.9, ...)` is allowed

#### Scenario: Taken query name
- **WHEN** `addon_mood` is registered and then `Addon_Mood` or the built-in `is_baby` is registered
- **THEN** both later calls fail and `addon_mood` keeps its first id

Evidence: crates/unastar-rules/src/molang/mod.rs (`Expr::bind`, `QueryRegistry::register`, `QueryContext`; test `registering_a_taken_query_name_fails_whatever_its_case`), crates/unastar-rules/src/molang/eval.rs (`evaluate`, `EvalError`, `MolangContext`, `LOOP_LIMIT`; tests `query_context_separates_saved_property_defaults_from_live_actor_queries`, `unbound_query_is_an_error`, `variables_temps_programs_and_loops`, `loops_run_at_most_1024_times`), crates/unastar-packs/tests/vanilla.rs (`every_vanilla_molang_string_parses_and_binds`)

### Requirement: Loot decoding keeps everything
`loot::decode_table` SHALL decode a Bedrock loot table into typed pools, entries, conditions and functions while keeping the full source JSON in `LootTable::canonical`. A condition or function it does not model MUST become an explicit `Other { name, json }` value, never be dropped. A non-object table, non-object pool or unknown entry type MUST be a `ParseError`. `LootTables` MUST accept rows by name before it is shared.

#### Scenario: Unmodelled clause retained
- **WHEN** a function guard uses `entity_properties` with a property other than `on_fire`
- **THEN** the table decodes and the clause is kept for evaluation to reject

Evidence: crates/unastar-rules/src/loot/decode.rs (`decode_table`), crates/unastar-rules/src/loot/mod.rs (`LootTable`, `LootTables::insert`, `LootCondition::Other`, `LootFunctionKind::Other`), crates/unastar-rules/src/loot/tests.rs (`additional_entity_predicates_are_not_silently_discarded`), crates/unastar-data/src/loot.rs (`vanilla`)

### Requirement: Loot enchantment names are typed at load
`loot::decode_table` SHALL decode the enchantment names in `match_tool`, `table_bonus`, `apply_bonus` and `specific_enchants` to `Enchantment` values. An unknown name MUST be a `ParseError` whose path starts with the table name, so a bad name in the embedded vanilla tables fails the `unastar-data` build.

#### Scenario: Unknown enchantment rejected at load
- **WHEN** a table's `apply_bonus` names `speed`, its `specific_enchants` names `foo:fortune`, or its `match_tool` names `speed`
- **THEN** decoding fails with a `ParseError` naming the table and the unknown enchantment

#### Scenario: Known names decode
- **WHEN** a table's `apply_bonus` names `minecraft:fortune` and its `specific_enchants` lists `swift_sneak` at `[1, 3]` and `protection` at 4
- **THEN** the decoded function holds `Enchantment::Fortune`, and the enchants hold `SwiftSneak` 1..=3 and `Protection` 4..=4

Evidence: crates/unastar-rules/src/loot/decode.rs (`enchantment`, `specific_enchant`), crates/unastar-rules/src/loot/tests.rs (`enchantment_names_are_typed_at_decode`, `an_unknown_enchantment_rejects_the_table_naming_it`), crates/unastar-data/build/loot.rs

### Requirement: Loot evaluation fails explicitly
`loot::roll` SHALL roll a table with the caller's RNG and `LootContext` and return owned `LootDrop`s. An `Other` condition or function MUST return `LootError::Unsupported`, a missing context fact `MissingContext` (distinct from a provider answering "no result"), an unknown referenced table `UnknownTable`, and nesting past 16 tables `TooDeep`. A function's conditions MUST run before it touches context or draws random numbers.

#### Scenario: Unsupported predicate
- **WHEN** `set_count` is guarded by `entity_properties` with `on_fire` and `is_baby`
- **THEN** rolling returns `Unsupported("entity_properties")`

#### Scenario: Missing context versus a miss
- **WHEN** a `furnace_smelt` table is rolled without a furnace provider, and again with a provider that finds no recipe
- **THEN** the first returns `MissingContext("furnace_smelt")` and the second returns the item unchanged with its count

#### Scenario: False guard
- **WHEN** a later `set_count` has a condition that fails
- **THEN** the count set by the earlier function stays

#### Scenario: Unknown or too-deep references
- **WHEN** an entry references an absent table, a chain of table references 17 deep, a table that references itself, or 17 levels of nested `pools`
- **THEN** the first returns `UnknownTable("entities/absent")` (never an empty drop) and the others return `TooDeep`, while a chain 16 deep still drops its item

#### Scenario: Tiered and nested pools
- **WHEN** a tiered pool and a pool whose entry carries nested `pools` are rolled
- **THEN** the tier is picked by index and bonus, ignoring entry conditions, and nested pools roll only after their entry created an item

Evidence: crates/unastar-rules/src/loot/evaluate.rs (`roll`, `LootContext`, `LootDrop`, `LootError`, `MAX_TABLE_DEPTH`), crates/unastar-rules/src/loot/tests.rs (`additional_entity_predicates_are_not_silently_discarded`, `missing_furnace_context_is_distinct_from_a_recipe_miss`, `false_function_guard_does_not_overwrite_an_earlier_count`, `no_this_actor_is_not_an_actor_whose_fire_state_is_false`, `tiers_pick_by_index_and_bonus_and_ignore_entry_conditions`, `nested_pools_roll_only_after_their_entry_created_an_item`, `a_reference_to_an_absent_table_is_an_error_not_an_empty_drop`, `table_references_resolve_up_to_sixteen_levels_deep`, `nested_pools_fail_past_the_depth_limit`), crates/unastar/src/server/game/loot/tests.rs (`block_tables_only_use_supported_features`)
Verified by: parity row `crafting.loot_tables` in [systems.toml](../../../docs/parity/systems.toml)

### Requirement: Random enchanting without an item registry
`enchanting` SHALL select and add enchantments from the caller's `ItemEnchanting` (slot mask and enchantability) and the caller's RNG, with no item registry. `select_enchantments` MUST return nothing for an item without slots or enchantability, else only discoverable enchantments that fit the item (treasure only when allowed), at levels the rolled cost reaches, never two that conflict. `add_enchantment` MUST refuse an enchantment that fits no slot or conflicts with the list.

#### Scenario: Selection stays valid
- **WHEN** enchantments are selected for many items and positive levels
- **THEN** every result is non-empty, fits the item, has a level within the enchantment's range and has no conflicting pair

#### Scenario: Adding to an existing list
- **WHEN** enchantments are added to a sword
- **THEN** compatible ones are added, a repeat raises the level, and an exclusive-group conflict or wrong slot is refused

Evidence: crates/unastar-rules/src/enchanting.rs (`select_enchantments`, `add_enchantment`, `ItemEnchanting`, `EnchantmentInstance`; tests `selected_enchantments_fit_the_item_and_never_conflict`, `levels_follow_the_minimum_costs`, `unenchantable_items_stay_plain`, `adding_respects_slots_lists_and_exclusive_groups`, `generated_rows_keep_the_measured_table`), crates/unastar-rules/src/ids.rs (`Enchantment::max_level`)
Verified by: parity row `crafting.enchantments` in [systems.toml](../../../docs/parity/systems.toml)

### Requirement: Comments name no engine internals
Comments in `unastar-rules` sources SHALL describe behaviour, never engine symbol names (decision 0016). A test MUST scan every comment in `src/` and fail on a backticked `Upper::Upper` or `Upper::lowerCamel` name, or on a backticked lowerCamelCase identifier. Intra-doc links, snake_case Rust items such as `Outer::new`, and namespaced data ids MUST pass. The test's own examples MUST be made-up names.

#### Scenario: Engine-shaped name in a doc comment
- **WHEN** a comment contains a backticked `Outer::Inner`, `Outer::doThing` or `doThing(50)`
- **THEN** `comments_name_no_engine_internals` fails naming the file and line

#### Scenario: Rust and data names pass
- **WHEN** a comment contains `Outer::do_thing`, `crate::loot`, `minecraft:muttonRaw` or the link [`ValueKind::Bool`]
- **THEN** none is flagged

Evidence: crates/unastar-rules/tests/provenance.rs (`comments_name_no_engine_internals`, `the_scanner_flags_both_shapes_and_spares_links_and_snake_case`), crates/unastar-rules/src/enchanting.rs, docs/decisions/0016-describe-behaviour-never-engine-internals.md

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
