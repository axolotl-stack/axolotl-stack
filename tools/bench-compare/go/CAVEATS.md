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

## Raw cross-implementation batch cases (`micro`)

The independent raw batch loader always selects all four `set=batch` fixtures,
even when typed codec support excludes one. SHA-256, marker, packet count, exact
uncompressed re-encoding and each decoded packet's bytes are checked before
timing. `area=batch` operations `framing_encode`/`framing_decode` call public
`Encoder.Encode`/`Decoder.Decode` with compression disabled; no typed body
marshalling occurs. `deflate_encode`/`deflate_decode` and
`snappy_encode`/`snappy_decode` enable negotiated compression at threshold zero
and include framing, compression selector and compression/decompression.
They use the complete original batch length as throughput denominator.

Raw `compress`/`decompress` use public `FlateCompression`, input `batch[1:]`,
level 6, with the exact output-length limit. The pinned fork above uses
klauspost/compress v1.18.1 and pooled compressor/decompressor state. Rust's
separate `flate2-rust` rows use fresh miniz_oxide-backed state, not a jolyne
compression-only API; bedrock-net's combined DEFLATE uses libdeflate level 6.
Compressed bytes need not match across backends; decompressed bytes must.
Go's owned slices, jolyne's ID-parsed raw descriptors and bedrock-net's iterator
have different ownership/validation work and must not be described as identical
allocation contracts. The Go encoder reuses its output buffer.

Isolated `encrypt`/`decrypt` are unavailable: the pinned public package exposes
encryption only through Encoder/Decoder, while the checksum/cipher primitives
are private. Coverage records explain this omission. No copied crypto, fresh
key setup measurement, or combined framing operation is mislabeled as isolated
encryption. These additions do not change the dependency pins above.

## RakNet transport and public API gaps

`loopback`, `serve`, and `client` use real fork `Listen`, `DialTimeout`, reliable-ordered `Conn.Write`, and `Conn.ReadPacket`; there is no TCP/UDP surrogate. Four consecutive connections carry payload sizes 32, 512, 4096, and 65536 bytes. Each payload starts with `0xfe`, with byte at index i thereafter equal to i modulo 251. Both endpoints check complete payload equality. Defaults are 100 measured sequential request/echo pairs per size plus one untimed readiness echo of the identical payload before each size. That first echo excludes application acceptance and any previous connection's asynchronous drain from measured throughput and percentiles. Connection setup and payload/sample allocation are excluded from measured exchange time; equality checks and timestamp recording are included. Latency is round-trip latency, nearest-rank p50/p99. Throughput counts completed measured echoes and one application payload per echo, not doubled bidirectional bytes or RakNet wire overhead. Allocation metrics are null for transport.

The pinned fork explicitly returns `ErrNotSupported` from `SetDeadline`, `SetReadDeadline`, and `SetWriteDeadline`. The harness therefore bounds each complete size group (including accept or dial) to 30 seconds plus one second per requested message, outside the timed per-message loop; dial separately has a 30-second timeout. A timeout exits the command with an error. Blocking library calls may remain in their worker goroutine until process exit because public `Close` is asynchronous; no unsupported deadline is silently ignored. `serve` emits `READY` on stderr after bind. The default server/client address is `127.0.0.1:19142`; standalone loopback uses an ephemeral localhost port.

After its last echo, the server waits for the client's close before draining `Conn.Context().Done()` and advancing or closing the listener. The client closes only after consuming its last response; transport-level acknowledgements alone do not prove application consumption, and starting server `Close` early can cancel a pending application read. This shutdown is outside client timing and keeps the UDP socket available for final-fragment retransmission and acknowledgement. Regression tests exchange a real 65,536-byte final echo, check lifecycle drain completion, and reject unread extra application data rather than initiating close first.

The public listener/dialer `ErrorLog` is routed to stderr so reference transport failures retain their actual cause. A 100-echo run failed in the pinned listener with `split packet: maximum concurrent splits 16 reached` (observed at 4,096-byte measured message 81); its `listenerConnectionHandler.limitsEnabled()` is private and always true, with no public listener override. This is not converted into a successful timing, retried, or worked around by changing reference sources. A smaller configured per-connection sample count may be used identically on both sides; the chosen count and resulting percentile limitation must be explicit in the report.

The comparison runner exports `BENCH_SPLIT_COUNT=10` to both languages while retaining `--count 100` for 32/512 B. Direct CLI calls without the override retain `--count` for all sizes, so the failing high-count scenario remains reproducible. The p99 estimate from ten split-message samples is explicitly low-sample.

`BENCH_TRACE_SPLITS=1` enables a diagnostic-only public UDP socket wrapper for Go listeners. It records completed split IDs and duplicate fragment indexes received after completion, with summaries on stderr; `BENCH_TRACE_DIRECTION` labels the peer direction. It never implements reassembly, changes packets, or participates in timed comparison runs. The runner explicitly disables it. Distilled evidence is committed in `../diagnostics.json`; raw logs are ignored. Source and observer tests verify the reliability-dependent header offsets and completion boundary.

Observer completion means every wire fragment index for an ID has been observed; it does not inspect the reference's private assembly map. Socket observation changes scheduling, so traced successes and negative duplicate observations are not substituted for uninstrumented failure evidence or timed results.

There are **no public standalone datagram, ACK-window, or split/reassembly APIs** in this fork. Relevant implementations are private: `packet.read/write`, `split`, `datagramWindow`, and connection receive/reassembly methods; low-level handshake messages are under Go's `internal` package boundary. `micro` explains this on stderr and emits only the separate Bedrock batch rows, not RakNet micro timings. No vendoring, fork modifications, `go:linkname`, copied implementation, raw-byte fallback, or fabricated timings are used. Real large-message loopback naturally exercises fragmentation and reassembly but is not an isolated microbenchmark.

