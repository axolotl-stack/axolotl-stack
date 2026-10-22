# Cross-implementation benchmark report

Measured UTC: 2026-10-21T22:49:33.5813184Z. Median of 3 alternating rounds; measured echoes per connection: 100 for 32/512 B and 10 for 4096/65536 B; untimed readiness echoes per connection: 1.

Machine: Microsoft Windows 10.0.26200 / X64; Intel64 Family 6 Model 183 Stepping 1, GenuineIntel; 32 logical processors.
Toolchains: `rustc 1.96.0-nightly (b90dc1e59 2026-03-04)`; `go version go1.27.0 windows/amd64`; Go GOMAXPROCS=4; Tokio multi-thread workers=4.
Source base: `78325e4416718a7fd39e73cf7986002bb51dbf66`; protocolgen corpus: `7b2e47c463fa425e25004673cf8960753dd2ff12`.

Shared Windows workstation; codec/micro inherit affinity to logical CPU 2; loopback/interop inherit logical CPUs 0,2,4,6 (distinct P-cores on measured i9-14900HX), matching 4 workers; other workers may run concurrently; no priority, power-plan or timer-resolution adjustment.

Ratios are row-side/Go ns/op (below 1 is faster); throughput is request payload only, decimal MB/s, one completed echo per message. Contract, Valentine and Cinnabar use the same custom-loop estimator in alternating rounds. Medians are descriptive, not significance tests.

**Exploratory record:** fewer than five alternating rounds. This is not the final exclusive-machine optimized-code comparison; do not use these measurements alone for optimization decisions.

Cinnabar pins: raknet/raknet-tokio `7eb32a31f5715ed6cd69120ef19118aa03fcab28`; bedrock-net `0eaf6d4cb38ac1b52328815302c0964645150d17` (root crate only; provider-verification disabled).

## Go transport clock validation

| Round | Clock | Frequency Hz | Tick ns | Minimum observed positive ns |
| ---: | --- | ---: | ---: | ---: |
| 1 | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 2 | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 3 | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |

Startup validation is outside timing. Windows transport uses QueryPerformanceCounter; codec/batch aggregate Go benchmarks retain Go testing.Benchmark over long intervals.

## batch

Re-measured for the jolyne sans-IO core (branch `Jviguy/stab-jolyne-core`,
commit `9d49930` for the batch and join rows, `f8577ce` for the stream rows)
against jolyne on main (`6e3b96c`), Cinnabar bedrock-net (`0eaf6d4`, the
`tools/bench-compare/cinnabar` harness) and gophertunnel (HashimTheArab fork
`80c811b61860`, `tools/bench-compare/go`).

Method: release builds with `-j8` on logical CPUs 16–23. Each round runs every
implementation once, alternating the order and the pinned P-core (logical CPU 2
on even rounds, 4 on odd rounds); 9 rounds for batch and join, 20 for stream.
Rust and Cinnabar use 5,000 iterations after 32 warm-ups (`bench-compare batch`,
`cinnabar-bench micro`); Go uses calibrated `testing.Benchmark` loops. Values
are medians across rounds, in ns/op. The workstation is shared with other
workers; differences inside about ±3% are noise. Go's isolated encryption
helpers are private, so it has no crypto rows.

| Case | Operation | jolyne main (ns) | jolyne branch (ns) | bedrock-net (ns) | gophertunnel (ns) | branch vs main | branch vs fastest reference |
|---|---|---:|---:|---:|---:|---:|---:|
| Spawn | framing_encode | 102 | 96 | 156 | 198 | -5.6% | -38.3% |
| Spawn | framing_decode | 74 | 74 | 428 | 2,198 | +0.0% | -82.6% |
| Spawn | deflate_encode | 15,342 | 14,323 | 19,419 | 29,423 | -6.6% | -26.2% |
| Spawn | deflate_decode | 1,708 | 1,691 | 3,164 | 10,423 | -1.0% | -46.6% |
| Spawn | snappy_encode | 1,204 | 1,224 | 1,751 | 1,439 | +1.7% | -15.0% |
| Spawn | snappy_decode | 560 | 570 | 938 | 2,592 | +1.9% | -39.2% |
| Spawn | encrypt | 2,073 | 2,096 | 2,119 | private | +1.1% | -1.1% |
| Spawn | decrypt | 2,051 | 2,082 | 2,126 | private | +1.5% | -2.0% |
| ServerTick | framing_encode | 209 | 189 | 274 | 467 | -9.5% | -31.0% |
| ServerTick | framing_decode | 160 | 158 | 1,471 | 2,239 | -1.2% | -89.2% |
| ServerTick | deflate_encode | 15,331 | 15,042 | 18,356 | 30,664 | -1.9% | -18.1% |
| ServerTick | deflate_decode | 4,396 | 4,360 | 13,276 | 11,704 | -0.8% | -62.7% |
| ServerTick | snappy_encode | 1,532 | 1,528 | 2,002 | 2,014 | -0.3% | -23.7% |
| ServerTick | snappy_decode | 641 | 644 | 2,093 | 2,947 | +0.4% | -69.2% |
| ServerTick | encrypt | 1,398 | 1,362 | 1,427 | private | -2.6% | -4.6% |
| ServerTick | decrypt | 1,349 | 1,350 | 1,399 | private | +0.0% | -3.6% |
| ChunkRadius | framing_encode | 2,656 | 2,707 | 5,087 | 5,464 | +1.9% | -46.8% |
| ChunkRadius | framing_decode | 66 | 65 | 230 | 24,747 | -1.0% | -71.5% |
| ChunkRadius | deflate_encode | 582,313 | 576,150 | 621,228 | 906,333 | -1.1% | -7.3% |
| ChunkRadius | deflate_decode | 132,992 | 132,948 | 194,150 | 378,480 | -0.0% | -31.5% |
| ChunkRadius | snappy_encode | 17,414 | 17,559 | 24,041 | 22,669 | +0.8% | -22.5% |
| ChunkRadius | snappy_decode | 6,194 | 6,388 | 9,154 | 39,032 | +3.1% | -30.2% |
| ChunkRadius | encrypt | 88,154 | 87,062 | 88,059 | private | -1.2% | -1.1% |
| ChunkRadius | decrypt | 87,582 | 87,355 | 86,885 | private | -0.3% | +0.5% |
| ClientTick | framing_encode | 26 | 26 | 70 | 59 | -0.8% | -55.4% |
| ClientTick | framing_decode | 39 | 38 | 119 | 379 | -1.7% | -68.0% |
| ClientTick | deflate_encode | 7,116 | 6,893 | 11,491 | 9,975 | -3.1% | -30.9% |
| ClientTick | deflate_decode | 531 | 507 | 1,125 | 1,943 | -4.6% | -54.9% |
| ClientTick | snappy_encode | 271 | 268 | 379 | 416 | -0.9% | -29.2% |
| ClientTick | snappy_decode | 166 | 160 | 277 | 557 | -3.5% | -42.2% |
| ClientTick | encrypt | 291 | 285 | 331 | private | -2.2% | -13.9% |
| ClientTick | decrypt | 285 | 282 | 323 | private | -1.2% | -12.7% |

Jolyne is fastest on 31 of 32 rows. ChunkRadius decrypt (151 KB) is a tie with
bedrock-net (+0.5%, inside noise): both run RustCrypto AES-CTR plus `sha2`
SHA-256, and the SHA-256 pass (about 80% of the time, SHA-NI bound) is the
limit. Measured separately, `ring`'s SHA-256 is no faster on this CPU, and
interleaving the AES and SHA passes in 2 KiB or 16 KiB chunks made large
batches slower, so neither was adopted. Against main, every row is within the
noise band; the branch replaced the per-byte `subtle` checksum compare with one
u64 compare, which removed an earlier small-batch decrypt regression.

### join (jolyne main vs branch)

