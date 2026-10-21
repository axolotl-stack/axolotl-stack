# Cross-implementation benchmark report

Measured UTC: 2026-10-21T21:57:23.0202789Z. Median of 3 alternating rounds; measured echoes per connection: 100 for 32/512 B and 10 for 4096/65536 B; untimed readiness echoes per connection: 1.

Machine: Microsoft Windows 10.0.26200 / X64; Intel64 Family 6 Model 183 Stepping 1, GenuineIntel; 32 logical processors.
Toolchains: `rustc 1.96.0-nightly (b90dc1e59 2026-03-04)`; `go version go1.27.0 windows/amd64`; Go GOMAXPROCS=4; Tokio multi-thread workers=4.
Source base: `7573909e8f75fd5e184d568572dd1dceb5b0e9d1`; protocolgen corpus: `7b2e47c463fa425e25004673cf8960753dd2ff12`.

Shared Windows workstation; other workers may build or benchmark concurrently; no affinity, priority, power-plan or timer-resolution adjustment.

Ratios are ours/Go ns/op (below 1 is faster); throughput is request payload only, decimal MB/s, one completed echo per message. Medians are descriptive, not significance tests.

## codec

| Case | Operation | Side | ns/op | allocs/op | allocated B/op | ours/Go | Winner |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| AddActor | decode | go | 959.09 | 23.00 | 1,168.01 | — | — |
| AddActor | decode | rust-owned | 454.88 | 7.00 | 784.00 | 0.47 | ours |
| AddActor | encode | go | 501.91 | 6.00 | 128.00 | — | — |
| AddActor | encode | rust-owned | 87.72 | 0.00 | 0.00 | 0.17 | ours |
| Animate | decode | go | 71.81 | 4.00 | 128.00 | — | — |
| Animate | decode | rust-borrowed | 46.26 | 0.00 | 0.00 | 0.64 | ours |
| Animate | decode | rust-owned | 63.50 | 1.00 | 40.00 | 0.88 | ours |
| Animate | encode | go | 44.12 | 2.00 | 56.00 | — | — |
| Animate | encode | rust-owned | 15.14 | 0.00 | 0.00 | 0.34 | ours |
| AvailableCommands | decode | go | 76,056.58 | 2,513.00 | 83,968.46 | — | — |
| AvailableCommands | decode | rust-borrowed | 51,622.76 | 289.00 | 76,201.00 | 0.68 | ours |
| AvailableCommands | decode | rust-owned | 91,314.28 | 2,061.00 | 87,035.00 | 1.20 | Go |
| AvailableCommands | encode | go | 36,454.97 | 451.00 | 2,792.03 | — | — |
| AvailableCommands | encode | rust-owned | 7,691.90 | 0.00 | 0.00 | 0.21 | ours |
| BiomeDefinitionList | decode | go | 19,556.15 | 291.00 | 103,808.35 | — | — |
| BiomeDefinitionList | decode | rust-borrowed | 8,881.66 | 90.00 | 79,546.00 | 0.45 | ours |
| BiomeDefinitionList | decode | rust-owned | 11,171.98 | 198.00 | 78,984.00 | 0.57 | ours |
| BiomeDefinitionList | encode | go | 8,243.79 | 91.00 | 392.00 | — | — |
| BiomeDefinitionList | encode | rust-owned | 1,190.80 | 0.00 | 0.00 | 0.14 | ours |
| ChunkRadius | compress | go | 778,583.11 | 1.06 | 49,960.11 | — | — |
| ChunkRadius | decode_batch | go | 19,076.45 | 49.00 | 167,496.84 | — | — |
| ChunkRadius | decode_batch | rust-borrowed | 683.86 | 0.00 | 0.00 | 0.04 | ours |
| ChunkRadius | decode_batch | rust-owned | 3,458.68 | 18.00 | 152,073.00 | 0.18 | ours |
| ChunkRadius | decode_compressed_batch | go | 306,153.36 | 50.33 | 168,032.07 | — | — |
| ChunkRadius | decompress | go | 297,609.54 | 2.32 | 156,157.88 | — | — |
| ChunkRadius | encode_batch | go | 8,203.55 | 20.00 | 380.98 | — | — |
| ChunkRadius | encode_batch | rust-owned | 3,696.06 | 0.00 | 0.00 | 0.45 | ours |
| ClientTick | compress | go | 8,196.18 | 1.00 | 295.22 | — | — |
| ClientTick | decode_batch | go | 1,808.18 | 60.00 | 2,312.00 | — | — |
| ClientTick | decode_batch | rust-owned | 1,089.84 | 18.00 | 1,463.00 | 0.60 | ours |
| ClientTick | decode_compressed_batch | go | 3,011.38 | 61.00 | 2,375.84 | — | — |
| ClientTick | decompress | go | 1,114.09 | 2.00 | 400.97 | — | — |
| ClientTick | encode_batch | go | 1,086.04 | 27.00 | 324.09 | — | — |
| ClientTick | encode_batch | rust-owned | 248.66 | 0.00 | 0.00 | 0.23 | ours |
| CraftingData | decode | rust-borrowed | 346,236.04 | 3,526.00 | 362,244.00 | — | — |
| CraftingData | decode | rust-owned | 523,559.24 | 9,971.00 | 395,719.00 | — | — |
| CraftingData | encode | rust-owned | 49,771.92 | 0.00 | 0.00 | — | — |
| CreativeContent | decode | go | 315,552.55 | 9,934.00 | 540,586.16 | — | — |
| CreativeContent | decode | rust-borrowed | 51,617.94 | 2.00 | 65,120.00 | 0.16 | ours |
| CreativeContent | decode | rust-owned | 72,503.56 | 1,182.00 | 75,075.00 | 0.23 | ours |
| CreativeContent | encode | go | 184,953.07 | 5,085.06 | 83,325.33 | — | — |
| CreativeContent | encode | rust-owned | 14,222.74 | 0.00 | 0.00 | 0.08 | ours |
| InventoryTransactionNormal | decode | go | 3,074.55 | 95.00 | 5,016.02 | — | — |
| InventoryTransactionNormal | decode | rust-owned | 519.68 | 10.00 | 699.00 | 0.17 | ours |
| InventoryTransactionNormal | encode | go | 1,828.23 | 53.00 | 800.13 | — | — |
| InventoryTransactionNormal | encode | rust-owned | 100.08 | 0.00 | 0.00 | 0.05 | ours |
| InventoryTransactionUseItem | decode | go | 402.38 | 16.00 | 570.67 | — | — |
| InventoryTransactionUseItem | decode | rust-owned | 189.18 | 3.00 | 210.00 | 0.47 | ours |
| InventoryTransactionUseItem | encode | go | 271.96 | 10.00 | 92.02 | — | — |
| InventoryTransactionUseItem | encode | rust-owned | 42.82 | 0.00 | 0.00 | 0.16 | ours |
| InventoryTransactionUseItemOnEntity | decode | go | 830.37 | 24.00 | 1,328.00 | — | — |
| InventoryTransactionUseItemOnEntity | decode | rust-owned | 166.44 | 3.00 | 219.00 | 0.20 | ours |
| InventoryTransactionUseItemOnEntity | encode | go | 503.61 | 13.00 | 220.04 | — | — |
| InventoryTransactionUseItemOnEntity | encode | rust-owned | 64.72 | 0.00 | 0.00 | 0.13 | ours |
| ItemRegistry | decode | go | 350,228.43 | 10,805.00 | 544,824.47 | — | — |
| ItemRegistry | decode | rust-borrowed | 77,890.90 | 1.00 | 104,000.00 | 0.22 | ours |
| ItemRegistry | decode | rust-owned | 101,942.16 | 1,301.00 | 120,192.00 | 0.29 | ours |
| ItemRegistry | encode | go | 175,652.40 | 3,802.00 | 112,836.91 | — | — |
| ItemRegistry | encode | rust-owned | 9,484.18 | 0.00 | 0.00 | 0.05 | ours |
| ItemStackRequest | decode | go | 914.16 | 35.00 | 1,000.01 | — | — |
| ItemStackRequest | decode | rust-owned | 669.18 | 12.00 | 773.00 | 0.73 | ours |
| ItemStackRequest | encode | go | 566.14 | 15.00 | 116.02 | — | — |
| ItemStackRequest | encode | rust-owned | 94.50 | 0.00 | 0.00 | 0.17 | ours |
| LevelChunk | decode | go | 2,231.66 | 5.00 | 18,596.09 | — | — |
| LevelChunk | decode | rust-borrowed | 77.00 | 0.00 | 0.00 | 0.03 | ours |
| LevelChunk | decode | rust-owned | 328.98 | 2.00 | 16,897.00 | 0.15 | ours |
| LevelChunk | encode | go | 430.39 | 2.00 | 36.00 | — | — |
| LevelChunk | encode | rust-owned | 210.88 | 0.00 | 0.00 | 0.49 | ours |
| LevelSoundEvent | decode | go | 134.21 | 5.00 | 180.00 | — | — |
| LevelSoundEvent | decode | rust-borrowed | 81.58 | 0.00 | 0.00 | 0.61 | ours |
| LevelSoundEvent | decode | rust-owned | 136.48 | 3.00 | 116.00 | 1.02 | Go |
| LevelSoundEvent | encode | go | 74.89 | 1.00 | 32.00 | — | — |
| LevelSoundEvent | encode | rust-owned | 21.96 | 0.00 | 0.00 | 0.29 | ours |
| MobEquipment | decode | go | 236.70 | 10.00 | 362.67 | — | — |
| MobEquipment | decode | rust-borrowed | 83.24 | 0.00 | 0.00 | 0.35 | ours |
| MobEquipment | decode | rust-owned | 104.16 | 2.00 | 74.00 | 0.44 | ours |
| MobEquipment | encode | go | 129.99 | 5.00 | 76.01 | — | — |
| MobEquipment | encode | rust-owned | 22.10 | 0.00 | 0.00 | 0.17 | ours |
| MoveActorDelta | decode | go | 106.79 | 3.00 | 160.00 | — | — |
| MoveActorDelta | decode | rust-borrowed | 62.00 | 0.00 | 0.00 | 0.58 | ours |
| MoveActorDelta | decode | rust-owned | 88.30 | 1.00 | 56.00 | 0.83 | ours |
| MoveActorDelta | encode | go | 84.87 | 1.00 | 32.00 | — | — |
| MoveActorDelta | encode | rust-owned | 20.76 | 0.00 | 0.00 | 0.24 | ours |
| MovePlayer | decode | go | 96.60 | 3.00 | 160.00 | — | — |
| MovePlayer | decode | rust-borrowed | 52.50 | 0.00 | 0.00 | 0.54 | ours |
| MovePlayer | decode | rust-owned | 77.56 | 1.00 | 64.00 | 0.80 | ours |
| MovePlayer | encode | go | 69.53 | 1.00 | 32.00 | — | — |
| MovePlayer | encode | rust-owned | 25.38 | 0.00 | 0.00 | 0.37 | ours |
| NetworkStackLatency | decode | go | 55.59 | 3.00 | 96.00 | — | — |
| NetworkStackLatency | decode | rust-borrowed | 41.98 | 0.00 | 0.00 | 0.76 | ours |
| NetworkStackLatency | decode | rust-owned | 27.82 | 0.00 | 0.00 | 0.50 | ours |
| NetworkStackLatency | encode | go | 28.12 | 1.00 | 32.00 | — | — |
| NetworkStackLatency | encode | rust-owned | 12.68 | 0.00 | 0.00 | 0.45 | ours |
| PlayerAuthInput | decode | go | 314.16 | 5.00 | 612.00 | — | — |
| PlayerAuthInput | decode | rust-owned | 193.10 | 2.00 | 456.00 | 0.61 | ours |
| PlayerAuthInput | encode | go | 182.93 | 2.00 | 36.00 | — | — |
| PlayerAuthInput | encode | rust-owned | 63.00 | 0.00 | 0.00 | 0.34 | ours |
| PlayerAuthInputInteract | decode | go | 685.79 | 18.00 | 856.00 | — | — |
| PlayerAuthInputInteract | decode | rust-owned | 344.12 | 4.00 | 530.00 | 0.50 | ours |
| PlayerAuthInputInteract | encode | go | 503.93 | 11.00 | 96.02 | — | — |
| PlayerAuthInputInteract | encode | rust-owned | 120.94 | 0.00 | 0.00 | 0.24 | ours |
| ServerTick | compress | go | 24,908.72 | 1.00 | 1,431.47 | — | — |
| ServerTick | decode_batch | go | 10,921.49 | 280.00 | 13,624.04 | — | — |
| ServerTick | decode_batch | rust-owned | 6,112.30 | 83.00 | 5,738.00 | 0.56 | ours |
| ServerTick | decode_compressed_batch | go | 19,249.80 | 281.06 | 13,779.83 | — | — |
| ServerTick | decompress | go | 7,775.40 | 2.00 | 2,355.45 | — | — |
| ServerTick | encode_batch | go | 6,935.06 | 84.00 | 2,464.30 | — | — |
| ServerTick | encode_batch | rust-owned | 1,642.82 | 0.00 | 0.00 | 0.24 | ours |
| SetActorData | decode | go | 689.82 | 18.00 | 960.00 | — | — |
| SetActorData | decode | rust-owned | 297.20 | 4.00 | 550.00 | 0.43 | ours |
| SetActorData | encode | go | 387.25 | 4.00 | 120.00 | — | — |
| SetActorData | encode | rust-owned | 54.96 | 0.00 | 0.00 | 0.14 | ours |
| SetActorMotion | decode | go | 67.85 | 3.00 | 112.00 | — | — |
| SetActorMotion | decode | rust-borrowed | 49.10 | 0.00 | 0.00 | 0.72 | ours |
| SetActorMotion | decode | rust-owned | 37.28 | 0.00 | 0.00 | 0.55 | ours |
| SetActorMotion | encode | go | 46.29 | 1.00 | 32.00 | — | — |
| SetActorMotion | encode | rust-owned | 17.38 | 0.00 | 0.00 | 0.38 | ours |
| Spawn | compress | go | 23,654.10 | 1.00 | 1,045.34 | — | — |
| Spawn | decode_batch | go | 19,604.77 | 531.00 | 27,976.19 | — | — |
| Spawn | decode_batch | rust-owned | 8,178.14 | 118.00 | 12,780.00 | 0.42 | ours |
| Spawn | decode_compressed_batch | go | 27,056.87 | 532.28 | 28,254.25 | — | — |
| Spawn | decompress | go | 6,694.44 | 2.01 | 4,150.76 | — | — |
| Spawn | encode_batch | go | 10,906.21 | 197.01 | 3,237.57 | — | — |
| Spawn | encode_batch | rust-owned | 1,562.60 | 0.00 | 0.00 | 0.14 | ours |
| StartGame | decode | go | 122,197.30 | 3,054.00 | 193,201.29 | — | — |
| StartGame | decode | rust-owned | 12,129.32 | 94.00 | 6,456.00 | 0.10 | ours |
| StartGame | encode | go | 60,472.50 | 975.00 | 17,540.12 | — | — |
| StartGame | encode | rust-owned | 853.88 | 0.00 | 0.00 | 0.01 | ours |
| SubChunk | decode | go | 33,208.00 | 464.00 | 80,452.31 | — | — |
| SubChunk | decode | rust-owned | 31,097.22 | 488.00 | 96,168.00 | 0.94 | ours |
| SubChunk | encode | go | 26,569.82 | 434.00 | 1,764.01 | — | — |
| SubChunk | encode | rust-owned | 5,696.04 | 0.00 | 0.00 | 0.21 | ours |
| Text | decode | go | 178.94 | 7.00 | 265.00 | — | — |
| Text | decode | rust-borrowed | 157.36 | 2.00 | 39.00 | 0.88 | ours |
| Text | decode | rust-owned | 187.84 | 4.00 | 199.00 | 1.05 | Go |
| Text | encode | go | 75.88 | 2.00 | 33.00 | — | — |
| Text | encode | rust-owned | 36.84 | 0.00 | 0.00 | 0.49 | ours |
| UpdateAttributes | decode | go | 604.30 | 18.00 | 784.00 | — | — |
| UpdateAttributes | decode | rust-borrowed | 366.40 | 2.00 | 480.00 | 0.61 | ours |
| UpdateAttributes | decode | rust-owned | 473.68 | 9.00 | 592.00 | 0.78 | ours |
| UpdateAttributes | encode | go | 313.75 | 7.00 | 56.00 | — | — |
| UpdateAttributes | encode | rust-owned | 62.08 | 0.00 | 0.00 | 0.20 | ours |
| UpdateBlock | decode | go | 76.83 | 3.00 | 104.00 | — | — |
| UpdateBlock | decode | rust-borrowed | 54.82 | 0.00 | 0.00 | 0.71 | ours |
| UpdateBlock | decode | rust-owned | 73.42 | 1.00 | 24.00 | 0.96 | ours |
| UpdateBlock | encode | go | 56.65 | 1.00 | 32.00 | — | — |
| UpdateBlock | encode | rust-owned | 20.54 | 0.00 | 0.00 | 0.36 | ours |

