# Jolyne Vanilla Compatibility Roadmap

This roadmap is scoped to the protocol/session layer. Gameplay, world state, entities, ticking, and persistence remain `unastar` responsibilities.

## Compatibility Target

Jolyne is considered compatibility-ready for the pinned Bedrock version only when a harness client can complete the documented `docs/LOGIN_SEQUENCE.md` flow and when each transition has deterministic validation, timeouts, and disconnect behavior.

## Current Foundation

- Sans-IO protocol core (`src/proto`) with consuming phase typestates and mandatory deadlines; async driver in `src/stream`.
- RakNet and NetherNet transport abstraction.
- Compression, encryption, and packet batching primitives.
- Online/offline auth paths.
- `WorldTemplate` injection point used by Unastar for real registries.

## Blocking Gates

### JOLYNE-M1: Auth Trust Hardening
- [ ] Validate online-mode JWT chains against trusted Microsoft/Xbox Live roots instead of trusting header-selected `x5u`/`x5c` key material.
- [ ] Add negative tests for forged key headers and malformed chains.
- [ ] Add explicit HTTP timeout/retry policy for auth metadata and key fetches.

### JOLYNE-M2: Resource-Pack State Machine (done)
- [x] Do not advance to start-game until `ResourcePackClientResponse::ResourcePackStackFinished` is observed; the stack is only sent after the client's first response (`proto::tests::join_sequence_order_matches_vanilla`, `proto::packs::tests::transfers_verifies_and_orders_stack_after_downloads`).
- [x] Non-empty pack transfer: `ResourcePackDataInfo`, ordered `ResourcePackChunkRequest`, paced `ResourcePackChunkData`, SHA-256, CDN URL and content keys on the server; bounded windowed download, hash and stack verification on the client (`proto::packs::tests::*`, `stream::tests::resource_packs_download_over_the_async_layer`).
- [x] `ClientCacheStatus` is deferred during negotiation and delivered to the play session in order (`proto::tests::packets_before_their_phase_are_replayed_into_play_in_order`); the client advertises no blob cache.
- [x] Pack negotiation has its own deadline (`resource_pack_timeout`, default 5 min) and failures map to `Resourcepackproblem` / `Timeout` disconnects.

### JOLYNE-M3: Start-Game Ordering Test (done; registry-data run re-scoped)
- [x] `proto::tests::join_sequence_order_matches_vanilla` asserts the server packet order through `PlayStatus(PlayerSpawn)` (BDS order: definitions follow `ItemRegistry`; `ChunkRadiusUpdated`, `NetworkChunkPublisherUpdate`, `PlayerSpawn` answer `RequestChunkRadius`).
- [x] `SetLocalPlayerAsInitialized` must carry the `StartGame` runtime ID (`proto::tests::runtime_id_mismatch_is_rejected`).
- [x] Re-scoped: the ordering test uses `WorldTemplate::default()`; real-registry coverage is the Unastar 5-bot smoke (`stress_test --clients 5`), not a jolyne unit test, because the registry data lives in `unastar-data`.

### JOLYNE-M4: Example And CI Health (done)
- [x] Examples compile against the current API; `cargo test -p jolyne --all-features` runs in CI.
- [x] CI checks jolyne feature combinations (`client`/`server` × `raknet`/`nethernet`/`discovery`) with clippy.
- [x] The pinned version (Bedrock 1.26.51, protocol 2193) is documented in `README.md`; the protocol check is exact in `proto::server::ServerHandshake` and is the single place an accepted-versions list would go.

## Documentation Rules

- `DefaultWorldTemplate` is a minimal test scaffold, not a vanilla-compatible registry source.
- Public compatibility claims must name the pinned protocol version and the smoke gates that passed.
- Keep protocol sequencing docs in `docs/LOGIN_SEQUENCE.md`; keep implementation milestones here.
