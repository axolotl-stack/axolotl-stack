---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Generate data schemas at build time; embed vanilla data and decode it at startup (D2)

## Context and Problem Statement

Earlier rule P2 ("no dynamic JSON parsing at runtime") generated Rust statics per entry, which ruled
out addon packs and was slow to compile (`items.rs` 1.8 MB, `blocks.rs` 0.9 MB, with loot and
recipes still to add).

## Considered Options

- Keep generating per-entry Rust statics
- Build-time codegen for schemas only; vanilla data embedded as a binary artefact, decoded at startup
- As above, plus addon packs loaded at runtime through a shared normaliser

## Decision Outcome

Chosen option: build-time codegen for the schemas (typed Rust structs/enums per data family); the
vanilla data itself is embedded and decoded at startup. No per-entry Rust code is generated. Runtime
addon packs are the 0015 extension path.

### Consequences

- Good, because addon rows can join the same registries later and build times dropped.
- Bad, because some data errors surface at startup rather than at compile time (decoded once, fail fast).
- Supersedes P2 and the "no runtime pack loading" premise of the January 2026 entities-gen plan.
  For worldgen this is overridden by 0017; spawn rules stay under this decision.

## More Information

Code: `crates/unastar-data/build.rs` (OUT_DIR, `include_bytes!`, decode once).
