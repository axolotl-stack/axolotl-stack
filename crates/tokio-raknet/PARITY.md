# Mojang RakNet parity specification

`TransmissionPolicy::Vanilla` reproduces **Mojang RakNet**, the fork of
Jenkins Software RakNet 4.081 inside the Minecraft Bedrock client and Bedrock
Dedicated Server (BDS): `RaknetStream` (default policy Vanilla) behaves like
the Mojang client, `RaknetListener` with `transmission_policy = Vanilla` like
BDS. Both roles run the same library, so most rows have one Mojang behaviour;
the BDS and client columns differ only where the builds or roles do.
`TransmissionPolicy::Optimized` (default for `RaknetListener` and the
low-level `Session`/`ManagedSession` constructors) keeps tokio-raknet's own
behaviour and is unchanged by this specification.

Until 2026-10 the Vanilla target was lunar-bedrock/go-raknet `87a6b7e`, a Go
reimplementation. Every row keeps a go-raknet column, now for lunar-bedrock
go-raknet `6124f52`, and says where go-raknet differs from Mojang. `6124f52`
adopted much of Mojang's transmission behaviour (sliding congestion window,
RTO and RTT estimate, 10 ms ACK delay, immediate NAKs, datagram flags, the
13-probe MTU ladder, the standard IPv6 address layout); where it changed a
row, the cell ends with what `87a6b7e` did. The go-raknet column documents a
reference only: Vanilla follows Mojang, not go-raknet.

## Evidence

| Source | What it decides |
|---|---|
| **BDS captures** `tests/captures/bds-1.26.51/*.jsonl` | Mojang server behaviour on the wire. BDS 1.26.51 (plain `bedrock_server.exe`) behind the loopback recording relay of `examples/raknet_capture`, driven by the gophertunnel/go-raknet bot; recorded with `tools/client-diff/raknet/capture.ps1` (see its README). Times are milliseconds since the relay started. Scenarios: `probe` (offline handshake cases), `baseline` (path MTU 1492, 16 s idle, client disconnect), `mtu-1400`, `mtu-1200`, `mtu-576`, `loss-5` (5% loss both ways), `reorder-5`, `kick`, `client-silence` (client black-holed after 8 s). Cited as `cap <scenario> <t ms>`. |
| **lens** (decompiled Mojang RakNet) | Constants and algorithms of both roles. Index `26.30` (macOS client build with the server code); Windows artifacts: client 1.26.33 / 1.26.50.26 / 1.26.60.21, BDS 1.26.40 / 1.26.50.25. Cited as `lens <function>:<lines>` (lines of the lens view of that function) with the functions listed below. |
| **Real client captures** `tests/captures/real-client/*.jsonl` | Mojang client behaviour on the wire: Minecraft 1.26.52 for Windows joined BDS 1.26.51 by hand through the same relay (`tools/client-diff/real-client/capture.ps1 -NoLaunch`; the relay advertises itself in the pong, see its README). Scenarios `baseline` (path MTU 1492), `mtu-1200`, `mtu-576`, `loss-5`. Cited as `rc <scenario> <t ms>`. The game was still connected when each recording stopped, so the client's disconnect (KA-5/KA-6 client side) rests on lens. |
| Jenkins RakNet 4.081 (BSD) | Cross-check only where lens and the captures agree, or to name the algorithm Mojang kept. |
| go-raknet | lunar-bedrock/go-raknet `6124f52657d4` (pinned by `tools/bench-compare/go/go.mod` and `tools/client-diff/gt-client/go.mod`); `file:line` references at that commit. The client in the BDS captures is go-raknet `87a6b7e`, the pin when they were recorded. |

lens functions (index 26.30 RVAs; `W4`/`W5` = BDS 1.26.40/1.26.50 Windows
builds):

| Short name | Function |
|---|---|
| `RUC` | `RakNet::RakPeer::RunUpdateCycle` 0xbc45c00 |
| `POP` | `ProcessOfflineNetworkPacket` 0xbc41410 |
| `ASA` | `RakNet::RakPeer::AssignSystemAddressToRemoteSystemList` 0xbc3fa30 (W4/W5 builds for the 1400 cap) |
| `OCR` | `RakNet::RakPeer::OnConnectionRequest` 0xbc3f2a0 |
| `RL.Update` | `RakNet::ReliabilityLayer::Update` 0xbc55e50 |
| `RL.Recv` | `RakNet::ReliabilityLayer::HandleSocketReceiveFromConnectedPlayer` 0xbc4fd30 |
| `RL.Send` | `RakNet::ReliabilityLayer::Send` 0xbc54e50 |
| `RL.SendACKs` | `RakNet::ReliabilityLayer::SendACKs` 0xbc58b00 |
| `UNL` | `UpdateNetworkLoop` 0xbc32a60 |
| `RNC.connect` / `RNC.ctor` | `RakNetConnector::connect` 0x5a50820 / `RakNetConnector::RakNetConnector` 0x5a4f1e0 |
| `Recv` | `RNS2_Berkley::RecvFromLoop` 0xbc27da0 |
| `RP.ctor` | `RakNet::RakPeer::RakPeer` 0xbc2e160 (W3/W6/W7 client and W4/W5 BDS constructors for `MAXIMUM_MTU_SIZE`) |

Source comments cite the same lens views by dump name, with the same line
numbers: `RN_006` = `POP`, `RP_113` = `RUC`, `RP_106` = `ASA`, `RP_105` =
`OCR`, `RL_010` = `RL.Update`, `RL_006` = `RL.Recv`, `RL_009` = `RL.Send`,
`RL_011` = `RL.SendACKs`, `RN_005` = `UNL`, `RNC_005` / `RNC_000` =
`RNC.connect` / `RNC.ctor`, `RP_001` = `RP.ctor`; `RP_029`, `RP_030` and
`RP_114` are `RakPeer::IsLoopbackAddress`, `RakPeer::SendBuffered` and
`RakPeer::OnRNS2Recv`; `W4_*`/`W5_*` are the BDS 1.26.40/1.26.50 Windows
builds of the named function.

### Status vocabulary

