# CLIENTDIFF-01 — offline BDS Login resolved

**Observed 2026-10-21:** the unmodified pinned gophertunnel reference and jolyne both spawn on Mojang BDS **1.26.51.1**, Endstone **0.11.12**, protocol **2193**, `online-mode=false`, and complete **join-idle** and **join-chat**. Minecraft was not launched; no live Xbox account or credentials were used.

## Root cause of `readNoHeader`

BDS rejects retired UWP **`DeviceOS: 7`** as an invalid connection request; `readNoHeader failed` is the enclosing packet-reader diagnostic, not evidence that the little-endian string lengths or JWT envelope failed to parse.

Evidence:

- The shared scenario forced `DeviceOS: 7` into both clients, and the Rust runner explicitly required it. Jolyne's ordinary offline client also defaulted to 7.
- Removing Go's `Certificate` in a temporary protocol adapter still produced the original warning (`runs/20261021-183829-781`). That adapter was removed.
- Changing **only** the shared identity's DeviceOS from 7 to **1**, with the pinned Go code and dependencies unmodified, produced ServerToClientHandshake, ClientToServerHandshake, LoginSuccess, resource-pack completion, StartGame, and SetLocalPlayerAsInitialised (`runs/20261021-184216-027/join-idle-vanilla-go.jsonl`). The Go process exited successfully. This established an accepted reference before changing jolyne's identity envelope.

The accepted Go request retains its stringified `Certificate = {"chain":[""]}` and self-signed multiplayer `Token`. The LE i32 authentication/client JWT lengths and the outer packet framing were already correct and were not changed.

## Client fixes

| Commit | Change | Regression evidence |
|---|---|---|
| `3d18c5e` | Use supported Android DeviceOS 1 in jolyne's offline metadata and shared harness identity/guard. | Encoded-byte test failed on BDS's platform whitelist before the fix, passed afterward. |
| `c60cd9b` | Send a modern self-signed multiplayer identity `Token`, rather than only a top-level legacy `chain`. Preserve `AuthenticationType: 2`; bind `cpk` to the client-data signing/encryption key. | Byte-framed envelope/identity-claim/signature test failed because Token was absent, then passed. |
| `107b881` | Add `encode_with_multiplayer_token` for an already acquired provider token: `AuthenticationType: 0`, unchanged provider Token, signed client metadata. | Missing-API regression failed before implementation; encoded-envelope and real ES384 client-signature checks pass. |
| `ef58caa` | Do not advertise unsupported client-side chunk generation, for either offline or provider-token metadata. | Regression failed on the advertised `true` capability, then passed for both encoding paths. |

The intermediate platform-only jolyne attempt reached an authentication Disconnect instead of PacketViolationWarning (`runs/20261021-184535-066/join-idle-vanilla-jolyne-client.stderr.log`). Sending the modern multiplayer Token then completed spawn. Server-side authentication/verification was not changed.

Online **wire shape** is covered without live Xbox. The caller must acquire a provider token bound to the same P-384 key, for example using the existing `PlayFabClient::multiplayer_token`, then use `encode_with_multiplayer_token` and `send_login_with_tokens`. The older XblCredentials/Mojang-chain acquisition path was not live-verified against BDS 26.51; no online-login success is claimed.

## Final completed scenario comparisons

Final post-merge artifacts: `tools/client-diff/runs/20261021-185050-556/`.

| Scenario | Direction | Identical | Masked-equal | Different | Missing |
|---|---|---:|---:|---:|---:|
| join-idle | c2s | 7 | 0 | 1 | 2 |
| join-idle | s2c | 107 | 1 | 84 | 106 |
| join-chat | c2s | 8 | 1 | 1 | 2 |
| join-chat | s2c | 110 | 1 | 84 | 137 |

Both clients sent exactly one local-player initialization in each scenario. Both sent exactly one chat packet and one command in join-chat; chat was **identical**, command **masked-equal**. These are completed spawn/action/idle captures, not rejection comparisons. Missing counts include unmatched packets on either side; timing-sensitive counts are observations of this run, not stable golden values.

### Top remaining divergences

