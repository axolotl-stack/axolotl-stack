# Jolyne batch optimization evidence

## Outcome

Jolyne has lower measured timing medians on **30 of 32** batch operations.
Every measured steady-state batch operation allocates **zero times**
(160 selected per-round allocation samples). The original all-row/all-size
target is **not fully met**: Spawn decrypt (+0.4%) and ChunkRadius DEFLATE
encode (+1.0%) are noise-band ties rather than strict wins; stock Snappy misses
three compressed-size gates. The coordinator approved landing the safe
measured pipeline and deferring deeper crypto/Snappy work rather than adding
unproven hardware-specific code.

### Final matched-profile pinned comparison

Medians in nanoseconds; lower is better. Negative percentages favor jolyne.
Every row comes from the same five interleaved rounds on logical CPU 4:
3,000 iterations for Rust/Cinnabar and calibrated Go loops. No individual
best rounds or mixed before/after cipher rows were selected. Both Rust
executables use release optimization level 3, thin LTO, 16 codegen units,
stripping and the repository's native target-CPU flags.

| Case | Operation | Jolyne (ns) | bedrock-net (ns) | Go (ns) | vs fastest reference |
|---|---|---:|---:|---:|---:|
| Spawn | framing_encode | 103 | 149 | 222 | -31.1% |
| Spawn | framing_decode | 63 | 431 | 1,925 | -85.4% |
| Spawn | deflate_encode | 14,103 | 17,067 | 34,728 | -17.4% |
| Spawn | deflate_decode | 1,619 | 3,139 | 11,110 | -48.4% |
| Spawn | snappy_encode | 1,216 | 1,686 | 1,507 | -19.3% |
| Spawn | snappy_decode | 611 | 978 | 2,463 | -37.6% |
| Spawn | encrypt | 2,020 | 2,197 | private | -8.1% |
| Spawn | decrypt | 1,970 | 1,963 | private | +0.4% |
| ServerTick | framing_encode | 177 | 262 | 506 | -32.6% |
| ServerTick | framing_decode | 136 | 1,369 | 2,043 | -90.1% |
| ServerTick | deflate_encode | 15,197 | 18,446 | 32,899 | -17.6% |
| ServerTick | deflate_decode | 3,987 | 11,650 | 10,908 | -63.4% |
| ServerTick | snappy_encode | 1,462 | 1,889 | 2,125 | -22.6% |
| ServerTick | snappy_decode | 674 | 2,092 | 2,595 | -67.8% |
| ServerTick | encrypt | 1,303 | 1,376 | private | -5.3% |
| ServerTick | decrypt | 1,240 | 1,276 | private | -2.8% |
| ChunkRadius | framing_encode | 2,513 | 4,500 | 5,661 | -44.2% |
| ChunkRadius | framing_decode | 61 | 213 | 19,236 | -71.2% |
| ChunkRadius | deflate_encode | 595,808 | 589,806 | 989,838 | +1.0% |
| ChunkRadius | deflate_decode | 125,853 | 171,250 | 367,554 | -26.5% |
| ChunkRadius | snappy_encode | 12,061 | 23,607 | 23,747 | -48.9% |
| ChunkRadius | snappy_decode | 5,475 | 8,316 | 34,916 | -34.2% |
| ChunkRadius | encrypt | 80,611 | 81,373 | private | -0.9% |
| ChunkRadius | decrypt | 79,533 | 85,881 | private | -7.4% |
| ClientTick | framing_encode | 24 | 78 | 59 | -59.6% |
| ClientTick | framing_decode | 31 | 111 | 318 | -72.0% |
| ClientTick | deflate_encode | 6,352 | 9,497 | 11,049 | -33.1% |
| ClientTick | deflate_decode | 470 | 1,168 | 1,632 | -59.8% |
| ClientTick | snappy_encode | 239 | 356 | 444 | -32.8% |
| ClientTick | snappy_decode | 164 | 273 | 485 | -39.9% |
| ClientTick | encrypt | 276 | 342 | private | -19.1% |
| ClientTick | decrypt | 267 | 335 | private | -20.5% |

