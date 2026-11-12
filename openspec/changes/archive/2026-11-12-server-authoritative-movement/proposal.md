# Proposal: server-authoritative player movement

## Why

BDS 1.26.51 does not take a client's word for where its player is. It simulates every
`PlayerAuthInput` itself, keeps its own copy of the player, and answers a client it disagrees
with by `CorrectPlayerMovePrediction`. unastar took any finite position within 64 blocks of the
last one. Speed, fly, phase and no-fall cheats worked, and an input sent before the client saw a
teleport could move the player back. The `server-authority` spec says this outright ("There is
no server-side collision or speed simulation"), and the `player.movement` parity row lists it as
the gap.

Prior work:
- `docs/solutions/tooling/a-scripted-client-must-move-exactly-as-bds-simulates-it.md` describes
  the sweep client's `f32` physics, which runs on BDS without corrections. It also notes that BDS
  pulls its copy towards a client that disagrees.
- `docs/solutions/testing/replay-a-moving-client-per-phase-from-its-own-stamps.md` describes the
  movement replays this change extends.
- `docs/quirks/bds.md` covers the movement relay and movement exhaustion.
- Decision 0040 (written with this change) records the architecture.

Nothing earlier measured corrections, the acceptance rules or how a teleport is acknowledged.

## What Changes

- **Shared simulation.** The sweep client's movement physics moves into `unastar-rules`
  (`movement`), which is Bevy-free and `f32`. The server and the measuring client run the same
  code.
- **The server's own copy.** The server keeps a copy of each player's movement. It simulates
  every input on that copy and judges the reported position against it with the acceptance rules
  measured on BDS (`docs/quirks/bds.md`: Entities / player movement authority, pull). It answers
  with `CorrectPlayerMovePrediction` where BDS would. The copy, not the report, is the player's
  position for viewers, falls, `MOVING` and exhaustion.
- **Teleports and respawns.** After a teleport (command, plugin, ender pearl, chorus fruit), the
  next input rests at the destination and is corrected there if it is far off, so it cannot undo
  the teleport. A respawn places the copy at the spawn point and simulates the next input from
  there. A knockback becomes the copy's velocity.
- **Fail safe.** Some inputs are trusted: the copy takes the client's state, and a trusted input
  more than 64 blocks from the copy is rejected whole. This covers what the simulation does not
  reproduce, or no BDS run has confirmed yet:
  - flight in a game mode that can fly, and spectators (a client's own `StartFlying` counts
    for nothing in survival);
  - movement effects;
  - an item in use;
  - a sprint at 6 food or less;
  - depth strider, swift sneak and soul speed where they act;
  - a riptide spin;
  - crawl spaces;
  - blocks with movement rules of their own (climbing, honey, soul sand, lava, flowing water,
    bubble columns, slime, beds, powder snow, cobwebs, ice);
  - the first input after joining or respawning near the spawn point.

  A replay of every BDS movement capture produces only the corrections BDS sent.
- **Sweep client probes.** New probe orders measure the rules: `teleports`, `corrections`,
  `offset`, `drift`. `tools/bds-sweep/authority_fixture.py` distils the runs into a value-only
  fixture.

Out of scope:
- Measuring and simulating the trusted blocks and effects (each needs its own BDS run).
- Creative flight, riding and vehicle corrections.
- Dimension changes (unastar has one dimension).
- BDS's searching-for-spawn respawn sequence.
- Suffocation and freezing damage.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `server-authority`: the requirement "Client-reported movement is accepted within a per-tick
  delta" becomes "Player movement is simulated and judged by the server". Two requirements are
  added: teleports are enforced on the next input, and movement the simulation does not cover is
  trusted, never corrected.

## Impact

- Crates:
  - `unastar-rules` (`movement`, `movement::authority`)
  - `unastar` (`entity/player_life/authority.rs`, `server/game/movement.rs`, and the teleport,
    respawn, knockback and plugin hooks)
  - `bds-sweep` (physics re-exported from `unastar-rules`, probe orders)
- Tools: `tools/bds-sweep/authority_fixture.py`. `player_fixture.py` now also records teleports
  and corrections.
- Docs:
  - `docs/quirks/bds.md`: rows for movement authority, pull, teleport, no-fall and respawn
  - decision 0040
  - a solution note
  - `crates/unastar/README.md` and `tools/bds-sweep/README.md`

### Parity rows

| Domain | Row | Target |
|---|---|---|
| systems | `player.movement` | stays `partial`: corrections, teleports and cheats are judged as BDS judges them; the trusted blocks and effects, flight and riding stay in the gaps |
| systems | `player.damage_sources` | stays `partial`: falls are measured on the server's copy (no-fall); suffocation and freezing stay missing |
| protocol | `CorrectPlayerMovePredictionPacket` | `missing` → `verified` (player corrections, against the authority capture) |
| protocol | `PlayerAuthInputPacket` | stays `partial`: positions are judged; item interactions in the input are still ignored |
| protocol | `MovePlayerPacket` | stays `verified`: the notes add the teleport rule |

No claim: the work landed before this change was written up, and the change is archived with
it.
