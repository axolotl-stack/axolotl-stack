# pack-loader Specification

## Purpose
`crates/unastar-packs` is the one bevy-free reader of Bedrock behaviour packs: `.brarchive` containers, JSON with comments, BDS pack-stack resolution and the actor-definition loader that accepts, normalises and rejects pack documents the way BDS 1.26.51 does. Build scripts (`crates/unastar-data`, `crates/unastar_bedrock_codegen`) read packs with it and the runtime uses its types to decode the embedded, normalised data; no pack is read from disk at runtime; the filter, event and Molang parsers it calls belong to `rules-engine`, and what the decoded data provides belongs to `game-data`. The NBT template reader in the same crate is not covered here. Why: [decision 0020](../../../docs/decisions/0020-entity-data-from-bedrock-samples-checked-against-bds.md) and [decision 0025](../../../docs/decisions/0025-one-rust-implementation-per-component-kind.md).

## Requirements

### Requirement: One bevy-free pack reader
`unastar-packs` SHALL be the only implementation of pack container reading, comment-aware JSON and pack-stack resolution in the workspace, and it MUST NOT depend on bevy or any ECS crate. Consumers MUST call it instead of keeping private copies, and every error MUST name the file, pack entry or document it came from.

#### Scenario: Dependencies stay bevy-free
- **WHEN** `crates/unastar-packs/Cargo.toml` is inspected
- **THEN** its dependencies are limited to `regex`, `serde`, `serde_json`, `thiserror`, `flate2` and `unastar-rules`

#### Scenario: Build scripts share the reader
- **WHEN** the `unastar-data` build script loads actors, biomes, loot, items, recipes and spawn rules, and the Bedrock-style worldgen codegen loads worldgen families
- **THEN** both read them through `unastar_packs::packs::PackStack`

#### Scenario: Errors carry the origin
- **WHEN** `json::parse("pack/x.json", b"{ nope }")` fails
- **THEN** the error message starts with `pack/x.json`

Evidence: crates/unastar-packs/Cargo.toml, crates/unastar-packs/src/error.rs (`Error`), crates/unastar-data/build.rs, crates/unastar-data/build/actors.rs, crates/unastar_bedrock_codegen/Cargo.toml, crates/unastar-packs/src/json.rs (test `errors_name_the_origin`)

### Requirement: No pack is loaded at runtime
The server SHALL read only vanilla data that `unastar-data` embedded at build time and MUST NOT load any behaviour pack from disk at startup or later. A startup loader for addon packs is planned (change `data-driven-gameplay`) but not built; the crate's docs MUST NOT describe one as existing. Addon packs are tracked by parity row `platform.addons` in [systems.toml](../../../docs/parity/systems.toml).

#### Scenario: Server start
- **WHEN** `unastar` starts with a world directory that contains behaviour packs
- **THEN** it ignores them and uses the embedded vanilla data only

Evidence: crates/unastar-packs/src/lib.rs (crate docs), crates/unastar-packs/src/actor.rs (module docs), crates/unastar-packs/README.md

### Requirement: brarchive container decoding
`brarchive::parse` SHALL decode the BDS `.brarchive` layout (8-byte magic, little-endian entry count and format version, 256-byte entry slots, then a data blob that entry offsets are relative to) and return entries in stored order. It MUST reject a short header, a wrong magic, any version other than 1, an entry table past the end of the file, a name longer than its slot or not UTF-8, and a payload outside the blob.

#### Scenario: Layout round trip
- **WHEN** entries `a.json`, an empty `empty.nbt` and `b.json` are encoded with `brarchive::encode` and parsed back
- **THEN** the same entries come back in the same order with the same bytes

#### Scenario: Offsets are relative to the blob
- **WHEN** two entries with payloads `XY` and `Z` are encoded
- **THEN** the file is 16 + 2 × 256 + 3 bytes long and the second entry's stored offset is 2

#### Scenario: Malformed containers
- **WHEN** the input is 10 bytes long, is truncated inside the entry table, has version 2, has a bad magic, or has an entry whose offset plus size runs past the blob
- **THEN** `parse` returns an error naming that cause (`shorter`, `exceeds`, `version 2`, `magic`, `outside data`)

