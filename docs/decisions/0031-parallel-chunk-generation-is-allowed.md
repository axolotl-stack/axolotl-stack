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
- Bad, because any generator state that is not a pure function of (seed, chunk) becomes a bug;
  `worldgen_output_hash` fixtures catch it.

## More Information

Origin: world research §1.3, adopted by the data-driven architecture §4.
