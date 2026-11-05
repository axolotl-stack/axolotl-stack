---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# One Rust implementation per component kind, on pack data, with a BDS-exact pack loader

## Context and Problem Statement

Vanilla actors are behaviour-pack JSON over built-in behaviour. The runtime needs a structure for
that behaviour, and the loader must agree with BDS on what pack input is valid.

## Considered Options

- One Rust implementation per component kind, filter test, event action and AI goal, generic over
  entities, plus registries for kinds BDS hard-codes
- Per-entity-kind systems (one implementation per actor type)

## Decision Outcome

Chosen option: hand-written Rust ECS systems that behave exactly like BDS and run on the same pack
data, one per component kind, filter test, event action and AI goal, plus an actor-kind registry for
kinds BDS hard-codes. Outside those registries no code branches on an identifier. The pack loader
accepts, normalises and rejects exactly what BDS does, quirks included; each rule lives next to the
loader, is proved by a crafted pack run against BDS plus a golden test, and the runtime only sees the
normalised form.

### Consequences

- Good, because a new vanilla actor usually needs no new code.
- Good, because loader fidelity is tested by 58 BDS probes
  (`crates/unastar-packs/tests/fixtures/bds-1.26.51-fidelity.json`).
- Gap: no `ActorKindRegistry` type exists yet; built-in kinds (item, xp_orb, projectiles) are
  separate modules.

## More Information

Origin: entities research Q3 and §4.7 ("parser fidelity"), both owner-resolved.
`docs/research/2026-10-24-data-driven-entities.md` §4.7 has the detail.
