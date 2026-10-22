# Repository maintenance audit

**Reviewed:** 2026-09-25 (findings) · **Remediation pass:** 2026-10-21  
**Base:** Review began at `main` commit `c454051`. The 2026-10-21 remediation pass starts at `b295743` and integrates worker branches on local `main`. The protocolgen submodule now tracks `https://github.com/cinnabar-mc/protocolgen.git` (previously `bedrock-mc/protocolgen`) at `7b2e47c`. Valentine supports only schema label `1.26.51`, Bedrock 26.51, protocol 2193.
**Scope:** Static source review, branch/workflow review, and local validation. Behavior findings below are source-backed review results, not all runtime-reproduced vulnerabilities.

## Current protocol baseline

The player-facing stable version is Bedrock **26.51**; Mojang's protocol changelog and the canonical protocolgen schema use `1.26.51`, protocol **2193**. Valentine generates its only version crate, `v1_26_51`, from protocolgen `7b2e47c`; the 1.26.0, 1.26.30, 1.26.40 and 1.26.44 crates were removed. Server game data (blocks, states, items, biomes) now comes from protocolgen's pinned native BDS inputs (`data/source/inputs/1.26.51/native`) through `unastar-data`; PrismarineJS `minecraft-data` remains checked in only as a reference/cross-check source.

Do not use protocolgen's `1.26.50` manifest as stable: it describes Preview 25, protocol 2187. See the [official version-numbering guide](https://www.minecraft.net/en-us/article/minecraft-new-version-numbering-system) and [Bedrock protocol changelog](https://mojang.github.io/bedrock-protocol-docs/changelog/). The 26.52 hotfix (2026-09-25) has no separate protocol entry in Mojang's protocol changelog (latest listed stable: 1.26.51 / protocol 2193), so it is treated as protocol 2193 with no packet changes.

## Readiness

All high-priority findings are remediated on `main` (see below). Online mode verifies logins against Minecraft services' published JWKS. Remaining release blockers: `LICENSE-01`, live verification of Xbox/franchise paths, and the deferred MPSD ConnectionType 7 migration.

## Remediation status (2026-10-21)

| ID | Status | Fix |
|---|---|---|
| `AUTH-01` | Fixed | jolyne trusts only build-discovered OpenID/JWKS keys with issuer/audience/time checks, ES384 client-data binding to the authenticated identity key, Mojang-rooted legacy chains; offline mode marks identities unauthenticated (`f8c4917`). |
| `CODEC-01` | Fixed | Preallocation bounded by remaining input; generated recursive values carry a depth budget; borrowed decoding likewise (`fa91aae`). |
| `WORLD-01` | Fixed | Async chunk path loads from storage before generating; loaded columns are never marked dirty or overwritten (`f8a69f8`). |
| `PLAYER-01` | Fixed | Player data restored on join (position, inventory, gamemode, vitals) and saved symmetrically; corrupt records preserved (`f8a69f8`). |
| `GAME-01` | Fixed | Server-authoritative inventory, reach/gamemode checks, item consumption, permissioned `tp`, i32 block Y (`c66ce25`, `f8a69f8`). |
| `PLUGIN-01` | Fixed | Plugin instantiation/`on_load` deferred until a live world exists (`c66ce25`). |
| `RAKNET-01` | Fixed | Receive window 2048 and bounded ACK/NAK/split/ordering state, ported from lunar-bedrock/go-raknet; MTU validated with typed errors (`dd5074d`). |
| `NETHERNET-01` | Fixed | Discovery receives full datagrams and rejects truncation (`2e6a4ac`). |
| `CREDENTIALS-01` | Fixed | Secrets removed from logs/errors; token cache written atomically with owner-only permissions (`4e05b3d`). |
| `EXTRACTOR-01` | Fixed | Play stream retained, bounded collection of post-spawn definitions, typed `MissingSection`/`InvalidData` errors, schema docs; relationship to protocolgen `vanilla-data` documented (`50fce49`). |
| `PLUGIN-02` | Fixed | Manifest memory/fuel limits enforced; cancellable events dispatched before mutation; chat broadcast once (`c66ce25`). |
| `DATA-01` | Fixed | Manifest/surface updates merge existing version crates instead of dropping them (`fa91aae`). |
| `FRAME-01` | Fixed | Owned frame decoding rejects trailing bytes (`fa91aae`). |
| `NBT-01` | Fixed | NBT decode handles segmented `Buf`s (`fa91aae`). |
| `GEN-01` | Fixed | Tokens formatted before writing; atomic output writes (`fa91aae`). |
| `TEST-01` | Fixed | Legacy-gated Valentine integration tests ported to 1.26.51 (`fa91aae`). |
| `CONFIG-01` | Fixed | Single embedded config source, `deny_unknown_fields` including nested world bounds/generator (`c66ce25`). |
| `NETHERNET-02` | Fixed | Callbacks, configured timeouts/ICE servers honoured, fragment bounds validated; go-nethernet HTTP `endpoint` signaling and identity assertions added (`2e6a4ac`, `7154670`). Franchise WebSocket friend-join signaling is retained (no current upstream implements it). |
| `DATA-02` | Fixed | XBL HTTP status checked, PlayFab uses the configured game version (`4e05b3d`); RakNet MTU panic fixed (`dd5074d`). |
| `API-01` | Fixed | Plugin macros target the WIT `Guest` API; `PLUGIN_ARCHITECTURE.md` rewritten (`c66ce25`). |
| `LICENSE-01` | Open | Legal review item. |
| `DOCS-01` | Fixed | README links/prerequisites, workflow references, CI clippy enforced, CI nightly aligned to `rust-toolchain.toml` (`966d817`). |