Evidence: crates/unastar-packs/src/brarchive.rs (`MAGIC`, `VERSION`, `parse`, `encode`; tests `roundtrips_entries_in_order`, `data_offsets_are_relative_to_the_blob`, `rejects_bad_magic_version_and_truncation`, `rejects_payload_outside_blob`)

### Requirement: JSON with comments
`json::parse` SHALL accept UTF-8 JSON with an optional leading byte-order mark and with `//` line comments and `/* */` block comments outside string literals. Comment markers inside strings, including after an escaped quote, MUST be kept as string content. Newlines inside removed block comments MUST be kept so parse-error line numbers still point at the source line. Input that is not UTF-8 MUST be rejected.

#### Scenario: Comments outside strings are removed
- **WHEN** a document has a leading `//` comment, a `/* */` block spanning a line break, a trailing `//` comment, and string values `"x // not a comment"` and `"/* kept */"`
- **THEN** it parses and both string values are unchanged

#### Scenario: Escaped quotes do not end a string
- **WHEN** `{"a": "q\" // still string", "b": 1} // c` is parsed
- **THEN** `a` is `q" // still string` and `b` is 1

#### Scenario: Byte-order mark
- **WHEN** the input starts with U+FEFF
- **THEN** it parses as if the mark were absent

Evidence: crates/unastar-packs/src/json.rs (`strip_comments`, `parse`; tests `strips_line_and_block_comments_outside_strings`, `escaped_quotes_do_not_end_strings`, `tolerates_byte_order_mark`)

### Requirement: BDS pack-stack layer order
`PackStack::open(server_dir)` SHALL build the layer stack of a BDS install as `definitions/`, then `behavior_packs/vanilla`, then every `behavior_packs/vanilla_<a.b.c>` directory with a version at or below `BASE_GAME_VERSION` (1.26.51) in ascending numeric version order. Other packs (experimental, editor, chemistry, library, `vanilla_beta`) and newer versions MUST be left out. A directory without `definitions/`, `behavior_packs/` or a `vanilla` pack MUST be an error that names what is missing.

#### Scenario: Numeric version order
- **WHEN** pack names `vanilla_1.21.100`, `vanilla_1.21.20`, `vanilla` and `vanilla_1.9` are sorted by `vanilla_pack_version`
- **THEN** the order is `vanilla`, `vanilla_1.9`, `vanilla_1.21.20`, `vanilla_1.21.100`
- **AND** `experimental_vanilla_shapes` and `vanilla_beta` have no version

#### Scenario: Newer packs are ignored
- **WHEN** an install holds `vanilla`, `vanilla_1.17.0`, `vanilla_1.18.0` and `vanilla_9.0.0`
- **THEN** the layers are `definitions`, `vanilla`, `vanilla_1.17.0`, `vanilla_1.18.0`

#### Scenario: Not a BDS directory
- **WHEN** `PackStack::open` is given an empty directory
- **THEN** it fails with a message that mentions `definitions`

Evidence: crates/unastar-packs/src/packs.rs (`BASE_GAME_VERSION`, `vanilla_pack_version`, `PackStack::open`; tests `pack_versions_sort_numerically_and_vanilla_is_base`, `later_layers_override_and_newer_than_base_version_is_ignored`, `missing_layout_names_the_missing_directory`)

### Requirement: Family sources and file order
Within a layer, `PackStack::files(family)` SHALL read `__brarchive/<family>.brarchive`, then nested `__brarchive/<family>/**.brarchive`, then loose `<family>/**.json`, paths sorted and archive entries in stored order. It MUST return only non-empty `.json` entries, with origin `<layer>/<family>:<entry>` and the entry path below the family directory. `PackStack::single_pack` MUST treat one pack directory as a one-layer stack.

#### Scenario: Path-keyed families keep subdirectories
- **WHEN** a single pack has loose `loot_tables/entities/shared.json` and `loot_tables/chests/shared.json`
- **THEN** `files(Family::LootTables)` returns entries `chests/shared.json` then `entities/shared.json`