`bench-compare join --joins 20 --count 200`: a full offline join over RakNet
loopback (settings, login, ECDH encryption, packs, StartGame, spawn), then Text
echoes through the server's typed path. Both peers are in one process pinned to
logical CPUs 2 and 4 with two Tokio workers. The join time is dominated by
RakNet connection setup and is the same on both revisions.

| Row | jolyne main | jolyne branch | branch vs main |
|---|---:|---:|---:|
| `join/raknet/join` (ms, mean of medians) | 115.40 | 115.32 | -0.1% |
| `join/raknet/echo` (µs, mean of medians) | 31.23 | 31.28 | +0.2% |

### stream (jolyne main vs branch)

`bench-compare stream --iterations 2000`: every batch fixture sent with
`send_batch_raw`/`send_batch` and received again with
`recv_packet_raw`/`recv_packet` through `BedrockTransport` over an in-memory
self-loop transport (DEFLATE 6 at threshold 256, AES-CTR). Allocation counts are
deterministic; timings on this shared machine vary by up to ±15% between runs.

| Case | Operation | main (ns) | branch (ns) | branch vs main | allocs/op main → branch |
|---|---|---:|---:|---:|---:|
| Spawn | raw_deflate | 21,019 | 22,285 | +6.0% | 6 → 2 |
| Spawn | raw_encrypted | 7,236 | 6,582 | -9.0% | 4 → 2 |
| Spawn | raw_deflate_encrypted | 23,708 | 24,024 | +1.3% | 6 → 4 |
| Spawn | typed_deflate_encrypted | 35,060 | 35,158 | +0.3% | 48 → 46 |
| ServerTick | raw_deflate | 25,850 | 24,207 | -6.4% | 8 → 2 |
| ServerTick | raw_encrypted | 7,656 | 6,739 | -12.0% | 6 → 2 |
| ServerTick | raw_deflate_encrypted | 28,383 | 26,477 | -6.7% | 8 → 4 |
| ServerTick | typed_deflate_encrypted | 36,439 | 35,272 | -3.2% | 18 → 14 |
| ChunkRadius | raw_deflate | 779,813 | 803,019 | +3.0% | 5 → 2 |
| ChunkRadius | raw_encrypted | 951,158 | 989,350 | +4.0% | 5 → 4 |
| ChunkRadius | raw_deflate_encrypted | 992,603 | 959,934 | -3.3% | 5 → 4 |
| ChunkRadius | typed_deflate_encrypted | 937,890 | 954,700 | +1.8% | 5 → 4 |
| ClientTick | raw_deflate | 9,571 | 9,882 | +3.2% | 4 → 2 |
| ClientTick | raw_encrypted | 1,193 | 1,378 | +15.5% | 2 → 2 |
| ClientTick | raw_deflate_encrypted | 10,373 | 10,184 | -1.8% | 4 → 4 |
| ClientTick | typed_deflate_encrypted | 11,968 | 12,177 | +1.7% | 12 → 12 |

The new core keeps one connection-owned queue instead of building a `Vec` per
batch, frames outbound packets straight into the pending batch with one
up-front reservation, and decrypts in place when it holds the only reference.
That cuts allocations per round trip on every raw row (up to 8 → 2).

## codec

| Case | Operation | Valentine/jolyne owned ns/op | Valentine borrowed ns/op | Contract ns/op | Cinnabar ns/op | Go ns/op | flate2-rust ns/op | Fastest measured |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| AddActor | decode | 523.14 | — | 385.04 | — | 1,446.39 | — | contract |
| AddActor | encode | 87.30 | — | 269.80 | — | 788.39 | — | rust-owned |
| Animate | decode | 65.08 | 47.46 | 33.10 | — | 122.90 | — | contract |
| Animate | encode | 13.42 | — | 24.26 | — | 75.50 | — | rust-owned |
| AvailableCommands | decode | 98,647.38 | 58,910.28 | 49,998.18 | — | 97,281.55 | — | contract |
| AvailableCommands | encode | 8,247.86 | — | 18,816.02 | — | 50,771.92 | — | rust-owned |
| BiomeDefinitionList | decode | 11,726.32 | 8,938.90 | 10,762.80 | — | 27,018.21 | — | rust-borrowed |
| BiomeDefinitionList | encode | 1,421.38 | — | 2,766.76 | — | 11,076.16 | — | rust-owned |
| ChunkRadius | compress | — | — | — | — | 922,933.93 | — | unmatched |
| ChunkRadius | decode_batch | 3,608.38 | 724.02 | 873.18 | — | 27,326.43 | — | rust-borrowed |
| ChunkRadius | decode_compressed_batch | — | — | — | — | 338,191.25 | — | unmatched |
| ChunkRadius | decompress | — | — | — | — | 379,762.66 | — | unmatched |
| ChunkRadius | encode_batch | 3,821.40 | — | 2,788.24 | — | 9,768.35 | — | contract |
| ClientTick | compress | — | — | — | — | 10,285.05 | — | unmatched |
| ClientTick | decode_batch | 1,167.50 | — | 1,091.40 | — | 2,985.81 | — | contract |
| ClientTick | decode_compressed_batch | — | — | — | — | 4,948.03 | — | unmatched |
| ClientTick | decompress | — | — | — | — | 1,554.45 | — | unmatched |
| ClientTick | encode_batch | 289.16 | — | 653.82 | — | 1,454.88 | — | rust-owned |
| CraftingData | decode | 501,066.26 | 371,836.40 | 350,563.32 | — | — | — | contract |
| CraftingData | encode | 54,636.04 | — | 122,584.68 | — | — | — | rust-owned |
| CreativeContent | decode | 76,425.54 | 50,126.30 | 48,229.78 | — | 597,561.29 | — | contract |
| CreativeContent | encode | 15,996.14 | — | 37,721.18 | — | 279,278.66 | — | rust-owned |
| InventoryTransactionNormal | decode | 592.72 | — | 435.26 | — | 4,525.31 | — | contract |
| InventoryTransactionNormal | encode | 100.68 | — | 266.58 | — | 2,742.99 | — | rust-owned |
| InventoryTransactionUseItem | decode | 193.42 | — | 141.72 | — | 651.75 | — | contract |
| InventoryTransactionUseItem | encode | 46.12 | — | 103.60 | — | 352.05 | — | rust-owned |
| InventoryTransactionUseItemOnEntity | decode | 165.84 | — | 97.56 | — | 1,183.83 | — | contract |
| InventoryTransactionUseItemOnEntity | encode | 65.04 | — | 82.24 | — | 839.01 | — | rust-owned |
| ItemRegistry | decode | 101,517.52 | 80,943.74 | 83,137.42 | — | 593,459.21 | — | rust-borrowed |
| ItemRegistry | encode | 9,831.36 | — | 22,892.40 | — | 268,769.88 | — | rust-owned |
| ItemStackRequest | decode | 896.70 | — | 738.30 | — | 1,409.20 | — | contract |
| ItemStackRequest | encode | 102.52 | — | 229.98 | — | 714.12 | — | rust-owned |
| LevelChunk | decode | 377.92 | 75.96 | 66.18 | — | 3,260.89 | — | contract |
| LevelChunk | encode | 207.68 | — | 132.44 | — | 531.13 | — | contract |
| LevelSoundEvent | decode | 138.98 | 81.12 | 106.22 | — | 235.59 | — | rust-borrowed |
| LevelSoundEvent | encode | 22.60 | — | 45.50 | — | 103.92 | — | rust-owned |
| MobEquipment | decode | 104.70 | 79.80 | 63.12 | — | 407.96 | — | contract |
| MobEquipment | encode | 21.24 | — | 49.12 | — | 199.51 | — | rust-owned |
| MoveActorDelta | decode | 90.66 | 66.58 | 58.50 | — | 164.09 | — | contract |
| MoveActorDelta | encode | 20.90 | — | 44.78 | — | 117.28 | — | rust-owned |
| MovePlayer | decode | 78.18 | 57.50 | 73.28 | — | 209.00 | — | rust-borrowed |
| MovePlayer | encode | 23.70 | — | 43.86 | — | 107.50 | — | rust-owned |
| NetworkStackLatency | decode | 28.86 | 44.54 | 31.62 | — | 101.24 | — | rust-owned |
| NetworkStackLatency | encode | 12.98 | — | 17.80 | — | 43.69 | — | rust-owned |
| PlayerAuthInput | decode | 198.62 | — | 176.44 | — | 437.12 | — | contract |
| PlayerAuthInput | encode | 65.70 | — | 109.62 | — | 244.59 | — | rust-owned |
| PlayerAuthInputInteract | decode | 351.80 | — | 283.10 | — | 1,060.95 | — | contract |
| PlayerAuthInputInteract | encode | 114.34 | — | 293.70 | — | 685.67 | — | rust-owned |
| ServerTick | compress | — | — | — | — | 33,173.31 | — | unmatched |
| ServerTick | decode_batch | 6,313.48 | — | 6,292.78 | — | 18,701.99 | — | contract |
| ServerTick | decode_compressed_batch | — | — | — | — | 25,565.99 | — | unmatched |
| ServerTick | decompress | — | — | — | — | 8,713.11 | — | unmatched |
| ServerTick | encode_batch | 1,930.44 | — | 3,892.26 | — | 10,978.40 | — | rust-owned |
| SetActorData | decode | 337.92 | — | 240.02 | — | 1,016.40 | — | contract |
| SetActorData | encode | 52.74 | — | 181.24 | — | 682.13 | — | rust-owned |
| SetActorMotion | decode | 37.84 | 51.24 | 38.80 | — | 122.76 | — | rust-owned |
| SetActorMotion | encode | 16.96 | — | 31.46 | — | 62.96 | — | rust-owned |
| Spawn | compress | — | — | — | — | 30,698.58 | — | unmatched |
| Spawn | decode_batch | 10,251.56 | — | 6,904.20 | — | 35,978.13 | — | contract |
| Spawn | decode_compressed_batch | — | — | — | — | 41,561.18 | — | unmatched |
| Spawn | decompress | — | — | — | — | 9,137.63 | — | unmatched |
| Spawn | encode_batch | 1,862.22 | — | 4,568.44 | — | 18,435.05 | — | rust-owned |
| StartGame | decode | 11,853.90 | — | 9,978.78 | — | 163,851.81 | — | contract |
| StartGame | encode | 912.44 | — | 1,510.90 | — | 89,808.74 | — | rust-owned |
| SubChunk | decode | 29,706.60 | — | 20,662.48 | — | 42,036.36 | — | contract |
| SubChunk | encode | 5,293.30 | — | 11,857.60 | — | 36,500.76 | — | rust-owned |
| Text | decode | 190.32 | 154.02 | 112.66 | — | 288.22 | — | contract |
| Text | encode | 27.92 | — | 55.12 | — | 112.01 | — | rust-owned |
| UpdateAttributes | decode | 480.68 | 440.50 | 398.96 | — | 866.76 | — | contract |
| UpdateAttributes | encode | 53.72 | — | 131.60 | — | 382.67 | — | rust-owned |
| UpdateBlock | decode | 71.62 | 55.44 | 48.72 | — | 162.50 | — | contract |
| UpdateBlock | encode | 20.22 | — | 43.28 | — | 80.38 | — | rust-owned |