Additional fixes found during the pass:
- jolyne's server `start_game` dropped `StartLoadingScreen` sent before `RequestChunkRadius` (jolyne's own client does this) and stalled joins (`f2f6cde`); unastar compiled against 1.26.51 for the first time (`d9c0959`).
- `CODEGEN-UNION-01`: the generator collapsed `DisconnectPacket.Messages` (a control-tagged union) into a bare struct through a name collision, so unastar sent malformed Disconnect packets; fixed with collision-safe naming and protocolgen-contract differential tests over BDS bytes (`a4776c0`). It was the only affected union of 45.
- `CLIENTDIFF-01`: BDS 1.26.51 rejects Login with DeviceOS 7 (UWP) via its native `isWellFormed` whitelist; jolyne also lacked the modern identity Token. Both fixed; gophertunnel and jolyne now complete join-idle/join-chat against BDS (`3d18c5e`, `c60cd9b`).
- `NETHERNET-03`: webrtc-rs offered an SRTP AES-256-GCM profile that webrtc-srtp keyed as AES-128 and panicked, breaking interop with pion/Cinnabar; `NETHERNET-04`: sends waited for the peer's delayed SACK (~600 ms small-message echo). Interim fix merged (`ffa64e5`); a rewrite onto the sans-IO `rtc` stack is in progress.

Known deferred items: MPSD ConnectionType 7/`PmsgId` friend broadcasting (go-xsapi sample) needs a signaling path no upstream provides yet; go-nethernet's default client verifier defers JWT verification to the Login GameServerToken, which jolyne must enforce when NetherNet identity is used; live Xbox/TLS paths are not exercised by tests.

## Performance and conformance tooling (2026-10-21)

- `tools/bench-compare` + `crates/bench-compare`: Valentine and tokio-raknet versus gophertunnel (HashimTheArab fork `80c811b`) and go-raknet (lunar-bedrock), over protocolgen's retail corpus; see `tools/bench-compare/report.md`. Owned codec geometric speedup versus gophertunnel across 25 matched packets: decode 2.0×, encode 5.3×. Cinnabar's protocolgen contract crate (measured on the same machine) decodes faster than Valentine owned (≈1.2–3×) and roughly ties Valentine borrowed; Valentine encodes ≈2–4× faster.
- `TRANSPORT-01` (fixed): ACKs were built/retired only on the 100 ms maintenance tick, so window-blocked sends stalled ~100 ms (`b4cf9b2`, `fd1f743`). Later work added bounded ACK coalescing with due-only wakes (`6b28ef9`, `9622547`): 32 B loopback echo mean ~30 µs vs go-raknet ~39 µs; 4 KiB/64 KiB means remain ~1.1–1.3× behind Go; Cinnabar raknet-tokio is behind on every loopback metric.
- Valentine frame dispatch no longer reserves a 10 KB probed stack frame per decode (`333a207`, 240 B). The earlier +78 % `mcpe_frame_decode` regression did not reproduce under pinned interleaved runs.
- Benchmarking on this i9-14900HX must pin to one P-core and interleave base/head executables; unpinned runs drift 2–3× across P/E cores.
- `tools/client-diff`: packet capture/diff harness (gophertunnel fork + lunar go-raknet vs jolyne vs BDS 1.26.51/Endstone 0.11.12). Both scripted clients spawn after the `CLIENTDIFF-01` fix; all 113 chunks match byte-for-byte by coordinate. The real-client launcher is prepared but has not been run.
- Optimization round (2026-10-22), all measured with pinned, alternating rounds against Cinnabar (protocolgen contract crate, bedrock-net, cinnabar raknet/nethernet) and Go (gophertunnel, go-raknet, go-nethernet):
  - Valentine: all 30 corpus packets decode (owned) and encode faster than the contract crate; zero-copy `ByteString`/`Bytes` fields; typed borrowed decoding for every packet (`04a99fc`).
  - jolyne batch pipeline: zero-copy framing, libdeflate, in-place AES-CTR/SHA-256 and a Go-size-matched Snappy encoder; every row ahead of bedrock-net and gophertunnel or within noise (`0cc495f`, `df91ffb`).
  - tokio-raknet: encode/ACK paths ahead of Cinnabar on 13/14 wire rows (`894e811`).
  - tokio-nethernet: signaling, ICE candidate and LAN discovery codecs ahead of Cinnabar and Go on every row (`adbf38b`).

