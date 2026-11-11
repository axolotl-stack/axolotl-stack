# Design: skins, abilities and client packets as BDS relays them

## Measurements

All on BDS 1.26.51 slot 3 (loopback, plain `bedrock_server.exe`, fresh seed-12345 world, operators),
clients `bds-extractor` with scripted actions; run table in
`crates/bds-extractor/captures/1.26.51/protocol/README.md`:

- `relay` twice: a classic-skin host and a persona guest; console (second run) or host commands
  (first run) change permissions and game modes; the guest sends 40 scripted client packets; two
  more skins join afterwards (one refused);
- `abilities`: console `deop`/`op` in each game mode, joins after `/defaultgamemode`;
- `skins`: 61 logins, one per persona piece type, tint type and odd value; `numbers`: 27 JSON number
  forms in a geometry;
- `ddui`: 15 close reasons, one session each; `cache`: a client claiming a blob cache.

Findings that look odd are in `docs/quirks/bds.md` (skins, abilities, emotes, blob cache,
data-driven UI) and the harness lessons in
[scripted-client-packets-in-bds-captures.md](../../../../docs/solutions/tooling/scripted-client-packets-in-bds-captures.md).

## Decisions

### D1: absence is evidence when the timeline is committed

Rows for packets BDS answers with nothing are `verified` only with two trials and a committed
`timeline.json` per session. The test checks that only ambient traffic follows the client packet in
BDS's timeline, and that unastar's router sends nothing.

### D2: the geometry gets its own JSON writer

BDS keeps integers of any 64-bit size and writes `1e400` as `inf`; `serde_json` cannot parse that,
and its `arbitrary_precision` feature would change number handling across the workspace. `skin.rs`
has a small parser that keeps each number's text and a writer with the measured float style.

### D3: login refusals follow BDS's rule, not its answer

jolyne refuses the persona types BDS refuses, but answers with its usual `Disconnect` instead of
BDS's terminating `PacketViolationWarning`. Copying the answer would need a jolyne login-refusal
path for one rule; the gap is in the quirk row.

### D4: no abilities on a game mode change

The commands lane resent abilities after a game mode change, because the old unastar abilities did
not depend on the mode. BDS sends only `UpdatePlayerGameType`, so `set_game_mode` does the same; the
next `/op`, `/deop` or join carries the mode's abilities.
