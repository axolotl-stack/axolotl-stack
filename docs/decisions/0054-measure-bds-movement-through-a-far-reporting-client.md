---
status: proposed
date: 2026-11-12
decision-makers: [project owner]
---

# Measure BDS's own movement with a client that reports a far position, and check two simulations against it

## Context and Problem Statement

unastar judges players with `unastar_rules::movement`. That simulation was checked on BDS
1.26.51 only by a scripted client that runs it and reports what it computes. BDS takes any
position within 0.1 of its own copy and pulls its copy towards one within its threshold, so a
run without corrections showed that the simulation stays within 0.1 of BDS's. It could not
show where they differ by less. A ground acceleration one ULP off, a box rebuilt from rounded
feet, or a diagonal 2 % too fast were all invisible. We needed BDS's own state after every
input, exactly. We also wanted to know whether a third implementation (oomph-ac/bedsim) is a
useful lead.

## Decision Drivers

- Copy BDS exactly, to the bit where possible ([0013](0013-copy-bds-behaviour-exactly.md),
  [0014](0014-measure-built-in-facts-from-bds.md)).
- Black-box only: packets and observed behaviour.
- One scenario corpus, played by every implementation the same way, kept as structured JSON.

## Considered Options

1. **Report a far position; read the copy from a viewer's relays and from the corrections.**
2. Report deliberately wrong positions close by, and read only the corrections.
3. Read the player's position and velocity every tick through an Endstone plugin.
4. Report the right positions, and trust that zero corrections means a match.

## Decision Outcome

Chosen option: **1**. The subject client reports a position 3 blocks above its own simulation
on every scenario input. That is far over the acceptance threshold, so BDS never takes or pulls
towards it, and BDS's copy moves by its own simulation alone. A second client receives one
`MovePlayer` relay of the copy per input, stamped with that input: the exact `f32` wire
position and ground contact. The forced corrections, at most one every six inputs, add the
exact velocity. Each correction's position and ground contact must equal the relay of the same
input, which checks the method on every run. In the first run all 470 corrections agreed.

The same corpus (`tools/movement-diff/corpus.json`) runs through unastar
(`movement-diff simulate`) and through bedsim, pinned in `go.mod` and never vendored
(`tools/movement-diff/bedsim-runner`). `compare.py` reports the first divergence of each pair.
A replay test in `bds-sweep` holds unastar to the committed BDS traces bit for bit. Each
scenario it does not match yet is listed with its first differing tick, so a fix or a
regression fails the test.

### Consequences

- Good: unastar's simulation can be fixed to the bit against BDS, case by case. The first
  pass fixed the ground acceleration, the kept collision box, the move vector, the input
  trigonometry, the edge friction, soul sand, honey, slime walking, slow falling, ladders,
  vines, effect speeds and signed zeros.
- Good: bedsim's agreements with BDS are a lead (read for the formula, re-implemented and
  re-verified); its disagreements are recorded for its users.
- Bad: velocity is seen on one input in six, and collision flags and fall distance not at all.
  Effects BDS applies on its own tick (attribute changes) depend on timing and are not
  reproducible per input.
- Bad: the subject derives its input flags from unastar's simulation. Where unastar and BDS
  disagree about ground contact, a flag can differ from a vanilla client's.

## Pros and Cons of the Options

- Option 2 pulls the copy on every input that is not corrected, so BDS's simulation is mixed
  with the pull. Option 3 reads on the server tick, not per input, needs Endstone, and its
  floats go through another conversion. Option 4 is what was there before, and it cannot see
  errors under 0.1.

## More Information

`tools/movement-diff/README.md` (method, precision limits, running it),
`docs/solutions/tooling/read-bds-movement-from-a-far-reporting-client.md`,
`docs/quirks/bds.md` (Entities / player movement rounding, move vector, teleport ground, slow
falling, slide stop, edge friction, effect sprint). Supplements
[0040](0040-judge-player-movement-with-a-shared-simulation.md).