## Original readiness note (2026-09-25)

Do not expose the server in online mode until `AUTH-01` is fixed. Packet-controlled allocation/recursion, persistence, and client-authoritative gameplay changes also need remediation before public service.

## High-priority findings

| ID | Finding | Evidence and impact |
|---|---|---|
| `AUTH-01` | Online OpenID validation can trust attacker-supplied key material. | `crates/jolyne/src/auth/openid.rs:142-158,278-312` accepts `x5u`/`x5c` header material without requiring a trusted provider key/issuer. Client-data claims are decoded unverified; identity-key mismatch only warns (`:356-371,418-428`). An attacker can self-sign a token with the expected audience. The embedded server default enables online mode (`crates/unastar/src/config/mod.rs:41-47`), while root `unastar.toml` disables it. |
| `CODEC-01` | Wire lengths can trigger large allocations; recursive values have no depth budget. | `crates/valentine/bedrock_core/src/bedrock/codec.rs:476-482` calls `Vec::with_capacity` from the decoded length before proving the payload contains that many items. Generated `CerealDynamicValue::{List,Map}` recurse through `Vec` in current default `crates/valentine/bedrock_versions/v1_26_51/src/types.rs:3200-3207`. Crafted input can exhaust memory or stack. |
| `WORLD-01` | Async chunk requests skip loading existing columns before generation. | `crates/unastar/src/world/ecs/systems.rs:461-467` calls `request_generation`; the sync path in `world/manager.rs:200-222` loads from the provider first, but the async path in `:308-334` does not. Dirty generated columns are later saved (`systems.rs:670-690`), risking replacement of existing world data. Root `unastar.toml` selects vanilla storage. |
| `PLAYER-01` | Player data is saved on disconnect but not restored through the server connection path. | Disconnect writes `PlayerData` in `crates/unastar/src/server/game/mod.rs:355-393`; the provider exposes `load` (`storage/provider.rs:114-121`), but no server call was found. `save_last_position` likewise has no caller, although `server/connect.rs:98-106` loads last position. |
| `GAME-01` | Client packet fields authorize inventory and block mutations without server-side validation. | `crates/unastar/src/server/game/inventory.rs:383-400` trusts client item ID/stack size. `blocks.rs:77-82` accepts destroy actions and immediately breaks blocks; `blocks.rs:552-615` places from a client-supplied item ID without inventory consumption. The `tp` command bypasses registry/permission handling (`commands.rs:160-170`), and block Y values narrow from `i32` to `i16` (`world/ecs/systems.rs:876-880`). |
| `PLUGIN-01` | Plugin `on_load` can call host APIs while the world pointer is null. | `crates/unastar/src/plugin/manager.rs:61-79,421-423` initializes `HostContext` with a null pointer and invokes `on_load` before setting it. A host call can dereference null. |
| `RAKNET-01` | A packet-controlled sequence gap can monopolize listener CPU. | `crates/tokio-raknet/src/session/mod.rs:220-249` walks missing sequence values and queues NAK ranges; one large gap can traverse up to roughly 8.4 million values. Bound the receive window. |
| `NETHERNET-01` | Discovery responses can be truncated. | `crates/tokio-nethernet/src/discovery/mod.rs:117-129` receives into 1 KiB, while `discovery/packet.rs:315-326` documents SDP payloads typically larger than 2 KiB. |
| `CREDENTIALS-01` | PlayFab secrets can enter logs; OAuth token files have no restrictive creation mode. | `crates/axolotl-xbl/src/playfab.rs:51-62,181-188` logs the raw successful response and includes it in parse/missing-data errors; response data includes session/entity tokens. `crates/axelerator/src/token_cache.rs:368-375` writes token files without restrictive permissions. |
| `EXTRACTOR-01` | BDS extraction silently writes incomplete game data. | `crates/bds-extractor/src/main.rs:131-145` discards the play stream returned by `join`; biome/entity/creative sections remain unset (`crates/jolyne/src/gamedata.rs:28-35`) and are accepted as optional by the extractor output path. |