#### Scenario: Nested archives before loose files
- **WHEN** a pack has `__brarchive/loot_tables/chests.brarchive`, `entities.brarchive` and `equipment/trials.brarchive` (each with `z.json` then `shared.json`) plus loose `loot_tables/chests/shared.json`
- **THEN** the entries are `chests/z.json`, `chests/shared.json`, `entities/z.json`, `entities/shared.json`, `equipment/trials/z.json`, `equipment/trials/shared.json`, then the loose `chests/shared.json`
- **AND** the fifth origin is `test/loot_tables:equipment/trials/z.json`

Evidence: crates/unastar-packs/src/packs.rs (`Family`, `PackStack::files`, `PackStack::inputs`, `PackStack::single_pack`; tests `loose_path_keyed_families_preserve_subdirectories`, `nested_archives_and_loose_files_share_family_relative_paths`)

### Requirement: Identifier resolution across layers
`PackStack::resolve(family)` SHALL key each document by `description.identifier`, keep the copy from the last layer that defines an identifier, and record that copy's origin in `Document::source`. The same identifier twice in one layer MUST be an error, and a document without exactly one schema key besides `format_version`, or without `description.identifier`, MUST be rejected. Path-keyed families such as loot tables MUST use `files` rather than `resolve`.

#### Scenario: Later layers override earlier ones
- **WHEN** `definitions/features` defines `minecraft:coal` (count 1) and `minecraft:iron` (count 2), and `vanilla_1.17.0` and `vanilla_1.18.0` redefine `minecraft:coal` with counts 5 and 10
- **THEN** `minecraft:coal` resolves to count 10 with a source starting `vanilla_1.18.0/`
- **AND** `minecraft:iron` keeps count 2 from `definitions`

#### Scenario: Duplicate identifier in one layer
- **WHEN** one archive in `vanilla` holds two features with identifier `minecraft:x`
- **THEN** `resolve(Family::Features)` fails with a message containing `same layer`

Evidence: crates/unastar-packs/src/packs.rs (`PackStack::resolve`, `parse_document`; tests `later_layers_override_and_newer_than_base_version_is_ignored`, `duplicate_identifier_in_one_layer_is_an_error`)

### Requirement: Actor document envelope
`actor::load_value` SHALL require a string `format_version` of one to three numeric parts, at most 1.26.51, and a `minecraft:entity` object. A file without `description.identifier` MUST return `Loaded::Skipped`, `events: null` MUST mean no events, other top-level keys MUST be kept with a diagnostic, and mistyped description fields or property definitions MUST reject the file. These match BDS ([quirks](../../../docs/quirks/bds.md)).

#### Scenario: Format ceiling
- **WHEN** an actor with `format_version` `1.26.51` is loaded
- **THEN** it loads
- **AND** the same actor with `1.26.52` or `99.0.0` is rejected

#### Scenario: Missing identifier and null events
- **WHEN** a 1.16.100 file has only `components`, and a 1.26.0 file has `"events": null`
- **THEN** the first is `Loaded::Skipped` and the second loads with no events and no diagnostics

#### Scenario: Malformed documents
- **WHEN** a file has no `format_version`, a non-boolean `is_spawnable`, an `int` property without the fields it requires, or `format_version` `one.two`
- **THEN** each is rejected with `Error::Data`

Evidence: crates/unastar-packs/src/actor.rs (`load`, `load_value`, `Loaded`; tests `rejects_future_format_and_accepts_registered_component_patterns`, `missing_identifier_and_null_events`, `rejects_malformed_documents`, `unknown_envelope_and_property_fields_survive_roundtrip`)

### Requirement: Component names gated by format version
`ActorParser` SHALL accept a component name only if the stable Mojang schemas list it or its pattern, the legacy form index lists it, or `register_component` registered it before loading. An unknown name MUST be kept inactive in `ignored_components` with a diagnostic up to format 1.21.80 and MUST reject the file from 1.21.90, in components and groups. A component first published in a newer schema MUST need that `format_version`, a gate that survives serialization and legacy merges.

