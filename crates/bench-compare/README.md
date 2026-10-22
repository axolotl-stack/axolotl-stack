# bench-compare

Non-publishable benchmark-only workspace crate; no production codec or transport sources are modified.

## Commands

```text
cargo run --release -p bench-compare -- codec --iterations 5000
cargo run --release -p bench-compare -- micro --iterations 5000
cargo run --release -p bench-compare -- loopback --count 100
cargo run --release -p bench-compare -- serve --address 127.0.0.1:19140 --count 100
cargo run --release -p bench-compare -- client --address 127.0.0.1:19140 --count 100
cargo bench -p bench-compare --bench codec
cargo test -p bench-compare
```

The executable prints JSONL measurements to stdout; diagnostics and the server's `READY address` notification go to stderr. Corpus paths are resolved relative to the crate, not the working directory. Codec/micro default to 5,000 iterations after 32 warmup operations. Transport defaults to 100 sequential echoes per size.

Use `micro --wire-only --iterations 1000000` to select only RakNet wire-codec
and split-reassembly rows, excluding batch/compression/cipher rows. This changes
selection only; wire timing loops, warmups, and allocation counting are unchanged.

The comparison script also sets `BENCH_SPLIT_COUNT=10`: sizes 4,096 and 65,536 use that measured count while 32/512 retain `--count` (normally 100). Without the variable, direct CLI calls use `--count` for all sizes. Both peers use the same override; each size opens a fresh connection. Ten-sample p99 is low-sample and is labeled as such in the comparison report.

## Codec measurement

The committed `crates/valentine_gen/protocolgen/testdata/bench/manifest.json` supplies every case. SHA-256 is checked before decoding. Valentine `v1_26_51` decodes actual typed packets. Setup checks full frame/payload consumption, owned/borrowed value equality where supported, typed encode/decode equality, stable reserialization, and byte-for-byte equality with the original framed entry. Corpus integrity/framing failures are fatal. Typed codec incompatibilities instead produce a JSON coverage record on stderr and no timing for that case; every corpus name receives owned and borrowed coverage records, including supported cases. Unsupported cases never use raw-byte copying as a fallback.

Each measured operation includes the entry length varuint, packet header varuint and typed body. `decode_batch` additionally reads and validates the outer `0xfe` marker and walks all entries; it does not allocate an aggregate result vector. Single-entry decode excludes that marker. Decode results are consumed by `black_box` and dropped inside the timing. Encode starts from already-decoded owned values and reuses a `BytesMut`, including clear and entry framing in the measurement; `encode_batch` also writes the outer marker. Single-entry `payload_bytes` excludes the length prefix; batch `payload_bytes` includes the complete original batch (marker and length prefixes), matching the reference Go benchmark denominators. A batch is one operation. Criterion uses precisely these same cases and operation functions.

Allocation snapshots count allocation, zeroed allocation and reallocation calls and their requested bytes (not live/retained memory). Allocation counting runs in a separate untimed pass after latency measurement; atomic counter increments are disabled during timing. The CLI allocator still checks one relaxed atomic boolean on each allocation/reallocation, so its overhead is not identical to Go's allocator instrumentation. The global allocator is installed only in the CLI binary. Criterion deliberately uses its normal allocator, without even this flag-check cost. Codec/micro do not start an async runtime, avoiding background-task allocations in these measurements. p50/p99 are null for aggregate timed-loop measurements.

## Raw batch measurements (`micro`)

The report's `rust-owned` batch rows mean the public jolyne raw APIs below,
not Valentine's typed codec. Compression-only rows keep a distinct backend label.

`micro` also emits `area=batch` for the four manifest `set=batch` fixtures.
This loader is independent of typed codec support: it checks SHA-256, packet
counts/IDs and byte-for-byte raw re-encoding before timing. `framing_encode` and
`framing_decode` use jolyne's public `encode_batch_raw`/`decode_batch_raw`, not
typed packet decoding. jolyne additionally parses packet IDs/subclients and
allocates `RawPacket` descriptors; Go returns owned opaque packet slices and
bedrock-net exposes a packet iterator. jolyne raw encoding copies cached inner
frames into fresh output; Go and bedrock-net may reuse encoder buffers. These
are public-API comparisons, not identical ownership/allocation policies.

