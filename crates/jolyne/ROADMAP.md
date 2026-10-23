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

### JOLYNE-M1: Auth Trust Hardening (done)
- [x] Validate online-mode tokens only against the discovered Minecraft OpenID JWKS; header-selected `x5u`/`x5c` key material is never trusted, and Mojang-rooted legacy chains are rejected online (BDS 1.26.51 no longer carries the Mojang root key). Tests: `auth::security_tests::{self_signed_header_key_is_not_online_trust, trusted_signatures_do_not_override_claim_validation, legacy_self_signed_chain_is_offline_only}`.
- [x] Negative tests for forged key headers, malformed envelopes/chains/JWTs and invalid client data. Tests: `auth::security_tests::{malformed_envelopes_never_panic, malformed_tokens_and_client_data_never_panic, signed_but_invalid_client_data_is_rejected}`, `auth::client_data::tests::*`.
- [x] HTTP timeout/refresh policy for auth metadata and key fetches: lock-free cached key set, single-flight refresh in a spawned task (a cancelled login keeps its outcome and backoff), rate-limited unknown-`kid` refresh, stale-key fallback, failure backoff and per-fetch timeout (`auth::openid::RefreshPolicy`). Tests: `auth::provider_tests::*` (incl. `cancelled_login_does_not_abort_or_forget_the_fetch`).
- [x] Replay resistance: a verified token is `Trust::AwaitingKeyProof` until the encryption handshake (or a transport-proven key) shows the client holds it; RakNet refuses online mode without encryption. Tests: `proto::tests::{online_identity_is_authenticated_only_after_the_encryption_handshake, replayed_online_login_never_authenticates_and_times_out_at_the_login_deadline, online_mode_without_encryption_is_refused_on_raknet, unencrypted_online_login_trusts_only_a_transport_proven_key}`, `tests/raknet_listener.rs::online_raknet_listener_requires_encryption`.
- [x] Typed, validated `ClientData` and PlayFab/title IDs returned with the identity; offline identities cannot expose a trusted XUID.
- [x] The Xbox client path sends the modern `AuthenticationType: 0` multiplayer token.

### JOLYNE-M2: Resource-Pack State Machine (done)
- [x] Do not advance to start-game until `ResourcePackClientResponse::ResourcePackStackFinished` is observed; the stack is only sent after the client's first response (`proto::tests::join_sequence_follows_the_bds_capture`, `proto::packs::tests::transfers_verifies_and_orders_stack_after_downloads`).
- [x] Non-empty pack transfer: `ResourcePackDataInfo`, ordered `ResourcePackChunkRequest`, paced `ResourcePackChunkData`, SHA-256, CDN URL and content keys on the server; bounded windowed download, hash and stack verification on the client (`proto::packs::tests::*`, `stream::tests::resource_packs_download_over_the_async_layer`).
- [x] `ClientCacheStatus` is deferred during negotiation and delivered to the play session in order (`proto::tests::packets_before_their_phase_are_replayed_into_play_in_order`); the client advertises no blob cache.
- [x] Pack negotiation has its own deadline (`resource_pack_timeout`, default 5 min) and failures map to `Resourcepackproblem` / `Timeout` disconnects.

### JOLYNE-M3: Start-Game Ordering Test (done; registry-data run re-scoped)
- [x] `proto::tests::join_sequence_follows_the_bds_capture` checks the server packets through `PlayStatus(PlayerSpawn)` against a recorded BDS 1.26.51 trace (`src/proto/testdata/bds-1.26.51-join-s2c.txt`): an order-preserving subsequence with the StartGame group in one BDS batch (`DimensionData?`, `JigsawStructureData`, `VoxelShapes` precede `StartGame`, which the real client requires; definitions follow `ItemRegistry`; `ChunkRadiusUpdated`, `NetworkChunkPublisherUpdate`, `PlayerSpawn` answer `RequestChunkRadius`). Unastar sends BDS 1.26.51's captured structure payloads (`crates/unastar/data/bds-1.26.51`).
- [x] `SetLocalPlayerAsInitialized` must carry the `StartGame` runtime ID (`proto::tests::runtime_id_mismatch_is_rejected`).
- [x] Re-scoped: the ordering test uses `WorldTemplate::default()`; real-registry coverage is the Unastar 5-bot smoke (`stress_test --clients 5`), not a jolyne unit test, because the registry data lives in `unastar-data`.

### JOLYNE-M4: Example And CI Health (done)
- [x] Examples compile against the current API; `cargo test -p jolyne --all-features` runs in CI.
- [x] CI checks jolyne feature combinations (`client,raknet`, `client,nethernet`, `server,raknet`, `server,nethernet`, `server,discovery`, all features) with clippy.
- [x] The pinned version (Bedrock 1.26.51, protocol 2193) is documented in `README.md`; the protocol check is exact in `proto::server::ServerHandshake` and is the single place an accepted-versions list would go.

## Documentation Rules

- `WorldTemplate::default()` is a minimal test scaffold, not a vanilla-compatible registry source.
- Public compatibility claims must name the pinned protocol version and the smoke gates that passed.
- Keep protocol sequencing docs in `docs/LOGIN_SEQUENCE.md`; keep implementation milestones here.
