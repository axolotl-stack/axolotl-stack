---
status: accepted
date: 2024-12-30
decision-makers: [project owner]
---

# Generate worldgen code in build.rs into OUT_DIR instead of committing it

## Context and Problem Statement

The first AOT worldgen output lived in a separate generator crate and was committed (~1.3 MB).
It was easy to forget to regenerate, and Cargo could not track the dependency between data and code.

## Considered Options

- Commit generated worldgen code
- Generate in `build.rs` into `OUT_DIR`

## Decision Outcome

Chosen option: `build.rs` into `OUT_DIR`, because the output then always matches the pinned data
and `rerun-if-changed` tracks it.

### Consequences

- Good, because there is no stale generated worldgen code.
- Bad, because builds are slower and the output is not reviewable in diffs.
- Note: this is the opposite of the protocol policy (committed `bedrock_versions`, checked by CI
  regeneration). Protocol output is committed because it is the public API other crates and users
  read; worldgen output is an implementation detail.

## More Information

Code: `crates/unastar_noise/build.rs`. The Bedrock-style generator follows the same pattern
(`crates/unastar_bedrock_codegen`).
