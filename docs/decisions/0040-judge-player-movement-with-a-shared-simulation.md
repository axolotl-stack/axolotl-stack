---
status: proposed
date: 2026-11-11
decision-makers: [project owner]
---

# Judge player movement with a simulation shared with the measuring client, trusting what it does not cover

## Context and Problem Statement

BDS 1.26.51 is server-authoritative for player movement: it simulates every `PlayerAuthInput`
itself, keeps its own copy of the player and answers a client that disagrees with
`CorrectPlayerMovePrediction`. unastar took the client's position within 64 blocks, so speed,
fly, phase and no-fall cheats worked, and an input sent before the client saw a teleport could
undo it. Matching BDS needs a simulation that agrees with a vanilla client to `f32` precision
(BDS takes a client's position only within 0.1 of its copy, pulls 0.005 a tick otherwise), and
that simulation can only be measured on BDS with a client that runs it. Measurements and the
acceptance rules are in `docs/quirks/bds.md` (Entities / player movement authority, pull,
teleport); the change is `openspec/changes/archive/2026-11-12-server-authoritative-movement/`.

## Decision Drivers

- Copy BDS exactly ([0013](0013-copy-bds-behaviour-exactly.md)); built-in rules are measured
  ([0014](0014-measure-built-in-facts-from-bds.md)).
- Never rubber-band legitimate play: where the simulation is wrong, BDS's corrections are the
  ground truth, and a vanilla movement BDS accepts must not be corrected.
- Teleports must not be undoable.

## Considered Options

- Keep trusting the client within a per-tick delta, adding speed and gap checks.
- Simulate in `unastar` only, separately from the sweep client's physics.
- One Bevy-free simulation in `unastar-rules` (`movement`), run by the server and by the sweep
  client; the server judges each input with BDS's acceptance rules and trusts the client where
  the simulation does not reproduce vanilla yet.

## Decision Outcome

Chosen option: the shared simulation, because the sweep client measures exactly the code the
server runs. A movement the client performs on BDS without one correction is one the server
simulates the same way; a correction BDS sends that client marks a simulation gap to fix
before the server judges that block or effect. The server keeps a per-player copy
(`player_life::authority`): each input is simulated (`unastar_rules::movement::step`) and judged
(`unastar_rules::movement::authority`), the copy is the player's position for viewers, falls,
`MOVING` and exhaustion, and a teleport makes the next input rest at the destination. Where
the simulation does not reproduce vanilla, or no BDS run has confirmed it, the copy takes the
client's position and velocity, as the server did before:
- blocks: climbing, honey, soul sand, lava, flowing water, bubble columns, slime and bed
  bounces, powder snow, cobwebs, ice;
- movement effects, items in use, low food, movement enchantments, riptide spins, crawl spaces;
- flight in a game mode that can fly.

A trusted input farther than 64 blocks from the copy is rejected whole.

### Consequences

- Good, because outside the trusted situations, cheats BDS stops are stopped the same way
  (verified input by input against BDS's own copy), and a teleport can no longer be undone by
  inputs sent before it.
- Good, because every new movement rule is measured once and serves both the server and the
  measuring client.
- Bad, because `unastar-rules` gains `f32` physics with its own `Terrain` trait, adapted to the
  server's chunks and to the client's sub-chunks.
- Bad, because the trusted situations stay client-authoritative until each is measured, and the
  list of them is kept by hand: anything vanilla play does that is neither replayed nor listed
  would be judged by an incomplete simulation.
- Bad, because BDS's acceptance of steady drifts is not fully known: unastar takes positions
  within 0.1 where BDS sometimes keeps its own copy.

### Confirmation

`crates/unastar/src/entity/player_life/tests/authority.rs` replays the authority probes and
requires BDS's copy and corrections, and checks that each trusted situation is trusted. The
movement replays (`tests/movement.rs`) require BDS's corrections and never one BDS did not send.
A situation leaves the trusted list only with a BDS run of the sweep client that covers it.

## More Information

Measurement tools: the sweep client's probe orders (`teleports`, `corrections`, `offset`,
`drift`, `walk`), `tools/bds-sweep/authority_fixture.py`. Solution note:
`docs/solutions/server/bds-judges-reported-positions-against-its-own-copy.md`.