#### Scenario: Legacy and modern unknown components
- **WHEN** an actor with component `addon:unknown` is loaded at format 1.21.80 and again at 1.21.90
- **THEN** the first loads with `addon:unknown` in `ignored_components`, a diagnostic for it, and the body kept in the normalised JSON
- **AND** the second is rejected

#### Scenario: Registered addon component
- **WHEN** a 1.26.50 actor uses `addon:custom` in `components` or in an unused group
- **THEN** it is rejected until `register_component("addon:custom")` is called, and then it loads with the body unchanged

#### Scenario: Introduced-version gate
- **WHEN** schemas publish `minecraft:uses_uniform_air_drag` first at 1.26.20 and a legacy form also lists it
- **THEN** an actor using it at format 1.26.10 is rejected and at 1.26.20 loads, before and after the registry is serialized and restored

#### Scenario: Pattern names
- **WHEN** `minecraft:memory_behavior.move_to_position.target_1` and `minecraft:memory_behavior.move_to_position.` are used
- **THEN** the first loads and the second is rejected

Evidence: crates/unastar-packs/src/actor.rs (`ActorParser::from_schemas`, `ActorParser::from_schema`, `ActorParser::register_component`, `ActorParser::register_form`, `ActorParser::to_schema`, `STRICT_ENTITY_SCHEMA_SINCE`; tests `legacy_unknown_components_warn_but_modern_ones_reject`, `rejects_unregistered_components_in_base_and_inactive_groups`, `versioned_components_remain_gated_after_registry_serialization_and_legacy_merge`, `rejects_future_format_and_accepts_registered_component_patterns`)

### Requirement: Unknown entity members
From format 1.21.90 the loader SHALL reject a `minecraft:entity` member that is neither `description`, `components`, `component_groups`, `events` nor a member declared by the registered `ActorDocument` schema. Schema-declared members such as `upgrades`, unknown top-level keys and unknown description fields MUST load and be kept for the normalised form.

#### Scenario: Schema-declared member
- **WHEN** a 1.26.50 actor has an `upgrades` member and the `ActorDocument` schema is registered
- **THEN** it loads with `upgrades` in `entity_other`
- **AND** adding a `test:unregistered` member makes it reject

#### Scenario: Envelope extras survive
- **WHEN** an actor has top-level `addon:envelope`, description `scripts` and a property with an extra `addon:metadata` field
- **THEN** `to_json` writes all three back unchanged and reloading gives an equal definition

Evidence: crates/unastar-packs/src/actor.rs (`ActorParser::register_document_schema`, `ActorDefinition::entity_other`; tests `modern_unknown_entity_member_rejects_but_envelope_and_description_survive`, `unknown_envelope_and_property_fields_survive_roundtrip`), crates/unastar-packs/tests/vanilla.rs (`canonical_entity_member_is_not_rejected_as_an_unknown_extension`)

### Requirement: Format upgrades and property defaults
Before format 1.26.10 the loader SHALL replace `minecraft:pushable` in base components and every group with `minecraft:pushable_by_entity` when `is_pushable` and `minecraft:pushable_by_block` when `is_pushable_by_piston` (both default `true`); from 1.26.10 it MUST be kept. Bool, int and float property defaults MUST be bound as Molang in the property-default context; one bad default MUST disable the whole property table without rejecting the actor. Enum defaults MUST stay literal strings.

#### Scenario: Pushable upgrade
- **WHEN** a 1.26.0 actor has `minecraft:pushable {is_pushable: false, is_pushable_by_piston: true}`
- **THEN** it has `minecraft:pushable_by_block` only
- **AND** an empty `minecraft:pushable` at 1.21.90 becomes both components while at 1.26.10 it stays `minecraft:pushable`

#### Scenario: Bad property default
- **WHEN** a bool property defaults to `query.is_baby`, which is unavailable in property defaults
- **THEN** the actor loads, a diagnostic ends with `/t:bad/default`, and `Description::active_properties()` is `None`

#### Scenario: Enum default that looks like a query
- **WHEN** an enum property's default is the value `query.is_baby`
- **THEN** it stays that string and no default expression is compiled

