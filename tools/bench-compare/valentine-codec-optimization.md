# Valentine codec optimization — Bedrock 1.26.51 / protocol 2193

## Outcome

All **30/30 owned-decode medians** and **30/30 encode medians** are below the protocolgen contract crate in the final post-merge cohort. Typed borrowed decode is supported for all 26 single-packet and four batch cases; the retained baseline supported 17/30 borrowed cases. The preferred route is listed per packet rather than claiming borrowed is universally fastest on numeric-only packets, as explicitly accepted by the coordinator. Every final owned median also improves on its pre-optimization baseline.

Implementation commits: `d298133`, `786772b`. Final measured source: `c5c0569106611cd4bd066526c790ee0b8c52e067`, integrating main `e1ed74e786069533df04efea010e3655ce448fb1`. The branch/worktree is retained for coordinator integration; nothing was pushed or merged into main.

## Measurement

- Six rounds, 5,000 iterations per operation, all six permutations of before/after/contract order.
- Windows x64 i9-14900HX; logical **CPU12**, affinity mask **4096**, with sibling13 idle under the coordinator's exclusive allocation.
- Release builds using the workspace's unchanged native target-CPU flags; builds/tests used E-cores 16–31 and at most eight Cargo jobs.
- Compiler: `rustc 1.96.0-nightly (b90dc1e59 2026-03-04)`, LLVM 22.1.0, `x86_64-pc-windows-msvc`.
- Same checksummed corpus and strict framing validation. Borrowed timings explicitly require zero trailing payload bytes. Each invocation validates owned/borrowed values and exact re-encoding outside the timing loop.
- Before is the retained pre-optimization executable, identified by SHA256 in the raw artifact; it is not reconstructed from the final source. Unsupported baseline borrowed cases are shown as `—`, not timed as raw forwarding.
- Timing is a median of the six independent runs, not a claim of sub-percent statistical separation. Earlier CPU2 runs showed timing drift; this table is the final isolated CPU12 cohort. No noise exception is needed for its observed median ordering.

All raw rows, invocation order, coverage diagnostics, executable hashes, corpus-manifest hash and regeneration digests are in [valentine-codec-optimization.json](valentine-codec-optimization.json).

## Decode — ns per packet or named batch, lower is better

| Packet | Before owned | Before borrowed | After owned | After borrowed | Contract | Preferred |
|---|---:|---:|---:|---:|---:|---|
| StartGame | 11,235.27 | — | 7,627.62 | 9,193.29 | 9,313.89 | owned |
| LevelChunk | 305.36 | 71.35 | 55.16 | 57.96 | 66.91 | owned |
| SubChunk | 28,974.06 | — | 16,903.92 | 16,419.04 | 20,610.77 | borrowed |
| CraftingData | 507,600.46 | 326,104.94 | 328,809.93 | 300,598.23 | 336,277.07 | borrowed |
| AvailableCommands | 84,046.39 | 47,054.95 | 47,099.64 | 46,948.50 | 50,295.35 | borrowed |
| BiomeDefinitionList | 10,596.76 | 8,304.72 | 7,437.49 | 7,393.32 | 9,609.75 | borrowed |
| ItemRegistry | 95,219.07 | 73,278.52 | 62,743.93 | 59,525.75 | 80,816.63 | borrowed |
| CreativeContent | 64,449.84 | 47,505.09 | 36,754.22 | 33,969.64 | 46,957.58 | borrowed |
| PlayerAuthInput | 192.52 | — | 153.01 | 126.05 | 178.16 | borrowed |
| PlayerAuthInputInteract | 333.69 | — | 258.39 | 259.05 | 281.60 | owned |
| InventoryTransactionUseItem | 189.90 | — | 111.62 | 123.38 | 147.35 | owned |
| InventoryTransactionUseItemOnEntity | 161.78 | — | 81.83 | 84.26 | 96.68 | owned |
| InventoryTransactionNormal | 498.01 | — | 312.68 | 325.16 | 423.65 | owned |
| ItemStackRequest | 653.98 | — | 438.14 | 462.96 | 551.65 | owned |
| MovePlayer | 74.93 | 53.45 | 32.97 | 39.89 | 74.18 | owned |
| MoveActorDelta | 90.52 | 64.66 | 34.17 | 41.31 | 56.64 | owned |
| SetActorMotion | 37.12 | 49.78 | 29.29 | 34.95 | 40.71 | owned |
| SetActorData | 293.40 | — | 234.28 | 240.12 | 243.96 | owned |
| AddActor | 439.30 | — | 349.51 | 340.42 | 362.35 | borrowed |
| UpdateBlock | 69.23 | 53.96 | 31.89 | 38.94 | 50.93 | owned |
| UpdateAttributes | 476.34 | 393.26 | 286.82 | 251.55 | 366.81 | borrowed |
| LevelSoundEvent | 137.84 | 78.86 | 67.41 | 72.53 | 111.47 | owned |
| Text | 181.26 | 153.91 | 92.13 | 84.26 | 114.54 | borrowed |
| Animate | 63.59 | 46.89 | 29.14 | 33.80 | 35.33 | owned |
| NetworkStackLatency | 27.37 | 41.81 | 21.53 | 26.07 | 31.40 | owned |
| MobEquipment | 103.10 | 79.55 | 53.24 | 60.58 | 62.26 | owned |
| ServerTick (batch) | 6,173.70 | — | 3,239.03 | 3,811.82 | 7,054.72 | owned |
| ChunkRadius (batch) | 3,375.22 | 706.20 | 523.85 | 556.15 | 861.83 | owned |
| Spawn (batch) | 7,837.93 | — | 5,899.70 | 5,857.82 | 6,417.32 | borrowed |
| ClientTick (batch) | 1,093.14 | — | 749.12 | 736.52 | 1,051.85 | borrowed |

