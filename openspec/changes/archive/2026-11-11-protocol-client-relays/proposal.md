# Proposal: skins, abilities and client packets as BDS relays them

## Why

Round 1 (`protocol-join-roster`) built `PlayerList` entries from the login but only measured a plain
custom skin: persona pieces and tints were dropped, and geometry, capes and trust flags were guesses.
It left out the per-player `UpdateAbilities` that BDS sends after the full list. unastar's abilities
were Dragonfly's, not BDS's. A whole group of client packets had no owner and no measurement:
emotes, emote lists, the blob-cache status, the data-driven-UI close, party and pack packets,
cookies, interactions, actor picks and book edits.

Prior work: [multi-client-bds-captures.md](../../../../docs/solutions/tooling/multi-client-bds-captures.md)
(`capture.py --pair`), the roster fixtures and decision
[0044](../../../../docs/decisions/0044-send-pre-start-game-roster-packets-after-start-game.md). No spec
covers relays; the `server-authority` spec is untouched.

## What Changes

- Tooling: `bds-extractor` scripts take `!` client actions (`src/actions.rs`), `--client-data`
  (login ClientData fields, for skins) and `--blob-cache`; jolyne's `ClientConfig::blob_cache`;
  `capture.py --guest-commands/--guest-client-data/--client-data/--blob-cache`;
  `protocol_probe.py` (`relay`, `abilities`, `cache`, `skins`, `ddui`) and
  `commit_protocol_fixtures.py`, which writes fixtures and packet timelines to
  `crates/bds-extractor/captures/1.26.51/protocol/`.
- unastar: skins forwarded like BDS (geometry rewritten, persona pieces and tints mapped, colours,
  base64 fields, empty PlayFab id); BDS's abilities for every game mode and permission, in
  `UpdateAbilities` and `AddPlayer`; the join's per-player abilities and the newcomer's abilities for
  online players; no abilities on a game mode change; emote and emote-list relay; the blob-status
  answer; the data-driven-UI close-reason check; the ignored serverbound packets.
- jolyne: login validation refuses persona piece and tint types BDS refuses.

## Impact

### Parity rows

`protocol` unless noted:

| Row | From | To |
|---|---|---|
| `EmotePacket`, `EmoteListPacket`, `ServerboundDataDrivenScreenClosedPacket`, `PartyChangedPacket`, `ResourcePacksReadyForValidationPacket`, `ServerboundDataStorePacket`, `PartyDestinationCookieResponsePacket` | missing | verified |
| `UpdateAbilitiesPacket`, `UpdatePlayerGameTypePacket` (mainline) | done | verified |
| `ClientCacheBlobStatusPacket`, `ClientCacheMissResponsePacket` | missing | partial |
| `PlayerListPacket`, `AnimatePacket` (entities), `InteractPacket` (items), `AddPlayerPacket` (entities) | partial | partial (tests, evidence, gaps) |
| `ClientCacheStatusPacket`, `ActorPickRequestPacket`, `BookEditPacket` | missing | missing (measured, notes) |
| systems `interface.player_list` | missing | partial |

No claim in `docs/parity/claims.toml` covers these rows.

## Out of scope

- The blob cache itself (cached chunks), actor picks and book editing: measured or attempted,
  not implemented (parity notes).
- `PlayerLocation` and `LocatorBar` (seen at joins and for spectators), `AddPlayer` metadata and
  position (player-movement and entities lanes).
- BossEvent, AnimateEntity, CameraShake and CameraInstruction: no unastar command or actor sends
  them yet; server-originated `Animate` (critical hits belong to PvP, wake-up needs sleeping).
