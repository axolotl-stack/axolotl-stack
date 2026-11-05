---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Match Bedrock decoration statistically first, exact placement later (WD4)

## Context and Problem Statement

Bedrock decoration uses its own per-chunk random and per-feature consumption order; reproducing
exact placement requires deriving that seeding from BDS fixtures.

## Considered Options

- Exact placement from the start
- Statistical match first (same rules, densities and shapes), exact placement later

## Decision Outcome

Chosen option: statistical decoration first; exact placement later (slice W14), with the seeding
derived from BDS fixtures.

### Consequences

- Good, because worlds look right before the seeding is understood.
- Bad, because seeded worlds don't match BDS feature-for-feature until W14.