<details><summary>Allocation requests (median per round)</summary>

| Case | Operation | Side | allocs/op | allocated B/op |
| --- | --- | --- | ---: | ---: |
| AddActor | decode | contract | 3.00 | 736.00 |
| AddActor | decode | go | 23.00 | 1,168.02 |
| AddActor | decode | rust-owned | 7.00 | 784.00 |
| AddActor | encode | contract | 0.00 | 0.00 |
| AddActor | encode | go | 6.00 | 128.00 |
| AddActor | encode | rust-owned | 0.00 | 0.00 |
| Animate | decode | contract | 0.00 | 0.00 |
| Animate | decode | go | 4.00 | 128.00 |
| Animate | decode | rust-borrowed | 0.00 | 0.00 |
| Animate | decode | rust-owned | 1.00 | 40.00 |
| Animate | encode | contract | 0.00 | 0.00 |
| Animate | encode | go | 2.00 | 56.00 |
| Animate | encode | rust-owned | 0.00 | 0.00 |
| AvailableCommands | decode | contract | 289.00 | 76,400.00 |
| AvailableCommands | decode | go | 2,513.01 | 83,969.66 |
| AvailableCommands | decode | rust-borrowed | 289.00 | 76,201.00 |
| AvailableCommands | decode | rust-owned | 2,061.00 | 87,035.00 |
| AvailableCommands | encode | contract | 0.00 | 0.00 |
| AvailableCommands | encode | go | 451.00 | 2,792.06 |
| AvailableCommands | encode | rust-owned | 0.00 | 0.00 |
| BiomeDefinitionList | decode | contract | 90.00 | 79,546.00 |
| BiomeDefinitionList | decode | go | 291.02 | 103,810.23 |
| BiomeDefinitionList | decode | rust-borrowed | 90.00 | 79,546.00 |
| BiomeDefinitionList | decode | rust-owned | 198.00 | 78,984.00 |
| BiomeDefinitionList | encode | contract | 0.00 | 0.00 |
| BiomeDefinitionList | encode | go | 91.00 | 392.01 |
| BiomeDefinitionList | encode | rust-owned | 0.00 | 0.00 |
| ChunkRadius | compress | go | 1.04 | 49,978.30 |
| ChunkRadius | decode_batch | contract | 0.00 | 0.00 |
| ChunkRadius | decode_batch | go | 49.03 | 167,499.40 |
| ChunkRadius | decode_batch | rust-borrowed | 0.00 | 0.00 |
| ChunkRadius | decode_batch | rust-owned | 18.00 | 152,073.00 |
| ChunkRadius | decode_compressed_batch | go | 50.23 | 167,855.05 |
| ChunkRadius | decompress | go | 2.20 | 155,957.06 |
| ChunkRadius | encode_batch | contract | 0.00 | 0.00 |
| ChunkRadius | encode_batch | go | 20.00 | 374.57 |
| ChunkRadius | encode_batch | rust-owned | 0.00 | 0.00 |
| ClientTick | compress | go | 1.00 | 296.14 |
| ClientTick | decode_batch | contract | 8.00 | 1,104.00 |
| ClientTick | decode_batch | go | 60.00 | 2,312.03 |
| ClientTick | decode_batch | rust-owned | 18.00 | 1,463.00 |
| ClientTick | decode_compressed_batch | go | 61.00 | 2,362.21 |
| ClientTick | decompress | go | 2.00 | 400.59 |
| ClientTick | encode_batch | contract | 0.00 | 0.00 |
| ClientTick | encode_batch | go | 27.00 | 324.02 |
| ClientTick | encode_batch | rust-owned | 0.00 | 0.00 |
| CraftingData | decode | contract | 3,527.00 | 362,516.00 |
| CraftingData | decode | rust-borrowed | 3,526.00 | 362,244.00 |
| CraftingData | decode | rust-owned | 9,971.00 | 395,719.00 |
| CraftingData | encode | contract | 0.00 | 0.00 |
| CraftingData | encode | rust-owned | 0.00 | 0.00 |
| CreativeContent | decode | contract | 2.00 | 65,120.00 |
| CreativeContent | decode | go | 9,934.09 | 540,594.12 |
| CreativeContent | decode | rust-borrowed | 2.00 | 65,120.00 |
| CreativeContent | decode | rust-owned | 1,182.00 | 75,075.00 |
| CreativeContent | encode | contract | 0.00 | 0.00 |
| CreativeContent | encode | go | 5,085.01 | 83,314.84 |
| CreativeContent | encode | rust-owned | 0.00 | 0.00 |
| InventoryTransactionNormal | decode | contract | 1.00 | 544.00 |
| InventoryTransactionNormal | decode | go | 95.00 | 5,016.08 |
| InventoryTransactionNormal | decode | rust-owned | 10.00 | 699.00 |
| InventoryTransactionNormal | encode | contract | 0.00 | 0.00 |
| InventoryTransactionNormal | encode | go | 53.00 | 800.06 |
| InventoryTransactionNormal | encode | rust-owned | 0.00 | 0.00 |
| InventoryTransactionUseItem | decode | contract | 1.00 | 144.00 |
| InventoryTransactionUseItem | decode | go | 16.00 | 570.68 |
| InventoryTransactionUseItem | decode | rust-owned | 3.00 | 210.00 |
| InventoryTransactionUseItem | encode | contract | 0.00 | 0.00 |
| InventoryTransactionUseItem | encode | go | 10.00 | 92.00 |
| InventoryTransactionUseItem | encode | rust-owned | 0.00 | 0.00 |
| InventoryTransactionUseItemOnEntity | decode | contract | 0.00 | 0.00 |
| InventoryTransactionUseItemOnEntity | decode | go | 24.00 | 1,328.02 |
| InventoryTransactionUseItemOnEntity | decode | rust-owned | 3.00 | 219.00 |
| InventoryTransactionUseItemOnEntity | encode | contract | 0.00 | 0.00 |
| InventoryTransactionUseItemOnEntity | encode | go | 13.00 | 220.01 |
| InventoryTransactionUseItemOnEntity | encode | rust-owned | 0.00 | 0.00 |
| ItemRegistry | decode | contract | 1.00 | 104,000.00 |
| ItemRegistry | decode | go | 10,805.09 | 544,834.16 |
| ItemRegistry | decode | rust-borrowed | 1.00 | 104,000.00 |
| ItemRegistry | decode | rust-owned | 1,301.00 | 120,192.00 |
| ItemRegistry | encode | contract | 0.00 | 0.00 |
| ItemRegistry | encode | go | 3,802.01 | 112,836.79 |
| ItemRegistry | encode | rust-owned | 0.00 | 0.00 |
| ItemStackRequest | decode | contract | 6.00 | 640.00 |
| ItemStackRequest | decode | go | 35.00 | 1,000.01 |
| ItemStackRequest | decode | rust-owned | 12.00 | 773.00 |
| ItemStackRequest | encode | contract | 0.00 | 0.00 |
| ItemStackRequest | encode | go | 15.00 | 116.01 |
| ItemStackRequest | encode | rust-owned | 0.00 | 0.00 |
| LevelChunk | decode | contract | 0.00 | 0.00 |
| LevelChunk | decode | go | 5.00 | 18,596.38 |
| LevelChunk | decode | rust-borrowed | 0.00 | 0.00 |
| LevelChunk | decode | rust-owned | 2.00 | 16,897.00 |
| LevelChunk | encode | contract | 0.00 | 0.00 |
| LevelChunk | encode | go | 2.00 | 36.00 |
| LevelChunk | encode | rust-owned | 0.00 | 0.00 |
| LevelSoundEvent | decode | contract | 1.00 | 112.00 |
| LevelSoundEvent | decode | go | 5.00 | 180.00 |
| LevelSoundEvent | decode | rust-borrowed | 0.00 | 0.00 |
| LevelSoundEvent | decode | rust-owned | 3.00 | 116.00 |
| LevelSoundEvent | encode | contract | 0.00 | 0.00 |
| LevelSoundEvent | encode | go | 1.00 | 32.00 |
| LevelSoundEvent | encode | rust-owned | 0.00 | 0.00 |
| MobEquipment | decode | contract | 0.00 | 0.00 |
| MobEquipment | decode | go | 10.00 | 362.67 |
| MobEquipment | decode | rust-borrowed | 0.00 | 0.00 |
| MobEquipment | decode | rust-owned | 2.00 | 74.00 |
| MobEquipment | encode | contract | 0.00 | 0.00 |
| MobEquipment | encode | go | 5.00 | 76.00 |
| MobEquipment | encode | rust-owned | 0.00 | 0.00 |
| MoveActorDelta | decode | contract | 0.00 | 0.00 |
| MoveActorDelta | decode | go | 3.00 | 160.00 |
| MoveActorDelta | decode | rust-borrowed | 0.00 | 0.00 |
| MoveActorDelta | decode | rust-owned | 1.00 | 56.00 |
| MoveActorDelta | encode | contract | 0.00 | 0.00 |
| MoveActorDelta | encode | go | 1.00 | 32.00 |
| MoveActorDelta | encode | rust-owned | 0.00 | 0.00 |
| MovePlayer | decode | contract | 1.00 | 64.00 |
| MovePlayer | decode | go | 3.00 | 160.00 |
| MovePlayer | decode | rust-borrowed | 0.00 | 0.00 |
| MovePlayer | decode | rust-owned | 1.00 | 64.00 |
| MovePlayer | encode | contract | 0.00 | 0.00 |
| MovePlayer | encode | go | 1.00 | 32.00 |
| MovePlayer | encode | rust-owned | 0.00 | 0.00 |
| NetworkStackLatency | decode | contract | 0.00 | 0.00 |
| NetworkStackLatency | decode | go | 3.00 | 96.00 |
| NetworkStackLatency | decode | rust-borrowed | 0.00 | 0.00 |
| NetworkStackLatency | decode | rust-owned | 0.00 | 0.00 |
| NetworkStackLatency | encode | contract | 0.00 | 0.00 |
| NetworkStackLatency | encode | go | 1.00 | 32.00 |
| NetworkStackLatency | encode | rust-owned | 0.00 | 0.00 |
| PlayerAuthInput | decode | contract | 2.00 | 464.00 |
| PlayerAuthInput | decode | go | 5.00 | 612.01 |
| PlayerAuthInput | decode | rust-owned | 2.00 | 456.00 |
| PlayerAuthInput | encode | contract | 0.00 | 0.00 |
| PlayerAuthInput | encode | go | 2.00 | 36.00 |
| PlayerAuthInput | encode | rust-owned | 0.00 | 0.00 |
| PlayerAuthInputInteract | decode | contract | 3.00 | 528.00 |
| PlayerAuthInputInteract | decode | go | 18.00 | 856.02 |
| PlayerAuthInputInteract | decode | rust-owned | 4.00 | 530.00 |
| PlayerAuthInputInteract | encode | contract | 0.00 | 0.00 |
| PlayerAuthInputInteract | encode | go | 11.00 | 96.01 |
| PlayerAuthInputInteract | encode | rust-owned | 0.00 | 0.00 |
| ServerTick | compress | go | 1.00 | 1,433.50 |
| ServerTick | decode_batch | contract | 14.00 | 2,640.00 |
| ServerTick | decode_batch | go | 280.00 | 13,624.18 |
| ServerTick | decode_batch | rust-owned | 83.00 | 5,738.00 |
| ServerTick | decode_compressed_batch | go | 281.01 | 13,683.44 |
| ServerTick | decompress | go | 2.00 | 2,356.30 |
| ServerTick | encode_batch | contract | 0.00 | 0.00 |
| ServerTick | encode_batch | go | 84.00 | 2,460.51 |
| ServerTick | encode_batch | rust-owned | 0.00 | 0.00 |
| SetActorData | decode | contract | 2.00 | 440.00 |
| SetActorData | decode | go | 18.00 | 960.01 |
| SetActorData | decode | rust-owned | 4.00 | 550.00 |
| SetActorData | encode | contract | 0.00 | 0.00 |
| SetActorData | encode | go | 4.00 | 120.00 |
| SetActorData | encode | rust-owned | 0.00 | 0.00 |
| SetActorMotion | decode | contract | 0.00 | 0.00 |
| SetActorMotion | decode | go | 3.00 | 112.00 |
| SetActorMotion | decode | rust-borrowed | 0.00 | 0.00 |
| SetActorMotion | decode | rust-owned | 0.00 | 0.00 |
| SetActorMotion | encode | contract | 0.00 | 0.00 |
| SetActorMotion | encode | go | 1.00 | 32.00 |
| SetActorMotion | encode | rust-owned | 0.00 | 0.00 |
| Spawn | compress | go | 1.00 | 1,049.83 |
| Spawn | decode_batch | contract | 42.00 | 11,864.00 |
| Spawn | decode_batch | go | 531.00 | 27,976.46 |
| Spawn | decode_batch | rust-owned | 118.00 | 12,780.00 |
| Spawn | decode_compressed_batch | go | 532.04 | 28,041.33 |
| Spawn | decompress | go | 2.01 | 4,149.24 |
| Spawn | encode_batch | contract | 0.00 | 0.00 |
| Spawn | encode_batch | go | 197.00 | 3,233.03 |
| Spawn | encode_batch | rust-owned | 0.00 | 0.00 |
| StartGame | decode | contract | 4.00 | 5,816.00 |
| StartGame | decode | go | 3,054.04 | 193,204.55 |
| StartGame | decode | rust-owned | 94.00 | 6,456.00 |
| StartGame | encode | contract | 0.00 | 0.00 |
| StartGame | encode | go | 975.00 | 17,540.19 |
| StartGame | encode | rust-owned | 0.00 | 0.00 |
| SubChunk | decode | contract | 433.00 | 29,376.00 |
| SubChunk | decode | go | 464.01 | 80,453.61 |
| SubChunk | decode | rust-owned | 488.00 | 96,168.00 |
| SubChunk | encode | contract | 0.00 | 0.00 |
| SubChunk | encode | go | 434.00 | 1,764.01 |
| SubChunk | encode | rust-owned | 0.00 | 0.00 |
| Text | decode | contract | 0.00 | 0.00 |
| Text | decode | go | 7.00 | 265.00 |
| Text | decode | rust-borrowed | 2.00 | 39.00 |
| Text | decode | rust-owned | 4.00 | 199.00 |
| Text | encode | contract | 0.00 | 0.00 |
| Text | encode | go | 2.00 | 33.00 |
| Text | encode | rust-owned | 0.00 | 0.00 |
| UpdateAttributes | decode | contract | 2.00 | 480.00 |
| UpdateAttributes | decode | go | 18.00 | 784.01 |
| UpdateAttributes | decode | rust-borrowed | 2.00 | 480.00 |
| UpdateAttributes | decode | rust-owned | 9.00 | 592.00 |
| UpdateAttributes | encode | contract | 0.00 | 0.00 |
| UpdateAttributes | encode | go | 7.00 | 56.00 |
| UpdateAttributes | encode | rust-owned | 0.00 | 0.00 |
| UpdateBlock | decode | contract | 0.00 | 0.00 |
| UpdateBlock | decode | go | 3.00 | 104.00 |
| UpdateBlock | decode | rust-borrowed | 0.00 | 0.00 |
| UpdateBlock | decode | rust-owned | 1.00 | 24.00 |
| UpdateBlock | encode | contract | 0.00 | 0.00 |
| UpdateBlock | encode | go | 1.00 | 32.00 |
| UpdateBlock | encode | rust-owned | 0.00 | 0.00 |