`deflate_encode`/`deflate_decode` include raw framing and negotiated DEFLATE
with threshold zero and level 6. Throughput uses the original complete batch,
including `0xfe`. Compression-only `compress`/`decompress` instead consume
framed entries excluding `0xfe` and are honestly labeled `flate2-rust`, **not**
jolyne: no public jolyne compression-only API exists. They use flate2's default
Rust backend at level 6 with fresh encoder/decoder output. The workspace
Cargo.lock pins the resolved flate2/miniz_oxide versions; jolyne is the workspace
source revision. Go uses pooled klauspost/compress level 6, while bedrock-net's
combined DEFLATE uses libdeflate level 6. Equal levels do not promise identical
compressed bytes or algorithmic effort. Setup checks decompressed bytes, not
cross-backend compressed-byte equality. Go's decompression output limit is the
exact expected length; these jolyne/flate2 calls have no output limit.

Isolated jolyne `encrypt`/`decrypt` rows are explicitly unavailable because the
checksum/cipher helpers are private to `BedrockTransport`; public send/receive
would include framing and transport. No copied crypto implementation or mock
transport stands in for that API. The public raw jolyne encoder supports
DEFLATE, not algorithm-selected Snappy, so no jolyne Snappy timing is claimed.
The batch tests cover all four exact frame roundtrips, compressed roundtrips,
and truncated frame rejection; the same smoke validation runs before timing.

## Public API gaps

* `BorrowedMcpePacket` has no public borrowed encode method. There are no `rust-borrowed` encode rows: converting to owned or re-emitting raw input would benchmark a different operation.
* Some generated borrowed variants are `BorrowedMcpePacketData::Raw`. Such variants do **not** count as typed decode. The loader emits an unavailable `rust-borrowed` coverage record on stderr and omits the entire borrowed case if any entry is Raw or fails typed validation. Owned decode/encode still run when supported, and Criterion applies the identical exclusion. This includes unsupported complex packet shapes rather than silently materializing them through `into_owned` during timing. If owned roundtrip validation fails, both implementations' coverage records report the precise failure and neither receives a timing for that case.
* RakNet public `Datagram`, `EncapsulatedPacket` and `SplitAssembler` APIs are available and measured. An encapsulated frame has a 16-bit bit-length field, so direct frame microbenchmarks use 32, 512 and 4,096 bytes, not a fictitious 65,536-byte unsplit frame. `split_2x512` includes construction of a fresh assembler, fragment cloning, two insertions, completed payload creation and destruction.

## Transport measurement

Uses the real public `RaknetListener`/`RaknetStream` with reliable-ordered messages on channel zero. Four consecutive connections carry 32, 512, 4,096 and 65,536-byte messages. Byte zero is `0xfe`; every remaining byte is its absolute index modulo 251. Both peers assert exact contents. Each client message waits for its echo before sending the next. Bind/connect/accept/send/receive operations are bounded by 30-second timeouts (the local ephemeral bind is immediate async setup). The server retains a connection through peer close to avoid discarding the last queued echo.

Each connection first exchanges one untimed readiness echo with the same payload. The configured count then measures exactly that many subsequent echoes, excluding the readiness response from both elapsed throughput and percentile samples. This prevents application acceptance or prior-session draining from being mistaken for steady-state RTT; Go uses the identical sequence.

Connection establishment and payload/sample-buffer setup are outside measurement. RTT includes send, receive and equality assertion. p50/p99 use nearest-rank microsecond samples; throughput counts one application payload per completed roundtrip, decimal MB/s (not both network directions). Transport allocation metrics are null: asynchronous work cannot be reliably attributed using process-wide allocation snapshots. Transport uses a multi-thread Tokio runtime; set the standard `TOKIO_WORKER_THREADS` environment variable to control concurrency. The comparison script sets it to the same value as Go `GOMAXPROCS` (default 4). `loopback` hosts both peers in that runtime; `client`/`serve` allow cross-language or cross-process peers with the same wire contract.

## NetherNet measurements

