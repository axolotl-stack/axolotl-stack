# movement-diff: BDS, unastar and bedsim, tick by tick

A three-way differential harness for Bedrock player movement. One scenario corpus
([`corpus.json`](corpus.json)) is played by three implementations, and every input's resulting
state is compared bit for bit:

1. **BDS 1.26.51** (protocol 2193), the reference, measured with the scripted sweep client;
2. **unastar** (`unastar_rules::movement`), the simulation the server judges players with;
3. **[oomph-ac/bedsim](https://github.com/oomph-ac/bedsim)**, a Go movement simulation used by
   an anticheat. MIT licensed. It is a Go module dependency of the runner, pinned in
   [`bedsim-runner/go.mod`](bedsim-runner/go.mod) to commit
   `9c12b344afdea7939b983fa15a762592aa179351` (2026-10-07). None of its source is in this
   repository.

## The corpus

Each scenario is:

- a small world: `fill`s of block states inside the area x -8..8, y 90..120, z -8..40;
- a start: the feet, yaw, pitch, status effects and an elytra;
- the inputs, in runs of identical ticks: the move vector, jump, sneak, sprint, glide, and yaw
  and pitch (set, or turned per tick);
- events between two inputs: a command for BDS (a `/damage` knockback) and the velocity it
  gives, for the simulations.

A block lists only the states that differ from its first canonical state.
`movement-diff resolve` spells out every state; the bedsim runner needs that, and the BDS runner
places every state explicitly. Every scenario starts with at least 3 idle inputs (see "BDS" below).
The player starts at rest and on the ground, as BDS's `/tp` leaves it, even in mid-air.

## Traces

Each implementation writes one JSON file in the same format
(`bds_sweep::scenario::Traces`). Every scenario has the state after every input:

- the wire position (the feet plus 1.62001, added in `f32`);
- the velocity;
- ground contact;
- the collision flags (the simulations only);
- the fall distance (bedsim only).

Every `f32` is written as its shortest decimal and its hex bits; the bits are authoritative.

## Running it

```sh
cargo build -p bds-sweep
# unastar
target/debug/movement-diff simulate --corpus tools/movement-diff/corpus.json --out unastar.json
# bedsim (Go 1.26+)
target/debug/movement-diff resolve --corpus tools/movement-diff/corpus.json --out resolved.json
(cd tools/movement-diff/bedsim-runner && go run . -corpus ../../../resolved.json -out ../../../bedsim.json)
# BDS, on one locked slot (tools/bds-sweep/README.md, "Parallel slots")
BDS_RUNTIME=.../bds-slots/slotN BDS_PORT=... python tools/movement-diff/bds_run.py [--only NAME ...]
# compare
python tools/movement-diff/compare.py RUN/bds.json unastar.json bedsim.json --report report.md [--scenario NAME --ticks]
```

A full BDS run of the corpus takes about 6 minutes (one scenario after another, about 30 setup
inputs each). The committed BDS traces are
`crates/bds-sweep/testdata/bds-1.26.51-movement-diff.json`. The test
`crates/bds-sweep/tests/movement_diff.rs` replays the corpus through unastar against them; see
"Known gaps" below.

## How BDS is measured

BDS keeps its own copy of every player and simulates each `PlayerAuthInput` on it. A client that
reports the right position hides that simulation: BDS takes any position within 0.1 of its copy,
and pulls its copy towards one within the threshold
(`docs/solutions/server/bds-judges-reported-positions-against-its-own-copy.md`). So the subject
client (`bds-sweep-client --scenarios`) reports a position **3 blocks off on every axis** from its own
simulation on every scenario input. That is far over `player-position-acceptance-threshold`, so BDS never
moves its copy towards the reported position. It answers with a correction at most every sixth
input, and the client ignores the reported position. BDS's copy then moves by BDS's
simulation alone.

- **Position and ground contact on every input**: a second client (`MoveViewer`) stands near the
  lobby and receives one `MovePlayer` relay of the subject per input, stamped with the input's
  tick. It carries BDS's copy after that input: the wire position as `f32` and `on_ground`.
- **Velocity on about every sixth input**: each `CorrectPlayerMovePrediction` carries the copy's
  position, velocity and ground contact after the input it is stamped with. `movement-diff
  extract` checks that each correction's position and ground contact equal the relay of the same
  input, bit for bit. In the committed run all 501 did.
- **Knockback**: a command sent after input N runs before BDS simulates input N, and its
  `SetActorMotion` is stamped N-1. The copy takes the velocity with input N, so the corpus gives
  the velocity "after N-1".
- **Speeds**: the movement attribute BDS sends (`UpdateAttributes`) is kept in the trace.

Each scenario is set up with `CommandRequest`s from the subject: clear the area, fill the blocks,
clear and give the effects, and give or take the elytra. After 20 quiet inputs the subject
teleports to the start. BDS rests the input after the teleport and simulates from the next one,
which is the scenario's first input. Inputs already in flight when the teleport arrives went out
idle, which is why every scenario starts idle.

### Precision limits

- The position is exact (`f32` bits on the wire), in wire space: feet + 1.62001. Two feet
  values that round to the same wire `y` cannot be told apart. Every pose uses the same offset.
- The velocity is known on one input in six. Between corrections it shows only through the
  following positions.
- Collision flags and the fall distance are not observable. A fall's damage would show the fall
  distance indirectly; it is not extracted yet.
- The subject derives its input flags (`StartSprinting`, `StartJumping`...) from its own
  simulation, which is unastar's. When unastar and BDS disagree about ground contact, a flag can
  differ from what a vanilla client would have sent.
- **Offset every axis.** A first run offset only the height. BDS still took the reported x and z
  whenever both were within 0.1 of its copy (quirk "player horizontal take"), and its copy then
  jumped to the subject's own simulation. That run made walls, edges, diagonal sneaks, stairs
  and sprints under speed or slowness look like divergences shared by unastar and bedsim. With
  3 blocks on x and z as well, no relayed position equals a reported one.

## The bedsim runner

`bedsim-runner` builds the scenario's world from Dragonfly's block registry (state names
`movement-diff resolve` spelled out). Stair corners and fence connections are dropped, because
Dragonfly computes them from the neighbours. Collision boxes come from Dragonfly's block models.
The runner calls `Simulator.Simulate` once per input in passive mode with
`IgnoreClientStepTiebreaker`. Input flags are derived from the keys and bedsim's own state, as a
vanilla client derives them from its prediction: `StartJumping` on ground with no jump delay,
`StartSprinting` and `StopSprinting` by the sprint rule, and the sneak edges. The movement
attribute includes speed and slowness as `0.1 × (1 + 0.2 × speed) × (1 − 0.15 × slowness)`.

## Known gaps

`report.md` is the comparison from the committed BDS run. Its scenarios where unastar still
differs from BDS are listed in the replay test with the tick they first differ at, so fixing one
fails the test until the list is updated.