Windows transport RTT and complete exchange intervals use QueryPerformanceCounter with one cached frequency, not `time.Now`/`time.Since`. Counter differences are converted before reporting; large absolute uptime does not multiply into nanosecond overflow. Timed client modes validate the clock at startup and log frequency and minimum observed positive increments to stderr; no silent coarse-clock fallback. Connection deadlines and timeout timers still use Go's standard runtime clocks. Per-message clocks include counter-read overhead just as Rust `Instant` does.

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

## NetherNet reference adapter

`nethernet-micro --iterations 5000`, `nethernet-loopback --count 100`,
`nethernet-serve --address 127.0.0.1:19142 --count 100`, and
`nethernet-client --address 127.0.0.1:19142 --count 100` are measurement-only
commands. They use upstream public APIs, not copied protocol implementations.
The exact df-mc/go-nethernet pin is
`215e46422b5831d6ddc6146003617dc3f1ff7c26`
(`v1.0.25-0.20260928201420-215e46422b58`). The standalone Go module records all
checksums and transitive versions. Transport is Pion WebRTC
`v4.2.21-0.20260920133716-91bfc6c2039f`, ICE `v4.4.4`, DTLS `v3.1.9`,
SCTP `v1.11.3`, and datachannel `v1.6.3`.

Public micro coverage is `Signal.UnmarshalText` / `Signal.String` for
CONNECTREQUEST, CONNECTRESPONSE, CANDIDATEADD, and CONNECTERROR, plus
`discovery.Marshal` / `discovery.Unmarshal` for the actual encrypted,
HMAC-authenticated LAN MessagePacket. The shared fixture has sender 1,
recipient 2, and data `CONNECTERROR 42 2`; discovery payload bytes are the
encrypted wire length. Signal fixtures in `nethernetSignalFixtures` are shared
with the Rust adapters, including literal SDP CRLF, connection ID 42, and the
same host ICE candidate text. Signal parsing does not parse SDP or ICE internals.
Each operation receives 32 untimed warmups and exactly the configured iteration
count. Timing uses the existing validated QPC clock; allocation columns are null
because this fixed-iteration measurement does not separately instrument allocation.

Standalone framing encode/reassembly, malformed counts, oversize framing, and
NetherNet-specific ICE string formatting are private in the pinned upstream.
Consequently no framing rows for 32/512/4096/65536/1048576 or standalone
`ice_candidate` rows are fabricated. Unavailable coverage is emitted as stderr
JSON. Pion's different generic ICE API is not substituted. Real transport echoes
still exercise the actual public reliable `Conn.Write` / `Conn.ReadPacket` path.

`--address` is a **local TCP signaling endpoint**, not the WebRTC data socket.
Every size opens a fresh TCP signaling connection and fresh NetherNet connection.
One newline-delimited JSON string carries the complete reference Signal text;
JSON escaping preserves SDP newlines. The bridge assigns server network ID 1 and
client network ID 2 out of band. No cloud endpoint, external signaling, STUN, or
TURN is used. ICE is UDP4 with loopback-only candidates and nontrickle gathering
completed before SDP transmission. Anonymous client offers and identityless
server answers are accepted by the benchmark dialer. The Go listener retains
its required default self-signed server identity: returning nil from
`IssueServerIdentity` is not supported and panics in upstream `identity.sign`.
This adds identity issuance/signing to Go setup; Rust/Cinnabar local listeners
can omit identity. This setup asymmetry is not patched out of the reference.
These local compatibility choices are not production authentication recommendations.

Same-implementation loopback uses this same TCP bridge, rather than an in-memory
substitute, so its setup rows include TCP connect, API/transport initialization,
ICE gathering, DTLS/SCTP negotiation and dial completion. The server emits `READY`
before accepting four sessions. Sizes are 32/512/4096/65536; measured counts default
to 100/100/10/10 (`--count` sets the first two and `BENCH_SPLIT_COUNT` overrides
the last two). Each session first performs one untimed echo of the same full-size
payload (index modulo 251, first byte `0xfe`); every response is validated.
Echo rows include RTT percentiles and maximum. Setup is one sample per size.
Payload/sample allocation, readiness, shutdown, and percentile sorting are outside
echo timing. After validating its final echo, the client sends the reserved JSON
string `"BENCH-DONE"` and waits for the same marker from the server. The server
finishes its final echo write, waits for the client marker, then sends its marker.
Both keep RTC and signaling live throughout this bounded two-phase handshake;
the bridge handles markers separately from Signal parsing without cancellation.
Only afterward do they close RTC and TCP; signaling EOF is not an acknowledgement.

The bridge starts with a 4 KiB scanner buffer capped at 1 MiB per signaling record.
Application payload and latency buffers are allocated once per size. Upstream Go
uses an unbuffered completed-packet channel and dynamically accumulated reassembly,
not an adapter-owned packet pool. Its advertised SCTP maximum message size is
262144 bytes with a one-byte NetherNet fragment counter (262143 payload bytes);
the peer's advertised limit affects negotiated segmentation. This differs from
other stacks' segmentation/buffering and is not patched for artificial parity.
Socket, SCTP, and DTLS buffer sizes otherwise remain upstream defaults.
NetherNet public Conn deadlines are unsupported; whole-round timers close live
connections, and signaling cancellation unblocks negotiation. Standard runtime
clocks are used only for deadlines/timeouts, never benchmark durations.

Added tests cover signal fixture round trips, authenticated discovery corruption
rejection, multiline JSON bridge preservation, fixed sender attribution, bridge
serialization, idempotent unsubscribe, default split-message counts, and a real
65536-byte final echo followed by the explicit two-phase completion handshake.