## Encode — ns per packet or named batch, lower is better

| Packet | Before Valentine | After Valentine | Contract | Contract / after |
|---|---:|---:|---:|---:|
| StartGame | 794.02 | 643.09 | 1,367.70 | 2.13× |
| LevelChunk | 193.77 | 109.19 | 126.14 | 1.16× |
| SubChunk | 5,087.95 | 4,476.18 | 9,227.32 | 2.06× |
| CraftingData | 45,942.78 | 43,614.37 | 115,170.07 | 2.64× |
| AvailableCommands | 7,370.28 | 7,031.21 | 18,007.40 | 2.56× |
| BiomeDefinitionList | 1,034.76 | 956.02 | 2,459.79 | 2.57× |
| ItemRegistry | 8,943.44 | 8,314.74 | 21,183.34 | 2.55× |
| CreativeContent | 13,729.29 | 13,058.34 | 37,653.76 | 2.88× |
| PlayerAuthInput | 61.79 | 64.19 | 103.75 | 1.62× |
| PlayerAuthInputInteract | 121.18 | 118.58 | 272.84 | 2.30× |
| InventoryTransactionUseItem | 40.92 | 43.59 | 102.25 | 2.35× |
| InventoryTransactionUseItemOnEntity | 60.06 | 55.74 | 78.47 | 1.41× |
| InventoryTransactionNormal | 96.04 | 89.94 | 259.56 | 2.89× |
| ItemStackRequest | 90.60 | 89.42 | 214.02 | 2.39× |
| MovePlayer | 23.48 | 21.45 | 41.00 | 1.91× |
| MoveActorDelta | 20.78 | 17.87 | 39.90 | 2.23× |
| SetActorMotion | 17.34 | 16.02 | 28.68 | 1.79× |
| SetActorData | 55.97 | 56.16 | 182.22 | 3.24× |
| AddActor | 88.92 | 84.21 | 235.95 | 2.80× |
| UpdateBlock | 22.03 | 19.57 | 42.38 | 2.17× |
| UpdateAttributes | 80.17 | 57.65 | 117.44 | 2.04× |
| LevelSoundEvent | 24.15 | 18.66 | 39.46 | 2.11× |
| Text | 29.46 | 28.98 | 52.91 | 1.83× |
| Animate | 14.44 | 10.99 | 21.97 | 2.00× |
| NetworkStackLatency | 12.29 | 9.95 | 15.88 | 1.60× |
| MobEquipment | 22.05 | 19.12 | 43.43 | 2.27× |
| ServerTick (batch) | 1,538.63 | 1,450.75 | 3,458.53 | 2.38× |
| ChunkRadius (batch) | 3,629.48 | 2,469.94 | 2,816.62 | 1.14× |
| Spawn (batch) | 1,614.38 | 1,487.48 | 4,078.50 | 2.74× |
| ClientTick (batch) | 251.27 | 240.55 | 482.40 | 2.01× |

## Decode allocations per operation

| Packet | Before owned | After owned | After borrowed | Contract |
|---|---:|---:|---:|---:|
| StartGame | 94 | 4 | 4 | 4 |
| ItemRegistry | 1301 | 1 | 1 | 1 |
| CreativeContent | 1182 | 2 | 2 | 2 |
| AvailableCommands | 2061 | 290 | 290 | 289 |
| InventoryTransactionUseItemOnEntity | 3 | 0 | 0 | 0 |

Bulk SubChunk decoding retains the same allocation profile as the contract: 433 allocations and 29,376 requested bytes per operation in the measured corpus. The bulk path removes per-element result plumbing, not required vector storage.

## Changes retained