</details>

## loopback

| Size / operation | Side | Samples | Mean µs | msgs/s | MB/s | p50 µs | p99 µs | Max µs | Side/Go |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 32 / echo | cinnabar | 100 | 359.67 | 2,780.37 | 0.09 | 40.80 | 10,322.20 | 21,365.10 | 10.30 |
| 32 / echo | go | 100 | 34.91 | 28,642.63 | 0.92 | 34.80 | 65.80 | 72.70 | — |
| 32 / echo | rust-owned | 100 | 1,282.42 | 779.78 | 0.02 | 62.40 | 225.30 | 120,259.90 | 36.73 |
| 4096 / echo | cinnabar | 10 | 8,539.06 | 117.11 | 0.48 | 238.80 | 23,117.20 † | 23,117.20 | 69.21 |
| 4096 / echo | go | 10 | 123.38 | 8,105.04 | 33.20 | 122.90 | 179.70 † | 179.70 | — |
| 4096 / echo | rust-owned | 10 | 39,727.29 | 25.17 | 0.10 | 315.00 | 101,627.20 † | 101,627.20 | 321.99 |
| 512 / echo | cinnabar | 100 | 771.76 | 1,295.74 | 0.66 | 71.80 | 15,028.80 | 18,194.90 | 18.94 |
| 512 / echo | go | 100 | 40.75 | 24,541.68 | 12.57 | 35.10 | 109.00 | 150.40 | — |
| 512 / echo | rust-owned | 100 | 2,857.12 | 350.00 | 0.18 | 79.60 | 82,774.00 | 98,797.40 | 70.12 |
| 65536 / echo | cinnabar | 10 | 7,790.56 | 128.36 | 8.41 | 1,106.40 | 22,808.90 † | 22,808.90 | 7.84 |
| 65536 / echo | go | 10 | 994.17 | 1,005.86 | 65.92 | 960.50 | 1,151.10 † | 1,151.10 | — |
| 65536 / echo | rust-owned | 10 | 32,541.27 | 30.73 | 2.01 | 1,203.60 | 101,414.90 † | 101,414.90 | 32.73 |

