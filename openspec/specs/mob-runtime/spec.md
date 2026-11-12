# mob-runtime Specification

## Purpose
The mob component runtime in `crates/unastar` (`entity::mob`) compiles each vanilla behaviour-pack component into typed ECS state at startup and runs it each tick. This spec holds the contracts that runtime keeps about what it accepts and how it reads pack data; which vanilla behaviours it reproduces is tracked in `docs/parity/entities.toml` and `docs/quirks/bds.md`, not here. Why: [decision 0025](../../../docs/decisions/0025-one-rust-implementation-per-component-kind.md) and [decision 0013](../../../docs/decisions/0013-copy-bds-behaviour-exactly.md).

## Requirements

### Requirement: Unmeasured component fields reject the definition
A mob component kind SHALL compile only the fields whose behaviour is implemented. An authored field it cannot honour MUST fail the component with a typed error naming the field, so the definition is reported unsupported and `/summon` refuses it, instead of spawning a mob that ignores the field.

#### Scenario: A charged ranged attack
- **WHEN** a definition authors `behavior.ranged_attack` with `burst_shots` above 1
- **THEN** the archetype is unsupported with an error naming `burst_shots`

#### Scenario: Avoidance with an unmeasured field
- **WHEN** a definition authors `behavior.avoid_mob_type` with `remove_target`
- **THEN** the archetype is unsupported with an error naming `remove_target`

Evidence: crates/unastar/src/entity/mob/components/ranged.rs (`RangedError`), crates/unastar/src/entity/mob/combat/bds_ranged.rs

### Requirement: Item descriptors resolve tags at startup
A mob component's item descriptor SHALL accept an item name, `{"item": ...}`, `{"name": ...}` or `{"tags": <Molang>}`. A tag descriptor MUST be a pure item-tag expression (`q.any_tag`, `q.all_tags` with string arguments) and MUST expand once at startup to every registered item it holds for; any other Molang MUST fail the component. Crafting ingredients MUST use the same resolver.

#### Scenario: The skeleton's spear exclusion
- **WHEN** `behavior.pickup_items` excludes `{"tags": "q.all_tags('minecraft:is_spear')"}`
- **THEN** the excluded items are exactly the seven spears, any aux value

Evidence: crates/unastar/src/entity/mob/components/equipment.rs (test `a_tag_descriptor_expands_to_the_tagged_items`), crates/unastar/src/crafting/execution.rs (`tag_expression_items`)

### Requirement: A damage-sensor filter faulty only by an unknown key never matches
A damage sensor trigger whose filter parses with no diagnostic other than unknown keys SHALL compile to a filter that never matches. Any other filter fault MUST still fail the component.

#### Scenario: The skeleton's lightning sensor
- **WHEN** the skeleton's `minecraft:lightning_immune` group, whose sensor filters on `other_with_families`, is active
- **THEN** entity-attack and fall hits are dealt in full, as BDS 1.26.51 deals them

Evidence: crates/unastar/src/entity/mob/components/damage_sensor.rs (`sensor_filter`), crates/unastar/src/entity/mob/combat/bds_ranged.rs (test `the_spring_trap_sensor_blocks_no_hit_like_bds`)

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
