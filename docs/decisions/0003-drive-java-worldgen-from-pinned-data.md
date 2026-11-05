---
status: accepted
date: 2025-01-02
decision-makers: [project owner]
---

# Drive Java-style worldgen from the pinned Java worldgen data, not hand-ported Rust

## Context and Problem Statement

The 2025-12-29 Java-parity plan chose hand-written Rust with Java's JSON as a reference only.
Hand ports drifted from the data, missed fields silently and had to be redone on each update.

## Considered Options

- Hand-port features, surface rules and biomes to Rust
- Generate everything from the pinned Java 1.21.11 worldgen data at build time

## Decision Outcome

Chosen option: generate from the data, because it is correct by construction, regenerates on a
data bump, and unknown types or fields fail the build instead of being skipped.

### Consequences

- Good, because carvers, features, structures and biomes come from one pipeline.
- Bad, because every data construct needs codegen support before any of it works.

## More Information

Data: `crates/unastar_noise/worldgen_data`. Related: 0004, 0005, 0017. The Java byte-parity
target of that plan was later reframed by 0018 (two generators, each with its own truth source).
