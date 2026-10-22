# Snappy encoder evidence

## Measured result

All four fixtures pass the requested Snappy encode/decode speed gates against
both reference implementations, and the encoded-size gate of Go +0.5%.
Every measured jolyne steady-state batch operation made zero allocations.
ChunkRadius is deliberately 0.275% larger than Go, not byte-identical or
smaller: faster literal skipping trades some ratio for a substantial speed
margin. This report does not claim smaller output than bedrock-net on every
fixture.
An untargeted-row timing-noise limitation and coordinator-approved exception
are documented below; this is not a blanket 3% non-regression guarantee.

### Five alternating CPU-4 rounds

Medians in nanoseconds, lower is better. All timing cells below come from the
same five rounds; no best-round selection or mixed comparator profiles.
`Before` is stock snap rebuilt from main `d2aa833` in this worktree with the
same harness, dependency graph and build command as the measured new encoder.
Rows include reusable public batch framing and negotiated markers, not just
isolated compression kernels.

| Fixture | Operation | Before | New encoder | bedrock-net | Go |
|---|---|---:|---:|---:|---:|
| Spawn | Snappy encode | 1,250 | 1,203 | 1,640 | 1,326 |
| Spawn | Snappy decode | 664 | 604 | 881 | 1,967 |
| ServerTick | Snappy encode | 1,464 | 1,444 | 2,052 | 1,798 |
| ServerTick | Snappy decode | 721 | 654 | 1,989 | 2,253 |
| ChunkRadius | Snappy encode | 13,737 | 17,532 | 21,451 | 22,563 |
| ChunkRadius | Snappy decode | 6,006 | 6,145 | 7,062 | 28,894 |
| ClientTick | Snappy encode | 234 | 277 | 332 | 396 |
| ClientTick | Snappy decode | 178 | 175 | 299 | 475 |

ChunkRadius and ClientTick encoding are slower than stock snap, but pass the
size gate that stock snap misses, while remaining faster than both references.
The four decoder rows remain within the 3% before/after noise band or improve.

### Matched-main regression control and noise caveat

23 of the 24 non-Snappy primary framing/DEFLATE/crypto rows are below the
approximate +3% regression gate. ChunkRadius framing encode exceeds it; this
is not represented as a strict gate pass. The primary rows below show each
fixture's largest difference, not its best row.

| Dataset | Fixture | Operation | Difference |
|---|---|---|---:|
| Primary, new vs stock | Spawn | DEFLATE decode | -0.95% |
| Primary, new vs stock | ServerTick | DEFLATE encode | +0.34% |
| Primary, new vs stock | ChunkRadius | framing encode | +5.27% |
| Primary, new vs stock | ClientTick | DEFLATE decode | +0.48% |
| Identical executable, second alias vs first | ChunkRadius | framing encode | +4.92% |
| Identical executable, largest absolute difference | ServerTick | framing decode | +20.62% |

The negative control ran the exact same new executable twice under separate
labels in each of five rounds, with identical arguments and SHA-256, alongside
the stock baseline. All 480 control observations are retained, without choosing
the better alias or replacing primary-table cells. No code changed between
primary and negative-control runs. All 160 primary jolyne allocation samples
and 320 identical-image control allocation samples are zero.

The coordinator explicitly approved delivery with this measurement-noise caveat
after reviewing the same-row identical-binary variability. A blanket 3%
non-regression bound is **not established** on this shared workstation; no
framing implementation change or benchmark-specific padding was added to mask
the result.

Both matrices ran with no local build/server processes active. Older
pre-review/cross-build matrices are diagnostic only and are not mixed into
either committed dataset.

### Encoded size

All sizes include the RakNet `0xfe` and negotiated compression marker. The
plain inputs are the exact pinned protocolgen batch files; compression excludes
only the initial `0xfe` marker.

| Fixture | Plain bytes | Stock snap | New encoder | bedrock-net | Go | New vs Go |
|---|---:|---:|---:|---:|---:|---:|
| Spawn | 3,476 | 1,168 | 1,150 | 1,161 | 1,150 | 0.000% |
| ServerTick | 2,250 | 1,591 | 1,591 | 1,586 | 1,592 | -0.063% |
| ChunkRadius | 151,491 | 79,902 | 78,875 | 79,092 | 78,659 | +0.275% |
| ClientTick | 326 | 290 | 286 | 281 | 286 | 0.000% |

Go sizes were obtained through the pinned fork's public
`packet.SnappyCompression.Compress`. bedrock-net sizes were obtained through
its public negotiated `BatchCodec`, linked against the audited standalone
comparator's release libraries. Its uncompressed batch output was first
compared byte-for-byte with the corpus. No private or noncommercial API was
used.

## Implementation and format compatibility

- `CompressionContext` retains a private encoder and existing output scratch;
  no public API/caller changes or new dependencies.
