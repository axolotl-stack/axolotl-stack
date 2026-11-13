# Proposal: a validated movement harness, and less trust

## Why

The movement differential (`tools/movement-diff`) is what decides which movement the server
judges and which it trusts. Until now nothing checked the harness itself. Its first setup check
showed that the committed run's `bubble_column_up` had no bubble column: BDS had turned the
column into air. Fence end posts and scaffolding also had other states than the corpus said.
The server still trusted slowness, levitation, lava, scaffolding, sweet berry bushes and the
climbing plants. Each of these can be measured on BDS and simulated.

Prior work:
- [decision 0054](../../../../docs/decisions/0054-measure-bds-movement-through-a-far-reporting-client.md)
  and `docs/solutions/tooling/read-bds-movement-from-a-far-reporting-client.md`: the
  far-reporting method this change validates;
- [decision 0040](../../../../docs/decisions/0040-judge-player-movement-with-a-shared-simulation.md)
  and the archived `server-authoritative-movement` change: the trust this change narrows;
- `docs/quirks/bds.md`: the player movement rows this change extends.

## What Changes

- **Harness validation.** `movement-diff extract` checks every run: the scenario's world, the
  effects, the chest slot and the start teleport against the corpus, read back from the
  subject's capture; every sent input against the corpus; one relay per input. Further checks:
  - known-answer scenarios (standing still, a free fall) checked against values worked out
    independently of unastar;
  - other BDS runs (another session, another reported offset, input flags from the keys alone)
    recorded as digests and compared by a test;
  - each runner's world compared with BDS's shapes;
  - a mutation check of the simulation;
  - the comparator and the bedsim runner run in CI.
- **Corpus.** 144 scenarios (from 59). Blocks BDS forms during setup are declared. The bubble
  columns are played last, and the area is filled with stone before it is cleared.
- **Simulation.** It now covers or fixes:
  - lava, sweet berry bushes, scaffolding, the climbing plants, honey's wall slide and the
    powder-snow landing;
  - the water box, the surface sprint and the swim stop;
  - the sine table (its angles in `f32`), the glide and swim look vector, and the sneak and
    sprint timing.
  Blocks reach the simulation through one conversion (`unastar_data::motion`) with a typed
  liquid state.
- **Trust.** Slowness, levitation, lava sources, scaffolding, sweet berry bushes and the
  climbing plants are judged. The movement attribute carries BDS's effect modifiers.

## Impact

Crates: `unastar-rules` (movement), `unastar-data` (motion), `unastar` (authority, attributes,
registry), `bds-sweep` (validation, scenario, extract, check, shift, random, world), the
movement-diff tools, the CI workflow.

### Parity rows

- `systems`: `player.movement` stays `partial` (bubble columns, beds, powder snow's
  slowdown, flowing water, gear and riptide are still trusted).
- `protocol`: `UpdateAttributesPacket` gap text (the effect modifiers are now sent).

## Out of scope

- Bubble columns, bed bounces, powder snow's freezing slowdown, flowing water currents, the
  movement enchantments and riptide stay trusted. Their scenarios are in the corpus where
  measurable.
- BDS's judgement of heights inside the acceptance threshold.