## loopback

| Size / operation | Side | Samples | ns/echo | msgs/s | MB/s | p50 µs | p99 µs | ours/Go | Winner |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 32 / echo | go | 100 | 45,370.00 | 22,041.00 | 0.71 | 0.00 | 546.70 | — | — |
| 32 / echo | rust-owned | 100 | 1,303,945.00 | 766.90 | 0.02 | 64.90 | 244.00 | 28.74 | Go |
| 4096 / echo | go | 10 | 110,380.00 | 9,059.61 | 37.11 | 0.00 | 588.70 † | — | — |
| 4096 / echo | rust-owned | 10 | 39,785,120.00 | 25.14 | 0.10 | 336.10 | 101,981.10 † | 360.44 | Go |
| 512 / echo | go | 100 | 50,085.00 | 19,966.06 | 10.22 | 0.00 | 581.90 | — | — |
| 512 / echo | rust-owned | 100 | 2,853,935.00 | 350.39 | 0.18 | 80.30 | 75,793.50 | 56.98 | Go |
| 65536 / echo | go | 10 | 1,054,070.00 | 948.70 | 62.17 | 1,052.00 | 1,342.40 † | — | — |
| 65536 / echo | rust-owned | 10 | 31,776,850.00 | 31.47 | 2.06 | 1,142.60 | 105,923.60 † | 30.15 | Go |