`nethernet-micro --iterations 5000` measures only public tokio-nethernet APIs:
`Signal::parse`/`Display` for CONNECTREQUEST, CONNECTRESPONSE, CANDIDATEADD and
CONNECTERROR, and `parse_ice_candidate`/`format_ice_candidate`. The exact shared
fixtures use connection ID 42, a loopback SDP offer/answer, host ICE candidate
`127.0.0.1:50000` and error code 2. Parse owns its output; format allocates a fresh
String. Both include output destruction. Fixture construction and roundtrip
checks precede timing; the existing separate allocation pass is unchanged.

Framing micro rows for 32, 512, 4096, 65536 and 1048576 bytes, invalid countdowns
and oversize rejection are **unavailable**: `segment_count`, `send_message` and
`ReassemblyState` are private. The encrypted/HMAC LAN packet codec is also
**unavailable**: its nominally public functions live in the private
`discovery::packet` module and are not re-exported. These gaps emit machine-readable
coverage records on stderr, not fabricated timings. No framing/crypto implementation
is copied, and transport timings do not stand in for missing isolated micros.
The sole production API addition (coordinator-approved) is optional WebRTC
`setting_engine` injection on dialer/listener configs; existing defaults are unchanged.

`nethernet-loopback --count 100` uses the real public
`NetherNetListener`/`NetherNetDialer`/`NetherNetStream` over ICE, DTLS and SCTP.
`nethernet-serve --address 127.0.0.1:PORT --count 100` and
`nethernet-client --address 127.0.0.1:PORT --count 100` expose the same protocol
across processes. The address is a **benchmark-local TCP signaling endpoint**,
not the SCTP data endpoint or the production HTTP/LAN discovery service.
Each TCP record is a JSON string containing the complete raw Signal text,
followed by newline; JSON escapes preserve multiline SDP. The bridge attributes
incoming signals to server network ID `1` or client ID `2`. TCP_NODELAY is set,
the signal receive queue holds 128 records, and candidates are embedded in SDP
(nontrickle). No external signaling service, STUN, TURN, Xbox login or credentials
are used. Local identityless operation is explicitly enabled; this intentionally
does not measure signed-identity authentication. Same-implementation loopback
uses this same TCP bridge rather than an in-memory signaling shortcut.
Both peers explicitly enable loopback candidates, filter candidates to IPv4 loopback
and restrict ICE network types to UDP4 through that setting engine. The actual data
path is therefore on `127.0.0.1`, not merely the signaling TCP listener.

Four fresh connections carry 32, 512, 4096 and 65536-byte reliable messages.
Counts are 100, 100, 10 and 10 by default; `--count` controls small messages and
`BENCH_SPLIT_COUNT` controls the two larger sizes. Every session first echoes
one untimed full-size readiness payload; all bytes match the RakNet fixture
(`index % 251`, byte zero `0xfe`). Both peers validate every message.
`nethernet_loopback` setup rows include TCP signaling establishment, initial
offer/answer, ICE/DTLS and SCTP channel opening, but not the readiness echo.
Each setup row has one sample per round; its p50/p99/max consequently agree.
Echo rows exclude setup and readiness and measure exactly the requested count,
including send, receive and validation. Nearest-rank percentiles and `max_us`
are emitted; allocation metrics remain null. Throughput counts one payload per
echo, not both directions. Client TCP shutdown acknowledges receipt of the last
echo before the server releases the data channel.

The stack is workspace `tokio-nethernet` 0.1.0, with resolved dependency versions
in Cargo.lock (webrtc-rs 0.12). Stream defaults remain unchanged: maximum outbound
segment payload 65534 bytes (webrtc-rs fixed receive-buffer limitation), one-slot
inbound/outbound queues, maximum reassembly 262143 * 255 bytes and 30-second
reassembly timeout. Thus 65536-byte messages really exercise segmentation.
Other adapters may negotiate different segment limits/buffer sizes; these are
public-stack comparisons, not artificially identical transport internals.
Dial timeout is 15 seconds, listener negotiation timeout 5 seconds, and benchmark
accept/send/receive/shutdown operations have 30-second bounds. Set
`TOKIO_WORKER_THREADS=8` (the runner does so); the existing Tokio runtime builder
honors that standard environment convention. Microbenchmarks do not start Tokio.
