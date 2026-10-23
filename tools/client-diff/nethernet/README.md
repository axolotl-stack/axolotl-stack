# NetherNet real-client probe

Records what the real Minecraft Bedrock client does on NetherNet discovery and
signaling (server data, offer/answer SDP, candidates, identity claims, error
codes and timing) against a loopback-only tokio-nethernet peer. The findings
feed `crates/tokio-nethernet/PARITY.md`.

## Why loopback works without prompts

The probe (`crates/tokio-nethernet/examples/real_client_probe.rs`) binds every
socket to `127.0.0.1` and disables mDNS, so it never listens on a network
interface and Windows Firewall has nothing to ask about. It needs no admin
rights, no firewall rule, no packet-capture driver and no Xbox signaling.

The client's LAN socket is a wildcard `[::]:7551` (or `0.0.0.0:7551`) that
accepts discovery datagrams from any source address (client 1.26.50
`LanThreadManager`, see PARITY.md rows L14-L15). That gives two paths:

| Mode | Client does | Probe does |
|---|---|---|
| `host` | hosts a world with *Visible to LAN players* | unicasts a discovery request to `127.0.0.1:7551`, reads the server data, dials the host, sends `RequestNetworkSettings`, logs the answer and packets |
| `server` | browses *Play → Friends* (LAN games) | sends unsolicited discovery responses to `127.0.0.1:7551` so "NETHERNET-TRUTH probe" is listed; when joined, logs the offer, candidates and identity, answers `RequestNetworkSettings` with `NetworkSettings`, logs the following packets (Login) |
| `route` | nothing | sends one datagram from `127.0.0.1` to each local interface address (closed port 9) and reports whether it was delivered |

Discovery and signaling run entirely over loopback; this was verified against
a NetherNet BDS 1.26.51, which has an inbound Block rule yet answered discovery
and `CONNECTREQUEST` from `127.0.0.1`. ICE does **not** connect: the client
gathers host candidates only on interface addresses (libwebrtc skips
loopback), and Windows refuses datagrams from a `127.0.0.1` socket to the
machine's own LAN address (`route`: `WSAENETUNREACH`). Expect `dial_failed` /
`no_join` after the offer/answer exchange; the transport rows of PARITY.md
therefore rest on binary analysis. The data-path code in the probe
(`RequestNetworkSettings`, `NetworkSettings`, packet logging) only runs if a
future setup lets ICE connect.

## Commands

Build once (worker, before the session; the probe binds only `127.0.0.1`, so
its new executable path raises no prompt):

```powershell
cargo build -p tokio-nethernet --features discovery --example real_client_probe
```

Each run writes JSON lines to stdout and to `--out`. Paths below are from the
worktree root; with `CARGO_TARGET_DIR` set, the executable is under that
directory instead of `.\target`.

```powershell
$probe = ".\target\debug\examples\real_client_probe.exe"
New-Item -ItemType Directory -Force tools\client-diff\nethernet\captures | Out-Null

# 0. Without the client: loopback -> interface delivery.
& $probe route --out tools\client-diff\nethernet\captures\route.jsonl

# 1. Client hosts: open a world with "Visible to LAN players" on, then:
netstat -ano -p udp | findstr ":7551"   # Minecraft's PID on 0.0.0.0:7551 or [::]:7551
& $probe host --wait 120 --out tools\client-diff\nethernet\captures\host.jsonl

# 2. Client joins: leave the world, open Play -> Friends, start the probe,
#    then click "NETHERNET-TRUTH probe" under LAN games within 3 minutes.
& $probe server --wait 180 --out tools\client-diff\nethernet\captures\server.jsonl
```

`--wait` bounds discovery (`host`) or the join (`server`). Set `RUST_LOG=debug`
for transport logs on stderr.

Captures contain the client's identity claims (`peer_token`: JWT header and
claims, never the signature; `a=identity` values are redacted in signal
logs), which identify the signed-in account: `captures/` is git-ignored;
share only redacted excerpts.

## Reading the log

| Event | Meaning |
|---|---|
| `discovery_bound` | probe's loopback discovery socket and network ID |
| `host_discovered` | the client's server data (`server_data_hex`, decoded v7 fields) |
| `signal_out` / `signal_in` | full signal text: `CONNECTREQUEST` SDP, `CONNECTRESPONSE`, every `CANDIDATEADD`, `CONNECTERROR` code |
| `peer_token` | the peer's identity JWT (recorded, not authenticated) |
| `connected` / `accepted` | ICE, DTLS, SCTP and both data channels are up (`after_ms`) |
| `dial_failed` / `accept_failed` / `no_join` | where negotiation stopped |
| `message_in` | reliability, length and first bytes of each NetherNet message |
| `stream_error` / `stream_end` / `closed` | how the session ended |