† Low-sample p99: for ten samples, nearest-rank p99 is the maximum, not a stable population tail estimate. Both implementations use the same per-size counts and fresh connections in every round.

## micro

| Case | Operation | Side | ns/op | allocs/op | allocated B/op | ours/Go | Winner |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| datagram_32 | decode | rust-owned | 66.42 | 1.00 | 80.00 | — | — |
| datagram_32 | encode | rust-owned | 13.24 | 0.00 | 0.00 | — | — |
| datagram_4096 | decode | rust-owned | 67.90 | 1.00 | 80.00 | — | — |
| datagram_4096 | encode | rust-owned | 33.24 | 0.00 | 0.00 | — | — |
| datagram_512 | decode | rust-owned | 69.14 | 1.00 | 80.00 | — | — |
| datagram_512 | encode | rust-owned | 14.74 | 0.00 | 0.00 | — | — |
| encapsulated_32 | decode | rust-owned | 26.46 | 0.00 | 0.00 | — | — |
| encapsulated_32 | encode | rust-owned | 11.14 | 0.00 | 0.00 | — | — |
| encapsulated_4096 | decode | rust-owned | 25.32 | 0.00 | 0.00 | — | — |
| encapsulated_4096 | encode | rust-owned | 36.32 | 0.00 | 0.00 | — | — |
| encapsulated_512 | decode | rust-owned | 25.38 | 0.00 | 0.00 | — | — |
| encapsulated_512 | encode | rust-owned | 12.38 | 0.00 | 0.00 | — | — |
| split_2x512 | reassemble | rust-owned | 365.64 | 4.00 | 1,972.00 | — | — |

