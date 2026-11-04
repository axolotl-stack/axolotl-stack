---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Measure built-in facts from BDS instead of copying wiki tables (D4)

## Context and Problem Statement

BDS keeps armour values, tool tiers, block fuels, block drops, knockback and fall multipliers in
code, not in shipped data, so they cannot be read from packs.

## Considered Options

- Author them from wiki or other-server values
- Measure them from BDS with sweeps and extraction, committed as fixtures

## Decision Outcome

Chosen option: measure from BDS (`/loot mine`, slot accept/reject, furnace timing, and similar).
Wiki and other-server values serve only as cross-checks.

### Consequences

- Good, because values are provably BDS's.
- Bad, because it needs the sweep harness (`crates/bds-sweep`, `tools/bds-sweep`) and committed
  measurements (`crates/unastar-data/data/measured/`).
- Partly implemented: armour, melee, fuel and armour slots are measured and used. Known deviations
  still open (see 0023): X1 block drops authored from Java tables (all 22,091 states swept but not
  wired in); X2 mining constants measured only for a 10-block slice.
