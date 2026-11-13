# movement-diff: BDS, unastar, bedsim and acacia-physics, tick by tick

A differential harness for Bedrock player movement. One scenario corpus
([`corpus.json`](corpus.json)) is played by four implementations, and every input's resulting
state is compared bit for bit:

1. **BDS 1.26.51** (protocol 2193), the reference, measured with the scripted sweep client;
2. **unastar** (`unastar_rules::movement`), the simulation the server judges players with;
3. **[oomph-ac/bedsim](https://github.com/oomph-ac/bedsim)**, a Go movement simulation used by
   an anticheat. MIT licensed. It is a Go module dependency of the runner, pinned in
   [`bedsim-runner/go.mod`](bedsim-runner/go.mod) to commit
   `9c12b344afdea7939b983fa15a762592aa179351` (2026-10-07). None of its source is in this
   repository.
4. **[acacia-physics](https://github.com/acacia-rs/acacia)** (`crates/acacia-physics`), a
   Rust port of bedsim with its own changes. MIT licensed. It is a git dependency of
   [`acacia-runner`](acacia-runner/Cargo.toml), pinned to commit
   `97ec4c7ce23e5abb7cde1e66026363d3c2e6fbf0` (2026-11-11; the crates.io 0.1.0 of 2026-10-03
   is older). None of its source is in this repository.

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

Besides the hand-written situations the corpus holds:

- probes that vary one thing at a time: `sr_*` (letting go of sneak), `ss_*` (sprint stops),
  `dt_*` (double-tap sprints), `sw_*` (sprinting into walls), `bed_drop_*` (bed bounces from
  several heights, measured for a fit);
- seeds of randomised runs, kept as regression scenarios: `random_N` (`movement-diff random`
  seed N) and `random_aN` (seed N of a batch made before the generator dropped carpets and
  snow, so the same seed now gives another scenario);
- mutation catchers (see "Harness validation"): `sprint_jump_yaw_*`, `step_lectern_slab`,
  `dive_90`, `slow_fall_tower`, `web_*`, `slime_web_jump`, `wall_slide_east`.

## Traces

Each implementation writes one JSON file in the same format
(`bds_sweep::scenario::Traces`). Every scenario has the state after every input:

- the wire position (the feet plus 1.62001, added in `f32`);
- the velocity;
- ground contact;
- the collision flags (the simulations only);
- the fall distance (bedsim only).

Every `f32` is written as its shortest decimal and its hex bits; the bits are authoritative.
The trace files (and the checks and dense-velocity fixtures) are laid out one tick a line
(`bds_sweep::json`): plain JSON, readable and diffable, at about a third of their
pretty-printed size.

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
# acacia-physics (its own Cargo project and toolchain, outside the workspace)
(cd tools/movement-diff/acacia-runner && CARGO_BUILD_JOBS=4 cargo run --release -- --corpus ../../../resolved.json --out ../../../acacia.json)
# the runners' worlds, for the runner-world check
target/debug/movement-diff world --corpus tools/movement-diff/corpus.json --out unastar-world.json
(cd tools/movement-diff/bedsim-runner && go run . -corpus ../../../resolved.json -out ../../../bedsim.json -world-out ../../../bedsim-world.json)
(cd tools/movement-diff/acacia-runner && cargo run --release -- --corpus ../../../resolved.json --out ../../../acacia.json --world-out ../../../acacia-world.json)
# compare, with the harness validation section
python tools/movement-diff/compare.py RUN/bds.json unastar.json bedsim.json acacia.json --report tools/movement-diff/report.md     --worlds unastar-world.json bedsim-world.json acacia-world.json --validation RUN/validation.json     --checks crates/bds-sweep/testdata/bds-1.26.51-movement-diff-checks.json --mutations tools/movement-diff/mutations.json
# harness checks: another session, another offset, flags from the keys alone
python tools/movement-diff/bds_run.py --out RUN2
python tools/movement-diff/bds_run.py --out RUN3 --offset=-5,5,-4
python tools/movement-diff/bds_run.py --out RUN4 --flag-source keys
target/debug/movement-diff check --traces RUN2/bds.json --against crates/bds-sweep/testdata/bds-1.26.51-movement-diff.json     --check repeat --offset 3,3,3 --into crates/bds-sweep/testdata/bds-1.26.51-movement-diff-checks.json
# velocity on every input
target/debug/movement-diff shift --corpus tools/movement-diff/corpus.json --shifts 1,2,3,4,5 --out shifted.json
python tools/movement-diff/bds_run.py --corpus shifted.json --out RUN5
target/debug/movement-diff velocities --base crates/bds-sweep/testdata/bds-1.26.51-movement-diff.json     --shifted RUN5/bds.json --out crates/bds-sweep/testdata/bds-1.26.51-movement-diff-velocities.json
# random scenarios: play them on BDS with the current simulation (the subject's flags come
# from it, so a run made before a rule change no longer scores it)
target/debug/movement-diff random --first 151 --last 300 --out random.json
python tools/movement-diff/bds_run.py --corpus random.json --out RUN6
target/debug/movement-diff simulate --corpus random.json --out random-unastar.json
# mutation check
python tools/movement-diff/mutate.py
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

## The acacia runner

`acacia-runner` is a separate Cargo project with its own `[workspace]`, lockfile and
`rust-toolchain.toml` (1.99.0). acacia-physics declares `rust-version` 1.98, newer than the
workspace's pinned nightly, and the workspace pin stays untouched. The block shapes are BDS's
(`movement-diff resolve` writes each block's collision boxes). The block physics are acacia's
own: its `TestBlock` definitions, and its `BlockPhysics` constructors for glass, fences and
bubble columns. The runner holds the controls and lets `tick` derive the start and stop
edges. `want_down` follows sneak, as a vanilla client sends it. `swim` is the sprint key while
the box touches water. The glide is requested from its press on. The effects go on
`state.effects`, and the movement attribute (with speed and slowness) through
`set_movement_attribute`.

`report.md` ends with a hand-written assessment of acacia against BDS ("How close is
acacia-physics to BDS?"). Regenerating the table does not rewrite it.

## Harness validation

The harness is checked as hard as the simulations it compares. Each check has a test or a
committed result; `report.md` ("Harness validation") gives the numbers of the committed run.

1. **Setup.** `movement-diff extract` replays the subject's capture: every chunk, sub-chunk and
   block update BDS sent, the effects (`MobEffect`), the chest slot and the start teleport
   (`MovePlayer`). At each scenario's first input it compares the whole scenario area, both
   layers, with the corpus. A position passes when it holds the corpus's block with the same
   collision boxes, friction, movement family and liquid; a state that differs only in what does
   not move a player (a plant's age) is logged and passes. The effects and amplifiers, the
   chest slot and the teleport's position and rotation must equal the corpus's bit for bit. Any
   difference fails the run (`validation.json` in the run directory says what differed;
   `bds.invalid.json` keeps the traces for diagnosis only).
2. **Inputs.** The same replay decodes every `PlayerAuthInput` the subject sent. Each scenario
   tick must have exactly one input, with the corpus's move vector (sent and raw), rotation and
   held keys (`JumpDown`, `SneakDown`, `SprintDown`, the direction flags), and no glide request
   without the glide key.
3. **Relays and knockbacks.** The viewer must have exactly one relay of every scenario input (a
   missing or second relay fails the extraction), and every velocity BDS set (`SetActorMotion`)
   must come after the input and with the value the corpus's events give. A `/damage` command
   that runs an input early or late fails the scenario; it once passed unnoticed, and every
   position after it differed.
4. **Known answers.** `known_still` and `known_free_fall` have physics simple enough to write
   down independently of unastar: a body standing on stone, and a fall from rest
   (`v = (v - 0.08) × 0.98` in `f32`, the first input resting as the teleport leaves it). The
   test `bds_reads_the_known_answers_bit_for_bit` works the answers out on its own and requires
   BDS's positions, ground contact and corrected velocities to equal them bit for bit. This
   checks the whole reading path: the relays, the corrections, the wire offset and the tick
   alignment.
5. **Repeatability and offset independence.** The corpus is played again in fresh BDS sessions:
   once as committed (offset 3, 3, 3), once with the reported position off by (-5, 5, -4) (other
   signs, sizes and axis order). `movement-diff check` reduces each run to one SHA-256 per
   scenario over every exact value of its trace
   (`crates/bds-sweep/testdata/bds-1.26.51-movement-diff-checks.json`), and the test
   `other_bds_runs_give_the_committed_traces` requires every scenario to be bit-identical to the
   committed run, except the scenarios a run lists with the reason it differs.
6. **Flag derivation.** The subject derives its derived flags (`StartSprinting`, `StartJumping`,
   the collision flags...) from unastar's simulation, as a vanilla client derives them from its
   prediction. A third run takes them from the keys alone (`--flag-source keys`: starts and stops
   on the key edges, no collision or swim flags). Every scenario whose trace stays bit-identical
   does not depend on the derived flags at all; the others are listed with what BDS took from
   the flag.
7. **Runner worlds.** `movement-diff world` and the runners' `--world-out` write each scenario's
   blocks as each implementation builds them (world-space boxes and friction, plain full cubes
   left out). `compare.py --worlds` marks a scenario "runner world" for a runner whose world
   differs from unastar's (BDS's shapes), so a divergence the runner causes is not reported as
   the library's physics.
8. **Mutations.** `mutate.py` changes the simulation a little (a constant one `f32` ULP off, two
   operations swapped, a threshold moved, a rule switched off), plays the corpus and lists the
   scenarios that catch each change (`mutations.json`). A mutation no scenario catches is a
   blind spot, and the next scenario to write. Write catchers by simulating candidates with and
   without the mutation and keeping those that differ in what a BDS trace shows: the position on
   every tick (smallest ULPs near the origin), ground contact, and the velocity on every sixth
   tick. A constant whose one-ULP change no product can show (the swim rate, slow falling's
   gravity) is mutated by the smallest change that shows.
9. **CI.** `cargo test -p bds-sweep` runs the replay, known-answer and other-run tests; the
   `movement-harness` job runs `test_compare.py` and the bedsim runner's `go vet` and `go test`.

### Velocity on every input

BDS corrects at most every sixth input, counted from the scenario's start, so its velocity shows
on one input in six. `movement-diff shift --shifts 1,2,3,4,5` writes the corpus again with one to
five more idle inputs before each scenario; BDS then corrects other inputs of the same movement.
`movement-diff velocities` merges the shifted run with the committed one: for every scenario
whose shifted copies move exactly as the original (positions and ground contact bit for bit,
which holds for a player resting on the ground at the start), it gives BDS's velocity after every
input (`crates/bds-sweep/testdata/bds-1.26.51-movement-diff-velocities.json`), and the replay
test compares unastar's velocity on every one of those inputs. A scenario whose start is not at
rest (a fall from a teleport in the air, levitation, liquids) moves during the extra idle inputs
and keeps one velocity in six.

Deriving the velocity from consecutive positions is not exact: the wire position is the feet
plus 1.62001 rounded to `f32`, and the feet are the centre of a translated box, so a position
difference is the velocity only up to the rounding of two sums (several ULPs at y 100).

## Known gaps

`report.md` is the comparison from the committed BDS run. Its scenarios where unastar still
differs from BDS are listed in the replay test with the tick they first differ at, so fixing one
fails the test until the list is updated.
