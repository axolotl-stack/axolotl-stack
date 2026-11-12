# server-config Specification

## Purpose
How the `unastar` server binary reads its configuration: the `unastar.toml` file, its embedded default, strict parsing, validation after parsing, and how each table reaches the runtime (`crates/unastar/src/config/`, `crates/unastar/src/server/config.rs`, `crates/unastar/src/world/mod.rs`). Generator selection and the build features generators need are specified in `worldgen-pipeline`; storage behaviour behind the storage keys in `world-storage`; operator and chunk-radius enforcement in `server-authority`. The parity row is `platform.server_config` in `docs/parity/systems.toml`.

## Requirements

### Requirement: Config file location and embedded default
The server SHALL read its configuration from the path in `UNASTAR_CONFIG`, or `unastar.toml` in the working directory when the variable is unset. When that file does not exist, `UnastarConfig::load_or_create` MUST write `DEFAULT_CONFIG_TOML` (the crate's `unastar.toml`, embedded at build time) to the path and load it. A read error other than "not found" MUST fail with `ConfigError::Read`, and a failed write with `ConfigError::WriteDefault`.

#### Scenario: First run
- **WHEN** the config path does not exist
- **THEN** the file is created with exactly the embedded template and the loaded config equals `UnastarConfig::default()`

Evidence: crates/unastar/src/main.rs, crates/unastar/src/config/mod.rs (`DEFAULT_CONFIG_TOML`, `UnastarConfig::load_or_create`, `ConfigError`), crates/unastar/unastar.toml, crates/unastar/src/config/tests.rs (`generated_and_shipped_configs_load_with_the_same_safe_defaults`)

### Requirement: Shipped samples load as the Rust defaults
The generated file, the crate sample `crates/unastar/unastar.toml`, the workspace-root sample `unastar.toml` and an empty file SHALL all load to the same effective configuration as `UnastarConfig::default()`. A field omitted from any table MUST take its `Default` value. The root sample MUST be a byte-for-byte copy of the crate sample.

#### Scenario: Every sample
- **WHEN** each of the root sample, the crate sample and an empty string is written to the config path and loaded
- **THEN** each serialises identically to `UnastarConfig::default()` and has `server.online_mode = true`

#### Scenario: Root sample is a copy
- **WHEN** the workspace-root `unastar.toml` is compared with the embedded template
- **THEN** the two are byte-identical, comments included

Evidence: crates/unastar/src/config/tests.rs (`generated_and_shipped_configs_load_with_the_same_safe_defaults`, runs with `java-worldgen`; `root_sample_is_a_copy_of_the_embedded_template`), unastar.toml, crates/unastar/unastar.toml

### Requirement: Unknown keys are rejected at every level
Parsing SHALL reject any key not defined by the schema, in the root and in `[server]`, `[world]`, `[world.bounds]` (for each `mode`), `[world.generator]` (for each `kind`), `[players]`, `[[spawn_rules]]` and spawn `location`, with `ConfigError::Parse`. Unit variants (`mode = "infinite"`, `kind = "super_flat"`) MUST also reject stray options. The unsupported `[world.storage]` table MUST fail to parse.

#### Scenario: Typos and stray options
- **WHEN** a config contains `[server] online_mod = false`, `[world.bounds] mode = 'infinite'` with `radius_chunks = 2`, `[world.generator] kind = 'super_flat'` with `seed = 42`, or `[world.storage] data_dir = 'custom-worlds'`
- **THEN** loading returns `ConfigError::Parse`

Evidence: crates/unastar/src/config/mod.rs (`deny_unknown_fields`, `deserialize_empty_variant`), crates/unastar/src/world/mod.rs (`WorldBounds`, `WorldGenerator`, `WorldConfig`), crates/unastar/src/config/tests.rs (`unknown_config_keys_are_rejected_instead_of_silently_ignored`, `strict_world_variants_still_load_valid_options`)

### Requirement: Enumerated values parse strictly
`server.default_gamemode` (`survival`, `creative`, `adventure`, `spectator`; default `survival`), `server.difficulty` (`peaceful`, `easy`, `normal`, `hard`), `server.block_network_ids` (`hashed`, `sequential`), `world.storage_provider` (`level_db`, `blaze_db`), `world.bounds.mode` and `world.generator.kind` SHALL accept only their listed lower-case snake_case values and MUST fail to parse with `ConfigError::Parse` otherwise.

#### Scenario: Unknown difficulty
- **WHEN** `[server] difficulty = 'brutal'` is loaded
- **THEN** loading returns `ConfigError::Parse`

#### Scenario: Misspelt or capitalised game mode
- **WHEN** `[server] default_gamemode` is `'creativ'`, `'Creative'`, `'1'` or `''`
- **THEN** loading returns `ConfigError::Parse`

#### Scenario: Game mode reaches the runtime
- **WHEN** `default_gamemode = 'adventure'` is loaded
- **THEN** `server_config().default_gamemode` is `GameMode::Adventure`

#### Scenario: Difficulty reaches the runtime
- **WHEN** no difficulty is set, and then `difficulty = 'hard'`
- **THEN** `server_config().difficulty` is `Easy`, and then `Hard`

Evidence: crates/unastar/src/config/mod.rs (`ServerConfigFile`, `UnastarConfig::server_config`), crates/unastar/src/world/mod.rs (`StorageProvider`), crates/unastar/src/entity/components/player.rs (`GameMode`), crates/unastar/src/config/tests.rs (`default_gamemode_parses_strictly_and_reaches_runtime_configuration`, `difficulty_defaults_to_easy_and_reaches_runtime_configuration`, `storage_settings_reach_runtime_configuration`)

### Requirement: Validation runs after parsing
After a successful parse `load_or_create` SHALL call `UnastarConfig::validate` and return `ConfigError::Validation` on failure. Validation MUST require at least one `[[spawn_rules]]` entry, `server.default_chunk_radius >= 1`, `server.max_chunk_radius >= 1` and default not above max, a non-empty `rect` bound and every spawn rule `location` inside `world.bounds`, and MUST refuse generators the build cannot run (see `worldgen-pipeline`). `max_players` is not range-checked.

#### Scenario: Radius above the maximum
- **WHEN** `default_chunk_radius = 16` and `max_chunk_radius = 12`
- **THEN** loading returns `ConfigError::Validation` naming both keys

#### Scenario: Empty rect bounds
- **WHEN** `[world.bounds] mode = 'rect'` has `min_x = 5` and `max_x = -5`
- **THEN** loading returns `ConfigError::Validation` naming `world.bounds`

#### Scenario: Fixed spawn outside the bounds
- **WHEN** bounds are `radius` with `radius_chunks = 2` and a spawn rule has `location = { x = 48.0, y = 17.0, z = 0.5 }`
- **THEN** loading returns `ConfigError::Validation` naming `world.bounds`

#### Scenario: Default world without Java templates
- **WHEN** the default config is loaded by a build without `java-worldgen`
- **THEN** loading returns `ConfigError::Validation` whose message names `java-worldgen` and `UNASTAR_JAVA_SERVER_JAR`

Evidence: crates/unastar/src/config/mod.rs (`UnastarConfig::validate`), crates/unastar/src/config/tests.rs (`default_java_world_requires_the_java_worldgen_feature`, `bedrock_config_requires_the_build_time_pack_feature`, `empty_rect_world_bounds_are_refused`, `fixed_spawn_locations_outside_the_world_bounds_are_refused`)

### Requirement: Online mode is on unless explicitly disabled
`server.online_mode` SHALL default to `true` in the Rust default, the embedded template and the root sample; offline mode MUST require an explicit `online_mode = false`. `online_mode`, `allow_legacy_auth` (default `true`), `encryption_enabled` (default `true`) and `require_resource_packs` (default `false`) MUST be passed unchanged to the jolyne listener.

#### Scenario: Explicit opt-out
- **WHEN** `[server] online_mode = false` is loaded
- **THEN** `server.online_mode` is `false`

Evidence: crates/unastar/src/config/mod.rs (`ServerConfigFile::default`), crates/unastar/src/server/runtime.rs (`run`, `BedrockListenerConfig`), crates/unastar/src/config/tests.rs (`offline_mode_requires_an_explicit_opt_out`, `generated_and_shipped_configs_load_with_the_same_safe_defaults`)

### Requirement: Listener address and player limit
The RakNet listener SHALL bind `server.bind_address`, which MUST default to the loopback address `127.0.0.1:19132` so a default run opens no public port. `server.motd` and `server.max_players` MUST be advertised in the server status; `max_players` (at least 1) also MUST size the connection gate, which with the per-session inbound queues in `server-authority` caps queued inbound packets at `max_players × 512`.

#### Scenario: Default bind
- **WHEN** the server starts with the default configuration
- **THEN** it listens on `127.0.0.1:19132` and advertises `Unastar Server` with a limit of 20

Evidence: crates/unastar/src/config/mod.rs (`ServerConfigFile::default`), crates/unastar/src/server/config.rs (`ServerConfig::default`), crates/unastar/src/server/runtime.rs (`run`), crates/jolyne/tests/raknet_listener.rs (`listener_advertises_status_and_enforces_max_players`)

### Requirement: World bounds are inclusive chunk ranges
`world.bounds` SHALL be `infinite`, `radius` (`radius_chunks`: every chunk with `|x|` and `|z|` at most the radius) or `rect` (`min_x..=max_x`, `min_z..=max_z`), all in chunk coordinates and inclusive. Every generator, inline or on the generation pool, MUST produce an all-air chunk for a position outside the bounds instead of terrain. A chunk already in storage MUST load as stored, inside or outside the bounds.

#### Scenario: Radius bounds
- **WHEN** bounds are `radius` with `radius_chunks = 2`
- **THEN** chunk (2, -2) is inside and chunk (3, 0) is outside

#### Scenario: Pool generator outside the bounds
- **WHEN** a `java` or `bedrock` world's generation pool is asked for a chunk outside the bounds that is not in storage
- **THEN** it returns an all-air chunk

Evidence: crates/unastar/src/world/mod.rs (`WorldBounds::contains`, `contains_block`), crates/unastar/src/world/generator/pipeline.rs (`ChunkPipeline::generate_within`), crates/unastar/src/world/ecs/manager.rs (`generate_chunk`, test `inline_generation_is_air_outside_world_bounds`), crates/unastar/src/world/ecs/generation_worker.rs (`run_worker`, test `pool_generates_air_outside_world_bounds`), crates/unastar/src/config/tests.rs (`world_bounds_contain_their_inclusive_chunk_ranges`, `strict_world_variants_still_load_valid_options`), crates/unastar/unastar.toml (`[world.bounds]` comments)

### Requirement: Storage and player settings reach the runtime
`world.storage_provider`, `world.blazedb_cache_chunks` (default 4096), `world.dimension` (default 0), `world.generation_threads` (default 0) and the `[players]` keys `data_dir` (default `playerdata`), `leveldb_enabled`, `save_on_disconnect` and `save_previous_position` (each default `true`) SHALL reach the runtime unchanged. What each one does is specified in `world-storage`.

#### Scenario: Non-default storage
- **WHEN** a config sets `storage_provider = 'blaze_db'`, `blazedb_cache_chunks = 73`, `data_dir = 'custom-playerdata'` and the three player flags to `false`
- **THEN** `server_config().world` and `players` carry exactly those values

Evidence: crates/unastar/src/config/mod.rs (`PlayerStorageConfig`, `UnastarConfig::server_config`), crates/unastar/src/world/mod.rs (`WorldConfig::default`), crates/unastar/src/config/tests.rs (`storage_settings_reach_runtime_configuration`)

### Requirement: Values the file does not expose use fixed defaults
`UnastarConfig::server_config` SHALL take `simulation_distance` (6 chunks) and `chunk_unload_ticks` (100 ticks) from `ServerConfig::default()`; the file MUST NOT be able to set them. `server.operators` (default empty) MUST be the starting operator list, matched by display name or XUID as specified in `server-authority`; `/op` and `/deop` change only the running server and are not written back to the file.

#### Scenario: Operators from the file
- **WHEN** `operators = ["MobBot", "2535400000000042"]`
- **THEN** players named `mobbot` or with that XUID are operators when they join

Evidence: crates/unastar/src/config/mod.rs (`UnastarConfig::server_config`), crates/unastar/src/server/config.rs (`ServerConfig::default`), crates/unastar/src/command/exec/op.rs, crates/unastar/src/server/game/mod.rs (test `configured_operators_are_granted_on_join_by_name_or_xuid`)

### Requirement: Spawn rules are evaluated in order
`resolve_spawn_location` SHALL walk `[[spawn_rules]]` top to bottom. A `previous_position` rule MUST use the saved record, then the last-position file, only when its dimension is the world's. A rule with `always_at_location` and a `location` MUST return that location. With no answer, the first rule `location` MUST be used, else the generator's spawn search inside `world.bounds` (pool generators) or the template spawn. Startup MUST fail if the resolved world spawn is outside `world.bounds`.

#### Scenario: Returning player
- **WHEN** a player saved at (33.25, 75, -8.5) in the world's dimension rejoins with the default rule
- **THEN** the resolved spawn is that position

#### Scenario: Template spawn outside the bounds
- **WHEN** a void world whose template spawn is at the origin has `rect` bounds x 10..=12, z 10..=12 and no rule gives a location
- **THEN** `UnastarServer::run` returns an error saying the world spawn is outside `world.bounds`

Evidence: crates/unastar/src/server/connect.rs (`resolve_spawn_location`, tests `spawn_rules_are_evaluated_top_to_bottom`, `without_an_answering_rule_the_first_location_then_the_template_is_used`, `the_generated_spawn_is_inside_the_world_bounds` with `java-worldgen`), crates/unastar/src/server/runtime.rs (`run`), crates/unastar/src/world/generator/spawn.rs (test `the_spawn_search_stays_inside_the_world_bounds`), crates/unastar/src/config/mod.rs (`SpawnRule`, `SpawnLocation`, `PlayerDataStore`), crates/unastar/tests/spawn_bounds.rs (`startup_fails_when_the_world_spawn_is_outside_the_bounds`), crates/unastar/tests/persistence.rs (`disconnect_reconnect_restores_inventory_and_player_state`)
