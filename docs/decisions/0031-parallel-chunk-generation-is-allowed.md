---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Chunk generation may run in parallel

## Context and Problem Statement

The January 2026 worldgen performance plan forbade parallel generation as a parity guard.

## Considered Options

- Generate chunks serially to protect parity
- Allow a worker pool

## Decision Outcome

Chosen option: allow a worker pool, because generation depends only on (seed, chunk position), so
order cannot change the output.

### Consequences

- Good, because generation scales with cores.
- Bad, because any generator state that is not a pure function of (seed, chunk) becomes a bug.
- As built, this holds for the Bedrock-style, flat and void generators (tested by
  `bedrock_decoration_is_independent_of_chunk_request_order`). The Java-style generator decorates
  in request order through one shared native session, so its output can depend on request order
  and worker assignment; `worldgen_output_hash` generates single-threaded in fixture order and
  cannot catch that. Contract in
  [openspec/specs/worldgen-pipeline](../../openspec/specs/worldgen-pipeline/spec.md).
- **Accepted trade-off for the Java-style generator (2026-11-05).** Its order dependence is
  deliberate, not a bug. FEATURES origins see the writes of earlier origins in one shared world,
  as Java's own chunk generation does, and Java climate ties keep the calling thread's previous
  search candidate. Making its output a pure function of (seed, chunk) would mean replaying
  decoration per origin from unmodified protos, which gives up those Java semantics. We keep the
  Java semantics and accept that Java-style output is reproducible only for the same request
  order. Its memory bound is [0034](0034-bound-the-java-native-session.md).

## More Information

Origin: world research §1.3, adopted by the data-driven architecture §4.