| Status | Meaning |
|---|---|
| `match` | Mojang, go-raknet and both policies agree. |
| `mojang` | Vanilla does what Mojang does (the row says how go-raknet differs, if it does); Optimized keeps its listed behaviour. |
| `shared` | Both policies deliberately deviate from Mojang: a RAKNET-01 safety bound or a tokio API rule. |
| `undecidable` | The evidence cannot decide the row; the reason is given and the current behaviour is kept. |

### Verification

```text
cargo test -p tokio-raknet                                   # unit, integration and capture-diff tests
cargo test -p tokio-raknet --test go_conformance -- --ignored --nocapture  # needs Go
```

`tests/capture_diff.rs` replays the BDS scenarios against a Vanilla listener
through the same relay and fault model and compares the server-side
features: the offline probe exactly; CRA layout, fragment sizes, flags of the
first split update, ping cadence, ACK delay, NAK and resend latency, silent
timeout and disconnect handling within 45 ms (relay latency plus a Windows
timer quantum). It also runs a Vanilla `RaknetStream` behind each real-client
capture's path MTU and compares the client side: probe sizes and 500 ms
cadence, ConnectionRequest and NewIncomingConnection reliability, ACK delay,
ping period and fragment sizes. `tests/go_conformance.rs` checks what go-raknet
`6124f52` and Mojang share (Request1 sizes and cadence, Request2,
NewIncomingConnection apart from its first system address, the
DisconnectNotification, datagram flags, the 10 ms ACK delay, NAKs of revealed
gaps) and pins each side's documented differences.

## 1. MTU discovery (client) and offline handshake

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| MTU-1 | — | Probe sizes `mtuSizes` {1492, 1200, 576}: 576 from `RP.ctor` (`defaultMTUSize`), 1492 = client `MAXIMUM_MTU_SIZE` (W3/W6/W7 ctor `"[MTU] Max MTU Size"`); the 1200 entry is not readable in lens. rc `mtu-576` 52477.6-56513.3 ms: 4×1492, 4×1200, then 576; rc `mtu-1200` 27490.7-29531.3 ms: 4×1492, then 1200. | `[1492, 1200, 576]`, `Dialer.MaxMTU` replaces the first (`dial.go:360-384`). | `[cap, 1200, 576]` filtered by `RaknetStreamConfig::mtu`. | `match` | `transport::handshake::tests::probe_sizes_follow_go_mtu_sizes_for`, `capture_diff::real_client_captures_match_vanilla_client` |
| MTU-2 | — | Bedrock calls `Connect(…, 12 attempts, 500 ms, timeout 0)` (`RNC.connect:147-150`): 13 Request1 at 0, 500 … 6000 ms, size index `min(n / 4, 2)` (4×, 4×, 5×), failure when the 14th would be due (6.5 s) (`RUC:953-1068`). Request1 keeps going until Reply2. rc `mtu-576`: probes 501-508 ms apart (the 10 ms update cycle adds up to one cycle); the failure after the 13th is not observable through a path that passes 576. | Mojang's ladder: 4 probes per size plus one more at the last size (13), 500 ms apart, probing until Reply2 (`mtu_probe.go:17-53`, `dial.go:386-416`); after the 13th it waits for the dial deadline (10 s for `Dial`). `87a6b7e`: 12 probes, stopped at Reply1. | Mojang schedule; `RaknetError` timeout at 6.5 s. go's 12 BDS-captured probes: `cap mtu-576` 32.7 … 4032.5 ms, 500 ms apart. | `mojang` | `transport::handshake::tests::vanilla_sends_thirteen_probes_then_fails_at_6500ms`, `…::vanilla_probing_continues_after_reply1_until_reply2`, `…::probes_keep_500ms_cadence_despite_stray_packets`, `…::late_reply_to_larger_probe_is_accepted`, `capture_diff::real_client_captures_match_vanilla_client` |
| MTU-3 | — | Request1 padded to `mtu - 28` for every family, protocol 11 (`RUC:953-1068`); rc `baseline` 84755.5 ms: 1464-byte Request1 for 1492. | Same (`internal/message/open_connection_request_1.go:21`). | Same. | `match` | `transport::handshake::tests::vanilla_request1_padding_ignores_address_family` |
| MTU-4 | — | A Reply1 with the magic gets one Request2 at once carrying its MTU as read, cookie only if the security byte is set (`POP:1253-1307,3065-3127`); rc `baseline`: Reply1 1400 at 84755.7 ms, Request2 1400 at 84756.6 ms. | Magic checked, a bad magic is ignored; OVH workaround (GUID 0 / MTU outside 400..=1500 → Request2 echoing it) kept; a Reply1 above `MaxMTU` ignored; another malformed Reply1 fails the dial; a repeated Reply1 restarts Request2 with its grant (`dial.go:419-473,533-546`). `87a6b7e`: no magic check. | Mojang: magic required, short replies dropped, every Reply1 (late or repeated) answered once. | `mojang` | `transport::handshake::tests::vanilla_every_reply1_gets_one_request2`, `…::vanilla_reply1_mtu_is_used_as_sent`, `…::vanilla_offline_replies_require_magic_and_full_fields` |
| MTU-5 | Wrong protocol → `0x19` with protocol 11 (cap `probe` 6096, 6606 ms). | `IncompatibleProtocolVersion` ends the attempt (Jenkins `RakPeer.cpp:5075-5126`). | `0x19` fails the dial; other failure IDs are ignored (`dial.go:465-470`). | Fails with `RaknetError::IncompatibleProtocolVersion`; other failure replies (0x11/0x12/0x14/0x17/0x1a) end the attempt too. | `mojang` | `transport::handshake::tests::incompatible_protocol_fails_discovery_immediately`, `…::vanilla_offline_failure_replies_end_the_attempt` |
| MTU-6 | — | Not decided: the error handling of Mojang's receive thread (`Recv`) was not compared, and no capture can provoke it on loopback. | 10 transient errors tolerated across both phases (`dial.go:428,522`); `6124f52` added EMSGSIZE and WSAENETRESET to ECONNREFUSED, ECONNRESET, ECONNABORTED, EHOSTUNREACH, ENETUNREACH (`udp_transient_error_windows.go:11-35`). | go's rule kept: `max_transient_errors` (default 10). | `undecidable` | `transport::handshake::tests::transient_receive_errors_are_tolerated_up_to_the_limit` |
| MTU-7 | — | EMSGSIZE on a probe skips to the next size block at once; other send errors keep the cadence (`RUC:1197-1221`). | Mojang: EMSGSIZE skips to the next size block at once; a write slower than 100 ms jumps to the last block (ends probing inside it); other write errors keep the cadence (`mtu_probe.go:36-48`). `87a6b7e`: write errors ignored, cadence kept. | Mojang; in the last block the count is kept (Mojang would rewind and resend every 10 ms forever); the ">100 ms send" rule is not reproduced. | `mojang` | `transport::handshake::tests::vanilla_probe_emsgsize_skips_to_next_size`, `…::vanilla_probe_send_errors_keep_the_cadence` |
| MTU-8 | Reply2 echoes the requested MTU unvalidated (cap `probe` 7626-16784 ms: 1492, 1400, 1200, 576, 400, 399, 300, 1500, 1600, 2000). | Reply2 accepted with GUID, address, MTU, security read; connection MTU `max(576, clamp(mtu, 400, 1492))` (`ASA:120-137`). | Reply2 only after a Reply1; MTU `clamp(mtu, 576, MaxMTU)` (`dial.go:549-559`), Mojang's clamp at the default 1492. `87a6b7e`: `0`/`>1492` → 1492, `<400` → 400. | Mojang clamp; no GUID/security checks; Reply2 accepted without a Reply1. | `mojang` | `transport::handshake::tests::vanilla_reply2_mtu_is_clamped_like_mojang`, `…::vanilla_reply2_is_accepted_without_reply1` |
| MTU-9 | — | No Request2 timer: Request2 answers each Reply1, and Request1 keeps probing (`POP:3065-3127`). | Request2 every 500 ms while Request1 keeps probing (`dial.go:565-597`). | Mojang. Optimized keeps the 500 ms Request2. | `mojang` | `transport::handshake::tests::vanilla_probing_continues_after_reply1_until_reply2`, `…::request2_is_retransmitted_every_500ms` (Optimized) |
| MTU-10 | `UDP_HEADER_SIZE` 28 for every family (CC `MTU - 28`, Reply1). | Same. | 28 (`conn.go:234-238`). | 28. | `match` | `session::policy::tests::fragment_and_datagram_budgets_follow_mojang_header_reserves` |
| MTU-11 | — | `recvfrom` buffer 1600 bytes (`Recv:94`). | 1492-byte buffer (`dial.go:304,422,516`); a longer datagram is truncated (Unix) or fails the read (Windows), which the connected reader logs and skips (`dial.go:327-333`). | Datagrams up to 1600 bytes handled, longer dropped; `WSAEMSGSIZE` ignored. | `mojang` | `transport::stream::tests::vanilla_client_handles_datagrams_up_to_1600_bytes`, `…::transient_receive_errors_do_not_close_the_client` |
| MTU-12 | — | No `SetDoNotFragment` around probes in the macOS build; the Windows probe path is not locatable in lens. | No DF bit. | No DF bit. | `undecidable` (Windows) | conformance `mtu_discovery_with_1200_byte_path` |
| MTU-13 | — | Request2/NIC carry the dialed address as stored (`POP:3065-3127`); rc: Request2 and NIC carry `127.0.0.1:39140` as joined. | IPv4-mapped IPv6 unmapped (`conn.go:782-792`). | Written as dialed. | `mojang` | `transport::handshake::tests::vanilla_server_address_is_written_as_dialed` |
| MTU-14 | — | Online phase dropped 10 s after the connection was created (`RUC:1576-1579`). | 10 s dial context (`dial.go:211-215`). | `connection_timeout` 10 s bounds the dial; sessions still in the online handshake close after 10 s. | `match` | `integration_handshake::test_stream_connect_timeout_is_enforced`, `session::manager::tick::tests::vanilla_handshake_times_out_after_ten_seconds` |