Evidence: crates/unastar-packs/src/actor.rs (`Description::active_properties`, `PropertyDef`; tests `pushable_is_upgraded_before_1_26_10_only`, `invalid_property_default_diagnoses_without_rejecting_actor`, `enum_defaults_are_literal_even_when_they_look_like_queries`)

### Requirement: Normalised form is a fixed point
`ActorDefinition::to_json` SHALL write the one canonical form: canonical filter and event spellings, upgraded components, and all retained data. Loading that output MUST give an equal definition with no diagnostics, and writing it again MUST give identical JSON. Component bodies other than their filters MUST be kept verbatim, including arrays and goal priorities. The runtime MUST decode only this normalised form, never raw pack spellings.

#### Scenario: Compact input normalises
- **WHEN** a 1.26.0 actor has an `environment_sensor` filter `{"AND": [{"test": "is_family", "subject": 1, "operator": 1, "value": "player"}]}`
- **THEN** the normalised filter is `{"all_of": [{"test": "is_family", "subject": "other", "operator": "!=", "value": "player"}]}`
- **AND** reloading the normalised JSON gives an equal definition, no diagnostics and the same JSON

#### Scenario: Embedded data is already normalised
- **WHEN** `unastar_data::actor_definitions` decodes its embedded rows at startup
- **THEN** every row loads as a definition with no diagnostics, or startup panics

Evidence: crates/unastar-packs/src/actor.rs (`ActorDefinition::to_json`; test `normalised_form_is_a_fixed_point`), crates/unastar-packs/tests/vanilla.rs (`every_vanilla_entity_loads_and_normalises_to_a_fixed_point`, `compact_filters_are_normalised_and_legacy_pushable_upgraded`), crates/unastar-data/src/actor_definitions.rs (`vanilla`), crates/unastar-data/tests/pack_data.rs (`actor_components_have_no_invented_defaults`)

### Requirement: Fidelity rules proven by BDS probes
Every accept/reject rule of the actor loader SHALL be backed by a crafted probe recorded from BDS 1.26.51. The probe inputs (`tests/fixtures/bds-1.26.51-fidelity-inputs.json`, authored, not copied vanilla data) and the recorded observations (`tests/fixtures/bds-1.26.51-fidelity.json`) MUST have the same probe names, the loader MUST accept exactly the probes BDS summoned, and each probe's self-only sensor filter outcome MUST equal BDS's recorded `filter_pass`.

#### Scenario: Golden matrix
- **WHEN** `crafted_actor_acceptance_matches_the_recorded_bds_matrix` loads each of the 60 probe inputs
- **THEN** a probe loads if and only if its observation has `summon_succeeded: true`
- **AND** each loaded probe survives a normalise-and-reload round trip unchanged

