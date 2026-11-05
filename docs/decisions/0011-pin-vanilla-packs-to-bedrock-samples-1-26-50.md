---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Pin the shared vanilla pack input to bedrock-samples 1.26.50 or newer (D1)

## Context and Problem Statement

The shared vanilla pack input (`crates/unastar-data/data/vanilla_bp`, bedrock-samples) was pinned
to 1.21.130.3, stale against BDS 1.26.51: 236 recipes missing and 154 differing, 102 loot tables
missing (99 of them block tables) and 6 differing, 2 items missing. The pin feeds entities, biomes,
items, recipes and loot.

## Considered Options

- Bump bedrock-samples to the 1.26.50/51 tag
- Keep 1.21.130 and patch the gaps with overrides
- Read the BDS `.brarchive` files at datagen time (needs BDS in CI; not redistributable)

## Decision Outcome

Chosen option: bump bedrock-samples to 1.26.50 or newer, so pack data matches BDS 1.26.51, plus a
dev-only diff check against a local BDS install's layered packs.

### Consequences

- Good, because entities, items and world share one current pin.
- Good, because drift is caught by `crates/unastar-data/tests/bds_pack_diff.rs`.
- Bad, because bedrock-samples can lag BDS; the diff check needs a local BDS to run.

## More Information

Implemented: submodule at `v1.26.50.4`. Source: items research §6 D1.