## Protocolgen contract baseline (third implementation)

Criterion median estimates; independent sampling, not the alternating custom-loop estimator. Length framing matches the original protocolgen benchmark; see README for the Valentine boundary caveat.

| Case | Operation | Contract ns/op |
| --- | --- | ---: |
| addactor | decode | 354.64 |
| animate | decode | 35.46 |
| availablecommands | decode | 51,634.93 |
| biomedefinitionlist | decode | 9,922.50 |
| craftingdata | decode | 430,811.73 |
| creativecontent | decode | 45,612.50 |
| inventorytransactionnormal | decode | 382.60 |
| inventorytransactionuseitem | decode | 146.23 |
| inventorytransactionuseitemonentity | decode | 103.24 |
| itemregistry | decode | 82,306.93 |
| itemstackrequest | decode | 540.94 |
| levelchunk | decode | 64.31 |
| levelsoundevent | decode | 112.61 |
| mobequipment | decode | 65.54 |
| moveactordelta | decode | 58.15 |
| moveplayer | decode | 72.79 |
| networkstacklatency | decode | 33.11 |
| playerauthinput | decode | 149.81 |
| playerauthinputinteract | decode | 284.41 |
| setactordata | decode | 243.60 |
| setactormotion | decode | 41.19 |
| startgame | decode | 10,048.69 |
| subchunk | decode | 20,097.12 |
| text | decode | 117.57 |
| updateattributes | decode | 360.20 |
| updateblock | decode | 47.71 |
| chunkradius | decodebatch | 883.95 |
| clienttick | decodebatch | 1,003.07 |
| servertick | decodebatch | 6,311.11 |
| spawn | decodebatch | 6,810.47 |
| addactor | encode | 236.73 |
| animate | encode | 22.84 |
| availablecommands | encode | 17,638.38 |
| biomedefinitionlist | encode | 2,684.76 |
| craftingdata | encode | 117,453.20 |
| creativecontent | encode | 38,738.89 |
| inventorytransactionnormal | encode | 273.58 |
| inventorytransactionuseitem | encode | 115.61 |
| inventorytransactionuseitemonentity | encode | 88.28 |
| itemregistry | encode | 21,766.78 |
| itemstackrequest | encode | 232.64 |
| levelchunk | encode | 123.90 |
| levelsoundevent | encode | 46.52 |
| mobequipment | encode | 47.66 |
| moveactordelta | encode | 41.76 |
| moveplayer | encode | 41.67 |
| networkstacklatency | encode | 16.53 |
| playerauthinput | encode | 114.39 |
| playerauthinputinteract | encode | 252.56 |
| setactordata | encode | 189.69 |
| setactormotion | encode | 30.01 |
| startgame | encode | 1,603.83 |
| subchunk | encode | 9,971.12 |
| text | encode | 63.58 |
| updateattributes | encode | 128.75 |
| updateblock | encode | 40.21 |
| chunkradius | encodebatch | 2,892.61 |
| clienttick | encodebatch | 612.97 |
| servertick | encodebatch | 3,722.57 |
| spawn | encodebatch | 4,403.79 |

