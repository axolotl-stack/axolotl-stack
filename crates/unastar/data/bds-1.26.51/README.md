# BDS 1.26.51 join payloads

Packet payloads (without the packet ID header) that Bedrock Dedicated Server
1.26.51 (protocol 2193) sends before `StartGame`. Unastar sends them through
`jolyne::WorldTemplate` in the same order; the client refuses to join
("Missing structure data from server.") without `JigsawStructureData`.

| File | Packet | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| `jigsaw_structure_data.nbt` | `JigsawStructureData` (313): network NBT | 209307 | `d15c9786ef12b21b7e67e022d884cc1a1d3e96081296d4f904eae95f34b3c3b1` |
| `voxel_shapes.bin` | `VoxelShapes` (337) | 23178 | `7cd81b39d20489d703c5ca0319ff8793a3b225e49a701edf504516a12fffc406` |

Source: `tools/client-diff` captures of BDS 1.26.51 / Endstone 0.11.12
(offline, fixed-seed harness world) taken on 2026-10-22. Every one of the 54
captures in that session carried byte-identical copies of both packets, so the
payloads do not depend on the player or session.

To refresh after a BDS update, capture a join with
`./tools/client-diff/run.ps1`, take the first `s2c` record with ID 313 / 337
from a `*.jsonl` log, strip the two-byte varuint packet ID from `hex`, and
write the remaining bytes here. `server::game::join_data` tests decode both
files with the generated codec and reject trailing bytes.