- A lazily allocated 16,384-entry `u32` table (64 KiB) supports the entire
  input block, including offsets beyond 65,535. DEFLATE-only connections do
  not allocate it. Only the active 8/10/12/14-bit region is cleared per block;
  stale entries never cross batch boundaries.
- The matcher adapts the BSD-licensed S2 Snappy search, with three adjacent
  probes per input load, a remembered offset, backward extension, chained
  matches, and full tail extension. Short inputs use four-byte hashes, medium
  inputs five-byte hashes, and large inputs six-byte hashes.
- Large-input skipping uses shift 5 instead of S2's shift 6. Copy tags use their
  full 64-byte capacity where possible instead of always splitting at 60.
  These two differences retain the size gate without the previous whole-block
  hash6 prototype's small-input size failures.
- Only ordinary Snappy literals and copy1/copy2/copy4 tags are emitted. There
  are no S2 repeat extensions, native FFI, compression-level switches, fixture
  special cases, retries, or second-pass compressors.
- `snap::raw::max_compress_len` remains the buffer bound. The parser also
  requires the S2 minimum saving: it stays under
  `header_len + input_len - input_len/32 - 5`, otherwise the caller emits one
  standard Snappy literal. This rejects barely-compressible/tag-heavy parses,
  not just buffer overflow. Inputs beyond the format bound return an error.
- Decoding remains `snap::raw::Decoder`; the declared decoded length is checked
  against the configured limit before output growth. Existing threshold,
  framing, packet-count and decompression-limit tests remain unchanged.
- Two private unaligned-load helpers are the only new unsafe code. Both are
  `unsafe fn` with explicit four/eight-byte preconditions, debug assertions,
  and a bound justification at every call site. Forward reads check the
  eight-byte tail bound; every stored table offset retains enough input for
  its four-byte comparison. A safe indexed-load probe measured approximately
  1,309 / 1,638 /
  20,958 / 293 ns for Spawn / ServerTick / ChunkRadius / ClientTick versus
  1,036 / 1,280 / 12,065 / 226 ns for unaligned loads after inlining. These
  standalone diagnostics justified the unsafe helpers but are not substituted
  for the full-pipeline acceptance table.
- The full BSD redistribution notice is retained in
  `src/compression/snappy.rs`; binary distributions must reproduce that notice.

