# Findings from the knowledge-system migration (2026-11-04)

While the old notes folder (removed) was triaged and the first OpenSpec specs were written from the code, reviewers
read the code against the docs and found the problems below. None were fixed in that migration,
which only changed documentation. Each item is open for anyone to pick up: fix it, then delete its
row here (or point it at the OpenSpec change that fixes it).

Specs describe the code as it behaves today, so where code and docs disagreed the specs follow the
code and the gap is listed here.

## Bugs

| Area | Finding | Where | Tracked by |
|---|---|---|---|
| Plugins | A fired timer is delivered to every plugin subscribed to `timer`, not only the plugin that set it; the stored owner is never read | `crates/unastar/src/server/timer.rs`, `crates/unastar/src/plugin/manager.rs` | change `plugin-async-tasks` (task 1.1) |
| Plugins | `on-player-join` never fires: nothing emits a `PlayerJoin` event | `crates/unastar/src/ecs/events.rs`, `plugin/manager.rs` | |
| Plugins | `on-unload` is never called; there is no unload path | `plugin/manager.rs` | |
| Plugins | A player handle the host does not know panics on a map lookup instead of returning an error | `plugin/manager.rs` (host player resource) | |
| Plugins | `api_version` in `plugin.toml` is parsed but never checked | `crates/unastar/src/plugin/manifest.rs` | change `plugin-async-tasks` (task 3.1) |
| Plugins | The manifest accepts unknown keys (no `deny_unknown_fields`), unlike the server config | `plugin/manifest.rs` | |
| Plugins | `get-tps` always returns 20.0 and `get-game-mode` always returns survival (TODOs) | `plugin/manager.rs` | |
| Plugins | Only the `filesystem` capability does anything; `network`, `write-player-data` and `process` are parsed and ignored | `plugin/manager.rs` | change `plugin-async-tasks` (network) |
| Storage | Subchunk palettes are saved as runtime palette positions | `crates/unastar/src/world/chunk.rs` | change `persist-block-states-by-name`, decision 0033 |
| Generator | Writing the version crate's `Cargo.toml` is not atomic (plain `File::create`), unlike the Rust modules | `crates/valentine_gen/src/main.rs` (`write_version_crate`, `update_valentine_manifest_at`) | |
| Generator | A type id seen again with a different shape is silently inlined instead of failing generation | `crates/valentine_gen/src/parser/protocolgen.rs` | (test `preserves_distinct_named_types_with_identical_wire_shapes` relies on it; decide before changing) |

## Docs that disagree with the code

| Doc | Says | Code does |
|---|---|---|
| `crates/tokio-raknet/README.md` | `send_encoded` defaults to ReliableOrdered | a `Bytes` argument converts to UnreliableSequenced, so the README echo server echoes unreliably |
| `crates/tokio-raknet/README.md` (security table) | MTU discovery sends four probes per size | that is the Optimized policy; Vanilla sends 13 probes (4/4/5) |
| `crates/valentine/README.md` | example `--versions …,1.26.30` cross-version dedup | the default protocolgen source rejects a `--versions` list without the manifest's version; it needs `--source prismarine` |
| `crates/example-plugin` | implements `on_player_join` | does not subscribe to `player_join` (and the event never fires; see above) |
| `crates/unastar/src/entity/systems/lifecycle.rs`, `server/game/items.rs` | item pickup reach and merge radius "to be confirmed" | confirmed by capture (entities research, deviation X6) |
| `mutants.toml` | excludes `crates/valentine_bedrock_1_21_130/**` | that crate no longer exists; it also excludes all of `crates/unastar-data/**`, which now holds hand-written loaders |

## Gaps in test coverage

- `encoded_size` equal to the encoded length is only tested by `start_game_roundtrip` and
  `union_conformance`, not for every packet.
- The Optimized RakNet graceful close (drains reliable sends for at most 5 s) has no test.
- Unused RakNet constants: `MAXIMUM_CONNECTION_ATTEMPTS`, `TIME_BETWEEN_SEND_CONNECTION_ATTEMPTS`,
  `DEFAULT_PACKET_LIMIT` in `crates/tokio-raknet/src/protocol/constants.rs`.
- The signed `VarInt`/`VarLong` decoders accept overlong encodings while the unsigned ones reject
  them; borrowed MCPE decoding returns the leftover byte count instead of rejecting trailing bytes.
  Both are described as-is in `openspec/specs/protocol-codec`; decide whether they are intended.
- No `ActorKindRegistry` exists yet although decision 0025 calls for one.

## Second round: findings from writing the remaining specs

Found while writing `session-handshake`, `xbl-auth`, `nethernet`, `friend-gateway`,
`world-storage`, `server-authority`, `server-config`, `pack-loader`, `rules-engine`, `game-data`,
`worldgen-pipeline` and `parity-catalog`. Same rule: fix, then delete the row.

### Rule violations

| Finding | Where |
|---|---|
| Doc comments name what look like BDS internal identifiers (`Enchant::Type`, `selectEnchantments`), against decision 0016 | `crates/unastar-rules/src/enchanting.rs` |

### Bugs: server