## Follow-up findings

| ID | Finding | Evidence and impact |
|---|---|---|
| `PLUGIN-02` | Plugin memory/fuel manifest limits are not enforced; some events dispatch after mutation. | `crates/unastar/src/plugin/manifest.rs:40-63` advertises limits, while manager setup only sets a fixed epoch deadline (`plugin/manager.rs:411-416`). Block changes happen before queued plugin events (`server/game/blocks.rs:605-615`; `plugin/manager.rs:505-509`), so cancellation cannot roll them back. Chat can be broadcast before default event handling and duplicated. |
| `DATA-01` | Prismarine data-only generation can damage Valentine protocol feature/dependency wiring. | `crates/valentine_gen/README.md:158` documents data-only generation. The Prismarine path later calls `update_valentine_manifest_at`; that function removes `valentine_bedrock_*` dependencies/features and reinserts only versions found in the source. The Prismarine corpus stops before current `1.26.51`, while `default` must reference `bedrock_1_26_51`. |
| `FRAME-01` | Owned MCPE frame decoding does not reject unconsumed payload bytes. | Generated decoder in `crates/valentine_gen/src/generator/mod.rs:688-720` takes the declared frame and returns without checking `payload_buf.remaining()`, risking desynchronization. |
| `NBT-01` | NBT decode assumes the entire value is in the first `Buf::chunk()`. | `crates/valentine/bedrock_core/src/bedrock/codec.rs:929-939` parses a cursor over `buf.chunk()`. A valid value split across a chained/segmented buffer can fail. |
| `GEN-01` | Generator can truncate an output file before discovering invalid generated syntax. | `crates/valentine_gen/src/generator/emitter.rs:25-34` calls `File::create` before `format_tokens` parses tokens (`:11-22`). Format before replacing output or use an atomic write. |
| `TEST-01` | Three broad protocol integration binaries remain gated to `bedrock_1_26_30`; Valentine now defaults to `bedrock_1_26_51`. Enable the explicit version features to run those legacy cases alongside default-version tests. |
| `CONFIG-01` | Embedded, root, and crate-local configuration samples drift; some advertised world-storage keys are ignored. | Embedded auth defaults differ from root `unastar.toml`; embedded world storage options are not represented in `WorldConfig` (`crates/unastar/src/world/mod.rs:87-98`). Serde defaults allow unknown keys to be ignored. |
| `NETHERNET-02` | NetherNet stream API has dead callbacks and ignored config fields. | `crates/tokio-nethernet/src/stream.rs:130-136` drops the callback receiver immediately. `connect_with_config` replaces configured timeout/ICE servers with hardcoded values (`:335-349`); fragment count narrows to `u8` (`:221-237`). |
| `DATA-02` | Extractor and transport APIs have additional correctness gaps. | `crates/axolotl-xbl/src/friends.rs:148-162` returns success without checking HTTP status; PlayFab session creation hardcodes `1.21.20` (`playfab.rs:204`). RakNet listener MTU configuration can panic below its minimum (`crates/tokio-raknet/src/transport/listener/offline.rs:161-165`). |
| `API-01` | Public plugin macros target the older plugin API. | `crates/unastar-api-macros/src/lib.rs:58-104` emits `PluginEvent`/`GameContext`/Borsh code, while current plugin API uses WIT `Guest`. |
| `LICENSE-01` | License declarations conflict; resolve before distribution. | Workspace metadata declares MIT, `crates/unastar/LICENSE` is GPLv3, and `deps/bleveldb-sys/Cargo.toml` says AGPL-3.0-only while its `LICENSE` is GPLv3. This is a release/legal review item, not a legal conclusion. |
| `DOCS-01` | Documentation and maintenance signals are stale. | Root README links to `crates/axolotl_xbl` instead of `crates/axolotl-xbl` and says latest stable despite nightly-only `portable_simd`. `.github/workflows/update-prismarine-data.yml` references `update-protocol-docs.yml`; the actual workflow is `update-protocolgen.yml`. CI marks Clippy `continue-on-error`. |