The two positive differences are inside the program's approximately 3% noise
band, not strict wins. Small differences in either direction should not be
interpreted as hardware-independent guarantees.

Final raw matrix evidence:
`tools/bench-compare/results/quiet-batch-1ef651cd19fc4d7fb37c6fcf57995baf/`.
Earlier matrices used the pre-audit non-LTO Cinnabar executable and are
diagnostic only; they are not mixed into this acceptance table.


## Scope and implementation

- Per-connection `batch::BatchCodec` retains libdeflate compressor/decompressor objects, Snappy tables, encode scratch, decode scratch and the last successful decoded-length hint. Convenience free functions retain their existing signatures.
- Typed and raw encoding frame directly into the reusable output, reserving transport/compression markers first. Compression writes to scratch and swaps buffers rather than copying compressed payloads through intermediate allocations.
- `decode_with` validates lengths and delivers synchronous borrowed packet slices. Its compressed output remains `BytesMut` until scratch is returned, avoiding a heap allocation just to freeze/reclaim a `Bytes` handle. Retained raw packets still own zero-copy `Bytes` slices.
- Packet count remains capped at 1,600 on raw/callback paths. Negotiated markers, compression thresholds and decompressed-size limits remain enforced. Raw header parsing uses borrowed input plus a two-byte stack varint rather than a heap buffer; error previews allocate only on failure.
- Raw DEFLATE uses libdeflate with numeric levels 1–9; transport default stays 7 and the comparison uses 6. Output grows geometrically under the limit, remembers the previous decoded length, and falls back to guarded zlib-rs streaming above the 1 MiB speculative ceiling.
- The local flate2 requirement resolves workspace users to 1.1.10; Cargo feature unification also enables its zlib-rs backend for other users of that resolved version. Strict workspace validation covers the resulting dependency graph.
- Public `crypto::BatchCipher` is the implementation used by `BedrockTransport`: owned in-place AES-256-CTR, continuous directional stream, little-endian checksum counter, SHA-256(counter || payload || key), first eight checksum bytes. AES/CTR/SHA dependencies are locally updated to 0.9/0.10/0.11, without changing workspace RUSTFLAGS or other crates' dependency declarations.
- Cipher tail storage is sized once to the selected hardware backend's parallel width. Prefetched bytes are consumed across batches; rejected prefetch at stream exhaustion falls back to exactly the requested tail. Independent Node/OpenSSL vectors cover partial AES blocks and 128-/480-byte group boundaries.
- Benchmark rows now exercise opaque framing rather than additionally parsing IDs and allocating `RawPacket` descriptors. Cinnabar yields `Bytes` views; jolyne's synchronous callback borrows slices; Go returns owned slices. These ownership differences are explicit, not hidden as identical APIs.

## Compression sizes

Bytes include RakNet `0xfe` and the negotiated compression marker. Go sizes were measured through its public `packet.FlateCompression.Compress` / `packet.SnappyCompression.Compress` on the exact pinned manifest batch bytes excluding `0xfe`, plus two envelope bytes.

| Fixture | Plain | Go DEFLATE 6 | Jolyne libdeflate 6 | Go Snappy | Jolyne stock snap |
|---|---:|---:|---:|---:|---:|
| Spawn | 3,476 | 1,009 | 983 | 1,150 | 1,168 |
| ServerTick | 2,250 | 1,352 | 1,334 | 1,592 | 1,591 |
| ChunkRadius | 151,491 | 46,066 | 45,069 | 78,659 | 79,902 |
| ClientTick | 326 | 271 | 257 | 286 | 290 |

