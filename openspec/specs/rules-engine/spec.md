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

Evidence: crates/unastar-rules/src/filter.rs (`FilterRegistry::vanilla`, `FilterRegistry::register`, `FilterRegistry::lookup`, `TestId`; tests `builtin_table_matches_enum_order`, `unknown_test_is_kept_logged_and_false`), crates/unastar-packs/tests/vanilla.rs (`every_vanilla_filter_test_is_implemented`)

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

Evidence: crates/unastar-rules/src/molang/mod.rs (`Expr::bind`, `QueryRegistry::register`, `QueryContext`), crates/unastar-rules/src/molang/eval.rs (`evaluate`, `EvalError`, `MolangContext`; tests `query_context_separates_saved_property_defaults_from_live_actor_queries`, `unbound_query_is_an_error`, `variables_temps_programs_and_loops`), crates/unastar-packs/tests/vanilla.rs (`every_vanilla_molang_string_parses_and_binds`)

### Requirement: Loot decoding keeps everything
`loot::decode_table` SHALL decode a Bedrock loot table into typed pools, entries, conditions and functions while keeping the full source JSON in `LootTable::canonical`. A condition or function it does not model MUST become an explicit `Other { name, json }` value, never be dropped. A non-object table, non-object pool or unknown entry type MUST be a `ParseError`. `LootTables` MUST accept rows by name before it is shared.

#### Scenario: Unmodelled clause retained
- **WHEN** a function guard uses `entity_properties` with a property other than `on_fire`
- **THEN** the table decodes and the clause is kept for evaluation to reject

Evidence: crates/unastar-rules/src/loot/decode.rs (`decode_table`), crates/unastar-rules/src/loot/mod.rs (`LootTable`, `LootTables::insert`, `LootCondition::Other`, `LootFunctionKind::Other`), crates/unastar-rules/src/loot/tests.rs (`additional_entity_predicates_are_not_silently_discarded`), crates/unastar-data/src/loot.rs (`vanilla`)

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

Evidence: crates/unastar-rules/src/loot/evaluate.rs (`roll`, `LootContext`, `LootDrop`, `LootError`, `MAX_TABLE_DEPTH`), crates/unastar-rules/src/loot/tests.rs (`additional_entity_predicates_are_not_silently_discarded`, `missing_furnace_context_is_distinct_from_a_recipe_miss`, `false_function_guard_does_not_overwrite_an_earlier_count`, `no_this_actor_is_not_an_actor_whose_fire_state_is_false`, `tiers_pick_by_index_and_bonus_and_ignore_entry_conditions`, `nested_pools_roll_only_after_their_entry_created_an_item`), crates/unastar/src/server/game/loot/tests.rs (`block_tables_only_use_supported_features`)
Verified by: parity row `crafting.loot_tables` in [systems.toml](../../../docs/parity/systems.toml)

### Requirement: Random enchanting without an item registry
`enchanting` SHALL select and add enchantments from the caller's `ItemEnchanting` (slot mask and enchantability) and the caller's RNG, with no item registry. `select_enchantments` MUST return nothing for an item without slots or enchantability, else only discoverable enchantments that fit the item (treasure only when allowed), at levels the rolled cost reaches, never two that conflict. `add_enchantment` MUST refuse an enchantment that fits no slot or conflicts with the list.

#### Scenario: Selection stays valid
- **WHEN** enchantments are selected for many items and positive levels
- **THEN** every result is non-empty, fits the item, has a level within the enchantment's range and has no conflicting pair

#### Scenario: Adding to an existing list
- **WHEN** enchantments are added to a sword
- **THEN** compatible ones are added, a repeat raises the level, and an exclusive-group conflict or wrong slot is refused

Evidence: crates/unastar-rules/src/enchanting.rs (`select_enchantments`, `add_enchantment`, `max_level`, `ItemEnchanting`; tests `selected_enchantments_fit_the_item_and_never_conflict`, `levels_follow_the_minimum_costs`, `unenchantable_items_stay_plain`, `adding_respects_slots_lists_and_exclusive_groups`)
Verified by: parity row `crafting.enchantments` in [systems.toml](../../../docs/parity/systems.toml)
