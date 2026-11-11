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

Chosen: trust input by input. An input is trusted when:
- the player flies or is a spectator;
- the player has speed, slowness or levitation;
- the search box contains a block the motion table marks unsupported. The box is the copy's box
  stretched by its velocity, grown by a block and reaching one layer below.

A trusted input sets the copy to the client's position and velocity, so judging resumes as soon
as the player leaves. The 64-block rejection still applies to trusted inputs. Teleports are
enforced even then. Effects are read through one accessor (`movement_effects_of`), so the
entities lane's effect state plugs in at one place.

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

- **The unsupported list is kept by hand.** A new block with movement rules of its own would be
  judged as a plain block. The movement replays (every BDS phase must produce only BDS's
  corrections) and `review-change` are the guard. Each new rule needs a BDS run.
- **Trusted areas stay open to cheats.** Near trusted blocks, and under trusted effects, a
  cheating client is believed as it was before this change.
- **Some differences from BDS remain.** BDS sometimes keeps its own copy of a client drifting
  less than 0.1 a tick, or hovering 0.09 up, where unastar takes the client's position. After a
  sprint-swim stop the copy takes a position up to 0.016 off where BDS kept its own. Both are
  pinned in tests with the measured inputs.
- **Cost.** Each input runs one simulation step and collects collision boxes around the player
  from the loaded chunks. That is comparable to the mob physics per tick.
