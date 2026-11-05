---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Implement the first mobs by gameplay value, then by component coverage (D11)

## Context and Problem Statement

The mob runtime had to pick which vanilla actors to make work first.

## Considered Options

- Order by gameplay value
- Order by component coverage

## Decision Outcome

Chosen option: zombie, skeleton, creeper, spider, cow, pig, sheep, chicken first; then by component
coverage for the long tail.

### Consequences

- Good, because a playable survival loop arrives early.
- Status at 2026-11-04: zombie, cow, pig, sheep and chicken verified; skeleton, creeper and spider
  missing (see `docs/parity/entities.toml`).

## More Information

Origin: entities research Q4 (O3).
