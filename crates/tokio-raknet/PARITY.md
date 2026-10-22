# go-raknet parity specification

Behavioural reference for `TransmissionPolicy::Vanilla` (default for
`RaknetStream::connect*`) and the documented differences of
`TransmissionPolicy::Optimized` (default for `RaknetListener` and the
low-level `Session`/`ManagedSession` constructors). Select a policy with
`RaknetStreamConfig::transmission_policy` or
`RaknetListenerConfig::transmission_policy`.

## Reference

| Item | Value |
|---|---|
| Reference | [lunar-bedrock/go-raknet](https://github.com/lunar-bedrock/go-raknet) default branch HEAD `87a6b7e53799290d70ac25bcd03645d7ead00d2c` (2026-10-11) |
| Pinned by `tools/bench-compare/go/go.mod` | `replace github.com/sandertv/go-raknet => github.com/lunar-bedrock/go-raknet v1.15.1-0.20261011161132-87a6b7e53799`: identical to HEAD |
| Upstream compared | [sandertv/go-raknet](https://github.com/sandertv/go-raknet) master `0aa71904844e59b47a02b47e248b5c9aa87916e6` (merge base of lunar) |
| Lunar-only diff | `internal/message/addr.go` (IPv6 system-address layout, commit `a71d0e9`) and a logging test `internal/message/t_test.go`. Every other file, including `dial.go`'s 1492-byte client read buffer (`9ee65c3`, upstreamed as `0d1fd09`), equals upstream. |

go-raknet line numbers refer to `87a6b7e`; "tokio main" locations refer to
`6e3b96c`, the base of this work. Test paths are relative to
`tokio_raknet` (unit tests) or `crates/tokio-raknet/tests/`.

### Status vocabulary

| Status | Meaning |
|---|---|
| `match` | Both policies already behaved like go-raknet. |
| `vanilla` | go-raknet behaviour is reproduced by `Vanilla`; `Optimized` keeps the listed tokio behaviour. |
| `bug` | Plain defect in both policies; fixed in both with a regression test that fails on `6e3b96c`. |
| `shared` | Both policies intentionally deviate (safety bound or strictly-more-correct receive semantics). |

### Differential conformance (opt-in, needs Go)

```text
cargo test -p tokio-raknet --test go_conformance -- --ignored --nocapture
```

The test builds the `tools/bench-compare/go` helper (`raknet-conformance-serve`
and `raknet-conformance-client`), starts a go-raknet echo server, and runs the
same scripted client twice through a recording, fault-injecting UDP relay: the
go-raknet client, then `RaknetStream` (Vanilla). It compares Request1 sizes and
500 ms cadence (with a dropping 1200-byte path), masked Request2 and
NewIncomingConnection bytes, ConnectionRequest/application frame metadata,
data-datagram flags and frames per datagram, NAK contents under injected loss
and reordering, the 100 ms ACK / 500 ms ping tick phase, the resend delay of a
dropped client datagram, and the final DisconnectNotification. Timing features
use a 45 ms tolerance (Windows timer quanta). Scenarios:
`mtu_discovery_with_1200_byte_path`, `loss_recovery_matches_go`,
`reordered_split_delivery_matches_go`.

## 1. MTU discovery

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| MTU-1 | `dial.go:331-352` | Probe list `[1492, 1200, 576]`; `Dialer.MaxMTU` (clamped to 576..=1492, 0 = default) replaces the first entry and keeps only smaller defaults. | `transport/stream.rs:821-830` | `match` | Kept in the shared handshake (`RaknetStreamConfig::mtu` is the cap). | `transport::handshake::tests::probe_sizes_follow_go_mtu_sizes_for` |
| MTU-2 | `dial.go:357-421` | Four `OpenConnectionRequest1` per size, each followed by one 500 ms ticker wait (sends at 0, 0.5, …, 5.5 s); replies accepted at any time, including a late reply to an earlier, larger probe; after the twelfth probe it waits until the dial deadline. | `stream.rs:671-722`: wait ≤500 ms for one datagram; stray/invalid datagrams and receive errors consume an attempt and resend immediately; late replies to larger probes rejected; dial fails at ~6 s. | `bug` | Sans-IO `transport/handshake.rs` with go's fixed cadence and concurrent reply acceptance (both policies). | `transport::handshake::tests::probes_keep_500ms_cadence_despite_stray_packets`, `…::late_reply_to_larger_probe_is_accepted`, `…::discovery_waits_for_deadline_after_last_probe`; conformance `mtu_discovery_with_1200_byte_path` |
| MTU-3 | `internal/message/open_connection_request_1.go:19-34` | `0x05`, magic, protocol `11`, zero padding; UDP payload `MTU - 28` for **both** families. | `stream.rs:652-661`: IPv6 payload `MTU - 48`. | `vanilla` | Vanilla: 28 bytes for both families. Optimized: real IPv6 header size. | `transport::handshake::tests::vanilla_request1_padding_ignores_address_family` |
| MTU-4 | `dial.go:380-395`, `open_connection_reply_1.go:15-29` | Magic unchecked; security/cookie taken from **every** Reply1. `ServerGUID == 0 \|\| MTU < 400 \|\| MTU > 1500` (OVH workaround): send Request2 with that MTU immediately and keep discovering; otherwise `mtu = Reply1.MTU` (up to 1500, not bounded by the probe). A malformed Reply1 fails the dial. | `stream.rs:685-702`: bad magic, GUID 0, MTU outside `400..=probe` rejected with an immediate resend; final `min(probe, Reply1.MTU)`. | `vanilla` | Vanilla: go exact. Optimized: same rejections, without extra sends; MTU `≤` the largest probe sent. | `transport::handshake::tests::vanilla_reply1_ovh_workaround_sends_request2_and_keeps_waiting`, `…::vanilla_reply1_mtu_is_used_as_sent`, `…::optimized_reply1_validation_rejects_without_resending` |
| MTU-5 | `dial.go:396-401` | `IncompatibleProtocolVersion` during discovery fails the dial. | `stream.rs:703-705` ignored it; dial timed out. | `bug` | Both: `RaknetError::IncompatibleProtocolVersion` immediately. | `transport::handshake::tests::incompatible_protocol_fails_discovery_immediately` |
| MTU-6 | `dial.go:367-375,435-443,237-239`, `udp_transient_error_windows.go:11-34` | ECONNREFUSED/EHOSTUNREACH/ENETUNREACH/ECONNRESET (and ECONNABORTED on Windows) ignored up to `MaxTransientErrors` (10) across both phases; other errors fail. | Discovery: errors consume attempts with immediate resends. Request2 phase: any receive error (e.g. Windows `ConnectionReset` from ICMP) aborted the dial (`stream.rs:766-770`). | `bug` | Both: go rule; `RaknetStreamConfig::max_transient_errors` (default `Some(10)`, `None` unlimited). | `transport::handshake::tests::transient_receive_errors_are_tolerated_up_to_the_limit` |
| MTU-7 | `dial.go:473-478` | Probe write errors ignored; the cadence continues with the same size. | `stream.rs:672-677` skips to the next smaller probe. | `vanilla` | Vanilla: ignore. Optimized: fast fallback; fails only when every probe write failed. | `transport::handshake::tests::vanilla_probe_send_errors_keep_the_cadence`, `transport::stream::tests::probe_write_errors_exhaust_candidates_instead_of_aborting_discovery` |
| MTU-8 | `dial.go:423-471`, `conn.go:103-105,128-134` | Request2 carries `Reply1.MTU`; any Reply2 accepted (no GUID/magic/MTU/security checks); final MTU = `Reply2.MTU` clamped (`0`/`>1492` → 1492, `<400` → 400). | `stream.rs:735-795`: strict Reply2 validation, dial fails on mismatch. | `vanilla` | Vanilla: go exact. Optimized: strict. | `transport::handshake::tests::vanilla_reply2_mtu_is_clamped_not_rejected`, `…::optimized_reply2_validation_is_strict` |
| MTU-9 | `dial.go:459-471` | Request2 immediately, then every 500 ms until Reply2/deadline; late Reply1 ignored. | `stream.rs:758-789` same cadence. | `match` | Kept. | `transport::handshake::tests::request2_is_retransmitted_every_500ms` |
| MTU-10 | `conn.go:136-140,117` | UDP/IP overhead always 28 bytes. | `session/outbound.rs:88-92,211-221`. | `match` | Kept. | `session::vanilla::tests::vanilla_split_threshold_uses_go_constant_overhead` |
| MTU-11 | `dial.go:287-305` | 1492-byte client read buffer; longer datagrams are truncated/errored (Windows `WSAEMSGSIZE`, logged) and never handled. | `stream.rs:344`: `mtu + 72` buffer; a Windows `WSAEMSGSIZE` receive error ended the client. | `vanilla` | Vanilla: drops datagrams longer than 1492 bytes. Both: `WSAEMSGSIZE` is ignored. | `transport::stream::tests::vanilla_client_drops_datagrams_longer_than_1492`, `transport::stream::tests::transient_receive_errors_do_not_close_the_client` |
| MTU-12 | — | No DF bit or PMTU socket options; discovery relies on replies only. | Same. | `match` | Kept. | conformance `mtu_discovery_with_1200_byte_path` |
| MTU-13 | `conn.go:530-540` | Server address in Request2/NIC: dialed address, IPv4-mapped IPv6 unmapped. | Address as given. | `vanilla` | Vanilla unmaps (`types::unmap_ipv4`). | `transport::handshake::tests::vanilla_server_address_unmaps_ipv4_mapped_ipv6`, `protocol::types::addr::tests::ipv4_mapped_addresses_are_unmapped` |
| MTU-14 | `dial.go:205-214,184-199` | `Dial` uses a 10 s context bounding every phase. | `connection_timeout` 10 s. | `match` | Kept. | `integration_handshake::test_stream_connect_timeout_is_enforced` |

## 2. Handshake

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| HS-1 | `dial.go:201-203,250,159` | GUID: process-global `dialerID = -rand.Int64()`, incremented per dial and per ping. | `stream.rs:832-838`: wall-clock nanos with the sign bit. | `vanilla` | Vanilla: go scheme (`handshake::vanilla_dialer_id`, also used by `transport::ping`). | `transport::handshake::tests::vanilla_guids_are_negative_and_consecutive` |
| HS-2 | `internal/message/open_connection_request_2.go:21-38` | `0x07`, magic, [cookie `u32` BE, `0x00`], server address, MTU, GUID. | `protocol/packet/open_connection.rs:142-157` identical. | `match` | Kept. | `transport::handshake::tests::request2_bytes_match_go_layout`; conformance (masked bytes) |
| HS-3 | `internal/message/addr.go:32-77` (lunar `a71d0e9`) | IPv6: `0x06`, `00 00`, family `23` BE at offset 3, port at 5, address at 11 (29 bytes). IPv4/zero as upstream (`04 ff ff ff ff 00 00` for zero). | `protocol/types/addr.rs:20-30`: `sockaddr_in6` layout. | `vanilla` | Vanilla client encodes its outgoing server address (Request2, NIC) with `AddressLayout::Lunar`; decoding stays standard (lunar's decoded IPv6 values are unused). | `protocol::types::addr::tests::lunar_ipv6_layout_matches_go_bytes`, `transport::handshake::tests::vanilla_request2_uses_lunar_ipv6_layout` |
| HS-4 | `dial.go:264-283`, `connection_request.go` | `ConnectionRequest{GUID, timestamp(), Secure=false}` once via `Write` (ReliableOrdered); resent only by the resend logic. | `session/manager/control.rs:19-52`: Reliable, `secure = Reply2.security`. | `vanilla` | Vanilla: ReliableOrdered, insecure. Optimized: Reliable. | `session::manager::tick::tests::vanilla_connection_request_is_reliable_ordered_and_insecure`; conformance |
| HS-5 | `conn.go:702-708` | Timestamps: ms since package initialisation. | `protocol/types/time.rs:7-11`: ms since the first RakNet timestamp. | `shared` | No portable pre-`main` hook in Rust; both are per-process monotonic ms clocks. | `protocol::types::time::tests::raknet_time_roundtrip_and_conversion` |
| HS-6 | `handler.go:259-275`, `connection_request_accepted.go:19-45` | CRA decoding reads up to 20 system addresses, stopping when exactly 16 bytes remain. A second CRA is rejected without another NIC. | `open_connection.rs:343-357` read exactly 10 addresses (20-address CRAs from go/Bedrock servers had their timestamps parsed from address bytes); `control.rs:128-161` answered every CRA with a NIC and could reopen a `Closing` session. | `bug` | Both: go decoding (`system_addresses: Vec<SocketAddr>`, NIC likewise); CRA handled only before connecting. | `protocol::packet::open_connection::tests::connection_request_accepted_with_twenty_addresses_keeps_timestamps`, `session::manager::tick::tests::duplicate_cra_does_not_resend_nic_or_reopen_closing_session` |
| HS-7 | `handler.go:270`, `new_incoming_connection.go:44-55` | NIC = server address, 20 zero addresses, `PingTime = CRA.PongTime`, `PongTime = timestamp()`; ReliableOrdered. | 10 addresses (`127.0.0.1:0` + 9 × any), `request = now`, `accepted = CRA.request_timestamp`. | `vanilla` | Vanilla: go bytes. Optimized unchanged. | `session::manager::tick::tests::vanilla_new_incoming_connection_matches_go_bytes`; conformance |
| HS-8 | `conn.go:145-191` | The connection ticker starts at `newConn`, before the request; pings and the inactivity check run during the online handshake. | Pings only when connected; half-open sessions closed at 10 s (`manager/tick.rs:30-38`). | `vanilla` | Vanilla: from session creation (no half-open rule; the dial deadline bounds it). | `session::manager::tick::tests::vanilla_pings_every_fifth_tick_from_session_creation` |
| HS-9 | `handler.go:234-257` | Client: CR/NIC → logged; ping → pong; disconnect → closeImmediately; `DetectLostConnections` (`0x04`) → ReliableOrdered ping. | `0x04` ignored. | `vanilla` | Vanilla answers `0x04`. | `session::manager::tick::tests::vanilla_detect_lost_connections_is_answered_with_ping` |
| HS-10 | `dial.go:287-305`, `conn.go:416-424` | Offline IDs after dialing are ignored (the dial times out). | Connection-failure IDs (`0x11`, `0x12`, `0x14`, `0x17`, `0x19`, `0x1a`) fail the connection. | `shared` | Kept: a typed error instead of retransmitting until the deadline. | `transport::stream::tests::connection_failure_packet_is_reported` |
| HS-11 | `handler.go:193-202,378-385` (listener) | Server answers every CR with CRA (ReliableOrdered, 20 zero addresses, index 0), state unchanged. | `control.rs:89-126`: CRA **Unreliable** (a lost CRA strands the client, whose CR was ACKed); a repeated CR moved `Connected` back to `OnlineHandshake`. | `bug` | Both: CRA ReliableOrdered, state kept. Vanilla listener: go's addresses/index; Optimized: 10 addresses, index 47. | `session::manager::tick::tests::connection_request_accepted_is_reliable_and_state_preserving` |

## 3. Reliability and ordering

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| REL-1 | `conn.go:236-238,392-405`, `handler.go:469` | `Write` and every control packet are ReliableOrdered; only `ConnectedPong` is Unreliable. | CR Reliable, ping Unreliable, disconnect Reliable, CRA Unreliable. | `vanilla` | Vanilla: go set. CRA: HS-11. | `session::manager::tick::tests::vanilla_connection_request_is_reliable_ordered_and_insecure`, `…::vanilla_pings_every_fifth_tick_from_session_creation`, `…::vanilla_close_drains_then_sends_bare_disconnect`, `…::connected_ping_is_answered_with_unreliable_pong` |
| REL-2 | `conn.go:263-303` | Indices assigned and frames sent in `Write` order. | Priority heap can reorder sends. | `vanilla` | Vanilla: FIFO, priorities ignored. | `session::vanilla::tests::vanilla_sends_in_write_order_ignoring_priority` |
| REL-3 | `conn.go:265-268`, `packet.go:106-110` | One order counter; channel byte 0. | Per-channel counters; identical for channel 0. | `match` | go's API only uses channel 0. | `session::vanilla::tests::vanilla_sends_in_write_order_ignoring_priority` |
| REL-4 | `packet.go:103-105` | Sequence index never assigned; unreachable via the public API. | Global sequence counter. | `shared` | Kept. | `session::outbound::tests::sequenced_packets_get_sequence_index` |
| REL-5 | `conn.go:263-303`, `packet.go:191-213` | Fragments keep the message reliability. | `outbound.rs:286-293` upgraded Unreliable splits to Reliable. | `vanilla` | Vanilla keeps it. | `session::vanilla::tests::vanilla_split_keeps_message_reliability` |
| REL-6 | `conn.go:487-505`, `packet_queue.go:17-52` | Only ReliableOrdered queued (one queue); others delivered immediately; no message-index de-duplication. | Per-channel ordering, message-index de-duplication. | `shared` | Strictly more correct; identical for channel-0 ReliableOrdered traffic. | `session::ordering_channels::tests::duplicate_packet_ignored`, `session::reliable_tracker::tests::drops_duplicates`; conformance `reordered_split_delivery_matches_go` |
| REL-7 | `conn.go:458-460,496-498`, `handler.go:224-226,415-417` | Window sizes > 2048 close listener connections; dialers are unlimited. | Enforced in both roles; violation closes the session. | `shared` | Bound kept (RAKNET-01). Vanilla client drops the offending datagram and stays connected (KA-8). | `transport::stream::tests::vanilla_client_survives_receive_window_violation` |
| REL-8 | `datagram_window.go:19-76` | Duplicate datagrams neither handled nor re-ACKed; NAKed gaps marked received. | `receive_window.rs:22-84` same. | `match` | Kept. | `session::receive_window::tests::missing_ranges_wait_for_a_later_datagrams_age`, `…::wrapped_sequences_preserve_gap_and_duplicate_detection` |
| REL-9 | `conn.go:545-576` | Reassembly by split ID; duplicates overwrite; count mismatch ignored; listener limits 512 parts and `len > 16` (17 live IDs); no expiry. | 512 parts / 16 IDs in both roles, mismatch rejected, 30 s expiry. | `shared` | Safety bounds kept (RAKNET-01). | `session::split_assembler::tests::out_of_order_parts_ignore_duplicates_and_rejected_fragments`, `…::rejects_when_buffer_full` |
| REL-10 | `conn.go:270-273` | Split ID `uint16(splitID)`, incremented only for split messages. | Same. | `match` | Kept. | `session::vanilla::tests::split_ids_advance_only_for_split_messages` |
| REL-11 | `packet.go:176-213` | `maxSize = (mtu - 28) - 14`, minus 10 once split; any reliability. | Per-reliability header size (same for ReliableOrdered). | `vanilla` | Vanilla: go constants. | `session::vanilla::tests::vanilla_split_threshold_uses_go_constant_overhead` |
| REL-12 | `conn.go:263-303` | No outgoing caps. | 2048 queued frames; listener `max_queued_reliable_bytes`. | `shared` | Kept. | `session::tests::peer_triggered_responses_cannot_grow_outgoing_queue` |

## 4. Datagram building

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| DG-1 | `conn.go:663-682` | One frame per datagram, written synchronously. | `outbound.rs:77-151` packs to the MTU. | `vanilla` | Vanilla: one frame per datagram. | `session::vanilla::tests::vanilla_one_frame_per_datagram_with_needs_bas_flag`; conformance |
| DG-2 | `conn.go:666`, `packet.go:10-21` | Flags always `0x84`, resends included. | `0x88` while more is queued or split, else `0x84`. | `vanilla` | Vanilla: always `0x84`. | `session::vanilla::tests::vanilla_one_frame_per_datagram_with_needs_bas_flag`; conformance |
| DG-3 | `conn.go:665-675` | Sequence per data datagram incl. resends; ACK/NAK take none. | Same. | `match` | Kept. | `session::tests::retransmitted_loss_recovers_ordered_delivery`, `session::vanilla::tests::vanilla_nak_resends_immediately_with_fresh_sequences` |
| DG-4 | `packet.go:91-117` | Frame header: reliability `<< 5`, split `0x10`; bit length `len << 3`. | Same. | `match` | Kept. | `session::outbound::tests::golden_datagram_bytes_match_expectation` |

## 5. ACK/NAK

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| ACK-1 | `conn.go:145-157,193-206,442-446` | ACKs only on the 100 ms connection ticker. | 1 ms coalescing deadline, immediate for handshake/split/continuous-send/half-MTU (`manager.rs:259-273`). | `vanilla` | Vanilla: tick-only, no ACK timer. Optimized unchanged. | `session::vanilla::tests::vanilla_acks_only_on_ticks`, `transport::stream::tests::vanilla_client_ticks_every_100ms_from_session_creation`; conformance |
| ACK-2 | `acknowledge.go:29-91`, `conn.go:595-610` | All pending sequences sorted and merged into single (`1`)/range (`0`) records; a new datagram starts when, before adding a sequence, `1 + 2 + completed records ≥ effectiveMTU - 7`. | `ack_queue.rs:52-104`: merges only with the last range, `28 + 3` budget. | `vanilla` | Vanilla: `ack_queue::go_ack_chunks` (+ 8192-sequence cap of RAKNET-01, unreachable at 100 ms cadence). | `session::ack_queue::tests::vanilla_ack_chunks_match_go_writer` (against a literal port of go's writer) |
| ACK-3 | `conn.go:448-457,220`, `datagram_window.go:55-76` | Gaps below a datagram older than `1.5 × conn.rtt` are NAKed inline; `conn.rtt` is stored by `checkResend` (0 before the first check, then the resend-map mean, 50 ms when empty). | Delay `1.5 ×` smoothed ACK RTT. | `vanilla` | Vanilla: go RTT source. | `session::vanilla::tests::vanilla_nak_delay_uses_rtt_stored_by_resend_checks`, `session::policy::tests::nak_delay_is_one_and_a_half_stored_rtt`; conformance (NAK records) |
| ACK-4 | `conn.go:585-590` | NAKs use the ACK writer. | `outbound.rs:182-207`. | `vanilla` | Vanilla: go writer. | `session::ack_queue::tests::vanilla_ack_chunks_match_go_writer` |
| ACK-5 | `conn.go:615-633`, `resend_map.go:31-58` | ACK removes the datagram, sample `now - lastSend`. | EWMA + congestion window. | `vanilla` | Vanilla: go samples. | `session::vanilla::tests::vanilla_rtt_is_the_mean_of_recent_samples` |
| ACK-6 | `conn.go:635-661`, `resend_map.go:43-58` | NAK: unacked datagrams removed (sample `2 × age`) and resent **immediately** with fresh sequences. | `next_send = now`, resent by the next 20/100 ms tick. | `vanilla` | Vanilla: immediate. Optimized unchanged. | `session::vanilla::tests::vanilla_nak_resends_immediately_with_fresh_sequences`; conformance `loss_recovery_matches_go` |
| ACK-7 | `acknowledge.go:95-127` | 8192 sequences; unknown record tags stop parsing silently. | Unknown tags, wrapped and over-wide ranges are errors. | `shared` | Strict decoding kept (RAKNET-01). | `protocol::ack::tests::rejects_unknown_record_type`, `protocol::ack::tests::accepts_exact_sequence_limit` |

## 6. Congestion control and resend

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| CC-1 | `conn.go:246-303,663-682` | No congestion window, pacing or in-flight cap. | `sliding_window.rs`: cwnd from one MTU, slow start, backoff. | `vanilla` | Vanilla: no window. Optimized unchanged. | `session::vanilla::tests::vanilla_sends_without_a_congestion_window` |
| CC-2 | `conn.go:158-160,211-230` | Every third 100 ms tick, resend each unacked datagram older than `1.5 × rtt(now)` (fresh sequence, sample `2 × age`), in Go map (random) order. | Per-datagram RTO `2·SRTT + 4·RTTVAR + 30 ms` (≤ 2000 ms) every tick, bounded by unacked bandwidth. | `vanilla` | Vanilla: go schedule, ascending sequence order (Go's random map order is not reproducible). | `session::vanilla::tests::vanilla_timeout_resends_on_every_third_tick_after_one_and_a_half_rtt`; conformance `loss_recovery_matches_go` |
| CC-3 | `resend_map.go:64-80` | RTT = integer-ns mean of samples within 5 s (strictly older pruned), 50 ms when empty. | EWMA of ACK samples. | `vanilla` | Vanilla: `policy::VanillaState`. | `session::policy::tests::rtt_is_the_integer_mean_of_samples_within_five_seconds`, `session::vanilla::tests::vanilla_rtt_is_the_mean_of_recent_samples` |
| CC-4 | `conn.go:145-152` | 100 ms ticker per connection (both roles), phased at `newConn`. | Client 20 ms; listener one global 100 ms interval. | `vanilla` | Vanilla client: 100 ms ticker from session creation (first tick +100 ms). Vanilla listener: the shared 100 ms interval (per-connection phase not reproduced). | `transport::stream::tests::vanilla_client_ticks_every_100ms_from_session_creation`; conformance (tick phase) |
| CC-5 | — | No pacing. | No pacing beyond cwnd. | `match` | Kept. | `session::vanilla::tests::vanilla_sends_without_a_congestion_window` |

## 7. Keepalive and timeouts

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| KA-1 | `conn.go:176-178` | Every fifth tick (first at 500 ms), unless closing: ReliableOrdered `ConnectedPing`, in every state. | Connected only, first immediately, Unreliable. | `vanilla` | Vanilla: go. | `session::manager::tick::tests::vanilla_pings_every_fifth_tick_from_session_creation`; conformance (500 ms grid) |
| KA-2 | `conn.go:179-185,412-414` | On ping ticks: `now - lastActivity > 5 s + 2 × rtt(now)` starts a graceful close; every received datagram refreshes activity. | Every tick, `>=`, smoothed RTT, connected only, Stale state, disconnect first. | `vanilla` | Vanilla: go rule (also refreshed by malformed datagrams). | `session::manager::tick::tests::vanilla_inactivity_is_checked_on_ping_ticks_with_strict_bound` |
| KA-3 | `handler.go:289-300` | Future-dated pongs are an error (client: logged); pongs never feed RTT. | Never errors. | `match` | Client-visible behaviour identical. | `session::manager::tick::tests::pong_never_affects_rtt` |
| KA-4 | `handler.go:277-287` | Ping answered by Unreliable `ConnectedPong{ping, now}`. | Same. | `match` | Kept. | `session::manager::tick::tests::connected_ping_is_answered_with_unreliable_pong` |
| KA-5 | `conn.go:161-175,333-336,347-362,516-519` | `Close` marks closing (Unix-second resolution); no pings/timeouts; frames ACKed but not handled. Each tick: close when unacked goes non-zero → zero, or (`unacked == 0 && since > 1 s`) or `since > 5 s`, by writing a bare `0x15` (ReliableOrdered, never resent) and closing the socket. | Disconnect (Reliable, with reason) first; `Closed` after ≥1 s empty or 5 s; client muxer flushed once and exited on drop. | `vanilla` | Vanilla: go sequence incl. the truncated-second start (`ManagedSession::close`). | `session::manager::tick::tests::vanilla_close_drains_then_sends_bare_disconnect`, `transport::stream::tests::vanilla_client_drop_closes_after_drain`; conformance |
| KA-6 | `handler.go:431-433`, `conn.go:347-362` | A received `DisconnectNotification` (not closing) is answered with one and the connection closes. | Closed silently. | `vanilla` | Vanilla echoes it. | `session::manager::tick::tests::vanilla_remote_disconnect_is_echoed` |
| KA-7 | `dial.go:291-304`, `conn.go:684-695` | Socket read/write errors (other than closed) are logged; the connection continues. | Any receive error except `ConnectionReset` ended the client. | `shared` | Both: the MTU-6 transient set and `WSAEMSGSIZE` are ignored; other receive errors end the connection (a persistent error would otherwise spin). | `transport::stream::tests::transient_receive_errors_do_not_close_the_client` |
| KA-8 | `dial.go:291-304` | Malformed datagrams / handler errors are logged; the connection continues. | Client reported the error and stopped. | `vanilla` | Vanilla drops the datagram. Optimized unchanged. | `transport::stream::tests::vanilla_client_survives_malformed_datagram`, `transport::stream::tests::malformed_server_datagram_reports_error_and_stops_muxer` |
| KA-9 | `conn.go:112`, `internal/chan.go:20-66` | Application queue grows to 4096 messages, then back-pressures. | 128 messages; the 129th unread message disconnected. | `bug` | Both: 4096; overflow still disconnects (blocking the single muxer task would stop ACKs/pings, unlike go's separate ticker goroutine). | `integration_handshake::client_buffers_4096_unread_messages`, `integration_handshake::full_client_receive_queue_notifies_server_of_disconnect` |

## 8. Other client behaviour

| ID | go-raknet | go behaviour | tokio main | Status | Resolution | Test |
|---|---|---|---|---|---|---|
| OT-1 | `conn.go:21-29` | Protocol 11, MTU 400/1492, window 2048. | `constants.rs:17-26`. | `match` | Kept. | `protocol::constants::tests::protocol_version_matches_expected` |
| OT-2 | `internal/message/*.go` | Offline messages never validate the magic. | Magic validated. | `vanilla` | Vanilla handshake ignores Reply1/Reply2 magic; Optimized and the shared decoder validate it. | `transport::handshake::tests::vanilla_offline_replies_ignore_magic`, `…::optimized_reply1_validation_rejects_without_resending` |
| OT-3 | `dial.go:121-177`, `unconnected_ping.go` | `Ping`: `0x01`, time, magic, GUID (33 bytes); one read; non-pong → error; 5 s default. | No client ping; `UnconnectedPing` without GUID. | `vanilla` | `transport::ping`/`ping_with_timeout`; GUID optional when decoding. | `integration_handshake::ping_returns_listener_advertisement`, `protocol::packet::unconnected::tests::unconnected_ping_carries_client_guid` |
| OT-4 | `unconnected_pong.go` | Pong: time, GUID, magic, `u16` length, data. | Same. | `match` | Kept. | `protocol::packet::unconnected::tests::unconnected_pong_roundtrip` |
| OT-5 | `conn.go:416-424` | Dispatch: ACK bit, NACK bit, then datagram bit. | Same order. | `match` | Kept. | `protocol::datagram::tests::rejects_data_without_datagram_flag` |
| OT-6 | `dial.go` (connected socket) | Kernel filters to the dialed peer. | Unconnected socket filtering `from == server`. | `match` | Equivalent. | `transport::stream::tests::client_acks_a_quiet_server_without_application_or_socket_traffic` |

## Summary

68 rows: 18 `match`, 35 `vanilla`, 6 `bug` (fixed in both policies), 9
`shared`. Every `vanilla` row is the only behavioural difference between the
policies; `Optimized` keeps the tokio main behaviour listed in its row.
