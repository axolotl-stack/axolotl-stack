# Proposal: the join packets and player roster BDS sends

## Why

The BDS 1.26.51 join (`crates/jolyne/src/proto/testdata/bds-1.26.51-join-s2c.txt`, the committed
member join fixtures in `crates/bds-extractor/captures/1.26.51/member/join/`) carries packets
unastar never sent: `PlayerList`, `LevelEventGeneric` (sleeping players), `UpdateAdventureSettings`,
`SetSpawnPosition`, `PlayerFog`, `CameraPresets`, `CameraAimAssistPresets`, `CameraSpline`,
`TrimData` and `SetHealth`. Other players never got a `PlayerList` entry, so they appeared without
skins. unastar also sent a `SetPlayerGameType` that BDS does not send.

Prior work: the parity rows of those packets name the join trace; `data-driven-gameplay` task D.3
asks for `SetHealth` and a `SetCommandsEnabled` test; `crates/unastar/data/bds-1.26.51/README.md`
vendors the pre-`StartGame` payloads the same way. Nothing in `docs/solutions` or `docs/decisions`
covers the roster. The multiplayer behaviour (what online players get when someone joins or leaves,
player colours) was not measured anywhere, so this change measures it first with a new two-client
capture mode.

## What Changes

- `tools/bds-sweep/capture.py --pair`: a host session running a command script while guests join and
  leave (one after the other or together); `roster_probe.py` for colour reuse;
  `commit_roster_fixtures.py` commits the roster packets to
  `crates/bds-extractor/captures/1.26.51/roster/`.
- unastar sends the missing join packets in BDS order, byte-identical to the captures; builds each
  player's `PlayerList` entry (skin, platform, colour) from its validated login; tells online
  players about joins and leaves the way BDS does; sends `SetHealth` before `PlayStatus(PlayerSpawn)`;
  puts the join game mode in `StartGame` and drops the join `SetPlayerGameType`.

Crates: `unastar`, `tools/bds-sweep` (Python), fixtures in `bds-extractor`.

## Impact

### Parity rows

`protocol`, owner `protocol` unless noted:

| Row | From | To |
|---|---|---|
| `SetCommandsEnabledPacket`, `UpdateAdventureSettingsPacket`, `CameraPresetsPacket`, `CameraAimAssistPresetsPacket`, `CameraSplinePacket`, `TrimDataPacket`, `SetHealthPacket` | missing | verified |
| `PlayerListPacket`, `LevelEventGenericPacket`, `PlayerFogPacket` | missing | partial |
| `SetSpawnPositionPacket` (commands) | partial | partial (join sent and verified; beds remain) |
| `SetPlayerGameTypePacket` | partial | partial (never sent, as BDS; client form unmeasured) |

No claim in `docs/parity/claims.toml` covers these rows.

## Out of scope

- `AddPlayer`, `RemoveActor` for leaving players, join/leave chat messages, `PlayerLocation` and
  `LocatorBar` (player-movement and other lanes); the per-player `UpdateAbilities` BDS sends after
  the full `PlayerList`.
- `/fog`, `/camera`, `/music` and the weather events themselves (commands lane); the thunder join
  is logged as a quirk for that lane.
- Persona skins (pieces, tints), which need a persona login to measure.
