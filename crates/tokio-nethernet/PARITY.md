# NetherNet parity: real Bedrock client vs BDS vs go-nethernet vs tokio-nethernet

What the real Minecraft Bedrock client does on NetherNet, and whether
tokio-nethernet does the same. "Vanilla" means the client's behaviour. Where
it differs from go-nethernet, tokio-nethernet follows the client, unless a
row says otherwise.

## Sources

| Column | Source |
|---|---|
| Client | lens binary analysis. **W** = Windows client 1.26.50.26 preview (catalog artifact 6; addresses are RVAs). **R** = retail Windows client 1.26.33 (artifact 3). **M** = symbolized macOS 26.30 build (named `NetherNet::*` owners). The installed client is 1.26.52 (hotfix of 26.51, same protocol). Constants were cross-checked in W wherever M was the first source. |
| BDS | BDS 1.26.51 (`C:/Users/jvigu/orca/bds-runtime`) strings, and lens BDS 1.26.40 (artifact 4) and 26.50 (artifact 5). BDS links the same NetherNet library: `handheld/src-deps/NetherNet/src/Lan/LanThreadManager.cpp`, `NetherNetTransportFactory::createTransport`, identical `ESessionError` names. It speaks NetherNet when `transport=nethernet` is set in `server.properties`; BDS 1.26.51 logs "Players will not be able to connect to your game without NetherNet. To switch, set 'transport=nethernet' in server.properties". |
| go | [df-mc/go-nethernet `215e464`](https://github.com/df-mc/go-nethernet/tree/215e46422b5831d6ddc6146003617dc3f1ff7c26) (pion). |
| tokio | This crate. "before" = `1527972` (base of this work). |

The client NetherNet stack is libwebrtc (statically linked: dcSCTP, OpenSSL
1.1.1n DTLS 1.2) driven by `NetherNet::SimpleNetworkInterfaceImpl`,
`NetworkSessionManager`, `ClientNegotiator`/`ServerNegotiator` (+`NoTrickleIce`
variants), `PacketQueue` and `LanThreadManager`. `[INFERENCE]` marks libwebrtc
defaults that the binary relies on but that were not read as constants.

### Verdicts

| Verdict | Meaning |
|---|---|
| `match` | tokio already behaved like the client. |
| `fixed` | tokio differed in a way a vanilla peer can observe; now matches the client, with the linked regression test. |
| `cosmetic` | Differs, but no vanilla peer can observe it, or the difference is a deliberate local bound. Documented, unchanged. |
| `n/a` | No counterpart in tokio. |

## Signaling

| # | Behaviour | Client | go | tokio before | Verdict | Test |
|---|---|---|---|---|---|---|
| S1 | Signal text | `TYPE <u64 decimal> <data>`; types `CONNECTREQUEST`, `CONNECTRESPONSE`, `CANDIDATEADD`, `CONNECTERROR`; error data is `std::to_string(int)` (W 0xe23110) | same | same | match | `signaling::tests` wire fixtures |
| S2 | Parse rules | split on the first two spaces (both required); id is a decimal prefix (`12x` = 12), at least one digit, overflow dropped; data may be empty (W 0xe246d0) | same | same | match | `signaling::tests` |
| S3 | `CONNECTERROR` code | signed i32 decimal prefix, trailing text ignored | same | same | match | `signaling::tests` |
| S4 | Unknown type / malformed signal | dropped silently (debug log) | parse error, dropped | parse error, dropped | match | `signaling::tests` |
| S5 | Error code numbering | same 0..37 numbering as go wherever the client uses a constant: 4, 5 ICE, 10-13, 14 no answer, 24, 26, 36, 37 identity not allowed | 0..37 | 0..37 | match | `signaling::tests` |
| S6 | Host rejects a client identity | replies `CONNECTERROR <id> 37` (W 0xe51ce0, "application declined the session") | sends 37 | sent nothing; the client waited 10-15 s and reported a timeout | **fixed** | `listener::tests::rejected_client_identity_replies_identity_not_allowed` |
| S7 | Received `CONNECTERROR` | dialing session closes with the received code; hosting session ignores it (`PeerConnectionNegotiator::onRemoteError`) | dialer aborts, listener closes | dialer aborts, listener closes the negotiation | cosmetic | — |
| S8 | Other codes sent | only 37; every other failure closes locally | many | 14 (dial timeout), 15 (accept timeout), 16 (limit), 17 (oversize SDP), 35 (contained panic) | cosmetic: a vanilla client turns each into its disconnect reason | — |
| T1 | Dialer deadline | 10 s from `CONNECTREQUEST` to `CONNECTRESPONSE` (config +0x9b8 = 10, flighting `mc-nn-timeout`), checked by a 5 s sweep, so 10-15 s; after the answer only ICE failure ends it | 15 s overall | 15 s overall | cosmetic: the same 10-15 s window | — |
| T2 | Host deadline | none: a host session ends only on ICE failure (~30 s) | 5 s accept | 5 s for offer handling + data-channel open; a slow (relayed) vanilla client that a vanilla host accepts was refused | **fixed**: 15 s (a bound is kept) | `listener::tests::slow_answer_application_still_connects` |
| N1 | Trickle on LAN | a LAN host always answers non-trickle: full gathering, candidates inline in `CONNECTRESPONSE`, no `CANDIDATEADD` (W 0xe246d0: `noTrickle = channel != 0 \|\| flag`) | per signaling | discovery trickled answers | **fixed** | `discovery_test::lan_listener_answers_with_inline_candidates` |
| N2 | Trickle elsewhere | dialer and Xbox/HTTP host follow `DisableTrickleIce`; flighting `mc-disable-trickle-ice` defaults to `"W10"` (consumer not located; capture will tell) | per signaling | per signaling | match | — |
| N3 | Trickle order | request/response sent at SetLocalDescription, then each candidate immediately; remote candidates buffered (unbounded) until both descriptions exist | ≤32 buffered | dialer buffers ≤32 before the answer; listener drops candidates for unknown sessions | cosmetic (bounded locally) | — |
| N4 | Candidate text | libwebrtc `candidate:... generation N [ufrag U] [network-id N] [network-cost C]`, no `a=` | `generation 0 ufrag <u> network-id <i> network-cost 0` | same as go | cosmetic: libwebrtc parses both | `signaling::tests` |
| N5 | Invalid remote candidate | ignored, session continues (`CreateIceCandidate` fails, candidate skipped) | closes | dialer failed the dial (and closed an established connection) | **fixed**: skipped; the 128-candidate budget still applies | `dialer::tests::invalid_trickled_candidate_is_ignored` |
| N6 | End of candidates | never signalled | none | none | match | — |
| I1 | `a=identity` | `{"idp":{domain,protocol},"assertion":A}` standard base64, inserted before the first `m=`; fingerprints from `a=fingerprint` lines | same | same | match | `identity::tests` |
| D1 | Duplicate `CONNECTREQUEST` | ignored | ignored | ignored | match | `listener::tests` |
| D2 | `CANDIDATEADD` before `CONNECTREQUEST` | creates the session and buffers | dropped | dropped | cosmetic (signaling preserves order) | — |
| D3 | Signals for unknown sessions | dropped | dropped | dropped | match | — |
| H1 | HTTP client | `POST {base}/v1/join/{id}`, `Content-Type: application/sdp`, any 2xx is success, else 26 | exactly 200 | exactly 200 | cosmetic (go endpoint protocol) | — |
| H2 | HTTP host | `GET /v1/join` 200 JSON; `POST /v1/join/<u64>`; 400 bad/zero id or empty body; 10 s then 504 | 15 s then 502 | 15 s then 502 | cosmetic (go endpoint protocol) | — |

## Transport (ICE, DTLS, SCTP, data channels)

| # | Behaviour | Client | go | tokio before | Verdict | Test |
|---|---|---|---|---|---|---|
| X1 | RTCConfiguration | only `tcp_candidate_policy = Disabled` and `rtcp_mux = Require` set (W 0xe1b6a0); type all, bundle balanced, gather once, pool 0 | pion defaults | UDP host/srflx/relay, gather once | match | — |
| X2 | Host candidates | default BasicNetworkManager: loopback ignored [INFERENCE], VPN used, IPv6 on (≤5 networks), no mDNS obfuscation (no responder, no `mdns` string) | non-loopback UDP4/6 | non-loopback UDP4/6 | match | `loopback_settings` |
| X3 | Remote `.local` candidates | never produced by vanilla | resolved (query only) | resolved by binding `0.0.0.0:5353`, even under `TransportConfig::loopback_v4()` | **fixed**: `TransportConfig::resolve_mdns`, off in `loopback_v4()` so loopback configurations never bind a wildcard socket | `transport::ice::tests::disabled_mdns_ignores_local_candidates` |
| X4 | Sockets | one per network, ephemeral port; SO_RCVBUF ≥ 256 KiB, SO_SNDBUF ≥ 16 KiB | per interface | one per address, 4 MiB buffers | cosmetic | — |
| X5 | ICE timing | libwebrtc: ping 48/480/2500 ms, receiving timeout 2.5 s, keepalive 10 s, failed ~30 s → session closes with 5 | pion | rtc-ice: 50 ms checks, disconnected 5 s, failed 30 s | cosmetic: same observable close time | — |
| X6 | ICE roles | offerer controlling | same | same | match | — |
| X7 | SDP | libwebrtc output unchanged: `group:BUNDLE 0`, `extmap-allow-mixed`, `msid-semantic: WMS`, `m=application 9 UDP/DTLS/SCTP webrtc-datachannel`, `ice-options:trickle`, `fingerprint:sha-256`, `setup:actpass`/`active`, `mid:0`, `sctp-port:5000`, `max-message-size:262144` | hand-built, same lines | same lines | match | `transport::sdp::tests` |
| X8 | Remote SDP without `max-message-size` | libwebrtc default 65536 [INFERENCE] | rejected | rejected | **fixed**: 65536 | `transport::sdp::tests::missing_max_message_size_defaults_to_libwebrtc_65536` |
| X9 | DTLS | 1.2 (OpenSSL 1.1.1n); ECDSA P-256 per PeerConnection [INFERENCE]; sha-256 fingerprint; SRTP `SHA1_80, AEAD_256_GCM, AEAD_128_GCM` offered | P-256 per conn | P-256 per dialer/listener; SRTP `AEAD_128_GCM, SHA1_80, SHA1_32` | cosmetic (profiles intersect; no SRTP keys used) | `webrtc_rs_interop` |
| X10 | SCTP | dcSCTP: heartbeat 30 s, unlimited retransmissions, 65535 streams, MTU 1191, no I-DATA | pion | rtc-sctp, unlimited retransmissions | cosmetic | — |
| X11 | Data channels | offerer creates `ReliableDataChannel` (ordered, reliable) and `UnreliableDataChannel` (unordered, maxRetransmits 0) by DCEP; host creates none | same | same | match | `stream::tests` |
| X12 | Remote opens another channel | host: any non-`ReliableDataChannel` label replaces the unreliable slot | closes | closes | cosmetic (vanilla never does it) | — |
| X13 | Remote closes one channel | session closes (0 clean / 32 `DataChannelClosed`) | closes | closes: rtc-sctp reports the incoming reset as `AssociationLost` | match | `stream::tests::real_loopback_fragmentation_unreliable_header_and_drained_close` |
| X14 | Local close on the wire | no explicit channel close: last reference drops the PeerConnection → SCTP ABORT "User-Initiated Abort" (`Close called`), DTLS close_notify [INFERENCE] | channel resets, ABORT, close_notify | channel resets, ABORT, close_notify, TURN dealloc | cosmetic: both end the vanilla session at once | `stream::tests::real_loopback_fragmentation_unreliable_header_and_drained_close` |
| X15 | Session open | at ICE connected; sends queue until the channel opens | both channels open | both channels open | cosmetic | — |
| X16 | Dead-session sweep | every 5 s: ICE failed/closed → 5; dialer past 10 s without answer → 14 | — | per-connection driver | n/a | — |

## Data path

| # | Behaviour | Client | go | tokio before | Verdict | Test |
|---|---|---|---|---|---|---|
| P1 | Segment header | 1 byte: segments still to follow, `n-1 … 0` (`PacketQueue::_sendFragment`) | same | same | match | `framing::tests` |
| P2 | Segment payload | negotiated SCTP max message size − 1 (262143), set at ICE connected; W closes the session if it is ≤ 1 | remote − 1 | min(remote − 1, local) | match | `framing::tests` |
| P3 | Max segments | 255 (first header ≤ 254); larger packets dropped | error | `TooManySegments` | match | `framing::tests` |
| P4 | Unreliable messages | never segmented; oversize dropped; header 0 | error | `MessageTooLarge` | match | `stream::tests` |
| P5 | Empty payload | sends one `[0x00]` segment, which W/BDS receivers close on and R receivers stall on | nothing | nothing | cosmetic: tokio never emits a header-only segment | `framing::tests` |
| P6 | Reassembly (W, BDS 1.26.40+) | strict countdown; `len < 2` or a header mismatch closes the channel; no size/time cap. R (1.26.33) and M do no validation | strict | strict; also rejects a first header of 255, caps at 255 × 262143 B and 30 s | cosmetic (local bounds above any vanilla sender) | `stream::tests::framing_errors_reach_the_reader_typed` |
| P7 | Unreliable receive | W ignores the header byte | requires 0 | requires 0 | cosmetic (vanilla sends 0) | `framing::tests` |
| P8 | Reliability mapping | `NetworkPeer::ReliableOrdered` → reliable channel, everything else → unreliable; no extra header | reliable only read | both channels, `reliable` flag | match | — |
| P9 | Send flow control | per channel: wait while `buffered + next > 16 MiB`; unbounded app queue | pion | writer waits above 1 MiB total, resumes at 256 KiB | cosmetic (local bound) | `stream::tests::small_reliable_sends_do_not_wait_for_delayed_sack` |
| P10 | RTT | ICE candidate-pair RTT / 2 as ping | SCTP SRTT / 2 | `rtt()` = full SCTP SRTT | cosmetic (local API, documented) | — |

## LAN discovery (UDP 7551)

| # | Behaviour | Client | go | tokio before | Verdict | Test |
|---|---|---|---|---|---|---|
| L1 | Header | u16 length (inclusive), u16 id, u64 sender, 8 zero bytes | same | same | match | `discovery::packet::tests` |
| L2 | Packet ids | 0 request, 1 response, 2 message; others dropped | same | same | match | `discovery::packet::tests` |
| L3 | Crypto | key = SHA-256(LE u64 `0xdeadbeef`), HMAC-SHA256(plaintext) then AES-256-ECB/PKCS7 (W 0xe112a0, 0xe11610, 0xe84240) | same | same | match | `discovery::packet::tests` |
| L4 | Message body | u64 recipient, u32 length, text; receiver takes text to the end of the datagram | same, tail tolerated | same, tail tolerated | match | `discovery::packet::tests` |
| L5 | Response body | u32 hex length, ASCII hex ServerData; the sender caps the declared length at 0x47c but writes every byte; the receiver ignores the length | strict | rejected data beyond the declared length | **fixed** | `discovery::packet::tests::response_data_beyond_declared_length_is_kept` |
| L6 | ServerData | W/26.50+: v7 (name, protocol, version, level, players, max, game type, editor, hardcore, online auth, self-signed auth, nonce, connection). R 1.26.33: v5; BDS 1.26.40: v6 | v7 | v7 | match for 26.50+ (only parses its own version; older clients don't see tokio hosts) | `discovery::packet::tests` |
| L7 | Connection type | 4; 7 when online signaling is connected | 4 | 4 | match | — |
| L8 | Request interval | 2000 ms | 2 s | 2 s | match | — |
| L9 | Request destinations | directed broadcast of every non-loopback, non-169.254 IPv4 network, plus `ff02::1` on an IPv6 socket; `255.255.255.255` only until network discovery completes; port 7551 (W 0xe17a10) | `255.255.255.255` | `255.255.255.255` only (misses hosts on multi-homed machines) | **fixed**: default sends to each directed broadcast (and `ff02::1` on IPv6 sockets), `255.255.255.255` when none is found | `discovery::tests::directed_broadcasts_follow_interfaces` |
| L10 | Who sends requests | only while browsing (`AddBroadcastId`) | only when not on port 7551 | always, also while announcing | **fixed**: no requests while server data is set | `discovery_test::announcing_listener_sends_no_requests` |
| L11 | Address learning | from responses and messages only (W 0xe4e750) | every packet | every packet | **fixed** | `discovery::tests::requests_do_not_make_a_peer_addressable` |
| L12 | Peer address expiry | 60 s (W 0xe19f10) | 15 s | 15 s | **fixed**: 60 s | `discovery::tests::expiry_keeps_addresses_60s_and_servers_5s` |
| L13 | Server list expiry | 5 s (W 0xe83560) | with the address | with the address | **fixed**: 5 s | `discovery::tests::expiry_keeps_addresses_60s_and_servers_5s` |
| L14 | Socket | `[::]:7551` dual stack (else `0.0.0.0:7551`), SO_BROADCAST, no SO_REUSEADDR; ephemeral fallback if 7551 is taken | bind address | bind address | cosmetic | — |
| L15 | Source filtering | none (127.0.0.1 accepted); own id and wrong recipient dropped | same | same | match | `discovery_test` |
| L16 | Responses | only while announcing, unicast to the request source | when pong data is set | when server data is set | match | `discovery_test` |
| L17 | LAN signaling | message packet unicast to the learned address; unknown peer dropped with an error | same | error if unknown | match | `discovery_test` |

## Real-client capture

The client cannot be captured with packet drivers (no admin) and DTLS hides the
data anyway, so the capture is a loopback-only participant. See
[`tools/client-diff/nethernet/README.md`](../../tools/client-diff/nethernet/README.md).
