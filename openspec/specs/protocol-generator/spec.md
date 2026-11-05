# protocol-generator Specification

## Purpose
`crates/valentine_gen` turns Bedrock protocol schemas into the checked-in Valentine version crate (`crates/valentine/bedrock_versions/v1_26_51`) and the version wiring in `crates/valentine/src/bedrock` and `crates/valentine/Cargo.toml`. Its default input is the pinned protocolgen canonical manifest; PrismarineJS, Mojang and Endstone frontends remain selectable explicitly. Runtime codec behaviour of the emitted code is specified in `protocol-codec`; block/item/entity/biome data generation is out of scope here except where it rewrites the shared version wiring.

## Requirements

### Requirement: Pinned protocolgen manifest is the default input
The generator SHALL, when no `--source` is given, read the protocolgen canonical manifest at `crates/valentine_gen/protocolgen/generated/1.26.51/manifest.json` (or the file named by `--protocolgen-manifest`) and MUST reject a manifest whose `schema_version` is not 2. With the protocolgen source it MUST refuse item/block/block-state/entity/biome generation, `--all`, `--emit-wire-manifest`, and any `--versions` list that does not include the manifest's target version.

#### Scenario: Unsupported manifest schema
- **WHEN** a manifest with `schema_version` other than 2 is parsed
- **THEN** generation fails with an "unsupported protocolgen manifest schema" error

#### Scenario: Data targets with protocolgen
- **WHEN** the generator runs with the default source and `--items`
- **THEN** it fails, directing the user to `--source prismarine`

#### Scenario: Version mismatch
- **WHEN** `--versions` names only a version other than the manifest's target version
- **THEN** generation fails without writing output

Evidence: crates/valentine_gen/src/main.rs (`generate_protocolgen_source`), crates/valentine_gen/src/parser/protocolgen.rs (`parse_manifest`, test `rejects_unknown_manifest_schema`), .gitmodules

### Requirement: Legacy frontends are explicit and isolated
The generator SHALL select the PrismarineJS, Mojang or Endstone frontends only through `--source prismarine|mojang|endstone`. Mojang generation MUST require an explicit `--output-dir` so it cannot overwrite the checked-in Valentine surface. `--emit-wire-manifest` SHALL require exactly one selected version.

#### Scenario: Mojang without an output directory
- **WHEN** the generator runs with `--source mojang` and no `--output-dir`
- **THEN** it fails before writing any file

Evidence: crates/valentine_gen/src/main.rs (`main`, `generate_schema_source`), crates/valentine_gen/README.md

### Requirement: Phased pipeline through a shared IR
The generator SHALL parse every source into the shared IR in `crates/valentine_gen/src/ir.rs` before analysis, and SHALL then analyze containers, discriminators and codec arguments (`generator/resolver.rs`, `generator/analysis.rs`), resolve names and canonical identities (`generator/utils.rs`, `generator/packet.rs`, `generator/definitions.rs`), and emit Rust modules. Emission MUST consume only the IR, so every frontend produces code through the same analysis and emission path.

#### Scenario: Protocolgen output compiles
- **WHEN** a small protocolgen manifest is generated into a temporary crate
- **THEN** `cargo check` on that crate succeeds and the union wire shape is preserved

#### Scenario: Mojang output compiles
- **WHEN** the pinned Mojang schema is generated into a temporary crate
- **THEN** `cargo check` on that crate succeeds

Evidence: crates/valentine_gen/src/parser/mod.rs, crates/valentine_gen/src/generator/mod.rs (`generate_protocol_module`), crates/valentine_gen/src/main.rs (tests `protocolgen_generation_compiles_and_preserves_resource_pack_wire_shape`, `real_mojang_generation_passes_cargo_check_in_a_temp_crate`)

### Requirement: One source of truth for packet argument typing
Packet signatures, `BedrockCodec` argument types, MCPE packet dispatch and borrowed view generation SHALL all derive a container's arguments and discriminator types from `ResolvedContainer::analyze`, so they cannot disagree about an argument's name or type.

#### Scenario: Dispatch and codec agree
- **WHEN** a packet body requires external arguments
- **THEN** the generated `<Packet>Args` type used by MCPE dispatch has the same argument names and types as the packet's `BedrockCodec::Args`

Evidence: crates/valentine_gen/src/generator/resolver.rs (`ResolvedContainer::analyze`, `compute_packet_signature`), crates/valentine_gen/src/generator/codec.rs, crates/valentine_gen/src/generator/mod.rs, crates/valentine_gen/src/generator/borrowed.rs

