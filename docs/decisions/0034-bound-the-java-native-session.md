---
status: proposed
date: 2026-11-05
decision-makers: [project owner]
---

# Bound the Java-style native session's memory without changing its output

## Context and Problem Statement

The Java-style generator decorates through one shared native session
(`crates/unastar/src/world/generator/pipeline/native.rs`). That session keeps every chunk it has
generated for the life of the server, and one mutex serialises all Java decoration. On a
long-running server that explores widely, memory grows without limit.

Evicting a chunk is not safe as things stand, for two reasons:

- `generate_native(pos)` serves a repeated request from the session snapshot, and
  `decorate(origin)` treats a missing entry as "not decorated yet". So evicting a sealed chunk
  either panics on a re-request (`completed native origin has no source chunk`, in
  `pipeline.rs`) or regenerates the chunk against a later history. That changes the output, which
  `native.rs` says must not happen.
- Re-requests happen whenever a generated chunk isn't saved: persistence is off, or a
  save-before-unload failed.

Found during the 2026-11-04 audit
([findings](../audits/2026-11-04-knowledge-migration-findings.md), "Worldgen") and confirmed in
review of PR #65.

## Decision Drivers

- Memory on a long-running server must be bounded.
- Java-style output must not depend on whether a chunk was evicted ([0031](0031-parallel-chunk-generation-is-allowed.md)).
- Fail fast: never silently regenerate different terrain.

## Considered Options

1. **Callers guarantee a delivered chunk is never requested again**, then evict sealed chunks.
   This needs persistence to be on and saves to be reliable. A re-request becomes a hard error.
2. **Keep a set of completed origins (small) and evict chunk data (large).** A re-request of an
   evicted chunk fails explicitly instead of regenerating.
3. **Spill sealed chunks to a session-local disk cache** and reload them on a re-request.
   Memory stays bounded and output stays exact, at the cost of I/O and code.
4. **Keep it unbounded** and document a memory ceiling per explored area.

## Decision Outcome

Proposed: option 3, with option 2's completed-origin set as the index.

- It is the only option that keeps output exact *and* tolerates re-requests when persistence is
  off. Options 1 and 2 turn an ordinary "persistence off" server into one that errors on revisits.
- The single mutex stays. Removing it needs region locking over the shared source world, which is
  a separate redesign.

The work is open for whoever owns worldgen. Start it as an OpenSpec change against
`openspec/specs/worldgen-pipeline`.

### Consequences

- Good, because memory is bounded and output stays exact.
- Bad, because revisiting a chunk that was spilled costs disk I/O, and the cache needs cleanup on
  shutdown.

### Confirmation

A test that generates, evicts and re-requests a chunk must get byte-identical output. A soak test
that walks a long path must show bounded memory.
