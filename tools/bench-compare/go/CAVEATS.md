# Go comparison benchmark caveats

## Reproducible dependencies

Resolved live with `git ls-remote` for both repositories' `refs/heads/master`, then `go list -m -json <fork>@<full-sha>` on 2026-10-21:

| Imported module | Replacement fork | Exact commit | Go pseudo-version |
| --- | --- | --- | --- |
| github.com/sandertv/gophertunnel | github.com/HashimTheArab/gophertunnel | 80c811b6186016b3860c358368cfa47e507f26e9 | v1.25.3-0.20260928233358-80c811b61860 |
| github.com/sandertv/go-raknet | github.com/lunar-bedrock/go-raknet | 87a6b7e53799290d70ac25bcd03645d7ead00d2c | v1.15.1-0.20261011161132-87a6b7e53799 |

These happen to match the existing `tools/client-diff/gt-client/go.mod` replacements; they were independently resolved, not inferred from that file. Installed toolchain: `go version go1.27.0 windows/amd64`. Module language minimum is Go 1.25.0, matching the gophertunnel fork. `go mod tidy` resolves the module graph and writes checksums; no production module or submodule is edited.

## Inputs and measured boundaries

The source of truth is `crates/valentine_gen/protocolgen/testdata/bench/manifest.json` and its committed binaries, from protocolgen commit `7b2e47c463fa425e25004673cf8960753dd2ff12`. Each file's SHA-256, packet count, and packet IDs are checked. No fixtures are regenerated. The original protocolgen benchmarks are a separate historical baseline, not another name for this newly pinned Go implementation.

`codec` uses `testing.Benchmark` (normal adaptive Go duration) and emits JSONL only to stdout. Each supported nonbatch packet has real typed `decode` and `encode` operations. Decode includes the entry-length varuint, packet-header varuint, and typed body; it requires exact consumption. Encode begins with an already decoded packet and writes header/body through a reusable scratch buffer into a reusable length-prefixed output buffer. Pools correspond to the **sender** (`NewClientPool` for client packets, `NewServerPool` for server packets). Reader limits are enabled only for client-sent packets, matching the reference. The corpus encoder is newer than this requested master pin; typed decode failures or differing re-encodings exclude that case and emit a JSON coverage record on stderr with the reason. Corpus hash/framing/manifest failures remain fatal. No raw-byte fallback measurements are used.

Batch `decode_batch` uses gophertunnel `Decoder.DecodeFunc`, including `0xfe`, all entry lengths and headers, and typed body decoding. The reusable source exposes `ReadPacket` so large batches do not truncate at the decoder's plain-reader buffer limit. `encode_batch` marshals every typed packet using reusable per-entry buffers and calls `Encoder.Encode`. Both encode setups require byte-for-byte equality before timing. Encryption and negotiated compression are disabled for these batch operations.

The four batch fixtures additionally have `compress`/`decompress` rows using the public `packet.FlateCompression` API: raw DEFLATE, library default level 6, input is the length-prefixed entries **excluding** the `0xfe` marker. Decompression uses the exact expected uncompressed length as its limit. These rows are compression-only, not typed decode or compressed-frame decoding. Public methods return allocated owned output (library internal pools remain enabled). No compressed-byte identity with another implementation is assumed; setup verifies exact decompression equality.

Supported batches also have `decode_compressed_batch`: setup constructs `0xfe`, the public DEFLATE compression ID, and compressed entries; a reusable decoder has compression enabled and calls `DecodeFunc` with the same typed callback. Timing includes the selector, decompression, entry framing, headers, typed bodies and buffer-pool lifecycle. It reports original uncompressed batch bytes as the throughput denominator. There is no Rust/jolyne counterpart claimed for this row.

`payload_bytes` counts header+body excluding the length prefix for single packets, the complete uncompressed batch for batch operations, and uncompressed framed entries for compression operations. `bytes_per_op` is Go heap allocation volume, not wire volume; fractional `allocs_per_op` and `bytes_per_op` use the raw benchmark totals. Throughput uses decimal MB/s. Codec latency percentiles are null because Go's aggregate benchmark does not provide a per-operation distribution. Rust borrowed decoding may avoid copies/allocations that Go typed structures own; do not label this an ownership-equivalent comparison. Reused-buffer Go encoding likewise must not be presented as identical allocation policy to a freshly allocated Rust output.

## RakNet transport and public API gaps

`loopback`, `serve`, and `client` use real fork `Listen`, `DialTimeout`, reliable-ordered `Conn.Write`, and `Conn.ReadPacket`; there is no TCP/UDP surrogate. Four consecutive connections carry payload sizes 32, 512, 4096, and 65536 bytes. Each payload starts with `0xfe`, with byte at index i thereafter equal to i modulo 251. Both endpoints check complete payload equality. Defaults are 100 measured sequential request/echo pairs per size plus one untimed readiness echo of the identical payload before each size. That first echo excludes application acceptance and any previous connection's asynchronous drain from measured throughput and percentiles. Connection setup and payload/sample allocation are excluded from measured exchange time; equality checks and timestamp recording are included. Latency is round-trip latency, nearest-rank p50/p99. Throughput counts completed measured echoes and one application payload per echo, not doubled bidirectional bytes or RakNet wire overhead. Allocation metrics are null for transport.

The pinned fork explicitly returns `ErrNotSupported` from `SetDeadline`, `SetReadDeadline`, and `SetWriteDeadline`. The harness therefore bounds each complete size group (including accept or dial) to 30 seconds plus one second per requested message, outside the timed per-message loop; dial separately has a 30-second timeout. A timeout exits the command with an error. Blocking library calls may remain in their worker goroutine until process exit because public `Close` is asynchronous; no unsupported deadline is silently ignored. `serve` emits `READY` on stderr after bind. The default server/client address is `127.0.0.1:19142`; standalone loopback uses an ephemeral localhost port.

After its last echo, the server calls asynchronous `Close` and waits for `Conn.Context().Done()` before advancing or closing the listener. This drain is outside client timing and keeps the UDP socket available for final-fragment retransmission and acknowledgement. The lifecycle regression exchanges a real 65,536-byte final echo and asserts that the drain does not return before the connection context ends.

There are **no public standalone datagram, ACK-window, or split/reassembly APIs** in this fork. Relevant implementations are private: `packet.read/write`, `split`, `datagramWindow`, and connection receive/reassembly methods; the low-level handshake messages are under Go's `internal` package boundary. Therefore `micro` succeeds with a clear stderr explanation and emits **no JSONL timings**. No vendoring, fork modifications, `go:linkname`, copied implementation, raw-byte fallback, or fabricated timings are used. Real large-message loopback naturally exercises fragmentation and reassembly but is not a substitute for isolated microbenchmarks.

## Commands

From this directory:

```text
go build -o bench-compare-go.exe .
./bench-compare-go.exe codec
./bench-compare-go.exe micro
./bench-compare-go.exe loopback --count 100
./bench-compare-go.exe serve --address 127.0.0.1:19142 --count 100
./bench-compare-go.exe client --address 127.0.0.1:19142 --count 100
go test ./...
go test -run '^$' -bench BenchmarkCodec -benchmem
```

The binary can run from the repository root: its default corpus path is anchored to the source location recorded by Go. Set `BENCH_CORPUS` or pass `codec --corpus <directory>` for relocated binaries or builds using `-trimpath`.
