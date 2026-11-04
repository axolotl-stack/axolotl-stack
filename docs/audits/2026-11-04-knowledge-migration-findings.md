# Findings from the knowledge-system migration (2026-11-04)

While `thoughts/` was triaged and the first OpenSpec specs were written from the code, reviewers
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