Paths are relative to `crates/unastar/src/` unless shown in full.

| Area | Finding | Where |
|---|---|---|
| Worldgen | The generation pool ignores `world.bounds`; only the synchronous path checks them, so `java` and `bedrock` worlds are unbounded whatever the config says | `crates/unastar/src/world/ecs/generation_worker.rs`, `world/ecs/manager.rs` |
| Worldgen | Java-style output is not a pure function of (seed, chunk): it decorates in request order through one shared native session, and the pool hands it chunks in nondeterministic order. `worldgen_output_hash` runs single-threaded and can't catch it. **Decided 2026-11-05:** accepted trade-off, see decision 0031 | `world/generator/pipeline.rs`, `pipeline/native.rs` |
| Worldgen | The Java native session's chunk map is never evicted (grows for the server's lifetime) and one mutex serialises all Java decoration. **Proposed fix:** decision 0034 | `world/generator/pipeline/native.rs` |
| Movement | A NaN or infinite `PlayerAuthInput` position passes the 64-block delta check and is stored; rotation is never checked | `server/game/movement.rs` |
| Player storage | A corrupt player record is kept only until that player's next save, which overwrites it with the first-join state | `server/connect.rs`, `server/game/mod.rs` |
| Storage | A LevelDB subchunk that fails to decode loads as air; editing the chunk then saves air over the original (BlazeDB fails the load instead) | `storage/leveldb_world.rs` |
| Storage | Shutdown never flushes or closes the world provider; BlazeDB's background queue can lose the final saves | `server/runtime.rs`, `storage/blazedb.rs` |
| Storage | A chunk whose pre-unload save fails is still despawned, losing its edits | `world/ecs/systems.rs` (`process_chunk_unloads`) |
| Storage | The synchronous chunk path panics (`expect`) on a storage read error, taking down the tick thread | `world/ecs/manager.rs` (`load_or_generate_chunk`) |
| Storage | If the world database can't be opened (e.g. locked by a second instance) the server runs with persistence off, logging only a warning | `server/runtime.rs` (`UnastarServer::new`) |
| Storage | LevelDB load and save hard-code subchunk Y -4..19 for every dimension | `storage/leveldb_world.rs` |
| Network | The inbound channel is shared, so when full the next sender is disconnected, not necessarily the flooder | `network/mod.rs` |
| Network | Packets over the 64-per-tick budget are dropped, not deferred, so a legitimate burst loses inputs | `server/runtime.rs` |
| Config | `server.default_gamemode` is a free string; a typo silently means survival | `config/mod.rs` |
| Config | World bounds aren't range-checked; `min_x > max_x` makes every chunk out of bounds | `config/mod.rs` (`validate`) |
| Logging | Every `InventoryTransaction` is logged at `info!` with its full Debug form, so clients control log volume | `server/game/inventory/mod.rs` |

### Bugs: protocol, auth and transports

| Area | Finding | Where |
|---|---|---|
| jolyne | With a plain spawn, `SetLocalPlayerAsInitialized` before any `RequestChunkRadius` completes the spawn with radius `None`; `ChunkRadiusUpdated` and `PlayStatus(PlayerSpawn)` are never sent | `crates/jolyne/src/proto/server.rs` |
| jolyne | `CompressionConfig::level` 0 sends every batch uncompressed while `NetworkSettings` still advertises Zlib; the doc says 1..=9 clamped | `crates/jolyne/src/config.rs` |
| axolotl-xbl | `MultiplayerTokenVerifier` is unused; jolyne has its own OpenID/JWKS verifier with separate refresh logic | `crates/axolotl-xbl/src/multiplayer.rs`, `crates/jolyne/src/auth/openid.rs` |
| axolotl-xbl | `DeviceCodeAuth::new` and `XblTokenClient::new` call `.expect` in library code | `crates/axolotl-xbl/src/auth/` |
| nethernet | One slow offer (credentials, token check, candidate gathering) blocks the listener actor for every peer, and its timeout sends no `CONNECTERROR` | `crates/tokio-nethernet/src/listener.rs` |
| nethernet | On non-Windows platforms any discovery receive error ends the listen loop permanently | `crates/tokio-nethernet/src/discovery/mod.rs` |
| nethernet | `set_pong_data` silently drops the update when its `try_write` is contended; `endpoint::Client::set_pong_data` panics | `discovery/mod.rs`, `endpoint.rs` |
| nethernet | `max_pending_connections` (64) also counts accepted connections, so it caps concurrent players per listener | `listener.rs` |
| axelerator | When the franchise WebSocket closes, `ConnectionClosed` is classified as transient and the transfer loop spins forever without reconnecting | `crates/axelerator/src/transfer.rs` |
| axelerator | After a failed token refresh the loop polls a finished `JoinHandle` again and panics | `crates/axelerator/src/session.rs` |
| axelerator | Fatal errors return `Ok(())`, and the systemd unit's `RestartPreventExitStatus=0` then never restarts it | `session.rs`, `deploy/axelerator.service` |
| axelerator | Ctrl+C/SIGTERM drops `run()` without the presence, RTA and Discord shutdown steps | `crates/axelerator/src/main.rs` |
| axelerator | Two in-memory copies of `player_history.json` diverge, so active players can be expired; the save isn't atomic and a corrupt file is silently replaced | `session.rs`, `player_history.rs` |
| axelerator | `retry_count` never resets after a successful bind; `ListenerClosed` / `FatalError` recovery branches are unreachable | `transfer.rs`, `session.rs` |
| axelerator | Discord rate limiting is set only after a successful post and shared by every message kind | `crates/axelerator/src/discord.rs` |
| axelerator | `auto_accept_friends`, `presence_heartbeat` and `logging.json_format` are never read (requests are always accepted); a malformed config only warns and runs on defaults; unknown keys are accepted | `crates/axelerator/src/config.rs` |

