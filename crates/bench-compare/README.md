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

## Codec measurement

The committed `crates/valentine_gen/protocolgen/testdata/bench/manifest.json` supplies every case. SHA-256 is checked before decoding. Valentine `v1_26_51` decodes actual typed packets. Setup checks full frame/payload consumption, owned/borrowed value equality where supported, typed encode/decode equality, stable reserialization, and byte-for-byte equality with the original framed entry. Corpus integrity/framing failures are fatal. Typed codec incompatibilities instead produce a JSON coverage record on stderr and no timing for that case; every corpus name receives owned and borrowed coverage records, including supported cases. Unsupported cases never use raw-byte copying as a fallback.

Each measured operation includes the entry length varuint, packet header varuint and typed body. `decode_batch` additionally reads and validates the outer `0xfe` marker and walks all entries; it does not allocate an aggregate result vector. Single-entry decode excludes that marker. Decode results are consumed by `black_box` and dropped inside the timing. Encode starts from already-decoded owned values and reuses a `BytesMut`, including clear and entry framing in the measurement; `encode_batch` also writes the outer marker. Single-entry `payload_bytes` excludes the length prefix; batch `payload_bytes` includes the complete original batch (marker and length prefixes), matching the reference Go benchmark denominators. A batch is one operation. Criterion uses precisely these same cases and operation functions.

Allocation snapshots count allocation, zeroed allocation and reallocation calls and their requested bytes (not live/retained memory). Allocation counting runs in a separate untimed pass after latency measurement; atomic counter increments are disabled during timing. The CLI allocator still checks one relaxed atomic boolean on each allocation/reallocation, so its overhead is not identical to Go's allocator instrumentation. The global allocator is installed only in the CLI binary. Criterion deliberately uses its normal allocator, without even this flag-check cost. Codec/micro do not start an async runtime, avoiding background-task allocations in these measurements. p50/p99 are null for aggregate timed-loop measurements.

## Public API gaps

* `BorrowedMcpePacket` has no public borrowed encode method. There are no `rust-borrowed` encode rows: converting to owned or re-emitting raw input would benchmark a different operation.
* Some generated borrowed variants are `BorrowedMcpePacketData::Raw`. Such variants do **not** count as typed decode. The loader emits an unavailable `rust-borrowed` coverage record on stderr and omits the entire borrowed case if any entry is Raw or fails typed validation. Owned decode/encode still run when supported, and Criterion applies the identical exclusion. This includes unsupported complex packet shapes rather than silently materializing them through `into_owned` during timing. If owned roundtrip validation fails, both implementations' coverage records report the precise failure and neither receives a timing for that case.
* RakNet public `Datagram`, `EncapsulatedPacket` and `SplitAssembler` APIs are available and measured. An encapsulated frame has a 16-bit bit-length field, so direct frame microbenchmarks use 32, 512 and 4,096 bytes, not a fictitious 65,536-byte unsplit frame. `split_2x512` includes construction of a fresh assembler, fragment cloning, two insertions, completed payload creation and destruction.

## Transport measurement

Uses the real public `RaknetListener`/`RaknetStream` with reliable-ordered messages on channel zero. Four consecutive connections carry 32, 512, 4,096 and 65,536-byte messages. Byte zero is `0xfe`; every remaining byte is its absolute index modulo 251. Both peers assert exact contents. Each client message waits for its echo before sending the next. Bind/connect/accept/send/receive operations are bounded by 30-second timeouts (the local ephemeral bind is immediate async setup). The server retains a connection through peer close to avoid discarding the last queued echo.

Each connection first exchanges one untimed readiness echo with the same payload. The configured count then measures exactly that many subsequent echoes, excluding the readiness response from both elapsed throughput and percentile samples. This prevents application acceptance or prior-session draining from being mistaken for steady-state RTT; Go uses the identical sequence.

Connection establishment and payload/sample-buffer setup are outside measurement. RTT includes send, receive and equality assertion. p50/p99 use nearest-rank microsecond samples; throughput counts one application payload per completed roundtrip, decimal MB/s (not both network directions). Transport allocation metrics are null: asynchronous work cannot be reliably attributed using process-wide allocation snapshots. Transport uses a multi-thread Tokio runtime; set the standard `TOKIO_WORKER_THREADS` environment variable to control concurrency. The comparison script sets it to the same value as Go `GOMAXPROCS` (default 4). `loopback` hosts both peers in that runtime; `client`/`serve` allow cross-language or cross-process peers with the same wire contract.
