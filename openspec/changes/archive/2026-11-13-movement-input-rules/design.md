# Design: movement input rules

## Decisions

### D1. Score rules with a fresh BDS run of the seeds, not a replay

- Options: replay the committed random run after each fix; play the seeds again on BDS.
- Chosen: play them again. The subject derives its flags from unastar's simulation, and BDS
  answers flags as well as keys, so a run made before a rule change measures the old rule's
  flags. Replays only propose rules.

### D2. One probe family per rule

- Options: keep only the random seeds that showed a rule; write probes that vary one thing at a
  time.
- Chosen: both. Seeds show that a rule matters, and probes pin its constants: the 7-input
  double-tap window, the 45-degree wall turn, the release at every tick offset.

### D3. Mutations at the smallest observable size

- Options: keep one-ULP mutations of every constant; mutate each constant by the smallest
  change some observation shows.
- Chosen: the smallest observable change. One ULP of the swim rate or of slow falling's gravity
  never moves a product past an `f32` rounding step, so no scenario can catch it.

### D4. Fixtures one tick a line

- Options: keep pretty-printed JSON; delta-encode positions; binary traces with a JSON index;
  one tick per line.
- Chosen: one tick per line. About a third of the size, still plain JSON, and a diff shows the
  ticks that changed.

These become [decision 0067](../../../../docs/decisions/0067-find-movement-rules-from-random-seeds-and-probes.md).