### Bugs: parity tool

| Finding | Where |
|---|---|
| `verified` evidence only has to exist on disk; an untracked or gitignored file passes | `crates/parity/src/check.rs` |
| Test references match any `fn name` text, including comments, not only `#[test]` functions | `crates/parity/src/check.rs` (`defines_fn`) |

### Docs that disagree with the code

| Doc | Says | Code does |
|---|---|---|
| `docs/audits/2026-09-maintenance-audit.md` (AUTH-01) | jolyne trusts Mojang-rooted legacy chains | legacy chains are accepted only in offline mode |
| `docs/audits/2026-09-maintenance-audit.md` (JOLYNE-AUTH) | two open items (unencrypted online replay, JWKS fetch cancellation) | both handled and tested |
| `docs/audits/2026-09-maintenance-audit.md` (NETHERNET-02) | callbacks honoured | the current API has no callbacks |
| `crates/unastar/README.md` (Configuration) | root sample mirrors the crate template | they differ (operators, block network ids, difficulty, comments) |
| `crates/unastar/README.md` (Persistence) | corrupt player records are left in storage; read errors are logged | records are overwritten on next save; the sync path panics |
| `crates/unastar/src/storage/blazedb.rs` | default cache 4096 chunks | `BlazeConfig::default` is 512 |
| `crates/unastar_noise/README.md` | `NoiseRegistry::with_router_settings` is in `unastar_noise` | it is in `crates/unastar/src/world/generator/density/types.rs` |
| `crates/unastar-packs/src/lib.rs`, `src/actor.rs` | a startup loader for addon packs | nothing at runtime loads packs (see change `data-driven-gameplay` group C) |
| `crates/unastar-rules/src/filter.rs` | compact subjects 0 and 1 only | 0–9 accepted |
| `crates/unastar-rules/README.md` (Loot) | tiered and nested pools fail when rolled | both implemented and tested |
| `docs/quirks/bds.md` (unknown event actions) | 58-case fidelity fixture | 60 probes |
| `crates/unastar-data/README.md` | run datagen twice to check determinism | no automated check |
| `crates/axelerator/deploy/DEPLOYMENT.md` | OAuth tokens last about 90 days | README: no lifetime is assumed |
| `crates/tokio-nethernet/src/xbox_signaling.rs` | deprecated "since 0.2.0" | crate is 0.1.0 |

### Gaps in test coverage and CI

- CI now runs `cargo test` for `parity`, `unastar-rules`, `unastar-packs` and `unastar-data` (added
  with these specs). Still not run in CI: `unastar_noise`, `unastar_bedrock_codegen`,
  `axelerator`, and `tokio-nethernet` beyond default features (discovery, endpoint and
  xbox-signaling tests). Feature-gated worldgen gates and the `worldgen_bench --ratchet`
  allocation ceilings need local installs.
- `#[cfg(test)]` tests in `crates/unastar_noise/codegen/parser/surface_rule.rs` are compiled only
  into `build.rs` and never run.
- No order-independence test for Java-style generation.
- `crates/parity`: no unit tests for `validate_entry`, `forbidden_provenance`, `TestFiles::resolve`,
  `status::render`, snapshot read/write.
- `crates/unastar-data/tests/pack_data.rs` checks 4 of 6 manifest families; spawn rules, item/recipe
  bundles and `actor_components.rs` have no manifest entry; nothing verifies
  `data/measured/manifest.json` hashes.
- Untested: jolyne out-of-order packets in network-settings/login, and client-side
  `UnsupportedNetworkCompression`, `InvalidHandshake`, `ConflictingStartGame`,
  `InvalidChunkRadius`; xbl refresh-token retention and `slow_down` back-off; the 64-per-tick packet
  cap, channel sizes, movement delta, `MAX_BLOCK_ACTIONS`, chunk-radius clamp, `WorldBounds`; spawn
  rule ordering; save-before-unload failure and shutdown order; pack-loader duplicate/override
  paths; Molang loop limit; loot `UnknownTable` / `TooDeep`; duplicate registry names; nethernet
  connection-limit and SDP-size rejections; most axelerator behaviour.
- Unused: `KEY_ENTITIES_OLD`, `PlayerProvider::delete`, `LevelDBWorldProvider::dimension`
  (`crates/unastar/src/storage/`); `is_shutdown_requested`, `run_transfer_server_legacy`,
  `TransferStats.is_connected` (`crates/axelerator/src/transfer.rs`).