## Corpus exclusions and API coverage

Corpus encoder: gophertunnel `85d4022854e1` (`v1.25.3-0.20261019151115-85d4022854e1`), as pinned by protocolgen retail/go.mod. Requested live-master reference: HashimTheArab/gophertunnel `80c811b6186016b3860c358368cfa47e507f26e9`; go-raknet: `87a6b7e53799290d70ac25bcd03645d7ead00d2c`. The newer corpus encoder is not silently substituted for the requested reference.

| Case | Implementation | Excluded because |
| --- | --- | --- |
| AddActor | rust-borrowed | public decoder returns Raw |
| ClientTick | rust-borrowed | public decoder returns Raw |
| CraftingData | go | typed decode: unknown value '"tags"' for enum type 'item descriptor type' |
| InventoryTransactionNormal | rust-borrowed | public decoder returns Raw |
| InventoryTransactionUseItem | rust-borrowed | public decoder returns Raw |
| InventoryTransactionUseItemOnEntity | rust-borrowed | public decoder returns Raw |
| ItemStackRequest | rust-borrowed | public decoder returns Raw |
| PlayerAuthInput | rust-borrowed | public decoder returns Raw |
| PlayerAuthInputInteract | rust-borrowed | public decoder returns Raw |
| ServerTick | rust-borrowed | public decoder returns Raw |
| SetActorData | rust-borrowed | public decoder returns Raw |
| Spawn | rust-borrowed | public decoder returns Raw |
| StartGame | rust-borrowed | public decoder returns Raw |
| SubChunk | rust-borrowed | public decoder returns Raw |

