# BDS 1.26.51 roster fixtures

What BDS 1.26.51 sends when players join and leave while others are online.
Measured on 2026-11-11 on loopback, plain `bedrock_server.exe` (no Endstone),
fresh world (seed 12345, survival, easy), every player an operator, all
clients `bds-extractor` with jolyne's default client data (an Android login
with a plain 64x64 custom skin).

Each run directory holds, per session, the server-to-client `PlayerList`,
`LevelEventGeneric`, `UpdateAdventureSettings`, `SetSpawnPosition`,
`SetHealth` and weather `LevelEvent` packets as `<seq>-<Packet>.bin` (varuint
packet ID header then payload, like `../member/join/`), the session's
`session.json`, and an `index.json` with every file's sequence number and time.
Written by `tools/bds-sweep/commit_roster_fixtures.py`.

| Run | Made with | Sessions kept | Shows |
|---|---|---|---|
| `thunder-join-leave` | `capture.py --pair` (host runs `commands.txt`: `/weather thunder`, `/list` fillers, `/gamerule pvp false`, `/gamerule showtags false`, `/weather clear`), one guest | `host`, `guest` | a second player's join and leave as both sides see them; a join during thunder; the game rules do not resend adventure settings |
| `rain-join` | `capture.py --pair --guests 4` (host sets rain) | `guest` | a join during rain |
| `twenty-one-players` | `capture.py --pair --guests 20 --guest-gap 7` | `host` | the colours of 21 players online at once; the last joiner after three players left |
| `colour-reuse` | `roster_probe.py` | `B` | a joiner takes the colour a leaving player freed |

In `twenty-one-players` five guests ran out of memory near the end of the run
(about 21 debug `bds-extractor` processes on one machine) and left within one
second; three of them had left when the last guest joined (the host sees
their removals first).

The tests in `crates/unastar/src/server/game/roster/tests.rs` rebuild these
packets and compare bytes, and replay the colour assignment.
