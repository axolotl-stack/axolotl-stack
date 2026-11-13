---
status: proposed
date: 2026-11-12
decision-makers: [project owner]
---

# Validate the movement harness against BDS itself

## Context and Problem Statement

The movement differential ([0054](0054-measure-bds-movement-through-a-far-reporting-client.md))
decides which player movement the server judges and which it trusts. Nothing checked the
harness itself. Its first setup check showed that the committed run's `bubble_column_up` never
had a bubble column: BDS had turned the column into air. A bubble column's water stays in the
second block layer when the area is cleared, which spoiled 85 later scenarios of the next run,
and BDS recomputes fence and pane connections when it fills them. We needed every run proven
to play the corpus exactly, and the method proven not to bias BDS.

## Decision Drivers

- Copy BDS exactly ([0013](0013-copy-bds-behaviour-exactly.md)); black-box only.
- A failed check must fail the run, not leave a note.
- Checks must stay cheap enough to run on every BDS run, and their results must be committed
  as structured JSON a test reads.

## Considered Options

1. **Read the setup back from the subject's capture; compare other runs by digest; shift
   scenarios for more velocities.**
2. Query the world with `/testforblock` between inputs; commit every other run's traces.
3. Trust the corpus and the client.

## Decision Outcome

Chosen: option 1.

- **Setup.** `movement-diff extract` replays the chunks and block updates BDS sent the subject,
  with the effects, chest slot and start teleport. It compares them with the corpus at each
  scenario's first input. A difference that moves a player fails the run. Blocks BDS forms are
  declared in the corpus (`formed`).
- **Inputs and relays.** Every sent `PlayerAuthInput` must carry the corpus input, and every
  input must have one relay.
- **Known answers.** Two scenarios whose physics a test works out independently.
- **Other runs.** Another session, another reported offset and keys-only input flags are each
  reduced to one SHA-256 per scenario, over everything a repeat must reproduce, plus the
  corrected velocities by tick: two runs correct different inputs of the same movement.
- **Runner worlds** are diffed against BDS's shapes, and divergences they cause are labelled.
- **Mutations** of the simulation must each be caught by a scenario.
- **Velocity on every input.** Shifted copies of scenarios that start at rest move the
  correction phase over the same movement. Position differences are not exact, and BDS
  corrects at most every sixth input whatever the offset.

### Consequences

- Good: a run that does not play the corpus exactly can no longer become a fixture, and the
  method's independence from the reported position and the derived flags is measured, not
  assumed.
- Good: the digest check is a test, so any later run can be added and compared in CI.
- Bad: each check is another BDS run (about 25 minutes for the corpus). The corpus must spell
  out the states BDS computes on placement.

### Confirmation

`crates/bds-sweep/tests/movement_diff.rs` (`bds_reads_the_known_answers_bit_for_bit`,
`other_bds_runs_give_the_committed_traces`, the dense velocities in
`unastar_moves_like_bds_in_every_scenario`); `tools/movement-diff/report.md` ("Harness
validation"); `tools/movement-diff/README.md`.