Borrowed encoding has no public API. Go RakNet datagram/split micro APIs are private; only Rust micro timings are available, with no speed ratio or winner. Go compression-only rows have no equivalent jolyne comparison in this run.

## Transport interop sanity

| Direction | Sizes | Echoes per size | Result |
| --- | --- | ---: | --- |
| go-to-rust | 32,512,4096,65536 | 100,100,10,10 | PASS |
| rust-to-go | 32,512,4096,65536 | 100,100,10,10 | PASS |

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

- loopback / 4096 / echo (rust-owned): 360.44× Go time. Profile this measured boundary before attributing a cause.
- loopback / 512 / echo (rust-owned): 56.98× Go time. Profile this measured boundary before attributing a cause.
- loopback / 65536 / echo (rust-owned): 30.15× Go time. Profile this measured boundary before attributing a cause.

## Limits and reproducibility

See [README.md](README.md), [Go API caveats](go/CAVEATS.md), and [Rust API caveats](../../crates/bench-compare/README.md). Missing rows are not zero-cost operations: private transport internals and untyped borrowed variants cannot be compared through public APIs.

Go GC accounting and Rust allocator accounting are not equivalent retained-memory measurements. Timing loops include destruction; Go GC may run during later iterations. Small timings on Windows depend on clock precision and scheduling. No production implementation was optimized for this run.

