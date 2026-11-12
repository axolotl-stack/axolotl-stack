# Proposal: the BDS join order before StartGame

## Why

BDS 1.26.51 opens the batch that carries `StartGame` with three game packets for the joining
player: the sleeping-player `LevelEventGeneric`, a `PlayerList` holding only that player, and the
world clock state (`SyncWorldClocks`) (member join fixtures `00011`-`00013`, every `roster/*` guest
join). unastar sent the first two right after `StartGame` and the clock state not at all, because
the connection task that runs the join through jolyne had no view of who is online.

Prior work: decision 0044 (proposed in `protocol-join-roster`) chose "right after StartGame"; the
owner chose to match BDS on 2026-11-11, so 0044 is rewritten as accepted. The quirk row
"Join / player list" and the parity rows of `PlayerList` and `LevelEventGeneric` listed the order
as a gap. `docs/solutions/tooling/multi-client-bds-captures.md` covers the roster captures; no
solution note covers the pre-`StartGame` packets.

## What Changes

- jolyne: `WorldJoinParams::before_start_game`, game packets the server queues first in the
  `StartGame` batch (empty by default; the proxy sends none).
- unastar: an `OnlineRoster` (session → player colour) shared by the connection tasks and the game
  loop. The connection task reserves the player's place before `StartGame` and sends the
  sleeping-player count, its own list entry and the clock state there; the game loop takes the
  reserved colour, counts from the roster and frees the place when the player leaves. The
  post-`StartGame` join no longer sends the count and the own entry.

Crates: `jolyne`, `unastar`.

## Impact

### Parity rows

`protocol`, owner `protocol` unless noted:

| Row | From | To |
|---|---|---|
| `PlayerListPacket`, `LevelEventGenericPacket` | partial | partial (join order gap closed; other gaps stay) |
| `StartGamePacket` | done | done (pre-`StartGame` packets in its batch) |
| `SyncWorldClocksPacket` (mainline) | verified | verified (pre-`StartGame` state added) |

No claim in `docs/parity/claims.toml` covers these rows.

### Specs

`session-handshake`: a new requirement for the application's pre-`StartGame` packets.

## Out of scope

- The rest of BDS's order around `StartGame` (`SyncActorProperty` before `ItemRegistry`, the
  definition lists interleaved with the post-`StartGame` join): jolyne still sends `ItemRegistry`
  and the definition lists right after `StartGame`.
- The order of a full `PlayerList` (BDS's is not stable).
