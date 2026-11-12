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