Raw per-round JSONL, stderr, metadata, interop outcomes and summary.json remain in the ignored results directory. Run `./tools/bench-compare/run.ps1 -Contract` to regenerate.

Observed Go p50 RTT values include zero. Clock quantization may contribute [INFERENCE]; these values must not be interpreted as physically zero RTT. Throughput uses the complete elapsed exchange interval.

## Validation of this committed report

| Command | Observed result |
| --- | --- |
| `pwsh -NoProfile -File tools/bench-compare/run.ps1 -Rounds 3 -Count 100 -SplitCount 10 -GoCpu 4 -Contract` | PASS: all three alternating rounds, four sizes, both interop directions, typed compressed Go batches and the original protocolgen Criterion baseline |
| `cargo fmt --all` | PASS |
| `cargo clippy -p bench-compare --all-targets -- -D warnings` | PASS |
| `cargo test -p bench-compare` | PASS: one contract test; no failures |
| `go vet ./...` in `tools/bench-compare/go` | PASS |
| `go test -timeout 60s ./...` in `tools/bench-compare/go` | PASS, including real fragmented-echo shutdown and diagnostic observer regressions |
| `python tools/bench-compare/test_report.py` | PASS: stale extra rounds cannot alter the current median |
| `cargo bench -p bench-compare --bench codec -- --warm-up-time 0.1 --measurement-time 0.2 --sample-size 10` | PASS: actual Valentine Criterion surface exercised separately; its estimates are not substituted into the alternating-loop tables |

Every normalized measurement row has three rounds. Coverage includes all 26 owned single-packet decodes, four typed compressed Go batch cases, and eight loopback implementation/size rows. The requested Go reference excludes CraftingData as documented above. This validation section records commands exercised for this delivery; regenerating the measurement report does not implicitly rerun separate lint/test commands.
