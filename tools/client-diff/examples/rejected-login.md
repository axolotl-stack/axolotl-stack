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
