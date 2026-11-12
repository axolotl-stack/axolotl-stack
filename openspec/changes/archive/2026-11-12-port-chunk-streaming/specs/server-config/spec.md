## MODIFIED Requirements

### Requirement: Validation runs after parsing
After a successful parse `load_or_create` SHALL call `UnastarConfig::validate` and return `ConfigError::Validation` on failure. Validation MUST require at least one `[[spawn_rules]]` entry, `server.max_chunk_radius >= 1`, `server.compression_level` in `1..=9`, a non-empty `rect` bound and every spawn rule `location` inside `world.bounds`, and MUST refuse generators the build cannot run (see `worldgen-pipeline`). The unused `server.default_chunk_radius` MUST still load.

#### Scenario: Radius above the maximum
- **WHEN** a file written before the view distance grant sets `default_chunk_radius = 4` and `max_chunk_radius = 12` (or a default above the maximum)
- **THEN** it loads, `server_config().max_chunk_radius` is 12 (a player gets the radius it requests up to that), and startup logs a warning that `default_chunk_radius` is ignored; `max_players` is not range-checked

#### Scenario: Empty rect bounds
- **WHEN** `[world.bounds] mode = 'rect'` has `min_x = 5` and `max_x = -5`
- **THEN** loading returns `ConfigError::Validation` naming `world.bounds`

#### Scenario: Fixed spawn outside the bounds
- **WHEN** bounds are `radius` with `radius_chunks = 2` and a spawn rule has `location = { x = 48.0, y = 17.0, z = 0.5 }`
- **THEN** loading returns `ConfigError::Validation` naming `world.bounds`

#### Scenario: Default world without Java templates
- **WHEN** the default config is loaded by a build without `java-worldgen`
- **THEN** loading returns `ConfigError::Validation` whose message names `java-worldgen` and `UNASTAR_JAVA_SERVER_JAR`

Evidence: crates/unastar/src/config/mod.rs (`UnastarConfig::validate`, `ServerConfigFile::default_chunk_radius`), crates/unastar/src/server/runtime.rs (`UnastarServer::new`), crates/unastar/src/config/tests.rs (`legacy_default_chunk_radius_still_loads`, `default_java_world_requires_the_java_worldgen_feature`, `bedrock_config_requires_the_build_time_pack_feature`, `empty_rect_world_bounds_are_refused`, `fixed_spawn_locations_outside_the_world_bounds_are_refused`)