## 2. Listener offline handshake (BDS)

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla listener | Status | Test |
|---|---|---|---|---|---|---|
| LS-1 | Reply1 MTU = `min(len + 28, 1400)`, no lower bound, security 0, no cookie: cap `probe` 503-5593 ms (1500/1492/1464 → 1400, 1300, 1000, 576, 400, 300, 100). BDS `MAXIMUM_MTU_SIZE` 1400 (W4/W5 `RP.ctor`). | Clients use 1492 (`RP.ctor`). | Reply1 with cookie, MTU `min(len + 28, MaxMTU)` with no floor; a grant above 1200 is padded to its size, at most 500 padded replies per second (then 1200 unpadded) (`handler.go:104-129`, `listener.go:91-122`). `87a6b7e`: never padded. | BDS rule, cap `min(max_mtu, 1400)`. Optimized: cookie, `[400, max_mtu]`. | `mojang` | `transport::listener::offline::tests::vanilla_reply1_mtu_is_payload_plus_28_capped_at_1400`, `…::reply1_wire_per_policy`, `capture_diff::offline_handshake_matches_bds_probe` |
| LS-2 | `0x19` + protocol 11, sender keeps being answered (cap `probe` 6096, 6606 ms). | — | `0x19`, then the IP is blocked for 10 s (`handler.go:112-116`, `listener.go:308-310`). | BDS. Optimized also blocks the IP for 10 s. | `mojang` | `…::incompatible_protocol_reply_per_policy` |
| LS-3 | Request2 without cookie or Request1 answered; Reply2 echoes the MTU (cap `probe` 7626-17294 ms). | — | Cookie (current or previous 2 s salt) and a negative client GUID required (`handler.go:133-147`). | BDS; malformed offline packets ignored, not blocked. | `mojang` | `…::request2_without_request1_per_policy`, `…::vanilla_reply2_echoes_requested_mtu_and_session_mtu_is_clamped` |
| LS-4 | Session MTU `max(576, clamp(mtu, 400, 1400))` (`ASA:120-137`, W4/W5). | — | `max(min(mtu, MaxMTU), 400)` (`handler.go:150`). | BDS. Optimized `min(max_mtu, mtu)`. | `mojang` | `…::vanilla_reply2_echoes_requested_mtu_and_session_mtu_is_clamped` |
| LS-5 | Repeated Request2 from the same address/GUID → Reply2 again (cap `probe` 18314, 18824 ms); same GUID from another address → `0x12` (19844 ms); Request1 after connecting → Reply1 (19335 ms). | — | Offline packets from an address with a session go to that session and are ignored; no GUID conflict check (`listener.go:325-341`, `conn.go:664-672`). | BDS (until the ConnectionRequest; later conflicts get `0x12`). | `mojang` | `…::vanilla_repeated_request2_replies_again_until_connection_request`, `…::vanilla_guid_or_address_conflict_is_already_connected`, `capture_diff::offline_handshake_matches_bds_probe` |
| LS-6 | Same IP within 50 ms of the previous connection → `0x1a`, loopback exempt (`ASA:63-101`, on in `RP.ctor:1173`). | — | None. | BDS (loopback IPs exempt; BDS also exempts its own interface addresses). | `mojang` | `…::vanilla_same_ip_within_50ms_is_ip_recently_connected`, `…::optimized_has_no_same_ip_reconnect_limit` |
| LS-7 | Server full: Mojang's ShadowBanList may evict an existing peer (`POP:1563-1660`). | — | No connection limit; `0x14` is never sent. | `0x14` with magic and GUID; eviction not reproduced. | `shared` | `…::vanilla_refusals_carry_magic_and_server_guid` |
| LS-8 | Unconnected ping answered when allowed (cap `probe` 2 ms). | — | Answered (`handler.go:84-101`). | Answered. tokio's per-IP ping/Request1 rate limiter and pending-handshake caps stay (BDS has none). | `shared` | `capture_diff::offline_handshake_matches_bds_probe` (limiter off), `integration_handshake::ping_returns_listener_advertisement` |

