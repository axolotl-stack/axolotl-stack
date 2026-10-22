# Cinnabar benchmark adapter

This standalone workspace keeps its dependency graph and `Cargo.lock` out of the production workspace. Build with `cargo +nightly-2026-03-05 build --release --manifest-path tools/bench-compare/cinnabar/Cargo.toml`. Initialize the existing corpus submodule first with `git submodule update --init crates/valentine_gen/protocolgen`.

## Pinned sources and licensing boundary

- [bedrock-net dev](https://github.com/cinnabar-mc/bedrock-net/tree/0eaf6d4cb38ac1b52328815302c0964645150d17): `0eaf6d4cb38ac1b52328815302c0964645150d17`, upstream HEAD resolved on 2026-10-21. Only the MIT OR Apache-2.0 root package is used, with default features disabled.
- [raknet main](https://github.com/cinnabar-mc/raknet/tree/7eb32a31f5715ed6cd69120ef19118aa03fcab28): `7eb32a31f5715ed6cd69120ef19118aa03fcab28`, upstream HEAD resolved on 2026-10-21. Both `raknet` and `raknet-tokio` use this revision. The core's `test-support` feature exposes its existing wire codecs; no upstream code is copied or patched.
- [nethernet main](https://github.com/cinnabar-mc/nethernet/tree/fed09fc05afb3b0fe34d0e7b18702f7ea86c3290): `fed09fc05afb3b0fe34d0e7b18702f7ea86c3290`, resolved 2026-10-21, Apache-2.0. Both `nethernet` and `nethernet-tokio` are standalone dependencies; no production workspace changes or copied implementation code.
- The generated contract comes from the existing protocolgen submodule at `7b2e47c463fa425e25004673cf8960753dd2ff12`, path `generated/1.26.51/rust-contract`. Generated files are not edited.

There is no dependency on bedrock-client, bedrock-client-tokio, bedrock-packs, or the login/provider-verification/messaging features. No login provider or client_data code is used. Codec rows say `implementation=contract`, not `cinnabar`: these measure protocolgen's public typed codec, not a packet codec attributed to bedrock-net.

## Commands and output

`cinnabar-bench codec --iterations 5000`, `micro --iterations 5000`, `loopback --count 100`, `serve --address 127.0.0.1:19132 --count 100`, and `client --address 127.0.0.1:19132 --count 100` use the shared JSONL row schema. Coverage and unsupported public boundaries are JSON on stderr. Tokio defaults to four workers and respects `TOKIO_WORKER_THREADS`. Transport uses the shared deterministic payload and split-count contract.

The typed corpus is the same committed `testdata/bench/manifest.json` as the other adapters. Input SHA-256, packet IDs/counts, typed equality, exact reserialization, and batch roundtrips are checked before timing. Unsupported typed cases are explicitly reported rather than timed as opaque forwarding. Ordinary loops use 32 warmups, an amortized wall-clock timing pass, and a separate allocation-counting pass. Output buffers are retained between encode calls where the public API allows it. No Criterion harness is used.

`micro --wire-only --iterations 1000000` selects only the existing RakNet
wire-codec rows, returning before batch/compression/cipher cases. Timing loops,
warmups, allocation counting, and wire validation are unchanged.

### NetherNet measurements

`nethernet-micro --iterations 5000`, `nethernet-loopback --count 100`,
`nethernet-serve --address 127.0.0.1:19133 --count 100`, and
`nethernet-client --address 127.0.0.1:19133 --count 100` use ordinary Row JSONL,
including `max_us`. The server writes `READY` to stderr after TCP bind and serves
four fresh connections, one each for 32/512/4096/65536-byte bodies. Echo counts
default to 100/100/10/10 (`BENCH_SPLIT_COUNT` overrides the two larger sizes).
An untimed full-size deterministic payload echo precedes each measured series.
Payload equality is checked inside each RTT sample; reliable ordered sends and
owned receives remain actual public transport operations. Echo ns/op and
throughput use the complete measured-loop duration, including checks and loop
overhead; percentiles use nearest-rank individual RTT samples. Setup includes the TCP
signaling connect, bridge creation, and complete ICE/DTLS/SCTP establishment.
Setup has one sample per size per process invocation; repetitions belong to the runner.
After validating the final echo, the client sends the reserved JSON string
`"BENCH-DONE"` and waits for the same marker from the server. The server finishes
its last echo send (including transport drain), waits for the client marker,
then sends its own marker. Both bridges keep signaling and RTC live through this
two-phase handshake; only then do peers close them. Marker writes and completion
waits are bounded, bridge errors propagate, and completion/retirement remain
outside measured setup and echo durations.

The cross-process signaling endpoint is benchmark-local TCP, not Minecraft LAN
discovery or an external HTTP service. Each line is a JSON **string** containing
the complete `Signal` wire text (SDP CRLFs escaped), except for the reserved
`"BENCH-DONE"` control record, which is never passed to the Signal parser.
Sender IDs are attributed by the bridge as server `1`, client `2`.
Each size opens a fresh TCP connection.
Public `CallerSignaling`/`CallerServerSignaling` drive the real transport with
full/nontrickle ICE, host candidates bound to `127.0.0.1:0`, no STUN/TURN servers,
one attempt, unsigned/anonymous answers and anonymous offers allowed, and
`TokenTrust::Any` for both token and server trust in this local benchmark.
Signal delivery completion records local socket acceptance, not a fabricated peer receipt.
Both same-process and cross-process cases use this same TCP signaling bridge.
This deliberately measures transport, not account authentication or WAN setup.

Framing rows use public `Message::encode_segments`, `MessageSegment::decode`,
and `Message::add_segment` at 32/512/4096/65536/1048576 bytes, with a selected
16383-byte segment-body limit. Encode consumes every resulting wire allocation;
reassemble starts a fresh owner and includes segment decoding, cheap `Bytes`
clones, and upstream reassembly copies. Roundtrips are checked before timing.
Invalid count 255 and nondecrementing sequence rejection use public reassembly;
oversize uses the public 255-segment encode limit. A configurable small receive
limit is unavailable because `Message::with_maximum` is crate-private; no copied
or synthetic decoder substitutes for it.

Signal parse/format fixtures are identical across adapters: connection ID 42,
the representative SDP with `setup:actpass`/`setup:active`, host candidate
`candidate:1 1 UDP 2130706431 127.0.0.1 50000 typ host generation 0 ufrag bench network-id 0 network-cost 0`,
and error code 2. ICE parse/format uses the public candidate conversion API.
Discovery uses the public encrypted-and-HMAC-checked `encode`/`decode` API with
MessagePacket sender 1, recipient 2, text `CONNECTERROR 42 2`; payload accounting
is the encrypted datagram size. Crypto and output allocation are included.

The pinned upstream manifest selects rtc 0.21.1 and rtc-turn revision
`ceef0780c54c4f5fdc08e7e343403f8026653171`; Tokio minimum 1.53.1, AES 0.9.3,
HMAC 0.13.0 and SHA-2 0.11.0. Actual resolved versions belong to the standalone
lockfile. Upstream runtime requests a 2 MiB UDP receive buffer and retains its
default application receive limit (66,846,465 bytes), queue/backpressure
settings, and SCTP defaults. The adapter does not tune those buffers. Caller
signaling uses its default 16-request capacity and signal-size/time limits;
TCP_NODELAY is enabled only on the local signaling carrier. Tokio worker count
is controlled by `TOKIO_WORKER_THREADS`; allocation totals are emitted only
for single-threaded micro measurements, not asynchronous transport.


## Measurement boundaries and asymmetries

- `codec`: single entries use `Packet::decode_framed`; batches use `Frames::from_batch` followed by `Packet::decode_frame`, including public framing traversal and direction validation. Encodes use `Packet::encode_frame`, retaining header subclient bits and adding the batch marker only for multi-packet cases. These APIs own decoded values; no borrowed implementation is claimed.
- `micro`: public `Frame` and `FrameSet` via `test-support` implement matching encapsulated/datagram encode and decode at 32, 512, and 4096 bytes. Cinnabar's decoder owns a boxed payload and its datagram decoder reserves a frame vector; those upstream costs remain measured. ACK `ack_3_ranges` uses sequences 1–8, 16, and 32–40; its payload accounting is 54 expanded sequence bytes. Isolated split reassembly is unavailable because the session implementation is private; no synthetic concatenation or full-session proxy is substituted.
- `batch`: the four multi-packet manifest cases use full corpus batch byte lengths for framing and compression row payload accounting. `framing_encode` uses `BatchCodec::encode_into`; `framing_decode` consumes every packet view returned by `decode`. Both include the `0xfe` envelope. No extra adapter-owned decoded vector is allocated in the timed loop.
- `deflate_encode`/`deflate_decode` and `snappy_encode`/`snappy_decode` include framing and the negotiated compression marker, with threshold zero. Native DEFLATE is upstream libdeflater 1.26.1 level six, not flate2's Rust backend. Snappy is the upstream root implementation. Raw compression/decompression APIs are private, so isolated `compress`/`decompress` are reported unavailable rather than replaced by a different dependency.
- `encrypt`/`decrypt` use persistent `BatchEncryptor`/`BatchDecryptor` streams, key `[0x42; 32]`, and corpus bytes after the `0xfe` marker, with no compression marker because these fixtures are uncompressed. Payload accounting excludes `0xfe` and the eight-byte checksum. Every warmup, timing, and allocation operation advances paired streams; outputs are authenticated and checked outside timing. Unique buffers with checksum capacity are prepared outside timing. Decrypt input encryption is also outside timing. Unlike the ordinary timing loop, these operations use per-call clocks and include clock overhead, explicitly avoiding producer encryption, input-copy, or key-initialization costs in cipher timings. The allocation pass counts only the public cipher call. The deterministic key is exclusively a benchmark fixture, never a reusable production session key.

Transport uses the public asynchronous adapter, not bedrock-net's optional client integrations. The server API cannot adopt an existing socket or report its bound ephemeral address, so loopback reserves a local UDP port and then releases/rebinds it through the public server API; a rebind race is an explicit error, not silently retried. Public transport ownership/flush behavior remains part of measurements.

ACK representations differ: Cinnabar's public `Ack` stores 18 expanded sequence IDs, compresses them into three records during encode, and expands records during decode. Ours retains three range records. Both process the same complete 21-byte wire datagram (`0xc0` plus three records), checked against canonical bytes before timing; the representation and range-expansion work are intentionally not hidden.

Framing decode ownership differs: jolyne returns `Vec<RawPacket>` and parses each packet header; Go's public `Decode` returns an owned slice of packet byte slices; bedrock-net exposes a lazy owned-buffer packet iterator, consumed without an extra adapter collection. These are public framing boundaries over identical bytes, not identical allocation contracts. Encoding jolyne uses prepared `RawPacket` headers and payloads; the other two accept complete packet bytes. Loopback Cinnabar's send API consumes `Box<[u8]>`, requiring a client-side payload copy; ours sends a cheap `Bytes` clone and Go passes its existing byte slice. The required ownership cost is inside the measured RTT rather than optimized away.

## Upstream parity audit

The final adapter follows upstream's echo example and loopback benchmark: `ReliableOrdered`, `Normal` priority, automatic flush enabled, default 10 ms maintenance interval, default MTU negotiation up to 1492, default socket/channel/backpressure settings, and `max_connections=1`. It does not switch to `Immediate` or invent a faster flush policy. Both Rust comparison executables use release `opt-level=3`, thin LTO, 16 codegen units and symbol stripping.

The first report used Cinnabar's default ten-connection admission limit and standalone Cargo's default release profile without LTO; the final report corrects those two configuration differences. This does not establish either as the cause of millisecond tails. Native upstream self-benchmarks use different sizes, response lengths, sample volumes and their unmodified bench profile; see [upstream-check.md](upstream-check.md) and [reproduction script](upstream-check.ps1) for separately measured diagnostics.
