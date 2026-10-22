# tokio-nethernet

Tokio NetherNet transport for Minecraft Bedrock: ICE, DTLS, SCTP, reliable/unreliable data channels, encrypted LAN discovery, and signaling implementations.

## Signaling and features

| Feature | Implementation | Intended use |
| --- | --- | --- |
| `discovery` | Encrypted UDP discovery/signals on port 7551 | LAN joins |
| `endpoint` | HTTP/TLS `GET /v1/join`, `POST /v1/join/{network_id}` | Address joins; upstream go-nethernet endpoint protocol |
| `xbox-signaling` | Retained franchise signaling WebSocket | Existing Xbox friend-broadcast flow |

The HTTP endpoint is **additional**, not a replacement for franchise friend signaling. No current go-nethernet, gophertunnel, or go-xsapi source implements the franchise WebSocket service. Its existing repository envelopes, authentication headers, message types and heartbeat are retained and hardened, not claimed as an upstream port. MPSD friend advertisements remain ConnectionType 3; go-xsapi's type-7/PmsgId example is not silently substituted.

## Protocol provenance

Transport, HTTP endpoint, identities, and discovery follow [df-mc/go-nethernet at `215e46422b5831d6ddc6146003617dc3f1ff7c26`](https://github.com/df-mc/go-nethernet/tree/215e46422b5831d6ddc6146003617dc3f1ff7c26). Constants carry upstream file/function comments near their implementation.

| Behavior | Source |
| --- | --- |
| `CONNECTREQUEST`, `CONNECTRESPONSE`, `CANDIDATEADD`, `CONNECTERROR`; decimal-prefix numeric parsing | `signal.go:Signal.UnmarshalText`, `decimalPrefix` |
| Two labeled channels; ordered reliable, unordered zero-retransmit unreliable | `message.go:MessageReliability.Parameters`, `Check` |
| One countdown byte on **both** channels; unreliable count must be zero; at most 255 outgoing segments | `conn.go:Conn.Send`, `message.go:dataChannel.handleMessage` |
| 15-second dial deadline; 5-second listener negotiation; 2-second terminal signal budget | `dial.go:Dialer.Dial`, `SignalErrorTimeout`; `listener.go:ListenConfig.connContext` |
| UDP port 7551; 2-second broadcasts; 15-second address expiry | `discovery/listener.go:DefaultPort`, `background`, `deleteInactiveAddresses` |
| Inclusive packet lengths, AES-ECB/PKCS7 and HMAC-SHA256; hex response application data | `discovery/packet.go`, `crypto.go`, `packet_response.go:ResponsePacket.Read` |
| Server-data v7, zigzag integers, auth flags and nonce | `discovery/server_data.go:ServerData.MarshalBinary` |
| ES384 detached SDP fingerprint assertion; `cpk` JWK/DER keys; nested assertion string | `identity.go:Identity.sign`, `identityData`, `generateFingerprints` |
| HTTP SDP bodies, status JSON, nontrickle ICE, per-negotiation credentials | `endpoint/client.go:Client.Signal`, `DisableTrickleICE`; `endpoint/handler.go:Handler.ServeHTTP` |

### Transport stack

The data path composes the sans-IO `rtc` 0.21 crates (rtc-ice, rtc-dtls,
rtc-sctp, rtc-datachannel, rtc-stun, rtc-turn) directly, as go-nethernet does
with pion's ORTC pieces, with one Tokio driver task per connection and sends
written to SCTP and the socket inline. [DESIGN.md](DESIGN.md) documents the
module layout, task/lock model and wire-parity rules. webrtc-rs 0.12 remains
only as an independent interop peer in tests.

- SDP follows `conn.go:description.encode`/`parseDescription` line for line,
  including `a=max-message-size:262144`; segments use the remote size minus
  one (262143 against go-nethernet), so the webrtc-rs 65535 receive-buffer
  deviation is gone.
- DTLS always offers SRTP profiles because pion fails data-channel-only
  connections without one (`dtlstransport.go:completeStart`), but no SRTP keys
  are ever derived. AES-256-GCM is withheld: as DTLS server we honour the
  client's preference, and webrtc-rs 0.12 peers panic keying that profile
  (webrtc-srtp 0.14 `key_derivation.rs:aes_cm_key_derivation`), which is what
  previously broke every pairing with pion's AES-256-GCM-first preference.
- Sends complete once SCTP accepted every segment, as `conn.go:Conn.Send`
  returns after pion queues them; they never wait for acknowledgement (the
  peer's delayed SACK is 200 ms). Senders pause while more than 1 MiB is
  unacknowledged and resume at 256 KiB.
- The ICE agent uses zero candidate acceptance delays, STUN responses are
  flushed before DTLS flights, and already-due timers run inline instead of
  through a Tokio sleep (whose tick is 15.6 ms on Windows), so connection setup
  never waits on a timer.

## Identities and authentication

Current upstream signaling paths reject identityless peers by default. Set `NetherNetDialerConfig.identity` / `NetherNetListenerConfig.identity` to an `Arc<identity::Identity>` containing a P-384 signing key, matching multiplayer JWT, and authorization domain. When no server identity is supplied, a listener generates one P-384 key and issues a fresh self-signed `"self"` token per negotiation. Server tokens are self-signature-verified by default, with an optional server-token policy override.

Client assertions always prove possession of the key bound to the DTLS fingerprint and validate JWT temporal claims. As in upstream `listener.go:ListenConfig.Listen`, the default **does not authenticate the client JWT signature**: the application must verify Login's GameServerToken and bind its `cpk` to `remote_identity().public_key`. Alternatively, configure `verify_client_token` implementing `identity::TokenVerifier` to authenticate the multiplayer JWT before accepting the transport. Merely decoding claims is not online authentication.

Minecraft-side JWT/key/domain wiring follows [gophertunnel `minecraft/nethernet.go:DialContextIdentity`](https://github.com/Sandertv/gophertunnel/blob/be557e33fe6f8d0c0db7ef05b5fd068725c325ad/minecraft/nethernet.go): use the **multiplayer JWT**, not the franchise service authorization header, and append `/` to the discovered authorization issuer. `axolotl-xbl` provides the service-token and multiplayer-token/verification APIs; adapt its verifier to the transport trait without coupling this crate to authentication services.

`allow_identityless_client` and `allow_identityless_server` are explicit local/test compatibility choices. Retained franchise signaling permits absent identity assertions to preserve its existing peer compatibility; supplied identity assertions are still verified. Valid identities are available through `NetherNetStream::remote_identity()`.

## Connection API

Use `NetherNetDialer::connect_with_signaling` and `NetherNetListener::bind_with_signaling` for automatic routing. Subscription registration happens before sending an offer, including HTTP implementations that return their answer synchronously. The low-level `new` constructors still return signal senders for custom routers.

`NetherNetStream::connect` and `connect_with_config` now take a `SignalingChannel` and return the stream directly. The old `(stream, signal_sender)` result could not be routed until after negotiation had already completed. The configured timeout covers credential acquisition and the entire negotiation. Nonempty explicit ICE servers override signaling credentials (the static equivalent of upstream's credential callback override); an empty list delegates to the signaling provider. No fixed public STUN service is injected.

`endpoint::{ClientConfig, HandlerConfig}` support credential providers that are consulted for each negotiation. HTTP signaling gathers candidates into the initial SDP rather than emitting trickle messages. `Handler::serve` and `serve_tls` expose the HTTP endpoint; `Client::status` reads upstream status JSON, and `Client::ping` converts it to RakNet pong data. The Jolyne builder adds `.endpoint(handler, transport_config)` behind its `endpoint` feature while keeping `.xbox(...)` intact.

`NetherNetDialerConfig.transport` and `NetherNetListenerConfig.transport`
(`TransportConfig`) choose the local addresses that gather host candidates.
The default matches go-nethernet's pion defaults: every non-loopback UDP4/UDP6
interface address. `addresses: Some(..)` binds exactly those addresses and
skips interface enumeration (several milliseconds per connection on Windows).
`TransportConfig::loopback_v4()` binds only `127.0.0.1`, confining the actual
ICE/DTLS/SCTP sockets for strict local tests; loopback is a deliberate testing
policy, not a production default. `ice_servers` uses the crate's own
`signaling::IceServer` (UDP `turn:` and `stun:` URLs).

## Bounds and error behavior

- Full UDP-sized discovery receives; truncated, unauthenticated, malformed, wrong-recipient and non-signal packets are rejected without panics.
- Discovery admits at most 1024 peers/responses, expires both maps together, and never waits on a full signal queue in the socket receive loop. `responses()` contains decoded application bytes, not ASCII hex.
- `ServerData` is v7; new protocol/version/auth/nonce fields have defaults. `transport_layer` remains application metadata and is not a v7 wire field.
- Fragment counters and sizes are validated before queueing and while reassembling (`framing::{encode_segments, Reassembler}`, public so framing can be measured without a transport). A first countdown of 255 (256 segments) is rejected because no conforming sender produces it. Local reassembly byte/time limits supplement the upstream countdown invariant. Timeout is checked on subsequent input; no timer task is allocated per fragmented message.
- Incoming handoff is a bounded queue of completed messages (`incoming_channel_capacity`); when it is full the session stops reading SCTP, so the advertised receive window backpressures the peer instead of dropping fragments. `flush` completes once SCTP accepted the write; graceful `close` additionally waits until both channels' queued bytes are acknowledged. Transport failures are returned to the sender. Error/drop cleanup aborts SCTP, sends DTLS `close_notify` and releases TURN allocations.
- A panic unwinding out of the `rtc` stack (driven inline under the connection lock) or an integration hook (signaling, token verification) becomes `NetherNetError::DependencyPanic` and fails only that connection; a listener negotiation panic replies `CONNECTERROR` 35 and the listener keeps serving other peers.
- Pending signal delivery, listener connection ownership, endpoint HTTP bodies/subscriptions/negotiations, WebSocket frames/queues, and anomaly-monitor state are bounded. Inline and trickled ICE candidates share a 128-candidate per-peer budget. Congested negotiation subscriptions are failed rather than blocking all peers. Cancellation suppresses pending terminal replies, and cleanup checks connection-instance ownership before removing a reused tuple.

The resource budgets are local hardening policy, not newly invented protocol constants.

## Run a real local pair

The listener example binds loopback only, generates a self-signed server identity, and explicitly permits identityless local clients. Production listeners must instead configure client-token authorization.

```sh
cargo run -p tokio-nethernet --features endpoint --example listener
cargo run -p tokio-nethernet --features endpoint --example client
```

LAN examples require `discovery`; configure identities or explicitly choose the local identityless options when integrating them.

## Validation

```sh
cargo test -p tokio-nethernet --all-features
cargo clippy -p tokio-nethernet --all-targets --all-features -- -D warnings
cargo check -p jolyne --all-targets --features nethernet,discovery,xbox-signaling
cargo check -p axelerator --all-targets
```

Tests include upstream wire fixtures, go-nethernet SDP line order and parse rules, authenticated malformed/truncated discovery input, configuration deadlines/ICE validation, fragment boundaries up to 262143-byte segments, receive-window backpressure, signed-identity tampering, HTTP signaling cancellation/credentials, retained WebSocket framing/closure, real loopback peers exchanging fragmented reliable and unreliable messages, a dial against an independent webrtc-rs 0.12 peer with pion's AES-256-GCM-first SRTP preference, sequential small sends that must not wait for delayed SACKs, and a contained listener negotiation panic.

## License

MIT OR Apache-2.0