#### Scenario: Sensor filter outcomes
- **WHEN** `crafted_actor_sensor_outcomes_match_the_recorded_bds_matrix` evaluates each probe's environment-sensor filter against the probe itself
- **THEN** the result equals the recorded `filter_pass` for the other 53 probes
- **AND** exactly the 3 probes with a `minecraft:entity_spawned` event and the 4 probes `subject_numeric_5/6/8/9` (subjects only the server's runtime filter context resolves) are skipped, the test asserting that those 4 are exactly the ones a self-only context gets wrong

#### Scenario: Live probe run
- **WHEN** `tools/bds_fidelity.py --check-report PATH` is given a report from a BDS run
- **THEN** it compares the report's acceptance, filter results and deferred event order with the recorded fixture without starting BDS

Evidence: crates/unastar-packs/tests/vanilla.rs (`crafted_actor_acceptance_matches_the_recorded_bds_matrix`, `crafted_actor_sensor_outcomes_match_the_recorded_bds_matrix`, `RUNTIME_SUBJECT_PROBES`), crates/unastar-packs/tests/fixtures/bds-1.26.51-fidelity.json, crates/unastar-packs/tests/fixtures/bds-1.26.51-fidelity-inputs.json, crates/unastar-packs/tools/bds_fidelity.py
Verified by: `python crates/unastar-packs/tools/bds_fidelity.py --harness tools/bds-sweep --output DIR` against a reserved BDS 1.26.51 slot

### Requirement: Whole-file actor overrides over a stack
`actor::resolve_stack` SHALL select one file per identifier over the stack, the last layer winning and replacing the whole definition, before any component is parsed, so an overridden body is never merged with or validated against its replacement. Two files with one identifier in one layer MUST be an error, and files without an identifier MUST be listed in `Resolved::skipped` with their origin and reason.

#### Scenario: Vanilla samples resolve
- **WHEN** the bedrock-samples behaviour pack is resolved as a single pack
- **THEN** 128 definitions load, none are skipped, and the only diagnostics are the two silverfish event `remove` paths ([quirk](../../../docs/quirks/bds.md))

#### Scenario: A later layer replaces a broken base file
- **WHEN** layer `base` has a `t:cow` file that would be rejected on its own, and layers `patch_1` and `patch_2` each define `t:cow` in differently named files
- **THEN** `t:cow` resolves from `patch_2`, the broken base copy is never parsed, and other base identifiers keep their base source

#### Scenario: Same identifier twice in one layer
- **WHEN** two files in layer `base` both define `t:cow`
- **THEN** `resolve_stack` fails naming the identifier and the first file (`t:cow already defined in the same layer by base/entities:a.json`)

Evidence: crates/unastar-packs/src/actor.rs (`resolve_stack`, `Resolved`, `ResolvedDefinition`; tests `a_later_layer_replaces_the_whole_file`, `a_second_definition_in_the_same_layer_is_an_error`), crates/unastar-packs/tests/vanilla.rs (`every_vanilla_entity_loads_and_normalises_to_a_fixed_point`)

### Requirement: Comparison against a BDS install
`actor::diff` SHALL compare two resolved sets by normalised definition and report identifiers only on the left, only on the right, and differing. With `UNASTAR_BDS_DIR` set to a BDS 1.26.51 server directory, the bedrock-samples entities MUST equal the install's layered entities except that only the install has `minecraft:agent`, and the install's skipped files MUST be exactly the two identifier-less `npc.json` entries. Without the variable the check MUST skip with a message, not fail.

#### Scenario: Install matches samples
- **WHEN** `bedrock_samples_match_the_bds_layer_stack` runs with `UNASTAR_BDS_DIR` set
- **THEN** `only_right` is `["minecraft:agent"]`, `only_left` and `differing` are empty
- **AND** skipped origins are `vanilla_1.16.100/entities:npc.json` and `vanilla_1.17.20/entities:npc.json`

#### Scenario: No install configured
- **WHEN** `UNASTAR_BDS_DIR` is unset
- **THEN** the test prints a skip message and passes

Evidence: crates/unastar-packs/src/actor.rs (`diff`, `StackDiff`), crates/unastar-packs/tests/vanilla.rs (`bedrock_samples_match_the_bds_layer_stack`)
Verified by: `cargo test -p unastar-packs --test vanilla bedrock_samples_match_the_bds_layer_stack` with `UNASTAR_BDS_DIR` set (local only)

### Requirement: Vanilla rule coverage
Every filter test name used by vanilla entity, spawn-rule, trading, item and block data SHALL resolve to a registered filter test, and every Molang string in vanilla entity, item, block, biome, spawn-rule, trading, recipe and loot data MUST parse and bind its queries, with expression property defaults bound in the property-default context and everything else in the runtime context.

#### Scenario: Filter coverage
- **WHEN** `every_vanilla_filter_test_is_implemented` collects every filter in vanilla data
- **THEN** no test name is unknown and at least 87 distinct names are used

#### Scenario: Molang coverage
- **WHEN** `every_vanilla_molang_string_parses_and_binds` collects every Molang string
- **THEN** each parses and `Expr::bind` succeeds against `QueryRegistry::vanilla()`

Evidence: crates/unastar-packs/tests/vanilla.rs (`every_vanilla_filter_test_is_implemented`, `every_vanilla_molang_string_parses_and_binds`)
