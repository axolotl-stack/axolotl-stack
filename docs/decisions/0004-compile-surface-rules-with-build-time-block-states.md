---
status: accepted
date: 2025-01-02
decision-makers: [project owner]
---

# Compile surface rules to straight-line code with block states resolved at build time

## Context and Problem Statement

Surface rules decide which blocks cap the terrain. The plans proposed a runtime `Box<dyn Rule>`
tree, then a runtime block-lookup closure so `unastar_noise` could ask the server for block ids.

## Considered Options

- Runtime rule tree (`Box<dyn Rule>`)
- Generated code with a runtime block-lookup closure
- Generated straight-line code with block-state literals resolved at build time

## Decision Outcome

Chosen option: straight-line code with build-time literals, because `unastar_noise` cannot depend
on `unastar` or `jolyne`, a per-block lookup costs time in the hottest loop, and unknown blocks
then fail the build.

### Consequences

- Good, because there is no lookup per block and no dependency cycle.
- Bad, because blocks are Java states inside the generator; conversion to Bedrock happens once,
  at the output boundary.

## More Information

Code: `crates/unastar_noise/codegen/emitter/surface_rule.rs`, `codegen/data/java_blocks.rs`.
Lesson: `docs/solutions/worldgen/generated-worldgen-resolves-block-states-at-build-time.md`.
