---
title: Vanilla actor JSON is a delta over built-in behaviour; an absent component is not an absent feature
area: data
tags: [entities, actor-definitions, behaviour-pack, defaults, kdl]
date: 2026-01-05
origin: thoughts/shared/plans/2026-01-05-unastar-entities-gen.md (removed; see git history)
status: current
---

## Context
The January 2026 entity pipeline planned to turn the 122 bedrock-samples entity files into
"complete" entities by layering a hand-written `_defaults.kdl`, the vanilla JSON and
per-entity KDL overrides, then generating Rust per entity. That pipeline was removed when
the data-driven architecture (October 2026) landed: definitions are now normalised at
build time, embedded, and decoded once at startup.

## What we learned
- Behaviour-pack entity files describe only what differs from the engine's built-in
  behaviour. A zombie without `minecraft:physics` or `minecraft:movement.basic` still has
  physics and turning. Treating the file as the whole entity yields mobs with missing
  behaviour; filling the holes from hand-authored defaults files turned out to be guessing.
- What fills the holes now: published component-schema field defaults (kept separate,
  not merged into actor bodies, and never invented where the schema says "dynamic"),
  plus Rust constants for behaviour the pack data does not carry (move control, jump
  power, `max_turn` when the component is absent, path search), each marked confirmed
  against BDS or provisional with its public source.
- The vanilla files are inconsistent in type for the same field, and the loader has to
  accept all of these spellings: `fuse_length` as a number or `{range_min, range_max}`,
  `damage` as a number or `[min, max]`, `deals_damage` as a bool or `"yes"`/`"no"`,
  `seats` as one object or an array, and Molang strings in numeric fields
  (`"query.is_baby ? 0.02 : 0.001"`).
- KDL survives only as the reviewable datagen output for blocks, items and creative
  inventory (`kdl` 6.5, chosen because it can both read and write while preserving
  formatting), not as an entity override layer.

## Where it lives
- `crates/unastar-data/src/actor_definitions.rs` (`vanilla`, `get`, `component_field_default`)
- `crates/unastar-data/src/types.rs`, `crates/unastar-data/src/actor_components/dynamic.rs`,
  `crates/unastar-data/src/actor_components/range.rs` (polymorphic values)
- `crates/unastar-packs/src/actor.rs` (shared actor loader, tolerated malformed vanilla nodes)
- `crates/unastar/src/entity/mob/constants.rs` (built-in behaviour constants)
- `crates/unastar-data/README.md` (artifact list)
