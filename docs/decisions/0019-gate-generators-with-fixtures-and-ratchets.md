---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Gate each generator with fixtures from its own truth source, and ratchet the metrics (D8)

## Context and Problem Statement

With two generators (0018), "correct" needs a separate, objective reference for each, and
improvements must not regress silently.

## Considered Options

- Visual inspection and spot checks
- Reference fixtures per generator from its truth source, with parity and performance ratchets

## Decision Outcome

Chosen option: each generator is gated by reference fixtures from its own truth source, a Java
Edition server (run locally, loopback only) for Java-style and the shared BDS runtime for
Bedrock-style. Parity metrics ratchet: a change may only raise them. Performance is measured
(ms/chunk, allocations) and ratchets too.

### Consequences

- Good, because worldgen progress is a number that only goes up.
- Bad, because fixtures need local Java/BDS installs to refresh.
- The performance ratchet is partial: no committed comparison against BDS speed yet.

## More Information

`crates/unastar/tests/worldgen_parity.rs`, fixtures `bds_1_26_51_seed_12345*.json`,
`java_1_21_11_seed_12345*.json`, `worldgen_output_hash_*.json`.
