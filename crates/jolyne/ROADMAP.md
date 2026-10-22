# Jolyne Vanilla Compatibility Roadmap

This roadmap is scoped to the protocol/session layer. Gameplay, world state, entities, ticking, and persistence remain `unastar` responsibilities.

## Compatibility Target

Jolyne is considered compatibility-ready for the pinned Bedrock version only when a harness client can complete the documented `docs/LOGIN_SEQUENCE.md` flow and when each transition has deterministic validation, timeouts, and disconnect behavior.

## Current Foundation

- Typestate protocol flow across handshake/login/resource-pack/start-game/play states.
- RakNet and NetherNet transport abstraction.
- Compression, encryption, and packet batching primitives.
- Online/offline auth paths.
- `WorldTemplate` injection point used by Unastar for real registries.

## Blocking Gates

### JOLYNE-M1: Auth Trust Hardening (done)
- [x] Validate online-mode tokens only against the discovered Minecraft OpenID JWKS; header-selected `x5u`/`x5c` key material is never trusted, and Mojang-rooted legacy chains are rejected online (BDS 1.26.51 no longer carries the Mojang root key). Tests: `auth::security_tests::{self_signed_header_key_is_not_online_trust, trusted_signatures_do_not_override_claim_validation, legacy_self_signed_chain_is_offline_only}`.
- [x] Negative tests for forged key headers, malformed envelopes/chains/JWTs and invalid client data. Tests: `auth::security_tests::{malformed_envelopes_never_panic, malformed_tokens_and_client_data_never_panic, signed_but_invalid_client_data_is_rejected}`, `auth::client_data::tests::*`.
- [x] HTTP timeout/refresh policy for auth metadata and key fetches: lock-free cached key set, single-flight refresh, rate-limited unknown-`kid` refresh, stale-key fallback, failure backoff and per-fetch timeout (`auth::openid::RefreshPolicy`). Tests: `auth::provider_tests::*`.
- [x] Typed, validated `ClientData` and PlayFab/title IDs returned with the identity; offline identities cannot expose a trusted XUID.
- [x] The Xbox client path sends the modern `AuthenticationType: 0` multiplayer token.

### JOLYNE-M2: Resource-Pack State Machine
- [ ] Do not advance to start-game until the correct `ResourcePackClientResponse::Completed` state is observed.
- [ ] Support non-empty pack transfer: `ResourcePackDataInfo`, `ResourcePackChunkRequest`, and `ResourcePackChunkData`.
- [ ] Handle `ClientCacheStatus` as an explicit negotiated capability; safely ignore when blob cache is disabled.
- [ ] Add timeout and disconnect reasons for stalled pack negotiation.

### JOLYNE-M3: Start-Game Ordering Test
- [ ] Add an integration test that asserts packet order from `docs/LOGIN_SEQUENCE.md`.
- [ ] Cover `StartGame`, `ItemRegistry`, `BiomeDefinitionList`, `AvailableEntityIdentifiers`, `CreativeContent`, `PlayStatus(Spawned)`, and `SetLocalPlayerAsInitialized`.
- [ ] Run the test with Unastar-provided registry data, not the empty default template.

### JOLYNE-M4: Example And CI Health
- [ ] Fix stale client examples so `cargo test -p jolyne --all-features` succeeds.
- [ ] Add CI coverage for server and client feature combinations.
- [ ] Document which protocol version is currently pinned and how compatibility is revalidated when that version changes.

## Documentation Rules

- `DefaultWorldTemplate` is a minimal test scaffold, not a vanilla-compatible registry source.
- Public compatibility claims must name the pinned protocol version and the smoke gates that passed.
- Keep protocol sequencing docs in `docs/LOGIN_SEQUENCE.md`; keep implementation milestones here.
