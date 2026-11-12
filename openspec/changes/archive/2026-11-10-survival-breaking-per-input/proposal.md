# Proposal: survival breaking advances per client input

## Why

A play-test reported survival block breaking as "very jank". The `server-authority` contract
timed a break in server ticks from `StartDestroyBlock`. It accepted the client's
`PredictDestroyBlock`, `StopDestroyBlock` or `CreativeDestroyBlock` after 90 % of that time.

Inputs that arrive bunched by latency therefore turned valid predictions into snap-backs. A break
the client never predicted was never completed.

BDS 1.26.51 captures of scripted breaking show a different model:
- BDS advances a break once per `PlayerAuthInput` of the breaking player, idle inputs included.
- It completes the break itself.
- A prediction never ends a break, and an early one is refused while the break goes on.

## Prior work

- [decision 0013](../../../../docs/decisions/0013-copy-bds-behaviour-exactly.md) (copy BDS
  exactly) and [0014](../../../../docs/decisions/0014-measure-built-in-facts-from-bds.md)
  (measure built-in facts from BDS).
- The 2026-10-25 quirk row "BDS breaks the block itself" in `docs/quirks/bds.md` (Blocks / mining),
  until now "Not copied yet".
- `docs/solutions/server/break-ticks-land-on-exact-multiples-despite-f32-hardness.md`, for the
  per-input break fraction.
- Mojang's public protocol notes on block breaking, in the `bedrock-protocol-docs` submodule:
  - the server follows the client's progress input by input;
  - `StopDestroyBlock` carries the default position.
- Nothing in `docs/solutions` covered the breaking packet flow.

## What Changes

- **Progress per input.** Every accepted `PlayerAuthInput` reaches the block system. A survival
  break's progress is kept by the server and advances once per input by the measured per-input
  fraction, never per server tick.
- **Bursts.** Inputs that arrive together are applied together; BDS does not hold a client to server
  time either (measured with a burst of 15 inputs). A player's actions after an input that completed
  a break wait one tick, for the plugin commit.
- **Completion.**
  - The server completes a break itself on the input whose progress completes it.
  - A prediction before that is refused with block corrections and does not end the break.
- **Client actions.**
  - A survival player's `CreativeDestroyBlock` is refused.
  - Breaking actions with no valid face are ignored.
  - `StopDestroyBlock` breaks nothing and, as in BDS, does not end the break.
- **Packets.** The crack, destroy and block-update packets of each input copy the BDS capture, and
  the server no longer sends hit or break sounds. Where a break goes on after it completed is
  recorded with them in `docs/quirks/bds.md` (Blocks / mining).
- **Harness.**
  - `bds-sweep-client --script`, with `pause N` (a frozen client), `burst N` and `release` orders;
  - `tools/bds-sweep/script.py` and `scripts/breaking.txt` (twelve scenarios);
  - the `breaking_fixture.py` extractor;
  - the committed capture `crates/unastar/src/server/game/blocks/testdata/bds-breaking.jsonl`,
    with a replay test.

Out of scope:
- block placing (the blocks lane);
- what other players see of a break;
- creative breaking packets;
- neighbour updates after a break, and ore experience;
- breaking while airborne, riding or underwater beyond the existing break-time rules;
- a per-tick input limit. BDS has none that a 15-input burst reached; the inbound per-session cap
  still applies.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `server-authority`, three requirement changes:
  - survival breaking advances per input on the server (renamed from "timed by the server");
  - client actions cannot break a survival block;
  - inputs that arrive together are applied together.

## Impact

- Crates:
  - `unastar`: `server/game/blocks.rs`, `server/game/movement.rs`,
    `entity/components/player.rs` and `server/broadcast.rs`;
  - `bds-sweep`: the scripted client.
- Tools: `tools/bds-sweep` (`script.py`, `scripts/breaking.txt`, `breaking_fixture.py`,
  `sweep.py mining --variants capture`).

### Parity rows

| Domain | Row | Target |
|---|---|---|
| systems | `player.block_breaking` | stays `partial`. Neighbour updates, ore experience, other viewers and creative breaking remain. Its tests and notes now point at the BDS breaking capture. |
| protocol | `PlayerActionPacket` | stays `partial`; its block-breaking test reference follows the renamed test |