† Low-sample p99: for ten samples, nearest-rank p99 is the maximum, not a stable population tail estimate. All implementations use identical counts and fresh connections. Columns are medians of per-round mean/percentile/max statistics, not pooled samples.

## micro

| Case | Operation | Valentine/jolyne owned ns/op | Valentine borrowed ns/op | Contract ns/op | Cinnabar ns/op | Go ns/op | flate2-rust ns/op | Fastest measured |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| ack_3_ranges | decode | 60.44 | — | — | 119.56 | — | — | rust-owned |
| ack_3_ranges | encode | 18.82 | — | — | 13.70 | — | — | cinnabar |
| datagram_32 | decode | 69.14 | — | — | 98.10 | — | — | rust-owned |
| datagram_32 | encode | 14.24 | — | — | 11.40 | — | — | cinnabar |
| datagram_4096 | decode | 79.66 | — | — | 102.44 | — | — | rust-owned |
| datagram_4096 | encode | 40.38 | — | — | 28.70 | — | — | cinnabar |
| datagram_512 | decode | 82.36 | — | — | 76.84 | — | — | cinnabar |
| datagram_512 | encode | 18.02 | — | — | 10.68 | — | — | cinnabar |
| encapsulated_32 | decode | 31.76 | — | — | 43.28 | — | — | rust-owned |
| encapsulated_32 | encode | 11.88 | — | — | 7.40 | — | — | cinnabar |
| encapsulated_4096 | decode | 36.78 | — | — | 78.12 | — | — | rust-owned |
| encapsulated_4096 | encode | 50.60 | — | — | 31.40 | — | — | cinnabar |
| encapsulated_512 | decode | 27.02 | — | — | 58.50 | — | — | rust-owned |
| encapsulated_512 | encode | 15.50 | — | — | 9.64 | — | — | cinnabar |
| split_2x512 | reassemble | 446.98 | — | — | — | — | — | unmatched |

<details><summary>Allocation requests (median per round)</summary>

