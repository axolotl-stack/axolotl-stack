# Design: pistons measured on BDS

## Decisions

### Pistons are circuit consumers with a measured block table

- Options: pistons as scheduled block ticks with hand-written block lists (the #71 code), or a
  consumer of the circuit update with the per-block movement measured on BDS.
- Chosen: the circuit consumer with a measured table, because the measured timings fall on the
  circuit update without extra latencies and the measured table corrects the hand-written lists.
  Recorded as [decision 0050](../../../docs/decisions/0050-move-pistons-from-the-circuit-update.md).

## Model

- In the update's consumer phase a piston that is not moving compares its power (every side but
  its face) with its arm; a disagreement schedules a move for the next tick.
- Extending: resolve the structure (line, slime and honey branches, at most 12 blocks, breaking
  blocks at the end of a line); send the piston's actor, break, write every moved block as a
  moving block at its destination and air at its place (farthest first), write the head, send the
  moving blocks' actors and `piston.out`. Retracting removes the head first and pulls with a
  sticky piston.
- Two ticks later the moving blocks land (an observer lands powered), their actors are restored,
  and the piston's actor shows the new state. All writes are circuit changes, whose block updates
  run a tick later.