All DEFLATE sizes beat Go's size. Stock Snappy exceeds the +0.5% gate for Spawn (+1.57%), ChunkRadius (+1.58%) and ClientTick (+1.40%); ServerTick passes. The coordinator explicitly directed retaining stock snap and landing framing/DEFLATE/crypto, with a separate Snappy encoder follow-up; this report does not claim the original all-row/all-size goal is complete.

## Snappy investigation for the follow-up

Throwaway probes, not landed production implementations:

- Google Snappy 1.3.1 (`snappy_src` 0.2.7), level 1: ServerTick 1,596 B, ChunkRadius 79,902 B, Spawn 1,170 B, ClientTick 284 B. It does not solve the size gate.
- Google Snappy 1.3.1, level 2: 1,564 / 78,021 / 1,138 / 284 B respectively. All sizes pass, but the unpinned ChunkRadius encode probe took about 230 microseconds versus about 18 microseconds at level 1. It was rejected rather than trading the size miss for a large encode regression.
- A safe whole-batch hash6 matcher used a reusable 16,384-entry u32 offset table, three probes per search step, backward extension, 64-bit forward matching, remembered offsets, and ordinary Snappy literal/copy1/copy2/copy4 tags. Unlike snap's 64 KiB blocks, it could reference across the entire batch. One variant emitted ChunkRadius in 77,944 B; a chained-match variant emitted 78,196 B, but small fixtures remained 1,620 / 1,179 / 294 B (ServerTick / Spawn / ClientTick), failing their size gates. Denser probing and reducing the safe tail margin to eight bytes did not solve the small-fixture sizes.
- Those prototype timings were unpinned and are diagnostic only. No prototype/native Snappy backend, custom Snappy FFI, new native Snappy dependency or speculative custom compressor is included in the branch.
- The pinned Go fork uses `s2.EncodeSnappy`, not the slower `EncodeSnappyBetter` wrapper. A follow-up should target a fast Snappy-compatible matcher and retain raw compatibility, exact output limits and allocation reuse; merely switching to Google Snappy level 1 is insufficient.

## Measurement discipline and provenance

- CPU: i9-14900HX, Windows x64; timed children inherit affinity mask `0x10`, logical CPU 4. This P-core and its sibling were reserved for jolyne timing by the coordinator.
- Cargo/Go compilation and final validation run on E-cores 16–31 (`ProcessorAffinity = 0xFFFF0000`), with `CARGO_BUILD_JOBS=8`. No workspace-wide compiler flags were changed; `.cargo/config.toml` retains `-C target-cpu=native`.
- Final cross-matrix ordering is Rust → Cinnabar → Go, Go → Cinnabar → Rust, then Cinnabar → Rust → Go, repeated over five rounds. Rust/Cinnabar use `micro --iterations 3000`; Go uses calibrated `testing.Benchmark` loops. `GOMAXPROCS=4` and `TOKIO_WORKER_THREADS=4`.
- A separate five-round original-main/final comparison alternates order each round, with `micro --iterations 1000` for both Rust binaries. Original-main framing rows include ID/subclient parsing and owned `RawPacket` descriptors; final framing rows use the new opaque synchronous API. The difference is an intentional API improvement, not a claim that these ownership contracts are identical.
- Original-main executable was copied from the cross-bench worker at `b49ecdf`. Its jolyne `batch.rs` Git blob is `8b0202e536c39dfb05ebcb3e2701b55027b10d87`, exactly matching main `01842a7`; its batch harness is the unmodified original raw-API measurement.
- Original-main executable SHA-256: `825869119d652ba1367bd502434dba98b96adaa0ad8107c6d4768113fefe2599`.
- Before-cache executable SHA-256: `6090707521063bf4b7f9716d98f78d095f8ec9792277973f51294123e1c1300d`. This intermediate already has reusable libdeflate framing, but retains the old owned callback slices and scalar-tail cipher behavior.
- Pre-audit benchmark executable SHA-256: `e5cc8b046d961724945d5e81793ec674a9daf69d628924794c5c414ac2f34776`.
- Final Rust executable SHA-256: `b5f19dd85f568e44952e067f1f16e751c1a77da3b48f2a86a0e629072b4d1de5`.
- Final matching-profile Cinnabar executable SHA-256: `49b38625d322568adbc7fc73e85509813c084f47e08a5f230dfe9c532a0635f2`.
- Go executable SHA-256: `af5bf50647d0fbd6ce9881899fc18cf817723fb54aa72c9a7c158706b362cd7f`.
- Initial CPU-2 runs overlapped other workers' timing/compilation and are excluded from acceptance. The interrupted fixed-128-byte-cache experiment is likewise excluded; the final cache follows the backend width (VAES256 uses 30 blocks, not eight).
- Encryption preparation, allocation/refill and verification remain outside timing. Cipher inputs are raw framed entries excluding `0xfe`, without compression. Each encrypt/decrypt call includes per-call `Instant` reads on both Rust implementations. Go crypto remains unavailable because its isolated helpers are private.