| Case | Operation | Side | allocs/op | allocated B/op |
| --- | --- | --- | ---: | ---: |
| ack_3_ranges | decode | cinnabar | 3.00 | 224.00 |
| ack_3_ranges | decode | rust-owned | 1.00 | 24.00 |
| ack_3_ranges | encode | cinnabar | 0.00 | 0.00 |
| ack_3_ranges | encode | rust-owned | 0.00 | 0.00 |
| datagram_32 | decode | cinnabar | 2.00 | 672.00 |
| datagram_32 | decode | rust-owned | 1.00 | 80.00 |
| datagram_32 | encode | cinnabar | 0.00 | 0.00 |
| datagram_32 | encode | rust-owned | 0.00 | 0.00 |
| datagram_4096 | decode | cinnabar | 2.00 | 4,736.00 |
| datagram_4096 | decode | rust-owned | 1.00 | 80.00 |
| datagram_4096 | encode | cinnabar | 0.00 | 0.00 |
| datagram_4096 | encode | rust-owned | 0.00 | 0.00 |
| datagram_512 | decode | cinnabar | 2.00 | 1,152.00 |
| datagram_512 | decode | rust-owned | 1.00 | 80.00 |
| datagram_512 | encode | cinnabar | 0.00 | 0.00 |
| datagram_512 | encode | rust-owned | 0.00 | 0.00 |
| encapsulated_32 | decode | cinnabar | 1.00 | 32.00 |
| encapsulated_32 | decode | rust-owned | 0.00 | 0.00 |
| encapsulated_32 | encode | cinnabar | 0.00 | 0.00 |
| encapsulated_32 | encode | rust-owned | 0.00 | 0.00 |
| encapsulated_4096 | decode | cinnabar | 1.00 | 4,096.00 |
| encapsulated_4096 | decode | rust-owned | 0.00 | 0.00 |
| encapsulated_4096 | encode | cinnabar | 0.00 | 0.00 |
| encapsulated_4096 | encode | rust-owned | 0.00 | 0.00 |
| encapsulated_512 | decode | cinnabar | 1.00 | 512.00 |
| encapsulated_512 | decode | rust-owned | 0.00 | 0.00 |
| encapsulated_512 | encode | cinnabar | 0.00 | 0.00 |
| encapsulated_512 | encode | rust-owned | 0.00 | 0.00 |
| split_2x512 | reassemble | rust-owned | 4.00 | 1,972.00 |

</details>

## Corpus exclusions and API coverage

Corpus encoder: gophertunnel `85d4022854e1` (`v1.25.3-0.20261019151115-85d4022854e1`), as pinned by protocolgen retail/go.mod. Requested live-master reference: HashimTheArab/gophertunnel `80c811b6186016b3860c358368cfa47e507f26e9`; go-raknet: `87a6b7e53799290d70ac25bcd03645d7ead00d2c`. The newer corpus encoder is not silently substituted for the requested reference.

| Case | Implementation | Excluded because |
| --- | --- | --- |
| AddActor | rust-borrowed | public decoder returns Raw |
| ChunkRadius | cinnabar | isolated compress/decompress: root compression module is private; deflate/snappy rows include public BatchCodec framing |
| ChunkRadius | go | isolated encryption/checksum helpers are private; public Encoder/Decoder also frame packets |
| ChunkRadius | rust-owned | isolated checksum/encryption helpers are private; public transport also frames and sends |
| ClientTick | cinnabar | isolated compress/decompress: root compression module is private; deflate/snappy rows include public BatchCodec framing |
| ClientTick | go | isolated encryption/checksum helpers are private; public Encoder/Decoder also frame packets |
| ClientTick | rust-borrowed | public decoder returns Raw |
| ClientTick | rust-owned | isolated checksum/encryption helpers are private; public transport also frames and sends |
| CraftingData | go | typed decode: unknown value '"tags"' for enum type 'item descriptor type' |
| InventoryTransactionNormal | rust-borrowed | public decoder returns Raw |
| InventoryTransactionUseItem | rust-borrowed | public decoder returns Raw |
| InventoryTransactionUseItemOnEntity | rust-borrowed | public decoder returns Raw |
| ItemStackRequest | rust-borrowed | public decoder returns Raw |
| PlayerAuthInput | rust-borrowed | public decoder returns Raw |
| PlayerAuthInputInteract | rust-borrowed | public decoder returns Raw |
| ServerTick | cinnabar | isolated compress/decompress: root compression module is private; deflate/snappy rows include public BatchCodec framing |
| ServerTick | go | isolated encryption/checksum helpers are private; public Encoder/Decoder also frame packets |
| ServerTick | rust-borrowed | public decoder returns Raw |
| ServerTick | rust-owned | isolated checksum/encryption helpers are private; public transport also frames and sends |
| SetActorData | rust-borrowed | public decoder returns Raw |
| Spawn | cinnabar | isolated compress/decompress: root compression module is private; deflate/snappy rows include public BatchCodec framing |
| Spawn | go | isolated encryption/checksum helpers are private; public Encoder/Decoder also frame packets |
| Spawn | rust-borrowed | public decoder returns Raw |
| Spawn | rust-owned | isolated checksum/encryption helpers are private; public transport also frames and sends |
| StartGame | rust-borrowed | public decoder returns Raw |
| SubChunk | rust-borrowed | public decoder returns Raw |
| split_2x512 | cinnabar | split reassembly lives in private session implementation; no isolated public assembler API, including test-support |

Borrowed encoding has no public API. Go RakNet datagram/split/ACK micro APIs and Cinnabar split assembly are private. Missing cells mean unavailable, not zero cost. Batch framing excludes typed packet decoding; callback-vs-owned API and compression backend asymmetries are documented in the READMEs. Jolyne has no public isolated encryption API.

## Transport interop sanity

| Direction | Sizes | Echoes per size | Result |
| --- | --- | ---: | --- |
| go-to-rust | 32,512,4096,65536 | 100,100,10,10 | PASS |
| rust-to-go | 32,512,4096,65536 | 100,100,10,10 | PASS |
| cinnabar-to-rust | 32,512,4096,65536 | 100,100,10,10 | PASS |
| rust-to-cinnabar | 32,512,4096,65536 | 100,100,10,10 | PASS |
| cinnabar-to-go | 32,512,4096,65536 | 100,100,10,10 | PASS |
| go-to-cinnabar | 32,512,4096,65536 | 100,100,10,10 | PASS |

## Upstream RakNet self-benchmark cross-check

Three alternating, unmodified upstream loopback pairs completed on logical CPUs 0,2,4,6 with Tokio/GOMAXPROCS=4. Rust Criterion 64-byte sample means were 45.961–47.990 us versus upstream Go aggregate means 40.173–41.621 us; 2 MiB requests with 1-byte acknowledgements were 15.523–17.054 ms for Rust and 37.703–74.245 ms for Go. These are separate estimators, not speed-ratio claims. Native aggregate outputs neither confirm nor rule out individual 10–23 ms stalls. The preliminary adapter used max_connections=10 and no LTO; the final adapter selects max_connections=1 and matching thin-LTO release settings, but neither change proves the cause of its elevated preliminary means.

The coordinator subsequently confirmed concurrent benchmark workers on the same P-cores. These native measurements are contention-exposed diagnostics, not isolated comparative performance evidence.

This separate diagnostic does not enter the alternating comparison medians. Exact upstream commands, configuration audit and limitations: [upstream-check.md](cinnabar/upstream-check.md); reproduction: [upstream-check.ps1](cinnabar/upstream-check.ps1).


## Reference split/retransmission finding (separate diagnostic runs)

An uninstrumented go-to-go run of `go run . loopback --count 100` failed at 4096 B, measured message index 81: `handle packet: handle datagram: receive packet: split packet: maximum concurrent splits 16 reached`. No Rust peer participated in that failure; no public listener API overrides this private limit.

The coordinator-approved timed matrix retains 100 small-message echoes and uses ten measured plus one readiness echo for split-message sizes on both implementations. Larger-count failures remain failures: the harness neither retries nor changes production/reference sources.

| Direction | Diagnostic attempt | Completed split groups | Duplicate indexes after completion | Exit code |
| --- | ---: | ---: | ---: | ---: |
| go-to-go | 1 | 202 | 0 | 0 |
| go-to-go | 2 | 202 | 0 | 0 |
| go-to-go | 3 | 202 | 0 | 0 |
| go-to-go | 4 | 202 | 29 | 0 |
| rust-to-go | 1 | 202 | 0 | 0 |

