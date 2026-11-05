---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Scope is vanilla only for now; registries stay open for addon packs and WASM mods (D5)

## Context and Problem Statement

Addon packs and plugin-registered content are valuable but large; building them now would delay
vanilla parity, while ignoring them would force a redesign later.

## Considered Options

- Vanilla only
- Also addon packs (custom items, blocks, recipes, loot) through the shared normaliser
- Also plugin registration APIs in WIT

## Decision Outcome

Chosen option: regular vanilla only for now, with registries and APIs designed so addon packs and
WASM mods can be added without redesign: data families are keyed by identifier, loaded into
registries that accept extra or overriding rows before they are frozen, and gameplay hooks go
through explicit events and components rather than per-entity code. WIT growth needs separate approval.

### Consequences

- Good, because addon rows already replace whole definitions before freeze (tested in
  `crates/unastar/src/registry/entity.rs`).
- Bad, because loading packs from disk is not done yet (`platform.addons` parity row is partial).
- Supersedes the "events are enums, not an event loop" premise of the January 2026 entities-gen plan.
