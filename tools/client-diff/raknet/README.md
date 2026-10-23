# RakNet wire captures (Mojang RakNet reference)

Records how Mojang RakNet — Bedrock Dedicated Server and the Minecraft client —
behaves on the wire, so `tokio-raknet`'s `TransmissionPolicy::Vanilla` can be
checked against it (`crates/tokio-raknet/PARITY.md`,
`crates/tokio-raknet/tests/capture_diff.rs`).

Everything runs on loopback without administrator rights, firewall changes or
packet-capture drivers: a userland UDP relay
(`crates/tokio-raknet/examples/raknet_capture`, `relay` subcommand) sits
between the client and the server on 127.0.0.1, timestamps every datagram in
both directions and injects faults.

## Prerequisites

- Windows, PowerShell 7, the pinned Rust toolchain, Go (`C:\Program Files\Go\bin`).
- The BDS 1.26.51 runtime in `tools/client-diff/endstone-capture/runtime`
  (`server/` with `bedrock_server.exe`, `world-template/`), e.g. copied from an
  existing harness runtime or created by `endstone-capture/setup.ps1`. The
  scripts run the plain `bedrock_server.exe` (no Endstone hooks) and write the
  port lines of its `server.properties`. BDS 1.26.51 ignores `server-ip` and
  binds every interface on its two ports; on a fresh path Windows may ask
  once about the firewall (blocking is fine, all traffic is loopback).

## BDS captures

```powershell
./tools/client-diff/raknet/capture.ps1                 # all scenarios into tools/client-diff/runs/raknet-<time>
./tools/client-diff/raknet/capture.ps1 -Scenario probe,baseline
./tools/client-diff/raknet/capture.ps1 -Commit         # also refresh crates/tokio-raknet/tests/captures/bds-1.26.51
```

| Scenario | Relay faults | Client |
|---|---|---|
| `probe` | — (direct) | `raknet_capture probe`: unconnected ping, Request1 of 1500…100 bytes, protocols 10/12, Request2 MTUs 1492…2000, Request2 without Request1, repeated Request2, same GUID from another address |
| `baseline` | path MTU 1492 | gophertunnel bot, 16 s idle, then disconnect (go-raknet finishes its close) |
| `mtu-1400`, `mtu-1200`, `mtu-576` | datagrams with `len + 28 > N` dropped both ways | bot, `join-idle` |
| `loss-5` | 5% random loss of connected datagrams both ways (seeded) | bot |
| `reorder-5` | 5% of connected datagrams held behind the next one | bot |
| `kick` | — | bot runs `/kick` on itself |
| `client-silence` | client datagrams dropped after 8 s | bot (BDS's resends and timeout) |

The bot is `tools/client-diff/gt-client` (gophertunnel `80c811b` over go-raknet
`87a6b7e`), so the client side of these captures is go-raknet, not Mojang.

## Real Minecraft client

```powershell
./tools/client-diff/real-client/capture.ps1 -Name baseline -NoLaunch    # 60 s session, path MTU 1492, join by hand
./tools/client-diff/real-client/capture.ps1 -Name mtu-576 -Mtu 576 -Seconds 75 -NoLaunch
./tools/client-diff/real-client/capture.ps1 -Name loss-5 -Loss 0.05 -Seconds 75 -NoLaunch -Commit
./tools/client-diff/real-client/capture.ps1 -Client gophertunnel        # plumbing check without the game
```

Starts BDS and the relay, opens
`minecraft://connect/?serverUrl=127.0.0.1&serverPort=<RelayPort>`, records for
`-Seconds` (join, idle or play; disconnect from the pause menu to capture the
client's close), closes the game, stops everything and prints the analysis.
With `-NoLaunch` the script neither opens nor closes the game: it prints the
relay address (127.0.0.1, `-RelayPort`, default 39140) and records while the
person at the machine joins it from the game's server list (the
`minecraft://connect` URI did not connect on the build machine). The game
joins the IPv4 port of the pong's advertisement, not the port it pinged, and
BDS 1.26.51 answers RakNet pings with a 33-byte pong without advertisement
(with that pong the game pinged every 5 s and never sent Request1), so the
relay runs with `--advertise-relay 2193;1.26.51`: it fills the empty pong
with a BDS-shaped advertisement naming the relay's port.
`-Commit` copies the capture to `crates/tokio-raknet/tests/captures/real-client/`,
which `cargo test -p tokio-raknet --test capture_diff` compares
with `RaknetStream` (Vanilla): MTU probe sizes and cadence, ConnectionRequest
and NewIncomingConnection reliability, ACK delay, ping period, fragment sizes.
Only run it with a person at the machine: the game must be installed and
signed in, and must not already be running.

Committed captures (`crates/tokio-raknet/tests/captures/real-client`,
Minecraft 1.26.52 for Windows, joined by hand with `-NoLaunch`): `baseline`
(path MTU 1492), `mtu-1200`, `mtu-576`, `loss-5` (5% loss both ways). The
game joined 29-85 s into each recording and was still connected when it
stopped, so the client's disconnect is not in them.

## Capture format

JSON lines: a `meta` line (`scenario`, `server`, `client`, `faults`), then one
record per datagram: `t_us` (since the relay started), `dir` (`c2s`/`s2c`),
`len`, `action` (`fwd`, `drop_mtu`, `drop_loss`, `drop_index`,
`drop_blackhole`, `reordered`) and the parsed body (`offline` with hex,
`ack`/`nak` with ranges, `data` with flags, sequence and per-frame
reliability, indexes, split info and length). Captures are reduced so they can
be committed: game payloads keep only their first byte, RakNet control
messages keep their bytes, Request1 padding is trimmed, and the system
addresses in ConnectionRequestAccepted/NewIncomingConnection (BDS lists the
host's interface addresses there) are replaced with loopback addresses.

`cargo run -p tokio-raknet --example raknet_capture -- analyze <file.jsonl>...`
prints the features the tests compare.