Late duplicate fragments can reopen incomplete split entries after completed assemblies are deleted [INFERENCE from wire observation and reference receiveSplitPacket source]; the traced successful run is not itself a reproduction of the limit failure. The observed failure and late duplicates occurred with Go peers only, so this evidence does not attribute the issue to tokio-raknet. The packet observer changes scheduling; no duplicate in one instrumented Rust-to-Go run does not prove duplicates are impossible.

| Late datagram sequence | Split ID | Fragment index | Fragment count |
| ---: | ---: | ---: | ---: |
| 4648 | 14 | 42 | 46 |
| 4649 | 15 | 6 | 46 |
| 4650 | 15 | 10 | 46 |

The successful Go-to-Go trace with late fragments covered 17 distinct completed split IDs. All instrumented captures used GOMAXPROCS=4, 100 measured echoes plus readiness; the exact failing standalone run did not record concurrency. The public UDP socket observer is disabled for timed measurements. Reproduction and distilled evidence: [diagnostics.json](diagnostics.json); raw packet metadata logs remain ignored under results/diagnostics/.


## Highest measured optimization gaps

- loopback / 4096 / echo (rust-owned): 321.99× Go time. Profile this measured boundary before attributing a cause.
- loopback / 512 / echo (rust-owned): 70.12× Go time. Profile this measured boundary before attributing a cause.
- loopback / 4096 / echo (cinnabar): 69.21× Go time. Profile this measured boundary before attributing a cause.

## Limits and reproducibility

See [README.md](README.md), [Go API caveats](go/CAVEATS.md), and [Rust API caveats](../../crates/bench-compare/README.md). Missing rows are not zero-cost operations: private transport internals and untyped borrowed variants cannot be compared through public APIs.

Go GC accounting and Rust allocator accounting are not equivalent retained-memory measurements. Timing loops include destruction; Go GC may run during later iterations. Small timings on Windows depend on clock precision and scheduling. No production implementation was optimized for this run.

Raw per-round JSONL, stderr, metadata, interop outcomes and summary.json remain in the ignored results directory. Run `./tools/bench-compare/run.ps1` to regenerate.

<!-- nethernet-bench:start -->

# NetherNet cross-implementation measurements

Measured UTC: 2026-10-22T15:06:52.6435111Z; median of 5 alternating rounds. Echoes: 100 for 32/512 B, 10 for 4/64 KiB, one untimed readiness echo; fresh connection per size and round.

NetherNet source base: `1f4c8fc93fc67522d72e019ef3a3c07ed86b38cc`; raw evidence: `results/nethernet-4e2e21af47874c2b915866e5ccbdbc98/`.

Shared i9-14900HX Windows workstation. Micro logical CPU 0; all three loopback/interop implementations on identical logical CPUs 16-23 (E-cores acceptable for this WebRTC comparison), eight Tokio workers/GOMAXPROCS. Builds only logical CPUs 24-31, at most eight jobs. No timing on logical CPUs 2,4,6,8,10,12,14. Alternating order; no priority, power-plan or timer-resolution adjustment.

Measurement only: no production optimization. Throughput counts completed reliable-channel echoes and request payload only (decimal MB/s), not both directions or saturation bandwidth. Setup spans offer/answer, ICE, DTLS and SCTP/data-channel opening; readiness is excluded. Each table cell is the median of its per-round statistic, not a pooled percentile.

## NetherNet pins and configuration

| Component | Version / configuration |
| --- | --- |
| buffers | Ours 64-message inbound queue backed by the SCTP receive window, inline sends, 4 MiB UDP socket buffers; Go unbuffered completed-packet handoff; Cinnabar requests2MiB UDP receive buffer; other socket/DTLS/SCTP buffers remain stack defaults (OS-effective sizes not queried) |
| channel_parameters | ReliableDataChannel: ordered, reliable, default retransmission policy; second unreliable channel also opens; no DTLS cipher, certificate, retransmission or SCTP congestion/RTO overrides |
| cinnabar_nethernet | cinnabar-mc/nethernet fed09fc05afb3b0fe34d0e7b18702f7ea86c3290; Apache-2.0 |
| cinnabar_stack | rtc / rtc-dtls / rtc-sctp / rtc-datachannel 0.21.1; rtc-turn git ceef0780c54c4f5fdc08e7e343403f8026653171 |
| go_nethernet | df-mc/go-nethernet 215e46422b5831d6ddc6146003617dc3f1ff7c26 |
| go_stack | Pion webrtc v4.2.21-0.20260920133716-91bfc6c2039f; ICE v4.4.4; DTLS v3.1.9; SCTP v1.11.3; datachannel v1.6.3 |
| go_toolchain | go version go1.27.0 windows/amd64 |
| identities | Go listener requires default self-signed server identity; ours/Cinnabar permit unsigned local answers; Go setup therefore includes identity key issuance/signing, which is not patched out |
| ours_stack | sans-IO rtc 0.21.1 (rtc-ice, rtc-dtls, rtc-sctp, rtc-datachannel, rtc-turn) composed directly by tokio-nethernet; one driver task per connection, inline sends |
| release_profile | Both Rust binaries: opt-level=3; lto=thin; codegen-units=16; strip=true |
| rust | rustc 1.96.0-nightly (b90dc1e59 2026-03-04) |
| scheduling | Both Rust NetherNet application futures are spawned onto the Tokio worker pool, matching Go application goroutines rather than driving Rust application channels on block_on external thread |
| sctp_limits | Advertised max-message-size: ours/Go/Cinnabar 262144 (segment payload 262143); outgoing segmentation follows peer limit; framing micros use 16383-byte segment payloads |
| signaling | Benchmark-local JSON-string TCP signals; server network ID 1, client ID 2; no external network signaling/STUN/TURN |

Stack parameters, public API boundaries and buffer asymmetries: [ours](../../crates/bench-compare/README.md), [Go](go/CAVEATS.md), [Cinnabar](cinnabar/README.md). Local signaling uses no public STUN/TURN service or credentials. The interop adapter exchanges complete signals as JSON strings over benchmark-local loopback TCP; reserved JSON-string BENCH-DONE markers acknowledge final delivery and server send/drain before RTC teardown (untimed). This bridge is measurement code, not a production transport change.

## NetherNet framing micro

| Case | Operation | Segments | tokio-nethernet ns/op | Cinnabar ns/op | Go ns/op |
| --- | --- | ---: | ---: | ---: | ---: |
| invalid_count | reassemble | — | 11.30 | 35.56 | — |
| invalid_sequence | reassemble | — | 72.86 | 91.80 | — |
| message_1048576 | encode | 65 | 35,188.74 | 36,270.96 | — |
| message_1048576 | reassemble | 65 | 295,505.38 | 569,354.02 | — |
| message_32 | encode | 1 | 46.90 | 51.70 | — |
| message_32 | reassemble | 1 | 28.50 | 31.22 | — |
| message_4096 | encode | 1 | 97.70 | 96.34 | — |
| message_4096 | reassemble | 1 | 24.82 | 34.52 | — |
| message_512 | encode | 1 | 76.08 | 79.74 | — |
| message_512 | reassemble | 1 | 26.36 | 31.34 | — |
| message_65536 | encode | 5 | 1,298.66 | 1,226.82 | — |
| message_65536 | reassemble | 5 | 1,700.56 | 10,461.44 | — |
| oversize | encode | — | 1.72 | 1.56 | — |

Cinnabar micro uses 16,383-byte segment payloads plus a one-byte countdown. Encode consumes every generated segment; reassembly includes segment decode. The oversize case exceeds the 255-segment encode limit; invalid-count/sequence rows exercise actual public rejection paths.

## NetherNet signaling micro

