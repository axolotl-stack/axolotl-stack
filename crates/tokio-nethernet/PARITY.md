# NetherNet parity: real Bedrock client vs BDS vs go-nethernet vs tokio-nethernet

What the real Minecraft Bedrock client does on NetherNet, and whether
tokio-nethernet does the same. "Vanilla" means the client's behaviour. Where
it differs from go-nethernet in a way a peer can observe, tokio-nethernet
follows the client.

## Sources

| Column | Source |
|---|---|
| Client | lens binary analysis. **W** = Windows client 1.26.50.26 preview (catalog artifact 6; addresses are RVAs). **R** = retail Windows client 1.26.33 (artifact 3). **M** = symbolized macOS 26.30 build (named `NetherNet::*` owners). The installed client is 1.26.52 (hotfix of 26.51, same protocol). Constants first read in M were cross-checked in W. |
| BDS | BDS 1.26.51 run locally with `transport=nethernet` and `enable-lan-visibility=true` (2026-10-23), probed over loopback with `examples/real_client_probe.rs` and `curl` ("observed"), plus lens BDS 1.26.40 (artifact 4) and 26.50 (artifact 5). BDS links the same NetherNet library as the client (`handheld/src-deps/NetherNet/src/Lan/LanThreadManager.cpp`, identical `ESessionError` names); "shared" means the row's client code is present in BDS. With the default `transport=raknet`, BDS 1.26.51 logs "Players will not be able to connect to your game without NetherNet". |
| go | [df-mc/go-nethernet `215e464`](https://github.com/df-mc/go-nethernet/tree/215e46422b5831d6ddc6146003617dc3f1ff7c26) (pion). |
| tokio | This crate: behaviour at `1527972` (base of this work) → now. |

The client NetherNet stack is libwebrtc (statically linked: dcSCTP, OpenSSL
1.1.1n DTLS 1.2) driven by `NetherNet::SimpleNetworkInterfaceImpl`,
`NetworkSessionManager`, `ClientNegotiator`/`ServerNegotiator` (+`NoTrickleIce`
variants), `PacketQueue` and `LanThreadManager`. `[INFERENCE]` marks libwebrtc
defaults that the binary relies on but that were not read as constants.

| Verdict | Meaning |
|---|---|
| `match` | tokio already behaved like the client. |
| `fixed` | tokio differed in a way a vanilla peer can observe; now matches, with the linked regression test (each fails on `1527972`). |
| `cosmetic` | Differs, but no vanilla peer can observe it, or the difference is a deliberate local bound. Documented, unchanged. |
| `n/a` | No counterpart in tokio. |

## Signaling

| # | Behaviour | Client | BDS 1.26.51 | go | tokio | Verdict | Test |
|---|---|---|---|---|---|---|---|
| S1 | Signal text | `TYPE <u64 decimal> <data>`; `CONNECTREQUEST`, `CONNECTRESPONSE`, `CANDIDATEADD`, `CONNECTERROR`; error data `std::to_string(int)` (W 0xe23110) | observed: same | same | same | match | `signaling::tests` |
| S2 | Parse rules | split on the first two spaces (both required); id is a decimal prefix (`12x` = 12), ≥ 1 digit, overflow dropped; data may be empty (W 0xe246d0) | shared | same | same | match | `signaling::tests` |
| S3 | `CONNECTERROR` code | signed i32 decimal prefix, trailing text ignored | shared | same | same | match | `signaling::tests` |
| S4 | Unknown type / malformed signal | dropped silently (debug log) | shared | dropped | dropped | match | `signaling::tests` |
| S5 | Error code numbering | go's 0..37 wherever the client uses a constant: 4, 5 ICE, 10-13, 14 no answer, 24, 26, 36, 37 identity not allowed | observed: 37 | 0..37 | 0..37 | match | `signaling::tests` |
| S6 | Host rejects a client identity | replies `CONNECTERROR <id> 37` (W 0xe51ce0, "application declined the session") | observed: 37 (HTTP body) | 37 | sent nothing (the dialer waited for its timeout) → 37 | **fixed** | `listener::tests::rejected_client_identity_replies_identity_not_allowed` |
| S7 | Received `CONNECTERROR` | dialing session closes with the received code; hosting session ignores it | shared | dialer aborts, listener closes | dialer aborts, listener closes the negotiation | cosmetic | — |
| S8 | Other codes sent | only 37; every other failure closes locally | shared | many | 14, 15, 16, 17, 35 | cosmetic: a vanilla client turns each into its disconnect reason | — |
| S9 | Identity on LAN | n/a (client host app callback) | observed: LAN answer has no `a=identity`; identityless LAN offer accepted; identityless HTTP offer rejected with 37 | identity required by default (`AllowAnonymous` false) | LAN required identity both ways (a tokio LAN dialer rejected BDS's answer) → not required on LAN, present identities still verified | **fixed** | `discovery_test::test_discovery_connect_flow` |
| T1 | Dialer deadline | 10 s from `CONNECTREQUEST` to `CONNECTRESPONSE` (config +0x9b8 = 10, flighting `mc-nn-timeout`), checked by a 5 s sweep, so 10-15 s; then only ICE failure | shared | 15 s overall | 15 s overall | cosmetic: same window | — |
| T2 | Host deadline | none: a host session ends only on ICE failure (~30 s) | shared | 5 s accept | 5 s → 15 s (offer handling, then data channels) | **fixed** (a bound is kept) | `listener::tests::slow_answer_delivery_still_connects` |
| N1 | Trickle on LAN host | always non-trickle: candidates inline in `CONNECTRESPONSE`, no `CANDIDATEADD` (W 0xe246d0 `noTrickle = channel != 0 \|\| flag`) | observed: 4 inline candidates, no `CANDIDATEADD` | per signaling | trickled → inline (`Signaling::disable_trickle_ice_for_answer`) | **fixed** | `discovery_test::lan_listener_answers_with_inline_candidates` |
| N2 | Trickle elsewhere | dialers and Xbox/HTTP hosts follow `DisableTrickleIce`; flighting `mc-disable-trickle-ice` defaults to `"W10"` (consumer not located) | shared | per signaling | per signaling (offers trickle on LAN) | match | — |
| N3 | Trickle order | request/response at SetLocalDescription, then each candidate at once; remote candidates buffered (unbounded) until both descriptions exist | shared | ≤ 32 buffered | dialer buffers ≤ 32 before the answer; listener drops candidates for unknown sessions | cosmetic (bounded locally) | — |
| N4 | Candidate text | libwebrtc `candidate:<fnd> 1 udp <prio> <ip> <port> typ host generation 0 [ufrag U] [network-id N] [network-cost C]`, no `a=` | observed: `... typ host generation 0 network-id 1 network-cost 10`, global IPv6 included | `generation 0 ufrag <u> network-id <i> network-cost 0` | as go | cosmetic: libwebrtc parses both | `signaling::tests` |
| N5 | Invalid remote candidate | skipped (`CreateIceCandidate` fails), session continues | shared | closes | failed the dial / closed the connection → skipped; 128-candidate budget kept | **fixed** | `dialer::tests::invalid_trickled_candidate_is_ignored` |
| N6 | End of candidates | never signalled | shared | none | none | match | — |
| I1 | `a=identity` format | `{"idp":{domain,protocol},"assertion":A}` standard base64 before the first `m=`; fingerprints from `a=fingerprint` | shared | same | same | match | `identity::tests` |
| D1 | Duplicate `CONNECTREQUEST` | ignored | shared | ignored | ignored | match | `listener::tests` |
| D2 | `CANDIDATEADD` before `CONNECTREQUEST` | creates the session, buffers | shared | dropped | dropped | cosmetic (signaling preserves order) | — |
| D3 | Signals for unknown sessions | dropped | shared | dropped | dropped | match | — |
| H1 | HTTP client | `POST {base}/v1/join/{id}`, `application/sdp`, any 2xx is success, else 26 (`HttpSignalingClient`) | n/a | exactly 200 | exactly 200 → any 2xx; numeric bodies (BDS replies `37` with 200) stay errors | **fixed** | `endpoint::tests::client_accepts_any_success_status` |
| H2 | HTTP host | `GET /v1/join` 200 JSON; `POST /v1/join/<u64>`; 400 bad/zero id or empty body; any reply signal as 200 `application/sdp`; 10 s then 504 (W 0xe3dff0) | observed on TCP `server-port` (19132): `GET /v1/join` → 200 `{"name","protocol","version","level","players","maxPlayers","gameType"}`; `/v1/join/0` → 400 `Invalid networkId`; empty → 400 `Missing SDP offer in request body`; other path → 404; identityless offer → 200 body `37` | 15 s then 502, error → 500 | as go; same status JSON fields | cosmetic (go endpoint protocol; go/tokio clients treat a numeric body as an error) | `endpoint::tests` |

## Transport (ICE, DTLS, SCTP, data channels)

| # | Behaviour | Client | BDS 1.26.51 | go | tokio | Verdict | Test |
|---|---|---|---|---|---|---|---|
| X1 | RTCConfiguration | only `tcp_candidate_policy = Disabled`, `rtcp_mux = Require` set (W 0xe1b6a0); type all, bundle balanced, gather once, pool 0 | shared | pion defaults | UDP host/srflx/relay, gather once | match | — |
| X2 | Host candidates | default network manager: loopback ignored [INFERENCE], VPN used, IPv6 on (≤ 5 networks), no mDNS obfuscation | observed: 1 IPv4 + 3 global IPv6 host candidates, no loopback, no `.local` | non-loopback UDP4/6 | non-loopback UDP4/6 | match | `loopback_settings` |
| X3 | Remote `.local` candidates | never produced | observed: none | resolved (query-only mDNS) | resolved by binding `0.0.0.0:5353`, even under `loopback_v4()` → `TransportConfig::resolve_mdns`, off in `loopback_v4()` | **fixed** (loopback configurations bind nothing else) | `transport::ice::tests::disabled_mdns_ignores_local_candidates` |
| X4 | Sockets / ports | one per network, ephemeral; SO_RCVBUF ≥ 256 KiB, SO_SNDBUF ≥ 16 KiB | observed: ICE on UDP `server-port` (19132) for every interface (port-range config of the dedicated server) | per interface, ephemeral | one per address, ephemeral, 4 MiB buffers | cosmetic (no fixed-port option) | — |
| X5 | ICE credentials | libwebrtc 4-char ufrag, 24-char pwd | observed: `8+FQ` / 24 chars | pion 16/32 | rtc-ice 16/32 | cosmetic | — |
| X6 | ICE timing | ping 48/480/2500 ms, receiving timeout 2.5 s, keepalive 10 s, failed ~30 s → session closes with 5 | shared | pion | rtc-ice: 50 ms checks, disconnected 5 s, failed 30 s | cosmetic: same close time | — |
| X7 | ICE roles | offerer controlling | shared | same | same | match | — |
| X8 | SDP | libwebrtc output unchanged: `o=- <id> 2 IN IP4 127.0.0.1`, `group:BUNDLE 0`, `extmap-allow-mixed`, `msid-semantic: WMS`, `m=application <port> UDP/DTLS/SCTP webrtc-datachannel`, `c=`, candidates, `ice-ufrag`, `ice-pwd`, `ice-options:trickle`, `fingerprint:sha-256`, `setup:actpass`/`active`, `mid:0`, `sctp-port:5000`, `max-message-size:262144` | observed: exactly this order (answer) | hand-built, same lines | same lines and order | match | `transport::sdp::tests` |
| X9 | Remote SDP without `max-message-size` | 65536 (RFC 8841, libwebrtc) [INFERENCE] | shared | rejected | rejected → 65536 | **fixed** | `transport::sdp::tests::missing_max_message_size_defaults_to_libwebrtc_65536` |
| X10 | DTLS | 1.2 (OpenSSL 1.1.1n); ECDSA P-256 per PeerConnection [INFERENCE]; sha-256 fingerprint; SRTP `SHA1_80, AEAD_256_GCM, AEAD_128_GCM` | observed: sha-256, `setup:active` answer | P-256 per conn | P-256 per dialer/listener; SRTP `AEAD_128_GCM, SHA1_80, SHA1_32` | cosmetic (profiles intersect; no SRTP keys) | `webrtc_rs_interop` |
| X11 | SCTP | dcSCTP: heartbeat 30 s, unlimited retransmissions, 65535 streams, MTU 1191, no I-DATA | shared | pion | rtc-sctp, unlimited retransmissions | cosmetic | — |
| X12 | Data channels | offerer creates `ReliableDataChannel` (ordered, reliable) and `UnreliableDataChannel` (unordered, maxRetransmits 0) by DCEP; host creates none | shared | same | same | match | `stream::tests` |
| X13 | Remote opens another channel | host: any non-`ReliableDataChannel` label replaces the unreliable slot | shared | closes | closes | cosmetic (vanilla never does it) | — |
| X14 | Remote closes one channel | session closes (0 clean / 32 `DataChannelClosed`) | shared | closes | closes (rtc-sctp reports the reset as `AssociationLost`) | match | `stream::tests::real_loopback_fragmentation_unreliable_header_and_drained_close` |
| X15 | Local close on the wire | PeerConnection released: SCTP ABORT "User-Initiated Abort" (`Close called`), DTLS close_notify [INFERENCE] | shared | channel resets, ABORT, close_notify | channel resets, ABORT, close_notify, TURN dealloc | cosmetic: both end the vanilla session at once | `stream::tests::real_loopback_fragmentation_unreliable_header_and_drained_close` |
| X16 | Session open | at ICE connected; sends queue until the channel opens | shared | both channels open | both channels open | cosmetic | — |
| X17 | Dead-session sweep | every 5 s: ICE failed/closed → 5; dialer past 10 s without answer → 14 | shared | — | per-connection driver | n/a | — |

## Data path

| # | Behaviour | Client | BDS 1.26.51 | go | tokio | Verdict | Test |
|---|---|---|---|---|---|---|---|
| P1 | Segment header | 1 byte: segments still to follow, `n-1 … 0` (`PacketQueue::_sendFragment`) | shared | same | same | match | `framing::tests` |
| P2 | Segment payload | negotiated SCTP max message size − 1 (262143), set at ICE connected; W closes the session if it is ≤ 1 | shared | remote − 1 | min(remote − 1, local) | match | `framing::tests` |
| P3 | Max segments | 255 (first header ≤ 254); larger packets dropped | shared | error | `TooManySegments` | match | `framing::tests` |
| P4 | Unreliable messages | never segmented; oversize dropped; header 0 | shared | error | `MessageTooLarge` | match | `stream::tests` |
| P5 | Empty payload | one `[0x00]` segment, which W/BDS receivers close on and R receivers stall on | shared | nothing | nothing | cosmetic: tokio never sends a header-only segment | `framing::tests` |
| P6 | Reassembly | W: strict countdown; `len < 2` or a header mismatch closes the channel; no size/time cap. R (1.26.33) and M: no validation | strict (BDS 1.26.40/26.50 strings) | strict | strict; also rejects a first header of 255, caps 255 × 262143 B and 30 s | cosmetic (bounds above any vanilla sender) | `stream::tests::framing_errors_reach_the_reader_typed` |
| P7 | Unreliable receive | W ignores the header byte | shared | requires 0 | requires 0 | cosmetic (vanilla sends 0) | `framing::tests` |
| P8 | Reliability mapping | `ReliableOrdered` → reliable channel, everything else → unreliable; no extra header | shared | reliable only read | both channels, `reliable` flag | match | — |
| P9 | Send flow control | per channel: wait while `buffered + next > 16 MiB`; unbounded app queue | shared | pion | writer waits above 1 MiB, resumes at 256 KiB | cosmetic (local bound) | `stream::tests::small_reliable_sends_do_not_wait_for_delayed_sack` |
| P10 | RTT | ICE candidate-pair RTT / 2 as ping | shared | SCTP SRTT / 2 | `rtt()` = full SCTP SRTT | cosmetic (local API) | — |

## LAN discovery (UDP 7551)

| # | Behaviour | Client | BDS 1.26.51 | go | tokio | Verdict | Test |
|---|---|---|---|---|---|---|---|
| L1 | Header | u16 length (inclusive), u16 id, u64 sender, 8 zero bytes | observed: decoded by tokio | same | same | match | `discovery::packet::tests` |
| L2 | Packet ids | 0 request, 1 response, 2 message; others dropped | observed | same | same | match | `discovery::packet::tests` |
| L3 | Crypto | key = SHA-256(LE u64 `0xdeadbeef`); HMAC-SHA256(plaintext) then AES-256-ECB/PKCS7 (W 0xe112a0, 0xe11610, 0xe84240) | observed: interoperates | same | same | match | `discovery::packet::tests` |
| L4 | Message body | u64 recipient, u32 length, text; reader takes text to the end of the datagram | observed | same, tail tolerated | same, tail tolerated | match | `discovery::packet::tests` |
| L5 | Response body | u32 hex length, ASCII hex ServerData; sender caps the declared length at 0x47c but writes every byte; reader ignores the length | shared | strict | rejected data past the declared length → kept | **fixed** | `discovery::packet::tests::response_data_beyond_declared_length_is_kept` |
| L6 | ServerData | W/26.50+: v7 (name, protocol, version, level, players, max, game type, editor, hardcore, online auth, self-signed auth, nonce, connection). R 1.26.33: v5; BDS 1.26.40: v6 | observed v7: `07 1d "client-diff vanilla reference" a2 22 07 "1.26.51" 07 "nntruth" 00 04 02 00 00 01 01 10 "cb3f…7ff5" 08` = protocol 2193, players 0/2, creative, both auth flags, connection 4 | v7 | v7 (decodes BDS byte-for-byte) | match for 26.50+ | `discovery::packet::tests` |
| L7 | Connection type | 4; 7 when online signaling is connected | observed: 4 (signed in to signaling) | 4 | 4 | match | — |
| L8 | Request interval | 2000 ms | shared | 2 s | 2 s | match | — |
| L9 | Request destinations | directed broadcast of every non-loopback, non-169.254 IPv4 network, plus `ff02::1` on an IPv6 socket; `255.255.255.255` until network discovery completes; port 7551 (W 0xe17a10) | shared | `255.255.255.255` | `255.255.255.255` only (misses hosts on multi-homed machines) → directed broadcasts (+ `ff02::1` on IPv6), limited broadcast when none | **fixed** | `discovery::tests::directed_broadcasts_follow_interfaces` |
| L10 | Who sends requests | only while browsing (`AddBroadcastId`) | shared | only when not on port 7551 | always, also while announcing → only without server data | **fixed** | `discovery_test::announcing_listener_sends_no_requests` |
| L11 | Address learning | from responses and messages only (W 0xe4e750) | shared | every packet | every packet → responses and messages | **fixed** | `discovery::tests::requests_do_not_make_a_peer_addressable` |
| L12 | Peer address expiry | 60 s (W 0xe19f10) | shared | 15 s | 15 s → 60 s | **fixed** | `discovery::tests::expiry_keeps_addresses_60s_and_servers_5s` |
| L13 | Server list expiry | 5 s (W 0xe83560) | shared | with the address | with the address → 5 s | **fixed** | `discovery::tests::expiry_keeps_addresses_60s_and_servers_5s` |
| L14 | Socket | `[::]:7551` dual stack (else `0.0.0.0:7551`), SO_BROADCAST, no SO_REUSEADDR; ephemeral fallback | observed: UDP `[::]:7551` | bind address | bind address | cosmetic | — |
| L15 | Source filtering | none (127.0.0.1 accepted); own id and wrong recipient dropped | observed: answers unicast requests from 127.0.0.1 | same | same | match | `discovery_test` |
| L16 | Responses | only while announcing, unicast to the request source | observed | when pong data is set | when server data is set | match | `discovery_test` |
| L17 | LAN signaling | message packet unicast to the learned address; unknown peer dropped with an error | observed | same | error if unknown | match | `discovery_test` |

## Real-client capture

Packet drivers need admin, and DTLS hides the data anyway, so the capture is a
loopback-only participant: `examples/real_client_probe.rs`, documented in
[`tools/client-diff/nethernet/README.md`](../../tools/client-diff/nethernet/README.md).
It binds only `127.0.0.1` (no firewall prompt).

What loopback can and cannot reach (measured 2026-10-23):

- **Discovery and signaling: yes.** BDS 1.26.51 has an inbound *Block* rule
  (Public profile) yet answered unicast discovery and `CONNECTREQUEST` from
  `127.0.0.1` (rows L6, L15, N1, S9). The client's LAN socket is the same
  wildcard `[::]:7551`, so the probe's `host` and `server` modes capture the
  client's server data, offer/answer SDP, candidates, identity claims and
  error codes.
- **ICE/DTLS/SCTP: no.** Windows refuses datagrams from a `127.0.0.1` socket
  to the machine's own LAN address (`route` mode: `WSAENETUNREACH` 10051), and
  vanilla host candidates are interface addresses only (row X2). The BDS probe
  negotiated signaling, then ICE never connected (dial timed out after 30 s).
  Capturing the client's transport would need a probe socket on a LAN
  address, i.e. a new firewall rule or prompt.
