# BDS 1.26.51 protocol probe fixtures

What BDS 1.26.51 relays between players and how it answers client packets
no command produces. Measured on 2026-11-11 on loopback, plain
`bedrock_server.exe` (no Endstone), fresh world (seed 12345, survival,
easy), every player an operator, all clients `bds-extractor`. Only the
`book` sessions (2026-11-12) send `PlayerAuthInput` (`--input`), and so do
the `pick` and `pick-actors` sessions; the others send none.

Made with `tools/bds-sweep/protocol_probe.py` (scripts, ClientData files and
console commands are copied into each run) and committed with
`tools/bds-sweep/commit_protocol_fixtures.py`. Per session:
`<seq>-<Packet>.bin` (server packets: varuint packet ID header then payload,
like `../member/join/`), `sent/<seq>-<Packet>.bin` (the client's scripted
packets), `timeline.json` (`[seq, t_ms, dir, packet]` of every packet after
the join, to show what BDS did not send) and `session.json`
(`started_unix` lines sessions up with `console.json`).

| Run | Probe | Sessions | Shows |
|---|---|---|---|
| `relay` | `relay` | `host` (classic skin with cape, geometry, animated face), `guest` (persona wearing every piece and tint type), `floats` (float-heavy geometry, base64 animation data, ARGB colour); `odd-client-data.json` is the refused login | console `deop`/`op`/`gamemode` of both players as each sees them; the guest's interactions, actor picks, emotes, emote list, animations, book edits and the serverbound packets of `GUEST_RELAY`; skins in `PlayerList`; join announcements with the newcomer's abilities |
| `relay-first` | `relay` (earlier script, host changes the guest by command) | `host`, `guest` | the same client packets, second trial; a creative actor pick of a cow (`GuiDataPickItem`, inventory, `PlayerHotbar`) |
| `relay-refused` | `relay` | `guest` | the terminating violation for the close reason `closed` |
| `abilities` | `abilities` | `host`, `guest`, `creative`, `adventure`, `spectator` | `UpdateAbilities` for `deop`/`op` in every game mode, as target and as other player; joins after `/defaultgamemode` |
| `cache` | `cache` | `host` | a client that claims a blob cache: cached `LevelChunk`s, the `ClientCacheMissResponse` |
| `numbers` | `skins numbers` | `numbers` | how BDS writes 27 JSON number forms of a skin geometry |
| `skins` | `skins` | `skins.json` only | which of 61 logins BDS refused (piece and tint types, colours, arm size), with the violation |
| `book` | `book` | `book`, `creative`, `unopened`, `limits`, `gate` (one session each, with input) | editing and signing a book and quill after opening it (item use on air): BDS's `InventoryTransaction` answers, padding, the 50-page cap, no text or title limit, which book stays open (`gate`), nothing without opening it (`unopened`); `<session>-commands.txt` beside them |
| `pick` | `pick` | `pick` (creative, then survival) | actor picks of 24 actor kinds without and with data, with a full hotbar and with the egg held; block picks without and with data of blocks beside the player (chest with items, log, wool, furnace, stairs, bed, flower pot, sign, wheat, one out of reach); `GuiDataPickItem`, `PlayerHotbar` and the requests (`sent/`) |
| `pick-actors` | `pick-actors` | `pick-actors` (creative) | a pick without and with data of every summonable actor: the requests and the `GuiDataPickItem` answers (no `timeline.json`: 0.9 MB of actor movement); `crates/unastar-data/data/measured/actor_pick_items.json` is built from this run with `tools/bds-sweep/pick_table.py` |
| `pick-recheck`, `pick-recheck2` | `pick-actors ACTORS` | `pick-actors` | the actors the sweep missed, picked again (`pick-recheck`: 1.5 s after the summon, 3 blocks away; `pick-recheck2`: at once, 1 block away): moving mobs are only missed when they leave the reach |
| `pick-reach` | `pick-reach` | `pick-reach` | creative picks of a cow 1 to 24 blocks away: answered up to 14 blocks, not from 16 |
| `ddui` | `ddui` | `ddui.json` only | which of 15 screen close reasons BDS refused, with the violation |

The tests in `crates/unastar/src/server/game/client_packets/tests.rs`,
`crates/unastar/src/server/game/books/tests.rs`,
`crates/unastar/src/server/game/inventory/actor_pick/tests.rs` and
`crates/unastar/src/server/game/roster/probe_tests.rs` rebuild these packets
and compare bytes, and read the timelines for packets BDS answers with
nothing.
