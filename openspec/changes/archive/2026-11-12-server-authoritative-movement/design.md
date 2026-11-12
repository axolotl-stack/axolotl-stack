# Design: server-authoritative player movement

## Context

`PlayerAuthInput` positions were taken as reported, if finite and within 64 blocks. Round 3 gave
the sweep client (`bds-sweep-client`) `f32` movement physics. That client walks, sprints, sneaks,
jumps, swims, glides and falls on BDS 1.26.51 without a single correction, so its simulation
already agrees with BDS's own copy for vanilla movement on plain blocks and in water. It does not
yet cover climbing, honey, soul sand, lava, bubble columns, slime and bed bounces, powder snow,
cobwebs, speed, slowness, levitation or flight.

BDS's acceptance rules were measured for this change with new probe orders and a second client
watching BDS's copy. The rules are recorded as quirk rows (Entities / player movement authority,
pull, teleport, no-fall, respawn). The hard constraint: a legitimate client must never be
corrected where BDS would not correct it.

## Goals / Non-Goals

**Goals:**
- Judge every input BDS would judge, with BDS's rules, wherever the simulation is proven.
- Make teleports impossible to undo with inputs sent before them.
- Make the server and the measuring client run the same simulation, so a run on BDS checks the
  server's physics.

**Non-Goals:**
- Anti-cheat beyond BDS: no extra speed, reach or timer heuristics.
- Simulating the blocks and effects listed above. Each needs its own BDS run first.
- Flight, riding, vehicle corrections, dimension changes.

## Decisions

### One simulation, shared with the measuring client (decision 0040)

Options considered:
- Keep trusting the client and add speed and gap checks. These are not BDS's rules: they would
  both miss cheats and flag legitimate moves.
- Write a separate simulation in `unastar`. It would drift from the client measured on BDS.
- Move the sweep client's physics into `unastar-rules` (`movement`) and run it in both.

Chosen: the shared module. It is Bevy-free and `f32`, and a `Terrain` trait (collision boxes,
loaded check, block kind) adapts it to the server's chunks and to the client's sub-chunks.
`bds-sweep`'s `physics` now re-exports it.

### BDS's acceptance rules, not a tolerance of our own

Options considered:
- Correct beyond a fixed distance.
- Copy BDS's measured cadence: take within 0.1 per axis, pull 0.005 a tick, correct on the
  eleventh disagreement or beyond the squared threshold at most every sixth input, then six
  quiet inputs.

Chosen: copy BDS. With any other tolerance the server would either correct inputs BDS accepts or
accept inputs BDS corrects. Where BDS's rule is unknown (steady drifts, hovering 0.09 up), the
copy takes the client's position inside 0.1. That is the lenient direction. The rules live in
`unastar_rules::movement::authority`. The per-player state and the ECS glue live in
`player_life::authority`.

### Trust what the simulation does not reproduce

Options considered:
- Judge everything. Players would be corrected on ladders, honey or soul sand, for example.
- Turn the authority off until all physics are measured. Cheats would keep working on plain
  ground.
- Trust input by input.

Chosen: trust input by input (`Trust`, decided from a per-input `Situation`), with each
trust bounded by who brings it about.

The server grants two trusts. A trusted input more than 64 blocks from the copy is rejected
whole:
- the player flies in a game mode that can fly, or is a spectator. A client's own
  `StartFlying` counts for nothing in survival;
- the player has a movement effect. Speed, slowness and levitation are not simulated; jump
  boost and slow falling are, but no BDS run has confirmed them.

A client can seek out the rest, so each covers about one tick of vanilla movement, per axis,
and never passes through a block the simulation reproduces. The ladder, scaffolding or cobweb
itself does not count, so climbing still works, but the wall a ladder hangs on does. Beyond
that the copy judges the input:
- the search box contains a block the motion table marks unsupported (1.5 horizontally, 4
  vertically). That covers unsupported identifiers, families the simulation has but no run
  confirmed, lava, flowing water (its current pushes), and friction other than the confirmed
  0.6 (ice), in either block layer. The box is the copy's box stretched by its velocity,
  grown by a block and reaching one layer below;
- a movement enchantment acts (same bounds): depth strider near water, swift sneak while
  sneaking, soul speed near soul sand or soil;
- a riptide trident's spin, started in water (this server has no rain), for at most 20
  inputs (4 per axis);
- the copy is in a crawl space. Only the copy's own position counts: a claimed crawl space
  would let a client make itself trusted anywhere.

Slowdowns get no trust reason. A client that is only slower than the copy's own simulation
of its keys is taken: horizontally no farther from the start of the input, vertically level,
on a move the keys drive (not a pull, nor momentum after the keys are let go). That covers
every slowdown the simulation does not model, such as an item in use (including ones the
server does not implement, like a spyglass or a trident) or a sprint the food level forbids.
It never gives an advantage. It is off for 20 inputs after a knockback, so a client that
refuses its knockback is still corrected as BDS corrects it.

A trusted input sets the copy to the client's position and velocity (capped at 4 a tick per
axis, the terminal falling speed), and to the state the simulation carries between inputs as
the client's flags report it: the sprint with its speed, sneak, swim, glide, jump delay and
collisions. Judging then resumes where the client is. A non-finite reported value rejects the
input. Teleports are enforced even for trusted inputs. Effects are read through one accessor
(`movement_effects_of`), so the entities lane's effect state plugs in at one place.

### Teleports rest the next input; a respawn does not

This copies BDS. The input after a teleport is not simulated. The copy rests at the destination
and is corrected there when the input is beyond the threshold. `HandledTeleport` is ignored, as
BDS ignores it.

A respawn places the copy at the spawn point and simulates the next input, because BDS does.
Every server-side move updates the copy: `/tp`, plugin teleports, ender pearls and chorus fruit
mark a teleport, a respawn places the copy, and a knockback becomes its velocity. Any move that
skips this would have its next input corrected back.

### The first input after a join or respawn may settle

A configured spawn point can sit inside a block. The loopback tests' spawn has the eyes at y 17
over a platform whose top is at 16, which puts the feet 0.62 into it. A copy started there would
fall through. So the first input after
joining or respawning is trusted when it is within 2 blocks of the spawn point. Farther off, it
is simulated and judged as BDS judges the input after a respawn. BDS's handling of a spawn
inside a block is unmeasured.

### Wire positions and server positions

Clients report the feet plus 1.62001 in every pose. The server's `Position` is the feet plus
1.62. Converting a report with 1.62 puts the copy 1e-5 inside the floor. Collision ignores boxes
the body already overlaps, so the copy falls through. Reports are therefore converted with the
wire offset (`NETWORK_OFFSET`), and server positions with the eye height.

## Risks / Trade-offs

- **The trust list is kept by hand.** A new block, effect or item with movement rules of its own
  would be judged as plain until it is added. The movement replays catch this for what they
  cover, since every BDS phase must produce only BDS's corrections. Anything else vanilla play
  does has to be listed. Each situation leaves the list only after a BDS run of the sweep client
  covers it without corrections.
- **Trusted areas stay open to cheats.** Near trusted blocks, and under trusted effects, a
  cheating client is believed as it was before this change.
- **Some differences from BDS remain.** BDS sometimes keeps its own copy of a client drifting
  less than 0.1 a tick, or hovering 0.09 up, where unastar takes the client's position. After a
  sprint-swim stop the copy takes a position up to 0.016 off where BDS kept its own. Both are
  pinned in tests with the measured inputs.
- **Cost.** Each input runs one simulation step and collects collision boxes around the player
  from the loaded chunks. That is comparable to the mob physics per tick.