| Case | Operation | tokio-nethernet ns/op | Cinnabar ns/op | Go ns/op |
| --- | --- | ---: | ---: | ---: |
| candidate_add | format | 39.78 | 109.86 | 74.92 |
| candidate_add | format_reused_buffer | 11.16 | — | — |
| candidate_add | parse | 64.82 | 84.16 | 144.24 |
| connect_error | format | 34.68 | 71.66 | 40.04 |
| connect_error | format_reused_buffer | 8.92 | — | — |
| connect_error | parse | 71.40 | 84.14 | 76.70 |
| connect_request | format | 34.28 | 108.32 | 270.44 |
| connect_request | format_reused_buffer | 13.14 | — | — |
| connect_request | parse | 104.02 | 117.56 | 124.16 |
| connect_response | format | 39.32 | 106.10 | 57.90 |
| connect_response | format_reused_buffer | 12.38 | — | — |
| connect_response | parse | 68.22 | 84.20 | 74.66 |
| ice_candidate | format | 73.46 | 395.58 | — |
| ice_candidate | parse | 100.46 | 591.64 | — |

`format` returns a fresh String per op on every side (ours `Signal::to_wire_string`, Go `Signal.String`, Cinnabar `to_string`). `format_reused_buffer` is ours only: `Signal::write_wire` appends into one reused String without allocating; it has no reference counterpart and is not a comparison.

## NetherNet discovery micro

| Case | Operation | tokio-nethernet ns/op | Cinnabar ns/op | Go ns/op |
| --- | --- | ---: | ---: | ---: |
| packet | decode | 127.72 | 305.46 | 842.04 |
| packet | encode | 168.80 | 174.64 | 911.52 |

## NetherNet loopback: setup and reliable echo

| Size | Operation | Side | Samples/round | Mean µs | p50 µs | p99 µs | Max µs | msgs/s | MB/s | Ours/reference time |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 32 | echo | cinnabar | 100 | 110.43 | 116.10 | 168.10 | 259.80 | 9,055.59 | 0.29 | 0.97 |
| 32 | echo | go | 100 | 280.06 | 269.60 | 516.10 | 567.90 | 3,570.68 | 0.11 | 0.38 |
| 32 | echo | rust-owned | 100 | 107.02 | 111.10 | 149.00 | 153.60 | 9,344.05 | 0.30 | — |
| 32 | setup | cinnabar | 1 | 9,722.10 | 9,722.10 | 9,722.10 | 9,722.10 | — | — | 1.36 |
| 32 | setup | go | 1 | 88,667.40 | — | — | — | — | — | 0.15 |
| 32 | setup | rust-owned | 1 | 13,191.70 | 13,191.70 | 13,191.70 | 13,191.70 | — | — | — |
| 512 | echo | cinnabar | 100 | 111.62 | 120.20 | 150.10 | 468.50 | 8,959.21 | 4.59 | 0.98 |
| 512 | echo | go | 100 | 229.49 | 224.00 | 578.10 | 602.80 | 4,357.47 | 2.23 | 0.48 |
| 512 | echo | rust-owned | 100 | 109.06 | 113.00 | 188.60 | 233.00 | 9,169.10 | 4.69 | — |
| 512 | setup | cinnabar | 1 | 12,716.00 | 12,716.00 | 12,716.00 | 12,716.00 | — | — | 0.34 |
| 512 | setup | go | 1 | 79,275.80 | — | — | — | — | — | 0.05 |
| 512 | setup | rust-owned | 1 | 4,297.00 | 4,297.00 | 4,297.00 | 4,297.00 | — | — | — |
| 4096 | echo | cinnabar | 10 | 364.73 | 354.70 | 524.30 | 524.30 | 2,741.75 | 11.23 | 0.87 |
| 4096 | echo | go | 10 | 634.08 | 541.60 | 1,118.10 | 1,118.10 | 1,577.09 | 6.46 | 0.50 |
| 4096 | echo | rust-owned | 10 | 315.86 | 265.80 | 349.50 | 349.50 | 3,165.96 | 12.97 | — |
| 4096 | setup | cinnabar | 1 | 13,049.70 | 13,049.70 | 13,049.70 | 13,049.70 | — | — | 0.69 |
| 4096 | setup | go | 1 | 103,913.10 | — | — | — | — | — | 0.09 |
| 4096 | setup | rust-owned | 1 | 8,963.50 | 8,963.50 | 8,963.50 | 8,963.50 | — | — | — |
| 65536 | echo | cinnabar | 10 | 4,348.95 | 3,943.00 | 9,076.70 | 9,076.70 | 229.94 | 15.07 | 0.69 |
| 65536 | echo | go | 10 | 5,024.13 | 4,976.90 | 6,336.80 | 6,336.80 | 199.04 | 13.04 | 0.60 |
| 65536 | echo | rust-owned | 10 | 3,021.75 | 2,888.30 | 3,385.80 | 3,385.80 | 330.93 | 21.69 | — |
| 65536 | setup | cinnabar | 1 | 11,686.00 | 11,686.00 | 11,686.00 | 11,686.00 | — | — | 0.22 |
| 65536 | setup | go | 1 | 79,734.50 | — | — | — | — | — | 0.03 |
| 65536 | setup | rust-owned | 1 | 2,583.90 | 2,583.90 | 2,583.90 | 2,583.90 | — | — | — |

Ten-sample p99 equals the maximum; it is not a stable population tail estimate. Setup has one observation per connection; setup percentile columns, if emitted, describe that single sample.

## NetherNet public API coverage

| Case | Side | Unavailable because |
| --- | --- | --- |
| nethernet_framing | go | Framing encode, reassembly, invalid segment counts and oversize rejection are private; no public micro API for 32/512/4096/65536/1048576-byte messages. |
| nethernet_framing_configured_limit | cinnabar | Message::with_maximum is crate-private; public oversize row exercises the 255-segment encoding limit instead |
| nethernet_signaling/ice_candidate | go | NetherNet ICE candidate formatting/parsing is private; CANDIDATEADD measures only public Signal text parsing/formatting. |

Missing cells are unavailable APIs, not zero time. No private framing algorithm was copied to manufacture a comparison.

## NetherNet interop sanity

| Direction | Result | Evidence |
| --- | --- | --- |
| rust-to-go | PASS | All four payload sizes verified byte-for-byte |
| go-to-rust | PASS | All four payload sizes verified byte-for-byte |
| rust-to-cinnabar | PASS | All four payload sizes verified byte-for-byte |
| cinnabar-to-rust | PASS | All four payload sizes verified byte-for-byte |
| cinnabar-to-go | PASS | All four payload sizes verified byte-for-byte |
| go-to-cinnabar | PASS | All four payload sizes verified byte-for-byte |

## NetherNet Go QPC validation

| Round | Phase | Clock | Frequency Hz | Tick ns | Minimum positive ns |
| ---: | --- | --- | ---: | ---: | ---: |
| 1 | loopback | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 1 | micro | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 2 | loopback | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 2 | micro | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 3 | loopback | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 3 | micro | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 4 | loopback | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 4 | micro | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 5 | loopback | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |
| 5 | micro | QueryPerformanceCounter | 10000000 | 100.00 | 100.00 |

## NetherNet top three measured gaps

- nethernet_loopback / 32 / setup: ours takes 1.36× cinnabar time. Profile this boundary before attributing a cause.
- nethernet_framing / oversize / encode: ours takes 1.10× cinnabar time. Profile this boundary before attributing a cause.
- nethernet_framing / message_65536 / encode: ours takes 1.06× cinnabar time. Profile this boundary before attributing a cause.

Reproduce: `./tools/bench-compare/run.ps1 -Area nethernet -Rounds 5`. Raw per-round JSONL, stderr, metadata, interop logs and summary.json are retained in the ignored results directory. Other workers may run concurrently on disjoint allowed CPUs; affinity does not eliminate shared package power/cache contention.
