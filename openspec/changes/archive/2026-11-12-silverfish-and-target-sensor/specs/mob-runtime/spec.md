## ADDED Requirements

### Requirement: Built-in block facts come from measured data resolved at load
A mob component that needs a block fact vanilla data does not carry (which blocks a silverfish hides in, and what they become) SHALL read it from a measured table in `unastar-data`, resolved once against the block registry. A name the registry does not know MUST fail the component with a typed error; runtime code MUST take resolved block entries, never build or strip block names.

#### Scenario: A silverfish merging into deepslate
- **WHEN** a silverfish merges into deepslate lying along x
- **THEN** the block becomes infested deepslate lying along x, as the measured pair and the carried state say

Evidence: crates/unastar-data/src/infested.rs, crates/unastar/src/entity/mob/components/silverfish.rs (`InfestedBlocks`), crates/unastar/src/entity/mob/tests/silverfish.rs (test `the_measured_infested_blocks_are_what_bds_merged_into`)

### Requirement: An empty sensor trigger fires nothing
A sensor trigger authored without an event (vanilla's `{}` in the creeper's forced groups) SHALL compile to a trigger that fires nothing, so a component group can switch a sensor off by replacing it. A trigger with filters the sensor cannot honour MUST fail the component.

#### Scenario: The creeper lit with flint and steel
- **WHEN** the creeper's `minecraft:forced_exploding` group replaces its `target_nearby_sensor`
- **THEN** the replaced sensor has no inside, outside or vision-lost event

Evidence: crates/unastar/src/entity/mob/components/sensors.rs (`compile_target_nearby`), crates/unastar/src/entity/mob/tests/creeper.rs (test `the_forced_groups_switch_the_sensor_off`)
