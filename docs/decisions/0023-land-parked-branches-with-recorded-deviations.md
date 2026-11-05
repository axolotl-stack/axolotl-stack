---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Land parked gameplay branches with their non-BDS numbers recorded as known deviations (D12)

## Context and Problem Statement

Parked entity and survival branches carried values that were not yet measured from BDS (0014).
Waiting for the sweeps would keep working code off `main`.

## Considered Options

- Land them now, recording each non-BDS value as a known deviation
- Wait for the BDS sweeps

## Decision Outcome

Chosen option: land them with the deviations recorded, to be replaced by BDS sweeps later.

### Consequences

- Good, because the work merged and the gaps are named.
- Open deviations: **X1**, 199 block loot tables authored from Java tables
  (`crates/unastar-data/data/overrides/loot_tables/blocks.json`); **X2**, tool tiers, mining wear
  and the break-time formula are unmeasured constants (`crates/unastar/src/server/game/mining.rs`).
  X3–X8 and the survival deviations are resolved.
- The mob runtime constants and their provenance, which grew under this decision, are a reference
  table: [docs/research/mob-runtime-constants.md](../research/mob-runtime-constants.md).

## More Information

Origin: entities research Q5 (O4) and survival research (O5). Remaining work: OpenSpec change
`data-driven-gameplay`.