### Requirement: Fail fast on unrepresentable wire types
The generator MUST fail rather than guess a wire type when input contains an unsupported node kind or primitive, a reachable `opaque`/`unresolved` node, an asymmetric encode/decode layout, an enum variant with a payload, an out-of-range discriminant, or an untyped Mojang reference. The only exception SHALL be a Mojang field explicitly marked untyped, which lowers to `Void` with a warning.

#### Scenario: Reachable unresolved node
- **WHEN** a manifest contains a reachable `unresolved` node
- **THEN** parsing fails with "reachable protocolgen unresolved node cannot be generated"

#### Scenario: Out-of-range discriminant
- **WHEN** an enum or union variant value does not fit the declared control primitive
- **THEN** parsing fails naming the variant and value

Evidence: crates/valentine_gen/src/parser/protocolgen.rs (`lower_primitive`, tests `rejects_asymmetric_fields`, `rejects_asymmetric_union_variants`, `rejects_enum_variants_with_ignored_payloads`, `rejects_discriminants_outside_the_declared_primitive_range`, `rejects_reachable_unresolved_nodes`), crates/valentine_gen/src/parser/mojang.rs

### Requirement: Input corrections are pinned and fail-closed
Corrections to inputs SHALL be applied in memory while lowering; pinned inputs MUST NOT be edited. Each protocolgen correction SHALL pin the packet, field, primitive code and encoding it replaces and the protocol version that proves it, and generation MUST fail if the pinned node changed or if a manifest for the verified protocol lacks the corrected field. Mojang and Endstone override operations MUST fail when they match nothing.

#### Scenario: Upstream node changed
- **WHEN** the manifest's `LevelEventGenericPacket` event-data field no longer has the pinned `nbt_le`/`network` node
- **THEN** generation fails instead of applying the stale correction

#### Scenario: Unmatched override
- **WHEN** a Mojang correction targets a property that does not exist
- **THEN** applying overrides fails

Evidence: crates/valentine_gen/src/parser/protocolgen_corrections.rs, crates/valentine_gen/src/parser/protocolgen.rs (tests `corrects_level_event_generic_data_to_a_compound_body`, `correction_fails_when_the_manifest_node_changes`, `correction_fails_when_its_verified_manifest_lacks_the_field`), crates/valentine_gen/src/overrides.rs (tests `rejects_unmatched_corrections`, `rejects_missing_field_in_file_scoped_property_patch`)

### Requirement: Token-based emission
The generator SHALL build Rust output as `proc_macro2::TokenStream` values with `quote`/`syn`, parse them with `syn`, and format them with `prettyplease` before writing. Every emitted Rust module SHALL begin with the `// Generated by valentine_gen. Do not edit.` header.

#### Scenario: Unparseable tokens
- **WHEN** generated tokens do not parse as a Rust file
- **THEN** writing fails with "Failed to parse generated code"

Evidence: crates/valentine_gen/src/generator/emitter.rs (`format_tokens`, `write_rust_file`, `GENERATED_HEADER`)

### Requirement: Atomic formatted writes
Generated Rust modules MUST be fully parsed and formatted before their destination is touched, then written to a uniquely named sibling temporary file and atomically renamed over the destination. A format or write failure MUST leave the previous file unchanged and remove the temporary file.

#### Scenario: Invalid tokens preserve existing output
- **WHEN** `write_rust_file` is called with invalid tokens for a path that already holds generated code
- **THEN** it returns an error and the file's bytes are unchanged

#### Scenario: Valid tokens replace output
- **WHEN** `write_rust_file` is called with valid tokens
- **THEN** the file contains exactly the generated header followed by the formatted code

Evidence: crates/valentine_gen/src/generator/emitter.rs (tests `invalid_tokens_preserve_existing_file`, `valid_tokens_replace_existing_file`)

### Requirement: Updates merge existing version crates
When generation rewrites the Valentine surface (`src/bedrock/{mod,protocol/mod,version}.rs`) and `Cargo.toml`, the generator SHALL retain every version declared in the existing `version.rs` whose `bedrock_versions/<module>` crate directory still exists, and SHALL drop declarations whose crate directory is gone. It MUST NOT replace an existing `default` feature; it SHALL seed `default` only when the manifest has none. Non-generated features SHALL be preserved.

