# Vanilla offline Login rejection — observed 2026-10-21

**Acceptance failed:** neither scripted client reached spawn. These are pre-spawn rejection captures, **not completed scenario comparisons**. Minecraft Bedrock was not launched.

Baseline: Mojang BDS **1.26.51.1**, Endstone **0.11.12**, protocol **2193**, offline mode. Both named scenarios were attempted for both clients, with the saved deterministic world template restored before each connection. Endstone independently recorded the same packets as each client.

| Attempted scenario | c2s identical | c2s masked-equal | c2s different | c2s missing | Spawn completed |
|---|---:|---:|---:|---:|---|
| join-idle | 1 | 0 | 1 | 0 | No |
| join-chat | 1 | 0 | 1 | 0 | No |

Both s2c comparisons contained two identical packets and no masked-equal, different, or missing packets. Chat, `/say`, idle-after-spawn, and local-player initialization were **not reached**.

## Sequence

| Direction | ID | Packet | Result |
|---|---:|---|---|
| c2s | 193 | RequestNetworkSettings | Identical (`c10100000891`) |
| s2c | 143 | NetworkSettings | Identical |
| c2s | 1 | Login | Different |
| s2c | 156 | PacketViolationWarning | Identical rejection |

Server warning: `Connection Request invalid.\nreadNoHeader failed! packetId: 1`.

## Top divergence: Login

- Go sends authentication JSON with `AuthenticationType: 2`, `Certificate` containing stringified `{"chain":[""]}`, and a self-signed multiplayer `Token` JWT.
- Jolyne sends `AuthenticationType: 2` with a top-level legacy-style `chain`, without `Certificate` or `Token`.
- Client-data claim sets and skin geometry differ. The harness fixes identity/device values and skin pixels; remaining capability fields are compared, not blanket-masked.
- Both Login packets have protocol 2193, consistent outer unsigned-varint lengths, and two correctly bounded little-endian length-prefixed authentication/client-data strings. Both JWT payloads parse as JSON. Public keys use base64 DER `x5u` headers.
- This envelope difference is real, but it is **not proven to cause BDS's rejection**: the pinned Go reference is rejected too. No speculative Jolyne authentication fix was applied.

## Isolation experiments

1. Directly launching unmodified `bedrock_server.exe` without Endstone reproduces the pinned Go rejection. The packet hook is not the cause.
2. Unmodified Sandertv/gophertunnel **be557e33fe6f8d0c0db7ef05b5fd068725c325ad**, with no fork replacements, also receives the same rejection against plain BDS. Its login/auth encoder source files match the pinned fork.
3. The Go legacy-auth experiment also fails; it was not made a production fallback.
4. Enabling BDS `content-log-file-enabled=true` did not reveal an additional authentication diagnostic in the observed server output.
5. Protocolgen's `differential/testdata/corpus/client/001/8855508aade16ec5.hex` is five zero bytes, generated as a zero-value packet-body seed. The 53 pinned retail fixtures contain no Login. Neither is an authentic accepted retail Login reference.

Missing prerequisite: a proven offline connection-request accepted by this BDS build, or an authentic accepted protocol-2193 Login capture with provenance. Native `BaseConnectionRequest` semantic validation is the remaining unverified boundary; empty claims/dummy certificates remain hypotheses, not established causes.

Raw captures, JWTs, binaries, worlds, virtual environments, and full byte reports remain ignored under `tools/client-diff/runs/` and `endstone-capture/runtime/`; this trimmed report contains no token, private key, or account credential.

## Verification and delivery

Implementation commit: `0e9a781` (`feat(client-diff): add pinned vanilla packet capture and comparison harness`).
Merged current `main` into this branch at `d601c1f2cbd29251cbe05c31c9d28966de4a5ea2`, with no conflicts; the following checks were repeated after that merge.

| Command | Observed result |
|---|---|
| `cargo fmt --all` | Exit 0 |
| `cargo test -p client-diff` | 11 passed |
| `cargo clippy -p client-diff --all-targets -- -D warnings` | Exit 0, clean |
| `cargo clippy -p jolyne --all-targets -- -D warnings` | Exit 0, clean |
| `cargo test -p jolyne` | 105 passed, 8 doctests ignored |
| `go test ./...` in `tools/client-diff/gt-client` | Exit 0 |
| `go vet ./...` in `tools/client-diff/gt-client` | Exit 0 |
| `tools/client-diff/endstone-capture/runtime/venv/Scripts/python.exe tools/client-diff/endstone-capture/test_capture.py` | 2 passed |
| `pwsh -NoProfile -File tools/client-diff/run.ps1` | Exit 1: all four client attempts rejected at Login; all captures and four Markdown reports retained |

Final post-merge run artifacts: `tools/client-diff/runs/20261021-151648-721/`.
For each of the four connections, the sequence of `(direction, numeric ID, complete packet bytes)` in the client capture exactly equaled the independent Endstone capture: IDs 193, 143, 1, 156. The summary counts above were extracted from the final generated Markdown reports, not inferred.

No Jolyne protocol fix was made: `send_login_with_tokens` is an additive client-only API used to sign the fixed harness identity. The existing README example was corrected to destructure `join`'s `(stream, GameData)` return. Review fixes in the new harness preserve noncanonical raw bytes, capture unknown numeric IDs before enum dispatch, and preserve complete timing intervals after slow sends; each has a deterministic regression.

`-Server unastar` is implemented but was not exercised. Real-client launcher parsing/source was inspected only; the launcher and all `minecraft://` URIs were never executed.

### Changed files

- `Cargo.toml`, `Cargo.lock`: workspace member/dependency resolution.
- `crates/client-diff/Cargo.toml`, `src/{main,lib,client}.rs`: CLI, typed comparison/reporting, observed real Jolyne client.
- `crates/client-diff/tests/diff.rs`, `tests/fixtures/latency-{a,b}.jsonl`: alignment/masking/raw-fidelity regression fixtures.
- `crates/jolyne/src/stream/client.rs`, `crates/jolyne/README.md`: caller-supplied signed client tokens and usage documentation.
- `tools/client-diff/{README.md,.gitignore,scenarios.json,masks.toml,run.ps1}`: shared contract, explicit masks, lifecycle/reset orchestration, instructions.
- `tools/client-diff/gt-client/{go.mod,go.sum,main.go,main_test.go}`: pinned reference implementation and regression tests.
- `tools/client-diff/endstone-capture/{setup.ps1,provision.py,pyproject.toml,test_capture.py}` and `src/endstone_client_diff/{capture,plugin}.py`: pinned provisioner and independent packet capture.
- `tools/client-diff/real-client/launch.ps1`: dormant manual launcher.
- `tools/client-diff/examples/rejected-login.md`: this trimmed secret-free report.