## 3. Online handshake

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| HS-1 | — | `resetMyGUID` before every connect (`RNC.connect:57`); Bedrock installs `Crypto::Random::generateUUID64Bit` as the generator (`RNC.ctor`), whose body lens cannot show. rc: the four Request2 GUIDs (`0x82b4…`, `0xbf12…`, `0x8754…`, `0xb28e…`) all have the sign bit set and differ from the GUID of the game's server-list pings, which stays fixed across a run's pings (rc `baseline` 18 pings 3040-83097 ms, `0x8234…`, also negative). go-raknet's listener rejects non-negative client GUIDs because "vanilla clients always provide a negative ClientGUID" (`handler.go:143-146`). | Fresh random GUID per dial and per `Ping`, top two bits `10` (`client_guid.go:15-21`, `dial.go:160,249`). `87a6b7e`: process counter from a random negative seed. | Fresh random negative GUID per dial and per `transport::ping` (one ping per call, so no GUID to keep). | `mojang` | `transport::handshake::tests::vanilla_guids_are_random_negative_per_dial` |
| HS-2 | Request2 layout: magic, [cookie, 0], address, MTU, GUID (`POP:1348-1460`). | Same (`POP:3065-3127`). | Same (`internal/message/open_connection_request_2.go`). | Same. | `match` | `transport::handshake::tests::request2_bytes_match_go_layout`; conformance (masked bytes) |
| HS-3 | System addresses: IPv4 inverted; IPv6 = tag 6 + raw `sockaddr_in6` (family 23 LE) — cap `baseline` CRA (anonymised). | Same library. | Standard layout: tag 6, family 23 LE, port, address at offset 9 (`internal/message/addr.go:66-99`). `87a6b7e`: lunar layout (family 23 BE at offset 3). | Standard layout for Request2, CRA and NIC; lunar layout removed. | `mojang` | `transport::handshake::tests::vanilla_request2_uses_standard_ipv6_layout`, `session::manager::tick::tests::vanilla_new_incoming_connection_bytes_and_connect_pings` |
| HS-4 | — | ConnectionRequest IMMEDIATE, RELIABLE, ch 0, security byte 0 (`POP:2647`); rc `baseline` 84758.8 ms: reliability 2, the Request2 GUID, security 0 (same in all four captures). | ReliableOrdered, security byte 0 (`dial.go:274`, `conn.go:623-627`). | RELIABLE, insecure. Optimized RELIABLE with Reply2's flag. | `mojang` | `session::manager::tick::tests::connection_request_is_reliable_and_vanilla_is_insecure`, `capture_diff::real_client_captures_match_vanilla_client`; conformance |
| HS-5 | u64 ms timestamps (`GetTime`). | Same. | ms since package init (`conn.go:1146-1152`). | ms since the first RakNet timestamp. | `shared` | `protocol::types::time::tests::raknet_time_roundtrip_and_conversion` |
| HS-6 | — | CRA accepted in REQUESTED/HANDLING modes; up to 20 addresses (`RUC:2028-2737`). | 20 addresses, tag 0 = IPv4 (`internal/message/addr.go:89-93`); CRA handled only before connecting (`handler.go:272-291`). | 20-address decoding, CRA handled only before connecting. | `match` | `protocol::packet::open_connection::tests::connection_request_accepted_with_twenty_addresses_keeps_timestamps`, `protocol::types::addr::tests::tag_zero_decodes_as_ipv4`, `session::manager::tick::tests::duplicate_cra_does_not_resend_nic_or_reopen_closing_session` |
| HS-7 | — | NIC: server address, 20 system addresses, `CRA.PongTime`, now; IMMEDIATE ReliableOrdered (`RUC:2633-2728`); rc `baseline` 84786.3 ms: ReliableOrdered, 5 interface addresses (anonymised) then 15 unassigned `04 ffffffff 0000`, packed with a ping and a game packet. | Same layout and reliability; system addresses: the local socket address, then 19 unassigned (`04 ffffffff 0000`) (`handler.go:282-287`, `internal/message/addr.go:11-20,66-71`). `87a6b7e`: lunar IPv6 layout, all 20 unassigned. | Same; all 20 addresses unassigned (`04 ffffffff 0000`, as the client writes its unused slots). | `mojang` | `session::manager::tick::tests::vanilla_new_incoming_connection_bytes_and_connect_pings`, `capture_diff::real_client_captures_match_vanilla_client`; conformance (masked bytes) |
| HS-8 | Two unreliable pings on NIC: cap `baseline` 50.0, 50.0 ms. | `PingInternal(UNRELIABLE)` after the NIC plus the first update's occasional ping (`RUC:2729-2737,1581-1597`); rc `baseline` 84786.3, 84786.5 ms. | Ticker from session creation, ping every 500 ms in every state but closing (`conn.go:240-292`). | Mojang: pings only when connected, two at connect. | `mojang` | `session::manager::tick::tests::vanilla_new_incoming_connection_bytes_and_connect_pings`, `…::vanilla_server_pings_twice_on_connect_then_every_five_seconds` |
| HS-9 | `0x04` DetectLostConnections ignored (`RUC:2020-2026`). | Same. | Answered with a ReliableOrdered ping (`handler.go:200-202,262-264`). | Ignored. | `mojang` | `session::manager::tick::tests::detect_lost_connections_is_ignored` |
| HS-10 | — | Failure IDs during the handshake end it (MTU-5); offline IDs after connecting are handled as offline traffic. | Ignored after dialing (`conn.go:664-672`). | Failure IDs after dialing still end the connection with a typed error. | `shared` | `transport::stream::tests::connection_failure_packet_is_reported` |
| HS-11 | CRA ReliableOrdered, index 0, 20 addresses: cap `baseline` (CRA `0x10`, rel 3, `accepted (3, 20, 0)`); `OCR:52-200`. | — | ReliableOrdered; system addresses `127.0.0.1:0` (`::1` on IPv6) then 19 × `0.0.0.0:0` (`handler.go:210-224`, `internal/message/addr.go:22-41`). `87a6b7e`: 20 zero addresses. | BDS layout (20 unassigned addresses; BDS lists its interfaces). Optimized: 10 addresses, index 47. | `mojang` | `session::manager::tick::tests::vanilla_connection_request_accepted_matches_bds_layout`, `…::connection_request_accepted_is_reliable_and_state_preserving`, `capture_diff::baseline_session_matches_bds` |

