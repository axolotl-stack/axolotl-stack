# BDS 1.26.51 join payloads

Packet payloads (without the packet ID header) that Bedrock Dedicated Server
1.26.51 (protocol 2193) sends during the join. The first two come before
`StartGame`; unastar sends them through `jolyne::WorldTemplate` in the same
order, and the client refuses to join ("Missing structure data from
server.") without `JigsawStructureData`. The other three come after it, in
the batch that ends with `AvailableCommands`; unastar sends them from
`server::game::join`.

| File | Packet | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| `jigsaw_structure_data.nbt` | `JigsawStructureData` (313): network NBT | 209307 | `d15c9786ef12b21b7e67e022d884cc1a1d3e96081296d4f904eae95f34b3c3b1` |
| `voxel_shapes.bin` | `VoxelShapes` (337) | 23178 | `7cd81b39d20489d703c5ca0319ff8793a3b225e49a701edf504516a12fffc406` |
| `camera_presets.bin` | `CameraPresets` (198) | 337 | `5cea450cbd4b48de442512d6efa3889d85c538f24e101371528fa3ebc566d0df` |
| `camera_aim_assist_presets.bin` | `CameraAimAssistPresets` (320) | 1096 | `6e98c043f457b00a43ef28b91e018d1b43d7bd159d9de1168c833209b9c12b5a` |
| `trim_data.bin` | `TrimData` (302) | 1269 | `db0fdab6ea1618b6fd634b7865d9f5856ad7b04c8b1959cdac2ca0ac4cbd5bed` |

Source: `tools/client-diff` captures of BDS 1.26.51 / Endstone 0.11.12
(offline, fixed-seed harness world) taken on 2026-10-22. Every one of the 54
captures in that session carried byte-identical copies of both packets, so the
payloads do not depend on the player or session.

To refresh after a BDS update, capture a join with
`./tools/client-diff/run.ps1`, take the first `s2c` record with ID 313 / 337
from a `*.jsonl` log, strip the two-byte varuint packet ID from `hex`, and
write the remaining bytes here. `server::game::join_data` tests decode both
files with the generated codec and reject trailing bytes.

The camera and trim payloads are the committed member join fixtures
`crates/bds-extractor/captures/1.26.51/member/join/000{43,44,48}-*.bin`
without their packet ID header. They were byte-identical in every join of
the 2026-11-11 `capture.py --pair` runs (host and up to twelve guests), so
they do not depend on the player or session. To refresh, take the same
packets from a new `capture.py` member join and strip the varuint header;
the `server::game::join_data` tests compare them with the fixtures.