Reference selection: the pinned HashimTheArab/gophertunnel fork calls
[`s2.EncodeSnappy`](https://github.com/klauspost/compress/blob/v1.18.1/s2/encode.go),
not `EncodeSnappyBetter` or the `snappy.Encode` compatibility wrapper. Its
amd64 dispatcher selects 8/10/12/14-bit tables by input size; the portable
14-bit hash6 implementation alone does not reproduce small-input behavior.

## Committed interoperability vectors

`tests/fixtures/snappy/` contains raw Snappy blocks generated by this encoder,
without the two Bedrock envelope bytes. Four vectors represent the pinned
batch corpus (excluding its initial `0xfe`); two additional vectors exercise
wire-format boundaries:

| Vector | Plain bytes | Raw Snappy bytes | Purpose |
|---|---:|---:|---|
| `long-literal.snappy` | 65,537 | 65,544 | One literal larger than 64 KiB |
| `copy4.snappy` | 78,192 | 7,705 | `R(4096) + zeros(70000) + R(4096)`, with a real offset >65,535 |

`R(n)` is the first `n` low bytes of a u32 xorshift stream seeded with
`0x12345678`, applying shifts `<<13`, `>>17`, `<<5` each step. The permanent
[Go interoperability test](../../tools/bench-compare/go/snappy_test.go)
regenerates expected plaintext, decodes all six vectors through the pinned
public Go decoder, and rejects a decoded-size limit one byte too small.
Rust tests inspect parsed tags to prove copy4 emission and the >64 KiB
literal, rather than allowing an all-literal encoder to pass a weak round trip.


## Verification observed

Formatting, the 107-test jolyne suite, strict crate/workspace Clippy, Go
vet/tests, release smoke builds, BDS Snappy scenarios and five-client smoke
were repeated after merging final main `ffa64e5` in `728167b`. That final
merge changes NetherNet, not this encoder or batch framing. The timing
datasets retain the explicit measured snapshot identities below; they are
not relabelled as newly timed executables after this final merge.

| Command / scenario | Result |
|---|---|
| `cargo test -p jolyne --lib snappy_corpus_size_and_reference_roundtrip` before implementation | Failed as intended: Spawn 1,168 > 1,155 bytes |
| Same regression after implementation | Passed |
| `cargo test -p jolyne --lib --tests` | 107 passed, zero failed |
| `go vet ./...` and `go test ./...` in `tools/bench-compare/go` | Exit 0; all six golden vectors decode exactly and enforce the size limit |
| Barely-compressible regression before/after review fix | Failed with literal128/copy32/literal1840, then passed with one literal2000 |
| Permanent seeded mixed-input test | 256 cases of random literals, overlapping copies, offsets 1/2/65,535/65,536/70,000, lengths 4–200, near misses and all eight tails passed |
| `cargo fmt --all` | Exit 0 |
| `cargo clippy -p jolyne --all-targets -- -D warnings` | Exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0; existing build-script informational warnings only |
| `cargo build --release -p bench-compare` | Exit 0 |
| `cargo build --release -p unastar -p client-diff --bins` | Exit 0 |
| `cargo build --release -p jolyne --example stress_test` | Exit 0 |
| Release unastar + `stress_test --addr 127.0.0.1:19158 --clients 5 --duration 15` after final main merge | Five joined and spawned; zero failures; 15.44 s; observed TPS 20.0 / 20.0 / 20.2 with five players |
| Copied `tools/client-diff/run.ps1`, release client/diff binary, isolated BDS runtime and UDP 19156 after final main merge | Exit 0 for `join-idle` and `join-chat`; Go and jolyne completed both; four directional comparisons completed |

Boundary tests cover empty/tiny inputs, every length below 256, table-size
transitions, 64 KiB transitions, random/incompressible bytes, repeated bytes,
periodic data, short matches, long offsets, and reuse across unrelated blocks.

The isolated BDS properties used `compression-threshold=1` and
`compression-algorithm=snappy`. All four captures contain NetworkSettings
`8f0101000100000000000000`: threshold 1, algorithm 1 (Snappy). This exercises
the changed outgoing encoder against BDS's native decoder, not merely a
DEFLATE-negotiated server. The differential reports retain the existing
session-specific Login/PlayerList/StartGame differences and optional loading
screen packets; exit 0 does not mean every captured packet is byte-identical.
The original shared runtime and checked-in server properties were not modified.
In the server smoke, intentional client disconnects are logged at ERROR level
by existing server code; all five clients reported successful completion.

## Measurement conditions and provenance

- i9-14900HX, Windows; coordinator-reserved logical CPUs 4/5. Timed children
  inherited affinity mask `0x10` before process startup, using logical CPU 4.
- Builds/acceptance commands used E-core logical CPUs 24–31, affinity
  `0xff000000`, and `CARGO_BUILD_JOBS=8`. `VCPKG_ROOT=C:/Users/jvigu/vcpkg` was
  supplied for workspace/server builds. Workspace compiler flags were not
  changed.
- Both Rust comparators use the existing release profile: optimization level
  3, thin LTO, 16 codegen units, stripping, and native target-CPU flags.
- Rust/Cinnabar: `micro --iterations 10000`. Go: `micro`, using calibrated
  `testing.Benchmark` loops. `GOMAXPROCS=4`, `TOKIO_WORKER_THREADS=4`.
- Odd rounds: stock snap → new encoder → Cinnabar → Go. Even rounds: reverse
  order. All four executables run in every one of the five rounds.
- Corpus: protocolgen `7b2e47c463fa425e25004673cf8960753dd2ff12`.
- Measured stock baseline: main `d2aa8336e807d4403cbb85f87483172d4e572df7`;
  measured integration: `289f79989ad7d2f26bc4f17f5505466cbdc4f3ce`, containing
  encoder commit `7773324`. Both executables were built separately with
  `cargo build --release -p bench-compare`, not a multi-package feature union.
  Rust compiler: `1.96.0-nightly (b90dc1e59 2026-03-04)`.
- Cinnabar/Go executables were reused with coordinator approval from the
  audited `stab-opt-jolyne` worktree, with the exact previously audited hashes.
  Cinnabar's standalone lockfile resolves snap 1.1.2; the stock workspace
  baseline resolves snap 1.1.1. The Go binary reports Go 1.27.0 / GOAMD64=v1,
  gophertunnel `80c811b61860` and klauspost/compress 1.18.1.
- Durable primary data/provenance: `benches/snappy-evidence.csv` (640 rows)
  and `benches/snappy-metadata.json`. Negative-control data/provenance:
  `benches/snappy-control-evidence.csv` (480 rows) and
  `benches/snappy-control-metadata.json`. Group by `side/name/operation` and
  take the median of five `ns_per_op` values; both Rust variants intentionally
  retain the harness's `rust-owned` implementation label.
  The report does not require local ignored result directories.

| Executable | SHA-256 |
|---|---|
| Stock-snap matched-main baseline | `43c2a3804825d16a871fae94b15e2df949831f020f3e83a87627e312aa222049` |
| New encoder (both control aliases) | `b337b679c0b82ad9dc8812e21fe1a0775c542cb9f945ebfe38745d96bb1f1189` |
| Cinnabar | `49b38625d322568adbc7fc73e85509813c084f47e08a5f230dfe9c532a0635f2` |
| Go | `af5bf50647d0fbd6ce9881899fc18cf817723fb54aa72c9a7c158706b362cd7f` |