- **Login (one c2s Different per scenario):** supported presentation/capability metadata differs. Examples include arm size, view distance/memory tier, skin color/geometry, and additional editor/party/profile claims emitted by Go. The fixed name, UUID, device identity, skin pixels and scenario actions remain controlled. Jolyne no longer claims client-side chunk generation. JWT keys/signatures/times are normalized only by the explicit existing masks; remaining fields are not blanket-erased.
- **Loading screens (two c2s Missing per scenario):** jolyne sends StartLoadingScreen and EndLoadingScreen around its join. Go sends neither. These are valid notifications, not a reason to delete jolyne's loading-state signaling merely to match a scripted reference.
- **LevelChunk (80 s2c Different and two unmatched alignment rows per scenario):** sequential packet-ID LCS pairs chunks in different coordinate orders. Independently indexing the captured packets by signed `(chunk_x, chunk_z)` yields the **same 113 coordinates and byte-identical complete packets at all 113 coordinates** in each scenario. No chunk bytes were masked to obtain this result.
- **NetworkChunkPublisherUpdate (104 / 135 unmatched rows in idle / chat):** Go receives many more repeated updates. In the preceding completed capture, both clients received the same set of distinct publisher-update payloads; repetition counts differed substantially. The different loading-screen signals and client pacing remain visible rather than being normalized away; their causal contribution was not isolated.
- **StartGame / CraftingData / PlayerList (one / one / two s2c Different per scenario):** a diagnostic typed decode of the preceding completed capture found different StartGame current ticks, enchantment seeds and multiplayer correlation IDs; CraftingData included per-server recipe UUID differences; PlayerList reflected the submitted skin geometry/color differences. Existing masks do not remove these fields. Raw reports remain available for inspection.

### Independent Endstone observations

For all four final sessions, the ordered c2s `(numeric ID, complete bytes)` sequence exactly matches the independent Endstone log. Both jolyne s2c sequences also match exactly. Go server-vantage logs contain additional publisher updates (four in idle, two in chat) and one Disconnect per session that are absent at the client vantage; there are no client-only packet-byte entries. Combined bidirectional event order can differ between observation points, so it is not represented as a global ordering guarantee.

## Verification after merging main

`git merge main` completed without conflicts before the final acceptance commands and captures; it integrated the concurrent bds-extractor changes without modifying this task's ownership boundaries.

| Command | Observed result |
|---|---|
| `cargo fmt --all` | Exit 0 |
| `cargo test -p jolyne --lib --tests` | 108 passed |
| `cargo test -p client-diff` | 11 passed |
| `cargo clippy -p jolyne -p client-diff --all-targets -- -D warnings` | Exit 0, clean |
| `go test ./...` in `tools/client-diff/gt-client` | Exit 0 |
| `go vet ./...` in `tools/client-diff/gt-client` | Exit 0 |
| `pwsh -NoProfile -File tools/client-diff/run.ps1 -Runtime C:/Users/jvigu/orca/workspaces/axolotl-stack/stab-client-diff/tools/client-diff/endstone-capture/runtime` | Exit 0; both clients completed both scenarios |

The Runtime override reuses the already provisioned pinned BDS/Endstone installation and closed-world template; the current worktree's clients, scenarios and report code ran. The runner restored the template between connections. No 26.52 executable or real Minecraft launcher was run: the single-variable 26.51 experiment resolved the original failure directly.

Additional out-of-ownership finding, reported to the coordinator: the intermediate authentication rejection's Disconnect body `6a000000` triggers Valentine's one-trailing-byte error. It is not suppressed or special-cased here and is not encountered in the completed accepted scenarios.

Changed task-owned files: `crates/jolyne/src/auth/client.rs`, `crates/jolyne/README.md`, `crates/client-diff/src/client.rs`, `tools/client-diff/scenarios.json`, `tools/client-diff/README.md`, and this report. The Go reference source/module pins, generated protocol files, server verification, and MAINTENANCE_AUDIT.md are unchanged. Raw captures, JWTs, binaries, worlds and full hex reports remain ignored; this trimmed report contains no account token, private key or credential.
