---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
supersedes: "0028"
---

# Compile worldgen ahead of time, end to end (D6)

## Context and Problem Statement

Density functions were already AOT-compiled (0001). The world research (WD3) recommended decoding
the rest of the worldgen data at startup, as gameplay data is under 0012.

## Considered Options

- Decode surface rules, carvers, features and structures at startup (WD3)
- Compile all worldgen data to straight-line Rust at build time

## Decision Outcome

Chosen option: worldgen is AOT-compiled end to end. Surface rules and builders, carvers, placed and
configured features, feature rules and structures (sets, placement, templates, jigsaw pools) are
compiled from their data at build time, as `unastar_noise` does. For worldgen this overrides 0012's
"no per-entry generated code": the goal is the most efficient generator possible. A later
addon/WASM overlay may use an interpreted path for extra content, but vanilla is compiled.

### Consequences

- Good, because generation speed is bounded only by the compiled code.
- Bad, because builds are heavier and need the data at build time.

## More Information

Implemented in `crates/unastar_noise` and `crates/unastar_bedrock_codegen`.