#### Scenario: Prismarine data refresh keeps the protocolgen crate
- **WHEN** a Prismarine surface update for 1.21.120 runs against a surface that declares 1.26.51 (crate present, default) and 1.20.0 (crate missing)
- **THEN** the manifest keeps `default = ["bedrock_1_26_51"]`, declares features and dependencies for 1.26.51 and 1.21.120, keeps the `custom` feature, and removes every 1.20.0 reference

Evidence: crates/valentine_gen/src/main.rs (`read_existing_version_decls`, `write_prismarine_surface`, `update_valentine_manifest_at`, test `prismarine_surface_preserves_existing_versions_and_default`)

### Requirement: Generated output is never hand-edited
Behaviour that must survive regeneration SHALL be changed in the parser, IR analysis, corrections or emitter, not in generated files. Regenerating from the pinned manifest followed by `cargo fmt --all` MUST reproduce the checked-in `crates/valentine/bedrock_versions/v1_26_51`, `crates/valentine/src/bedrock` and `crates/valentine/Cargo.toml` byte-for-byte.

#### Scenario: CI drift check
- **WHEN** CI runs `cargo run -p valentine_gen -- --latest` and `cargo fmt --all`
- **THEN** `git diff --exit-code` over the generated paths reports no changes

Evidence: .github/workflows/test.yml ("Verify checked-in protocolgen output"), crates/valentine_gen/README.md

### Requirement: Union identity is never collapsed into its payload
Every canonical union SHALL retain its control codec and every data or void variant. When an inline union's name would equal a different registered type (such as its named payload), the generator SHALL append `Union` until the name is unique, so a name collision can never select the payload codec in place of the union codec.

#### Scenario: Disconnect message union
- **WHEN** the `DisconnectPacket` messages field is generated
- **THEN** its type is `DisconnectPacketMessagesUnion` with variants `DisconnectPacketMessages(DisconnectPacketMessages)` (control 0) and `Empty1` (control 1), and the encoded body includes the control varint

Evidence: crates/valentine_gen/src/generator/definitions.rs, crates/valentine/tests/union_conformance.rs

### Requirement: Decode resource guards are emitted from schema analysis
The generator SHALL emit each collection's `MIN_ENCODED_SIZE` from a conservative wire-size lower bound computed from the schema, where unknown or cyclic references contribute zero. It SHALL detect cycles through named type references and wrap decoding of each recursive type, in both owned codecs and borrowed views, in `RecursiveDecodeGuard`. Owned MCPE dispatch SHALL check for unconsumed payload bytes before converting to the dispatch enum.

#### Scenario: Recursive type
- **WHEN** a named type reaches itself through references
- **THEN** its generated owned decode and borrowed view decode both enter `RecursiveDecodeGuard` before decoding children

Evidence: crates/valentine_gen/src/generator/analysis.rs (`minimum_encoded_size`, `is_recursive_type`), crates/valentine_gen/src/generator/codec.rs, crates/valentine_gen/src/generator/borrowed.rs, crates/valentine_gen/src/generator/mod.rs

### Requirement: Symbol names are collision-checked
The generator SHALL emit protocolgen symbol tables into `symbols.rs`, replacing the module on every run even when no symbols remain, and MUST fail when two names normalize to the same constant or module identifier instead of shadowing or merging them. Symbol tables MUST carry versioned provenance and unique, non-empty names.

#### Scenario: Normalized constant collision
- **WHEN** two symbol values normalize to the same Rust constant name
- **THEN** generation fails

Evidence: crates/valentine_gen/src/generator/symbols.rs (tests `normalized_constant_collision_is_rejected_instead_of_shadowed`, `normalized_module_collision_is_rejected_instead_of_merged`), crates/valentine_gen/src/parser/protocolgen.rs (`lower_symbols`), crates/valentine_gen/src/generator/mod.rs

### Requirement: Structured wire manifest for legacy sources
With a legacy source and exactly one selected version, `--emit-wire-manifest <FILE>` SHALL write a JSON view of the fully lowered IR in which every packet lists its ordered wire operations with references and nested containers expanded recursively, and recursive references become explicit cycle markers instead of recursing forever.

#### Scenario: Recursive reference
- **WHEN** a lowered type references itself
- **THEN** the wire manifest records a cycle marker at that reference and terminates

Evidence: crates/valentine_gen/src/wire_manifest.rs (tests `recursively_flattens_references_and_preserves_wire_structure`, `marks_recursive_references_without_recursing_forever`)
