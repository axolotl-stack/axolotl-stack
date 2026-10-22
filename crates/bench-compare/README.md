# bench-compare

Non-publishable benchmark-only workspace crate; no production codec or transport sources are modified.

## Commands

```text
cargo run --release -p bench-compare -- codec --iterations 5000
cargo run --release -p bench-compare -- micro --iterations 5000
cargo run --release -p bench-compare -- loopback --count 100
cargo run --release -p bench-compare -- batch --iterations 5000   # batch rows only
cargo run --release -p bench-compare -- join --joins 20 --count 200
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

`BENCH_RAKNET_POLICY=vanilla|optimized` selects the tokio-raknet
`TransmissionPolicy` of both the client and the listener for
`loopback`/`serve`/`client`. Without it the library defaults apply: a Vanilla
client (go-raknet behaviour) and an Optimized listener. Use `optimized` for A/B
comparisons against builds older than the policy split (which were Optimized on
both sides) and `vanilla` to compare against go-raknet.

## Codec measurement

The committed `crates/valentine_gen/protocolgen/testdata/bench/manifest.json` supplies every case. SHA-256 is checked before decoding. Valentine `v1_26_51` decodes actual typed packets. Setup checks full frame/payload consumption, owned/borrowed value equality where supported, typed encode/decode equality, stable reserialization, and byte-for-byte equality with the original framed entry. Corpus integrity/framing failures are fatal. Typed codec incompatibilities instead produce a JSON coverage record on stderr and no timing for that case; every corpus name receives owned and borrowed coverage records, including supported cases. Unsupported cases never use raw-byte copying as a fallback.

Each measured operation includes the entry length varuint, packet header varuint and typed body. `decode_batch` additionally reads and validates the outer `0xfe` marker and walks all entries; it does not allocate an aggregate result vector. Single-entry decode excludes that marker. Decode results are consumed by `black_box` and dropped inside the timing. Encode starts from already-decoded owned values and reuses a `BytesMut`, including clear and entry framing in the measurement; `encode_batch` also writes the outer marker. Single-entry `payload_bytes` excludes the length prefix; batch `payload_bytes` includes the complete original batch (marker and length prefixes), matching the reference Go benchmark denominators. A batch is one operation. Criterion uses precisely these same cases and operation functions.

Allocation snapshots count allocation, zeroed allocation and reallocation calls and their requested bytes (not live/retained memory). Allocation counting runs in a separate untimed pass after latency measurement; atomic counter increments are disabled during timing. The CLI allocator still checks one relaxed atomic boolean on each allocation/reallocation, so its overhead is not identical to Go's allocator instrumentation. The global allocator is installed only in the CLI binary. Criterion deliberately uses its normal allocator, without even this flag-check cost. Codec/micro do not start an async runtime, avoiding background-task allocations in these measurements. p50/p99 are null for aggregate timed-loop measurements.

## Raw batch measurements (`micro`)

The report's `rust-owned` batch rows exercise jolyne's public reusable
`BatchCodec`, not Valentine's typed packet codec. The four manifest batch
fixtures are checked by digest, packet count/ID and exact raw re-encoding
before timing.

`framing_encode` writes packet lengths and opaque packet bytes directly into a
reused `BytesMut` with `encode_frames_into`. `framing_decode` uses synchronous
`decode_with` callbacks over borrowed packet slices; no packet-ID parsing,
per-packet copy, reference-count update or aggregate result vector is charged.
Go returns owned packet slices; bedrock-net exposes a `Bytes` packet iterator.
These are public-API comparisons with explicit ownership differences.

`deflate_encode`/`deflate_decode` and `snappy_encode`/`snappy_decode` include
framing, the negotiated compression marker and raw compression. Threshold is
zero; DEFLATE uses libdeflate level 6 in both Rust implementations, while Go
uses pooled klauspost/compress level 6. Equal numeric levels need not produce
identical compressed bytes. jolyne enforces a 4 MiB decompressed-size limit;
Go's fixture decoder uses the exact expected length. Throughput counts the
original complete batch including `0xfe`; stderr emits `batch_sizes` records
with actual encoded byte counts. The obsolete flate2-only timing rows were
removed because flate2 is no longer jolyne's normal compression backend.

`encrypt`/`decrypt` exercise the public in-place `proto::crypto::BatchCipher`
used by jolyne's connection core, with independent continuous directional streams. Input
allocation/refill and output verification are outside timing; each measured
operation includes its own `Instant` reads, matching the Cinnabar prepared
measurement. Input excludes `0xfe`, has eight spare bytes, and uses key/IV
bytes `0x42`. Every output is checked against the opposite cipher direction.
Allocation accounting is a separate untimed pass. Go's private crypto helpers
remain unavailable rather than being reimplemented in the harness.

Framing, compression, size limits, truncated inputs and continuous crypto
streams have deterministic behavioral coverage; the corpus smoke validation
also runs before timing.


## Public API gaps

* `BorrowedMcpePacket` has no public borrowed encode method. There are no `rust-borrowed` encode rows: converting to owned or re-emitting raw input would benchmark a different operation.
* Some generated borrowed variants are `BorrowedMcpePacketData::Raw`. Such variants do **not** count as typed decode. The loader emits an unavailable `rust-borrowed` coverage record on stderr and omits the entire borrowed case if any entry is Raw or fails typed validation. Owned decode/encode still run when supported, and Criterion applies the identical exclusion. This includes unsupported complex packet shapes rather than silently materializing them through `into_owned` during timing. If owned roundtrip validation fails, both implementations' coverage records report the precise failure and neither receives a timing for that case.
* RakNet public `Datagram`, `EncapsulatedPacket` and `SplitAssembler` APIs are available and measured. An encapsulated frame has a 16-bit bit-length field, so direct frame microbenchmarks use 32, 512 and 4,096 bytes, not a fictitious 65,536-byte unsplit frame. `split_2x512` includes construction of a fresh assembler, fragment cloning, two insertions, completed payload creation and destruction.

## Transport measurement

Uses the real public `RaknetListener`/`RaknetStream` with reliable-ordered messages on channel zero. Four consecutive connections carry 32, 512, 4,096 and 65,536-byte messages. Byte zero is `0xfe`; every remaining byte is its absolute index modulo 251. Both peers assert exact contents. Each client message waits for its echo before sending the next. Bind/connect/accept/send/receive operations are bounded by 30-second timeouts (the local ephemeral bind is immediate async setup). The server retains a connection through peer close to avoid discarding the last queued echo.

Each connection first exchanges one untimed readiness echo with the same payload. The configured count then measures exactly that many subsequent echoes, excluding the readiness response from both elapsed throughput and percentile samples. This prevents application acceptance or prior-session draining from being mistaken for steady-state RTT; Go uses the identical sequence.

Connection establishment and payload/sample-buffer setup are outside measurement. RTT includes send, receive and equality assertion. p50/p99 use nearest-rank microsecond samples; throughput counts one application payload per completed roundtrip, decimal MB/s (not both network directions). Transport allocation metrics are null: asynchronous work cannot be reliably attributed using process-wide allocation snapshots. Transport uses a multi-thread Tokio runtime; set the standard `TOKIO_WORKER_THREADS` environment variable to control concurrency. The comparison script sets it to the same value as Go `GOMAXPROCS` (default 4). `loopback` hosts both peers in that runtime; `client`/`serve` allow cross-language or cross-process peers with the same wire contract.

Application futures run in a spawned Tokio worker task, not directly on
`Runtime::block_on`'s external calling thread. Go schedules its application
goroutines on the same worker pool as its transport goroutines. Driving Rust's
applications on the external thread instead adds OS thread-unpark round trips
to every channel delivery, measuring a different application scheduling model.

## Jolyne join measurement (`join`)

`join` binds a jolyne RakNet listener on `127.0.0.1:0` and joins it with a
jolyne client in the same Tokio runtime, `joins + 1` times (the first is a
discarded warm-up). Each join runs the full sequence: settings, offline login,
ECDH encryption, resource packs (none), StartGame and spawn; the listener uses
`online_mode = false` and compression threshold 256. After each join the
client echoes `count` `TextPacket`s through the server's typed
`recv_packet`/`send_packet` path. Rows: `join/raknet/join` (dial to play, mean
with p50/p99) and `join/raknet/echo` (one round trip). There is no
cross-implementation counterpart; it compares jolyne revisions.

## NetherNet measurements

`nethernet-micro --iterations 5000` measures only public tokio-nethernet APIs:
`Signal::parse` plus `Signal::to_wire_string` (`format`) for CONNECTREQUEST,
CONNECTRESPONSE, CANDIDATEADD and CONNECTERROR, `parse_ice_candidate`/
`format_ice_candidate`, and the encrypted/HMAC LAN packet codec
(`discovery::packet::encode_message`/`decode`). The exact shared fixtures use
connection ID 42, a loopback SDP offer/answer, host ICE candidate
`127.0.0.1:50000`, error code 2, and a discovery MessagePacket from sender 1 to
recipient 2 carrying `CONNECTERROR 42 2`. Parse owns its output (`DATA` copied,
network ID `"2".into()` per op, as on the Cinnabar side); `format` returns a fresh
exactly-sized String, as Go `Signal.String` and Cinnabar `to_string` do. Both
include output destruction. `format_reused_buffer` is an ours-only row:
`Signal::write_wire` appends into one reused String, so it allocates nothing and
has no reference counterpart; do not compare it with the fresh-String rows.
`parse_ice_candidate` borrows its fields from the input (no allocation), whereas
Cinnabar's parser returns owned fields. Discovery encode returns a fresh Vec and
decode an owned packet on every side. Fixture construction and roundtrip checks
precede timing; the existing separate allocation pass is unchanged.

Framing micro rows use the public `tokio_nethernet::framing` API with the same
16383-byte segment body as the Cinnabar adapter: `encode_segments` consumes every
encoded segment for 32, 512, 4096, 65536 and 1048576-byte messages; `Reassembler`
rows push cloned wire segments (including header validation) into a fresh
reassembler; `invalid_count` is a first countdown of 255 (256 segments),
`invalid_sequence` a countdown gap, and `oversize` a body needing 256 segments.
No framing/crypto implementation is copied, and transport timings do not stand in
for missing isolated micros. Production API used only by these rows: the public
`tokio_nethernet::framing` and `tokio_nethernet::discovery::packet` codecs
(bench-compare enables `discovery`).

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
Both peers use `TransportConfig::loopback_v4()`, which binds the ICE sockets
only on IPv4 loopback addresses. The actual data path is therefore on
`127.0.0.1`, not merely the signaling TCP listener.

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
echo, not both directions. After final payload validation the client sends the
reserved JSON string `"BENCH-DONE"`; the server finishes its SCTP send/drain,
receives that marker and returns the same marker. Only then do peers tear down
RTC/signaling. This two-phase barrier is untimed: TCP EOF alone would cancel the
server's live transport before its pending send completed.

The stack is workspace `tokio-nethernet` 0.1.0 on the sans-IO `rtc` 0.21 crates
(resolved versions in Cargo.lock). Stream defaults: maximum outbound segment
payload 262143 bytes (go-nethernet `conn.go:maxMessageSize`, further limited by
the peer's `max-message-size - 1`), a 64-message inbound queue backed by the SCTP
receive window, inline sends, maximum reassembly 262143 * 255 bytes and 30-second
reassembly timeout. 65536-byte messages are one segment against every adapter.
Other adapters may negotiate different buffer sizes; these are public-stack
comparisons, not artificially identical transport internals.
Dial timeout is 15 seconds, listener negotiation timeout 5 seconds, and benchmark
accept/send/receive/shutdown operations have 30-second bounds. Set
`TOKIO_WORKER_THREADS=8` (the runner does so); the existing Tokio runtime builder
honors that standard environment convention. Microbenchmarks do not start Tokio.
