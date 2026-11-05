---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Source entity data from bedrock-samples, checked against the BDS install (D9)

## Context and Problem Statement

BDS 1.26.51 ships 129 entity files to bedrock-samples' 128 (BDS adds `agent`) and runs a layered pack
stack that could drift from bedrock-samples in later releases.

## Considered Options

- bedrock-samples (the `vanilla_bp` submodule)
- Read packs directly from a BDS install

## Decision Outcome

Chosen option: entity data comes from bedrock-samples (1.26.50.4+), checked against the BDS
install's layered packs where one is present, because bedrock-samples is redistributable and
reproducible in CI while the diff check catches drift.

### Consequences

- Good, because default builds need no BDS install.
- Bad, because a drift is only caught when someone runs the check with a local BDS.

## More Information

`crates/unastar-packs`, `crates/unastar-data/tests/bds_pack_diff.rs`. Origin: entities research Q1 (O1).
