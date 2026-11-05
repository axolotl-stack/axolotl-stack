---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Put filters, events, Molang and loot in a bevy-free unastar-rules crate (D10)

## Context and Problem Statement

Items, blocks, entities and worldgen codegen all need the same filter tests, event trees, Molang
evaluator and loot evaluator; worldgen codegen and datagen must not depend on bevy.

## Considered Options

- Keep the engines inside `unastar`
- A separate bevy-free crate shared by runtime, datagen and worldgen codegen

## Decision Outcome

Chosen option: a separate bevy-free `unastar-rules` crate, so there is one Molang parser and one
filter evaluator for runtime and build-time use.

### Consequences

- Good, because build-time and runtime evaluate the same rules identically.
- Bad, because the crate boundary must stay bevy-free (checked by `review-change`).

## More Information

`crates/unastar-rules/src/{filter,event,molang,loot,enchanting}`. Origin: entities research Q2 (O2).
