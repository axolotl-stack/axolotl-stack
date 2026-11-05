---
status: accepted
date: 2025-12-27
decision-makers: [project owner]
---

# Generate protocol code; take game data from BDS-native sources, not minecraft-data

## Context and Problem Statement

The December 2025 codebase review found PrismarineJS `minecraft-data` excellent for protocol
definitions but incomplete for game data (block states and properties, block entities, items,
recipes), and several bugs traced back to that.

## Considered Options

- Abandon generation entirely
- Hybrid: generate protocol code; source game data from BDS-native and measured data
- Find a single better data source for both

## Decision Outcome

Chosen option: the hybrid, because protocol schemas are mechanical and well served by generation,
while game data must match BDS 1.26.51 exactly. Protocol now comes from the pinned protocolgen
manifest; blocks, states, items and biomes come from protocolgen's native BDS inputs through
`unastar-data`; `minecraft-data` remains only as a reference and cross-check.

### Consequences

- Good, because the data bugs from the review are gone.
- Bad, because there are two pipelines (`valentine_gen` and `unastar-data`) to maintain.

## More Information

Origin: [docs/audits/2025-12-codebase-review.md](../audits/2025-12-codebase-review.md) §4 Option B.
Extended by 0012 and 0014.