- `bedrock_core::codec::BedrockString` is a `bytestring::ByteString` alias: valid UTF-8 shares immutable packet storage; malformed input is normalized lossily. One unchecked constructor is preceded by complete ASCII/UTF-8 validation of immutable `Bytes`.
- Generated binary buffers and opaque NBT retain `Bytes`; explicit byte collections keep their `Vec` representation. Byte-vector decoding validates length before fallible reservation and uses bulk copying. The signed owning-iterator conversion keeps the observed allocation count unchanged.
- `BorrowedReader` references immutable input instead of cloning a source handle for framing. Owned and typed-borrowed entry APIs bound each frame independently; validated entries are consumed completely even when their header/payload fails. Generic `Buf` fallback remains available for segmented inputs.
- Typed borrowed unions, fixed arrays, packed values and bitsets cover all 30 corpus cases. Legacy frontend `Type::Switch` remains on its existing deferred raw path; canonical protocolgen emits `Union`/`Option`, so no current canonical packet is deferred.
- Native-layout storage estimates replace field-count boxing: 128-byte union payload budget; packet budget reserves a discriminant word within three 64-byte cache lines. Recursive payloads remain boxed. This removed the 128-byte UseItemOnEntity allocation without unchecked layout assumptions.
- Fixed arrays decode through stack `ArrayVec` with prefix drop on error. Known positive wire minima reserve collections once. Network/little-endian NBT scanning is const-mode specialized; one-byte varints use a contiguous fast path with the existing checked segmented fallback.
- Container and union decode methods are inline candidates; only small **one-byte** enum codecs receive the extra inline hint. Blanket small-enum inlining was rejected after a paired StartGame regression.
- `encode_inner_bytes_mut` predicts prefix width from reusable capacity, writes in place, and relocates only on a prediction miss. Contiguous encoding still copies a payload once; this report does not call that scatter/gather zero-copy encoding.
- Application mutable `String`/`Vec` boundaries and direct enum constructors were migrated across Jolyne, Unastar, client-diff, axelerator and bds-extractor. Prefer generated `From<Packet>` construction rather than hand-written boxing.

## Safety and regression coverage

CODEC-01 bounded/fallible preallocation and depth 64, NBT-01 depth 512 and segmented reads, FRAME-01 trailing-byte rejection, and GEN-01 atomic/deterministic output remain exercised. Added/updated behavioral coverage includes all-corpus typed owned/borrowed parity and exact re-encoding, valid UTF-8 sharing and lossy malformed strings, arbitrary binary payloads, adjacent frame/prefix-width boundaries, failed-frame cursor recovery, unknown/truncated union controls, fixed-array prefix drop, segmented varints, and every signed-byte bit pattern at every segment boundary. Synthetic generated-crate regressions reproduce bare-byte-array prefix swallowing and boxed fixed-array type mismatch before their fixes. Independent core, generator, layout and consumer reviews were completed; their actionable findings were corrected and executable regressions passed.

Rejected experiments also included a generic chunk varint scanner, direct source-offset extraction, and replacing the contiguous NBT cursor with a slice cursor; the retained code does not include those regressions.


## Verification

All commands below were exercised successfully; workspace lib/tests, all-target Clippy and the release build were rerun after the final ACK-fix integration. `VCPKG_ROOT=C:\Users\jvigu\vcpkg` was set for workspace native builds, and the protocolgen, vanilla_bp and bedrock-protocol-docs submodules were initialized.

```text
cargo fmt --all
cargo test -j 8 -p valentine --tests -p valentine_bedrock_core -p valentine_gen
cargo test -j 8 --workspace --lib --tests
cargo clippy -j 8 --workspace --all-targets -- -D warnings
cargo build -j 8 --release -p bench-compare
cargo run -j 8 -p valentine_gen -- --latest --proto
```

The core suite passes 92 tests, the generator suite 31, and Valentine integration suites 33. Workspace lib/tests and all-target deny-warning Clippy pass. An intermediate latest-main workspace run exposed the quiet-client ACK test's maintenance/deadline timing overlap; it was reported without a retry, fixed by its owner on main, merged, and the complete workspace gate then passed. No production timing was weakened by this worker.

Determinism: two consecutive raw generator passes are byte-identical across 15 generated version/runtime-surface/manifest files. Running generation followed by `cargo fmt --all` reproduces all 15 previously verified checked-in surfaces; both raw and formatted SHA256 sets are recorded in the JSON. Regenerated artifacts were never hand-edited.

The actual release CLI exercised all 30 cases with typed borrowed coverage and exact canonical roundtrips. Boundary regressions include all 256 signed byte patterns at every segmented-input split, zero length, truncation-before-consumption, malformed UTF-8, varint overflow/overlong encodings, NBT depth boundaries and recursive-budget restoration.

## Reproduction

Build with the commands above. For one timing invocation in PowerShell:

```powershell
[System.Diagnostics.Process]::GetCurrentProcess().ProcessorAffinity = 4096
& target/release/bench-compare.exe codec --iterations 5000
& tools/bench-compare/cinnabar/target/release/cinnabar-bench.exe codec --iterations 5000
```

Alternate runner order across rounds; do not compare an unpinned process or a concurrent process on the same physical core. The standalone Cinnabar runner uses its own project/lockfile. This report covers protocolgen contract codec comparisons; it does not claim unmeasured transport or Go wins.
