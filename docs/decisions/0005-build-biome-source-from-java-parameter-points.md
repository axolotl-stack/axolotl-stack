---
status: accepted
date: 2025-01-02
decision-makers: [project owner]
---

# Build the overworld biome source from Java's exported parameter points and R-tree

## Context and Problem Statement

The overworld biome builder plan hand-copied Java's biome tables into Rust.

## Considered Options

- Hand-copied biome parameter tables
- Java's exported parameter points, with an R-tree built exactly as Java builds it

## Decision Outcome

Chosen option: exported points and a faithful R-tree, because the tree's search order decides
ties between equally close biomes, so only an identically built tree gives Java's biomes everywhere.

### Consequences

- Good, because biome choice matches Java including at ties, and quantisation matches.
- Bad, because the tree build must track Java's algorithm, not just its data.

## More Information

Code: `crates/unastar_noise/codegen/data/biome_parameters.rs`, `codegen/emitter/biome_source.rs`,
`java_data/biome_parameters_*.json`.
