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
  cannot catch that. Open finding in
  [docs/audits/2026-11-04-knowledge-migration-findings.md](../audits/2026-11-04-knowledge-migration-findings.md);
  contract in [openspec/specs/worldgen-pipeline](../../openspec/specs/worldgen-pipeline/spec.md).

## More Information

Origin: world research §1.3, adopted by the data-driven architecture §4.
