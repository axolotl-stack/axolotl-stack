# Design: a validated movement harness

## Decisions

### D1. Read the setup back from the subject's capture, not with commands

- Options: query each block with `/testforblock`; read Endstone's world; replay the chunks and
  block updates BDS sent the subject.
- Chosen: replay the capture. It needs no extra commands between the inputs, covers both block
  layers and the whole area, and is what the client actually moved through. The check fails the
  extraction on any difference that moves a player (block, collision boxes, friction, family,
  liquid).

### D2. Compare other runs by digest, and velocities only where both read one

- Options: commit every alternative run's traces; commit one digest per scenario.
- Chosen: one SHA-256 per scenario over the positions, ground contact, velocity events and
  speeds, plus the corrected velocities by tick. Two runs of the same corpus correct different
  inputs (the phase depends on when the first far input arrives), so a whole-trace digest would
  report a difference that is not one.

### D3. Velocity on every input from shifted copies

- Options: derive it from consecutive positions; force more corrections; play each scenario
  with one to five more idle inputs.
- Chosen: shifted copies. Positions are rounded wire values, so differences are not exact.
  BDS corrects at most every sixth input, so a different offset adds no corrections. Shifting
  the scenario moves the correction phase over the same movement, for scenarios that start at
  rest.

These become [decision 0060](../../../docs/decisions/0060-validate-the-movement-harness-against-bds-itself.md).