## Branch and pull-request disposition

After review, local `main` was fast-forwarded to `06a36d0` on 2026-09-25. Counts below are relative to the current local `main`; `origin/main` still points to the review base until pushed:

| Remote branch | Ahead / behind `main` | Disposition |
|---|---:|---|
| `origin/fix/decode-allocation-safety` | 2 / 5 | Valuable decoder work, but a merge-tree check reports conflicts in `bedrock/borrowed.rs`, `bedrock/codec.rs`, generator `borrowed.rs`, and generator `codec.rs`. Preserve and port selectively; do not merge blindly. |
| `origin/feat/borrowed-byte-slices` | 3 / 5 | Contains the decoder commits plus borrowed-slice work; not a clean merge onto current main. Preserve for a deliberate port after safety changes. |
| `origin/automation/protocolgen-update` | 0 / 0 | Fast-forwarded into local `main` as `06a36d0`; then advanced the protocolgen submodule to upstream `1b105d8` and generated stable Bedrock 26.51 (protocol 2193). |
| `origin/automation/prismarine-data-update` | 1 / 4 | Do not merge as-is: generated gameplay data is not exported from the version module. |
| `origin/agent/prismarine-data-workflow` | 3 / 18 | Workflow work appears superseded by main; compare unique commits before deleting the ref. |
| `origin/fix/oracle-pin-full-sha` | 1 / 14 | Pin appears already present on main; verify unique gitlink/content before removing the ref. |

One open PR was visible: draft PR #2 (`realsexmc-changes`), marked dirty and 8 commits ahead / 82 behind; its description reports three failing world-loader tests. Keep draft until rebased and those tests are addressed.

2026-10-21: the decoder-safety work from `origin/fix/decode-allocation-safety` was ported selectively in `fa91aae`; review `origin/feat/borrowed-byte-slices` for anything still unique before retiring both refs. Local worker branches `Jviguy/stab-*` are merged into `main` and can be deleted.

## Validation and environment

### 2026-10-21 (remediation pass, `nightly-2026-03-05` from `rust-toolchain.toml`)

- Windows `bleveldb-sys` now builds: vcpkg at `C:\Users\jvigu\vcpkg` with `zlib`/`snappy` for `x64-windows-static-md`, `VCPKG_ROOT` set.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace --lib --tests --no-fail-fast`: all pass once the `vanilla_bp` and `bedrock-protocol-docs` submodules are initialised (`unastar-data-gen` biome tests and the `valentine_gen` Mojang generation gate read them; CI checks out submodules recursively).
- RakNet smoke: release `unastar` (offline config) + `jolyne` `stress_test` with 5 clients for 15 s — 5/5 joined and spawned, steady 20.0 TPS, clean despawn on disconnect.

### 2026-09-25

- `cargo fmt --all -- --check` passed on installed `nightly-2026-05-12`.
- `cargo test -p jolyne --lib --tests` passed 99 tests on `nightly-2026-05-12` with the 1.26.51 protocol feature.
- `cargo test -p valentine --tests --features bedrock_1_26_30,bedrock_1_26_44` passed 22 tests across 7 suites, including the 1.26.51 default and compatibility versions.
- `cargo clippy -p valentine -p jolyne --all-targets` completed with two existing `clippy::question_mark` warnings in `crates/jolyne/src/auth/legacy.rs:53,69`.
- `cargo check -p bds-extractor --all-targets` passed after updating its current-version enum references.
- `cargo test --workspace --lib --tests` stopped in `bleveldb-sys` build script with `Failed to determine vcpkg root directory`. This Windows environment has no vcpkg; workspace-wide tests and compilation of the updated Unastar callsites remain unverified.
- `cargo clippy --workspace --all-targets -- -D warnings` failed at `crates/jolyne/src/auth/legacy.rs:53,69` on `clippy::question_mark`. CI currently ignores this failure.
- The repository-pinned nightly could not be installed in this environment; checks used `nightly-2026-05-12`, while the test workflow pins another nightly.

## Cleanup performed

- Removed two tracked root `rustc-ice-*.txt` dumps. They record a Rust 1.96 nightly codegen panic after Windows reported that the paging file was too small; they are local diagnostics, not project source.
- Removed the tracked 2,745,810-byte `crates/valentine_gen/debug_gen_error_types.rs` token dump. The generator can recreate such a dump on parse failure; the pattern is now ignored to avoid re-adding diagnostics.