## Post-merge verification

Implementation commits `10fe8ec` and `3c4268d` were merged with main
`ca93af9` in `7cc01b6`, including the RakNet changes and comparator profile
audit. The following checks then completed on the E-core validation affinity:

| Command / scenario | Observed result |
|---|---|
| `cargo fmt --all` | Exit 0 |
| `cargo test -p jolyne --lib --tests -p bench-compare` | 101 jolyne tests + 3 harness tests passed; 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo clippy -p jolyne -p bench-compare --all-targets -- -D warnings` | Exit 0; repeated with tests, formatting and workspace Clippy after the final merge check |
| `cargo build --release -p bench-compare` | Exit 0 |
| `cargo build --release -p unastar -p client-diff --bins` | Exit 0 |
| `cargo build --release -p jolyne --example stress_test` | Exit 0 |
| `pwsh -NoProfile -File tools/client-diff/run.ps1 -Runtime C:/Users/jvigu/orca/workspaces/axolotl-stack/stab-client-diff/tools/client-diff/endstone-capture/runtime` | Both BDS scenarios completed; differential runner exited 0 |
| Release unastar, offline copied config, encrypted networking, `stress_test --addr 127.0.0.1:19144 --clients 5 --duration 15` | 5/5 spawned, 0 failures; observed TPS 20.0 / 19.9 / 19.8 with 5 players |

`VCPKG_ROOT=C:/Users/jvigu/vcpkg` was set for server/workspace commands.
Cargo still prints existing build-script informational warnings from
`unastar_noise` and `bleveldb-sys`; strict Clippy completed with exit 0.

Final BDS captures:
`tools/client-diff/runs/20261021-203856-940/`.
Final release smoke logs/config:
`tools/bench-compare/results/smoke-f51789d20e8c4029b6a0fb08cc1e9017/`.
The copied config set `online_mode = false` and bound `127.0.0.1:19144`;
the checked-in server config was not modified and no real client was used.


## Original-main versus new API

Medians of five interleaved, uncontended CPU-4 rounds; 1,000 iterations per
operation in both binaries. These measure the original raw public API versus
the new reusable opaque framing/callback API, including the explicitly
different packet ownership/ID-parsing costs described above.

| Case | Operation | Main before (ns) | New API after (ns) | Speedup |
|---|---|---:|---:|---:|
| Spawn | framing_encode | 330 | 81 | 4.05x |
| Spawn | framing_decode | 2,360 | 60 | 39.46x |
| Spawn | deflate_encode | 28,397 | 13,888 | 2.04x |
| Spawn | deflate_decode | 9,221 | 1,566 | 5.89x |
| ServerTick | framing_encode | 368 | 164 | 2.24x |
| ServerTick | framing_decode | 7,324 | 139 | 52.65x |
| ServerTick | deflate_encode | 126,418 | 13,906 | 9.09x |
| ServerTick | deflate_decode | 14,957 | 3,797 | 3.94x |
| ChunkRadius | framing_encode | 8,425 | 2,323 | 3.63x |
| ChunkRadius | framing_decode | 1,206 | 53 | 22.89x |
| ChunkRadius | deflate_encode | 3,259,210 | 540,476 | 6.03x |
| ChunkRadius | deflate_decode | 260,611 | 117,021 | 2.23x |
| ClientTick | framing_encode | 105 | 23 | 4.58x |
| ClientTick | framing_decode | 621 | 29 | 21.35x |
| ClientTick | deflate_encode | 14,003 | 5,866 | 2.39x |
| ClientTick | deflate_decode | 5,098 | 438 | 11.65x |

Raw paired evidence:
`tools/bench-compare/results/main-pair-a89522419b63477f9243bc75d3d2a676/`.
The original harness had no isolated encryption/decryption or Snappy rows;
unavailable before values are not invented.


## Crypto optimization experiments

The retained follow-up replaces zero-fill-plus-XOR cache generation with
`StreamCipher::try_write_keystream`, preserving the checked exhaustion fallback.
Five CPU-4 rounds compared the same public jolyne API before/after that change:

| Case | Operation | Before (ns) | Direct generation (ns) | Change |
|---|---|---:|---:|---:|
| Spawn | encrypt | 1,944 | 1,886 | -3.0% |
| Spawn | decrypt | 1,854 | 1,910 | +3.1% |
| ServerTick | encrypt | 1,288 | 1,231 | -4.4% |
| ServerTick | decrypt | 1,280 | 1,213 | -5.2% |
| ChunkRadius | encrypt | 75,996 | 73,108 | -3.8% |
| ChunkRadius | decrypt | 73,539 | 78,540 | +6.8% |
| ClientTick | encrypt | 271 | 248 | -8.6% |
| ClientTick | decrypt | 265 | 240 | -9.6% |

The decrypt medians were mixed; they are not hidden as universal improvements.
The direct API removes redundant work and improved every encode median in this
pass, while the residual decrypt uncertainty/misses were explicitly accepted
for follow-up. This pre-profile-audit diagnostic is separate from the final
matched-profile matrix. Raw evidence:
`tools/bench-compare/results/cipher-pair-4d4d2bcc0b4f424fa6ba4ad5ef4d6722/`.

A safe large-batch experiment interleaved SHA/AES over approximately 16 KiB
windows aligned to AES groups and SHA blocks, draining cached stream bytes
first. It preserved independent OpenSSL vectors at 65,535 / 65,536 / 65,537 /
151,490 payload bytes, but five-round measurements showed only approximately
2% lower ChunkRadius encrypt and 2% higher decrypt, with another small-row
regression. It was reverted; no size-selected fusion path remains. Evidence:
`tools/bench-compare/results/fusion-pair-e31f1216a45e48129b2f97dbcbd97dd6/`.

A separate same-process diagnostic alternated both public cipher APIs over
5,000 samples per operation. Its central-80% trimmed means favored jolyne on
all eight crypto cases by approximately 0.5–11%, while the ordinary large
encrypt mean still suffered an outlier-driven loss. That diagnostic used a
different dependency closure and estimator and is **not** substituted for the
acceptance medians; it only confirmed that the large kernels are near parity
and sensitive to scheduling/cache effects. The throwaway probe was not added
to production or the permanent benchmark harness.

The Rust native target was checked on the timing P-core and E-core build mask:
both report `raptorlake` and AES / AVX2 / SHA / VAES support, ruling out different
native feature selection as the cause of the fluctuating crypto gaps.
The `snap` 1.1.1 and 1.1.2 `compress.rs` files also have identical SHA-256
(`e6f3160be400c2a4869d71210a2558ba8b438b835b632a12b7ff487660db093f`);
a patch-only encoder upgrade does not address the documented Snappy matcher gap.