## 4. Reliability and ordering

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| REL-1 | Game batches ReliableOrdered ch 0 (cap `baseline` `0xfe` rel 3); pings/pongs Unreliable; CRA, NIC and `0x15` ReliableOrdered. | Same; ConnectionRequest RELIABLE; reliable keepalive pings (KA-2). | Everything ReliableOrdered except the pong (`conn.go:623-634`, `handler.go:302`). | Mojang set. | `mojang` | `session::manager::tick::tests::connection_request_is_reliable_and_vanilla_is_insecure`, `…::vanilla_server_pings_twice_on_connect_then_every_five_seconds`, `…::vanilla_close_sends_bare_disconnect_and_closes_when_acked` |
| REL-2 | Priority heap, weights 0/3/10/27 (`RL.Send`); message numbers assigned at send (`RL.Update:1220-1255`). | Same. | FIFO, control queue (pings, disconnect) before application data; message numbers assigned at write (`conn.go:441-488,1007-1047`). | Priority heap; message numbers at send. | `mojang` | `session::vanilla::tests::message_numbers_follow_send_order_not_queue_order` |
| REL-3 | 32 ordering channels (`RL.Recv:501`). | Same. | Channel 0 only. | 16 channels (`MAXIMUM_ORDERING_CHANNELS`); Bedrock uses channel 0. | `shared` | `session::ordering_channels::tests::duplicate_packet_ignored` |
| REL-4 | Sequenced reliabilities supported. | Same. | Unreachable. | Supported. | `match` | `session::outbound::tests::sequenced_packets_get_sequence_index` |
| REL-5 | Split messages sent reliably (0→2, 1→4, 5→6) (`RL.Send:169-176`). | Same. | Keeps the reliability (`conn.go:441-485`). | Upgraded (both policies). | `mojang` | `session::vanilla::tests::split_fragments_are_mtu_minus_60_and_reliable` |
| REL-6 | Per-channel ordering heap (≤ 4096), message-number de-duplication (hole limit 32768). | Same. | One ordered queue; a repeated order index is dropped, no message-number de-duplication (`packet_queue.go:17-30`, `conn.go:738-758`). `87a6b7e`: no de-duplication. | tokio's bounded per-channel queues and de-duplication. | `shared` | `session::ordering_channels::tests::duplicate_packet_ignored`, `session::reliable_tracker::tests::drops_duplicates` |
| REL-7 | Datagram gap > 50000 rejects the datagram (`RL.Recv:372-411`). | Same. | Mojang 50000 rule: a datagram further ahead is dropped unacknowledged (`datagram_window.go:3-45`); an ordered window > 2048 closes the connection (`conn.go:747-751`). `87a6b7e`: datagram window > 2048 closed listener connections. | Mojang 50000 rule for datagrams; tokio's 2048 message window stays. | `mojang` | `transport::stream::tests::vanilla_client_accepts_gaps_up_to_50000_datagrams`, `session::vanilla::tests::gaps_are_naked_immediately_and_duplicates_are_reacked` |
| REL-8 | Every data datagram ACKed, duplicates included; messages de-duplicated (`RL.Recv:415-417`). | Same. | Every datagram at or behind the expected number is handled and ACKed; repeats are dropped only by ordered delivery (`datagram_window.go:12-16`, `conn.go:678-713`). `87a6b7e`: duplicates neither handled nor re-ACKed. | Mojang. | `mojang` | `session::vanilla::tests::gaps_are_naked_immediately_and_duplicates_are_reacked` |
| REL-9 | Splits: count ≤ 2048, ≤ 256 concurrent, expiry after `timeoutTime` (`RL.Recv:1373`, `RL.Update:228-397`). | Same. | 8192 parts, 256 concurrent IDs, 16384 fragments / 16 MiB retained, reassemblies idle for 10 s evicted (`conn.go:44-59,799-867`). `87a6b7e`: 512 parts, 17 IDs, no expiry. | 512 parts, 16 IDs, 30 s (RAKNET-01). | `shared` | `session::split_assembler::tests::rejects_when_buffer_full` |
| REL-10 | Split ID u16 per split message. | Same. | Same (`conn.go:448-451`). | Same. | `match` | `session::vanilla::tests::split_ids_advance_only_for_split_messages` |
| REL-11 | Fragment payload `MTU - 60` for every reliability (`RL.Send:149`): cap `baseline` 1340 at 1400, `mtu-1200` 1140, `mtu-576` 516. | Same: rc `baseline`/`loss-5` 1340, `mtu-1200` 1140, `mtu-576` 516 (the client's ~4 KB login split). | `MTU - 28 - 14 - 10` (1348 at 1400) (`packet.go:194-219`). | `MTU - 60`. | `mojang` | `session::vanilla::tests::split_fragments_are_mtu_minus_60_and_reliable`, `capture_diff::baseline_session_matches_bds`, `capture_diff::real_client_captures_match_vanilla_client` |
| REL-12 | At most 512 reliable messages in flight (`RL.Update:924`); no outgoing byte cap. | Same. | Mojang's 512 reliable datagrams awaiting ACK; 16 MiB send queue, `Write` blocks at the limit (`conn.go:61-75,385-426,995-1005`). `87a6b7e`: unbounded. | 512 in flight; RAKNET-01 caps kept: 2048 queued frames and `max_queued_reliable_bytes` counting reliable bytes until ACKed. | `shared` | `session::vanilla::tests::new_sends_stop_at_512_messages_in_flight`, `session::manager::tick::tests::vanilla_queue_limit_counts_unacked_reliable_bytes` |

## 5. Datagram building

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| DG-1 | Messages packed while `size ≤ (MTU - 28) * 8 - 72` bits, resends first (`RL.Update:1033`): cap `baseline` 13 frames in one datagram, max datagram 1364 at MTU 1400. | Same: rc `baseline` 84786.3 ms NIC, ping and game packet in one datagram; game packets queued one at a time leave one per datagram. | One frame per datagram (`conn.go:1077-1091`). | Mojang packing (`MTU - 37` bytes of messages). | `mojang` | `session::vanilla::tests::packs_messages_and_flags_continuous_send_after_the_first_datagram`, `capture_diff::baseline_session_matches_bds` |
| DG-2 | 0x80, plus 0x04 in slow start, plus 0x08 for a continuous send (`RL.Update:459-465,1306`): first split update cap `baseline` [0x84, 0x8c]; 0x80/0x88 after a backoff (cap `baseline` flags {80, 84, 88, 8c}). | Same: rc login split [0x84, 0x8c] in all four captures; only 0x84/0x8c seen (the client's window never backed off). | Mojang flags: 0x80, plus 0x04 in slow start, plus 0x08 for a continuous send (`conn.go:1078-1087`, `congestion.go:32-35`). `87a6b7e`: always 0x84. | Mojang flags. | `mojang` | `session::vanilla::tests::packs_messages_and_flags_continuous_send_after_the_first_datagram`, `…::needs_bas_flag_clears_once_the_window_exceeds_the_threshold`, `transport::stream::tests::vanilla_client_delivers_datagrams_without_the_slow_start_bit` |
| DG-3 | Fresh datagram number per datagram, resends included. | Same. | Same (`conn.go:1089`). | Same. | `match` | `session::vanilla::tests::nak_resends_messages_repacked_with_new_data` |
| DG-4 | Message header layout (`RL.Update:1451-1753`). | Same. | Same (`packet.go`). | Same. | `match` | `session::outbound::tests::golden_datagram_bytes_match_expectation` |

## 6. ACK/NAK

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| ACK-1 | ACKs at once until the first RTT sample, then when the oldest unacknowledged datagram is 10 ms old (SYN) on the 10 ms update, at once when a split completes (`RL.Update:407-412`, `RL.Recv:1438`): cap `baseline` ACKs 0.1-0.4 ms after each datagram up to 111 ms, then 13.7-19.9 ms after the datagram. | Same: rc ACK delay after the first RTT sample p50 19.4 ms (`baseline`), 19.5 (`mtu-1200`, `mtu-576`), 19.9 (`loss-5`). | At once until the peer's first ACK, then 10 ms after the oldest unacknowledged datagram (`conn.go:66-69,299-326,690-703`); no immediate ACK on split completion. `87a6b7e`: 100 ms ticker. | Mojang; the deadline is serviced by the transport timer, so ACKs leave 10 ms (plus timer slack) after the oldest datagram. | `mojang` | `session::vanilla::tests::acks_are_immediate_until_rtt_is_known_then_wait_10ms`, `…::split_completion_acks_at_once`, `transport::stream::tests::vanilla_client_acks_10ms_after_the_oldest_datagram`, `capture_diff::baseline_session_matches_bds`, `capture_diff::real_client_captures_match_vanilla_client` |
| ACK-2 | RangeList: sorted, merged; a range added while `16 + written + 65 ≤ (MTU - 28) * 8 - 72` bits (`RL.SendACKs`; record encoding assumed Jenkins'). | Same. | go writer, `mtu - 35` byte budget (`acknowledge.go:29-73`). | Mojang budget; 8192 sequences per datagram (RAKNET-01). | `mojang` | `session::vanilla::tests::ack_chunks_merge_and_respect_the_range_list_budget` |
| ACK-3 | A forward gap NAKs each skipped number (≤ 1000) in the next update: cap `loss-5` NAK 0.5 ms after the revealing datagram. | Same: rc NAKs 0.0-0.3 ms after the revealing datagram (`loss-5`; `baseline` 85525.6 ms, gaps in BDS's 222-datagram burst that the relay forwarded). | Mojang: a forward gap NAKs the skipped numbers (the latest ≤ 1000) at once (`datagram_window.go:28-45`, `conn.go:705-711`). `87a6b7e`: gaps NAKed once older than `1.5 × rtt`. | Mojang. | `mojang` | `session::vanilla::tests::gaps_are_naked_immediately_and_duplicates_are_reacked`, `capture_diff::loss_recovery_matches_bds` |
| ACK-4 | One NAK datagram per update, same RangeList budget. | Same. | go writer, one NAK per revealing datagram (`conn.go:876-901`). | Mojang. | `mojang` | `session::vanilla::tests::ack_chunks_merge_and_respect_the_range_list_budget` |
| ACK-5 | RTT per ACKed datagram (history entry), EWMA gain 0.05 (`RL.Recv:2432-2462`). | Same. | Mojang EWMA, gain 0.05, per ACKed datagram (`resend_map.go:84-97`). `87a6b7e`: mean of samples within 5 s. | Mojang EWMA. | `mojang` | `session::policy::tests::rto_is_two_rtt_plus_four_deviation_plus_30ms_capped_at_two_seconds` |
| ACK-6 | A NAK makes the datagram's messages due; the next update resends them packed with new data: cap `loss-5` resends 0.2-0.3 ms after the client's NAK. | Same: rc `loss-5` client resends p50 0.2 ms after BDS's NAK. | A NAK makes the datagram due; the next update resends its single frame in a new datagram (`conn.go:931-993`). `87a6b7e`: whole datagrams resent at once. | Mojang message-level resend. | `mojang` | `session::vanilla::tests::nak_resends_messages_repacked_with_new_data`, `capture_diff::loss_recovery_matches_bds` |
| ACK-7 | Inbound ACK/NAK: ≤ 1024 ranges, ≤ 16384 numbers, max ≠ 0xFFFFFF (`RL.Recv:239-353`). | Same. | 8192-sequence rule, unknown tags stop parsing (`acknowledge.go:93-127`). | tokio's 8192 (+1) bound, wrapped ranges and unknown tags rejected (RAKNET-01). | `shared` | `protocol::ack::tests::rejects_unknown_record_type`, `…::accepts_go_range_boundary_of_8193_sequences`, `…::rejects_overwide_inbound_range` |

## 7. Congestion control and resend

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| CC-1 | `CCRakNetSlidingWindow`: window from `MTU - 28`, slow start +1 MTU per ACK while continuously sending, congestion avoidance +MTU²/cwnd on **every** ACK, a NAK halves the threshold once per block (Mojang latch), a resend collapses the window (`RL.Recv:274-281,2463-2484`, `RL.Update:841-856`): cap `baseline` fragment bursts of 2, 4, 8, 16, 32, 62 datagrams at 259, 320, 421, 520, 620, 720 ms, one per round of go's 100 ms ACKs. | Same library. | Mojang's window, counting encapsulated bytes without the 4-byte datagram header (`congestion.go:5-95`, `conn.go:906-993`). `87a6b7e`: no window. | Mojang window. | `mojang` | `session::policy::tests::slow_start_grows_one_mtu_per_ack_only_while_continuously_sending`, `…::congestion_avoidance_grows_on_every_ack_not_once_per_block`, `…::nak_latches_the_block_and_suppresses_resend_collapse`, `session::vanilla::tests::slow_start_doubles_the_burst_per_acked_round` |
| CC-2 | Resend list scanned from its head each update; RTO `min(2·RTT + 4·dev + 30 ms, 2 s)`, 2 s before the first sample, no backoff (`RL.Update:804-919,857-867`): cap `client-silence` resends every 218-238 ms to the silent client. | Same: rc `loss-5` timeout resends 60-109 ms (median 91 ms) after the original at an ACK delay of ~20 ms. | Mojang RTO, no backoff (`resend_map.go:109-114`); due records resent on the next update (`conn.go:328-369`). `87a6b7e`: every third 100 ms tick, `1.5 × rtt`. | Mojang. | `mojang` | `session::vanilla::tests::timeout_resends_repeat_every_rto_without_backoff`, `capture_diff::silent_client_is_dropped_after_ten_seconds_like_bds` |
| CC-3 | RTT EWMA (ACK-5). | Same. | Mojang EWMA (ACK-5). `87a6b7e`: mean of recent samples. | Mojang. | `mojang` | `session::policy::tests::rto_is_two_rtt_plus_four_deviation_plus_30ms_capped_at_two_seconds` |
| CC-4 | Update every 10 ms (`WaitOnEvent(10)`), woken by received datagrams and IMMEDIATE sends; MEDIUM game packets wait for the next update (`UNL:16-29`). | Same. | Update on every received datagram, ACK/NAK, write and ACK timer, plus a 100 ms ticker; writes leave at once when the window allows (`conn.go:240-297,1066-1073`). `87a6b7e`: 100 ms ticker. | 10 ms maintenance (client and listener), an update on every received datagram and IMMEDIATE send; other priorities wait for the tick. | `mojang` | `capture_diff::baseline_session_matches_bds`, `session::manager::tick::tests::vanilla_server_pings_twice_on_connect_then_every_five_seconds` |
| CC-5 | No pacing beyond the window and the 512 in-flight cap; `maxOutgoingBPS` 0. | Same. | None beyond the window and the 512-datagram resend buffer (`conn.go:995-1005`). | Same. | `match` | `session::vanilla::tests::new_sends_stop_at_512_messages_in_flight` |

## 8. Keepalive, timeouts and disconnect

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| KA-1 | Occasional ping: Unreliable, every 5 s while connected (`nextPingTime < now`, Bedrock enables it in `RNC.ctor`): cap `baseline` 50, 50, 5069, 10079, 15100 ms. | Same: rc `baseline` 84786, 84787, 89789, 94797, 99799 ms (Unreliable; intervals p50 5002-5010 ms in all four). | ReliableOrdered every 500 ms (`conn.go:281-282`). | Mojang. | `mojang` | `session::manager::tick::tests::vanilla_server_pings_twice_on_connect_then_every_five_seconds`, `capture_diff::baseline_session_matches_bds`, `capture_diff::real_client_captures_match_vanilla_client` |
| KA-2 | Dead connection: messages unacknowledged and no datagram for `timeoutTime` (10 s) → dropped silently: cap `client-silence` last server datagram 17923.9 ms, 9999.7 ms after the client's last (7924.2 ms), no `0x15`. Reliable ping after `timeoutTime / 2` with nothing unacknowledged (`RUC:1528-1554`). Activity = every connected datagram ≥ 3 bytes (`RL.Recv:143-152`). | Same library. | Graceful close after `5 s + 2·rtt` idle (rtt 50 ms before the first sample); offline IDs refresh activity (`conn.go:283-291,652-656`, `resend_map.go:100-105`). | Mojang (fixed 10 s; `session_timeout` applies to Optimized). | `mojang` | `session::manager::tick::tests::vanilla_dead_connection_closes_silently_after_ten_seconds`, `…::vanilla_reliable_keepalive_after_five_idle_seconds`, `…::vanilla_activity_counts_connected_datagrams_only`, `transport::listener::online::tests::vanilla_offline_packets_from_a_session_peer_do_not_refresh_activity`, `capture_diff::silent_client_is_dropped_after_ten_seconds_like_bds` |
| KA-3 | Pongs feed ping statistics only. | Same. | Never feed RTT (`handler.go:307-318`). | Never feed RTT. | `match` | `session::manager::tick::tests::pong_never_affects_rtt` |
| KA-4 | Ping answered by an Unreliable pong (cap `baseline` client pongs `0x03` rel 0). | Same (`RUC:1922-1999`). | Same (`handler.go:295-303`). | Same. | `match` | `session::manager::tick::tests::connected_ping_is_answered_with_unreliable_pong` |
| KA-5 | `CloseConnection(…, true, 0, LOW)`: bare `0x15` ReliableOrdered queued at once; the connection closes when nothing is queued or unacknowledged, or at the dead-connection timeout (`RUC:1564-1567`). | Same library. | Waits until queued and unacknowledged data drains, then a bare `0x15` ReliableOrdered, resent until ACKed; closes when it is ACKed or 5 s after `Close` (`conn.go:262-279,515-523,582-593`). A go listener never ACKs it (KA-6), so a go client resends it until the 5 s limit. `87a6b7e`: `0x15` never resent. | Mojang. | `mojang` | `session::manager::tick::tests::vanilla_close_sends_bare_disconnect_and_closes_when_acked`, `transport::stream::tests::vanilla_client_drop_sends_disconnect_and_closes_when_acked` |
| KA-6 | A received `0x15` is ACKed, never echoed; the connection closes once the ACK is out (`DISCONNECT_ON_NO_ACK`, `RUC:2008-2018`): cap `baseline` client `0x15` at 17920.7 ms, server ACK at 17937.3 ms, nothing else. | Same library. | Echoes `0x15` (unless already sent), flushes it and closes without ACKing the received one (`handler.go:197-199,259-261`, `conn.go:532-570`). | Mojang. | `mojang` | `session::manager::tick::tests::vanilla_remote_disconnect_is_acked_not_echoed`, `capture_diff::baseline_session_matches_bds` |
| KA-7 | Not decided (as MTU-6). | Same. | Logged, connection continues (`dial.go:327-333`, `listener.go:298-304`). | Transient set and `WSAEMSGSIZE` ignored; other receive errors end the connection. | `undecidable` | `transport::stream::tests::transient_receive_errors_do_not_close_the_client` |
| KA-8 | Invalid datagrams dropped, connection kept (`RL.Recv:199-209`). | Same. | Client: logged, connection continues (`dial.go:327-333`). Listener: the session closes and the IP is blocked for 10 s (`listener.go:308-310,336-339`). | Client and listener drop the datagram and keep the connection; unknown offline IDs ignored. Optimized reports/blocks. | `mojang` | `transport::stream::tests::vanilla_client_survives_malformed_datagram`, `transport::listener::online::tests::vanilla_listener_ignores_invalid_datagrams_and_unknown_offline_ids` |
| KA-9 | — | Application-level queue; no wire behaviour to compare. | 4096 messages (`conn.go:205`). | 4096 messages, overflow disconnects. | `shared` | `integration_handshake::client_buffers_4096_unread_messages`, `integration_handshake::full_client_receive_queue_notifies_server_of_disconnect` |

## 9. Other

| ID | BDS (Mojang) | Client (Mojang) | go-raknet | Vanilla | Status | Test |
|---|---|---|---|---|---|---|
| OT-1 | Protocol 11; MTU 400 floor / 576 default / 1400 max (W4/W5). | Protocol 11; 400 / 576 / 1492. | 11; 400/1492, grants above 1200 only after a padded Reply1 (`conn.go:21-38`). | Client 400/1492, Vanilla listener 1400 max. | `mojang` | `protocol::constants::tests::protocol_version_matches_expected`, `transport::listener::offline::tests::vanilla_reply1_mtu_is_payload_plus_28_capped_at_1400` |
| OT-2 | Offline messages identified by the magic. | Same (Jenkins `RakPeer.cpp:4604-4618`). | Magic checked on Reply1/Reply2 only (`internal/message/offline_magic.go:12-21`). `87a6b7e`: never checked. | Magic required. | `mojang` | `transport::handshake::tests::vanilla_offline_replies_require_magic_and_full_fields` |
| OT-3 | Unconnected ping answered with the advertisement. | — | `Ping`: 33-byte request with a fresh random GUID, one read (`dial.go:153-182`). | `transport::ping` with a fresh random GUID. | `match` | `integration_handshake::ping_returns_listener_advertisement`, `protocol::packet::unconnected::tests::unconnected_ping_carries_client_guid` |
| OT-4 | Pong: time, GUID, magic, length-prefixed data. | — | Same (`internal/message/unconnected_pong.go`). | Same. | `match` | `protocol::packet::unconnected::tests::unconnected_pong_roundtrip` |
| OT-5 | Dispatch: ACK bit, then NAK, then data (0x20 inside an ACK = B/AS float, skipped). | Same. | Same order (`conn.go:664-672`). | Same order. | `match` | `protocol::datagram::tests::rejects_data_without_datagram_flag` |
| OT-6 | — | Datagrams from other addresses are other systems. | Connected socket (`dial.go:191-204`). | Filters `from == server`. | `match` | `transport::stream::tests::client_acks_a_quiet_server_without_application_or_socket_traffic` |
| OT-7 | `SO_RCVBUF` 256 KiB, `SO_SNDBUF` 16 KiB (macOS build; Windows not checked). | Same (macOS build). rc `baseline` 85525.6 ms: the Windows client NAKed datagram numbers 199-230 during BDS's 222-fragment (~300 KB) burst although the relay dropped none, consistent with a receive buffer near 256 KiB [inference]. | OS defaults. | Configurable, 1 MiB receive default (both policies). | `shared` | `transport::tests::configured_socket_buffers_are_applied` |

## Summary

77 rows (the 69 rows of the former go-raknet specification plus listener
rows LS-1..8): 46 `mojang`, 17 `match`, 11 `shared`, 3 `undecidable`
(MTU-6 and KA-7, receive-error handling; MTU-12, DF bit on Windows). The
client column combines lens with the real-client captures, which agree with
lens and with Vanilla `RaknetStream` on every row they show (probe sizes and
cadence, Request2, ConnectionRequest, NIC, pings, ACK/NAK timing, resends,
fragment sizes, flags); the client's disconnect (KA-5/KA-6) and the 6.5 s
discovery failure (MTU-2) were not captured and rest on lens.
