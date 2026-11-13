//! Join payloads captured from BDS 1.26.51.
//!
//! The client requires `JigsawStructureData` before `StartGame`; BDS also
//! sends `VoxelShapes` there. After `StartGame` BDS sends its camera presets,
//! aim-assist presets and armour trim definitions, the same for every player
//! and session. All five payloads are vendored verbatim under
//! `data/bds-1.26.51/` (provenance and refresh steps in its README).

use bytes::Bytes;
use jolyne::valentine::{
    CameraAimAssistPresetsPacket, CameraPresetsPacket, JigsawStructureDataPacket, TrimDataPacket,
    VoxelShapesPacket,
};
use valentine::bedrock::codec::{BedrockCodec, Nbt};

const JIGSAW_STRUCTURE_DATA: &[u8] =
    include_bytes!("../../../data/bds-1.26.51/jigsaw_structure_data.nbt");
const VOXEL_SHAPES: &[u8] = include_bytes!("../../../data/bds-1.26.51/voxel_shapes.bin");
const CAMERA_PRESETS: &[u8] = include_bytes!("../../../data/bds-1.26.51/camera_presets.bin");
const CAMERA_AIM_ASSIST_PRESETS: &[u8] =
    include_bytes!("../../../data/bds-1.26.51/camera_aim_assist_presets.bin");
const TRIM_DATA: &[u8] = include_bytes!("../../../data/bds-1.26.51/trim_data.bin");

/// BDS's jigsaw structure definitions. The vendored bytes are checked to be
/// exactly one network-NBT compound by the tests below.
pub(crate) fn jigsaw_structures() -> JigsawStructureDataPacket {
    JigsawStructureDataPacket {
        jigsaw_structure_data_tag: Nbt(Bytes::from_static(JIGSAW_STRUCTURE_DATA)),
    }
}

/// Decodes a vendored payload that must be exactly one `T`.
///
/// # Panics
/// If the payload does not decode or has trailing bytes, which the tests
/// below rule out.
fn vendored<T: BedrockCodec<Args = ()>>(mut payload: &[u8], name: &str) -> T {
    let packet = T::decode(&mut payload, ()).unwrap_or_else(|error| {
        panic!("vendored {name} does not decode: {error:?}");
    });
    assert!(payload.is_empty(), "vendored {name} has trailing bytes");
    packet
}

/// BDS's named voxel shapes.
pub(crate) fn voxel_shapes() -> VoxelShapesPacket {
    vendored(VOXEL_SHAPES, "VoxelShapes")
}

/// BDS's built-in camera presets (`minecraft:first_person`,
/// `minecraft:fixed_boom`, ...), sent once during the join.
pub(crate) fn camera_presets() -> CameraPresetsPacket {
    vendored(CAMERA_PRESETS, "CameraPresets")
}

/// BDS's built-in aim-assist categories and presets, sent once during the join.
pub(crate) fn camera_aim_assist_presets() -> CameraAimAssistPresetsPacket {
    vendored(CAMERA_AIM_ASSIST_PRESETS, "CameraAimAssistPresets")
}

/// BDS's armour trim patterns and materials, sent once during the join.
pub(crate) fn trim_data() -> TrimDataPacket {
    vendored(TRIM_DATA, "TrimData")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reencode<T: BedrockCodec>(value: &T) -> Vec<u8> {
        let mut out = Vec::new();
        value.encode(&mut out).unwrap();
        out
    }

    #[test]
    fn vendored_payloads_decode_exactly_and_reencode_identically() {
        let mut buf = JIGSAW_STRUCTURE_DATA;
        let jigsaw = JigsawStructureDataPacket::decode(&mut buf, ()).unwrap();
        assert!(buf.is_empty(), "jigsaw payload has trailing bytes");
        assert_eq!(jigsaw, jigsaw_structures());
        assert_eq!(reencode(&jigsaw), JIGSAW_STRUCTURE_DATA);

        let shapes = voxel_shapes();
        assert!(!shapes.shapes.is_empty() && !shapes.name_map.is_empty());
        assert_eq!(reencode(&shapes), VOXEL_SHAPES);
    }

    /// The vendored camera and trim payloads are the committed member join
    /// fixtures without their packet ID header, and survive a decode/encode
    /// round trip unchanged.
    #[test]
    fn post_start_game_payloads_match_the_bds_join_capture() {
        let capture = |file: &str| {
            let path = format!(
                "{}/../bds-extractor/captures/1.26.51/member/join/{file}",
                env!("CARGO_MANIFEST_DIR")
            );
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
            let header = bytes.iter().position(|byte| byte & 0x80 == 0).unwrap() + 1;
            bytes[header..].to_vec()
        };
        assert_eq!(
            capture("00043-CameraPresetsPacket.bin"),
            reencode(&camera_presets())
        );
        assert_eq!(CAMERA_PRESETS, reencode(&camera_presets()).as_slice());
        assert_eq!(
            capture("00044-CameraAimAssistPresetsPacket.bin"),
            reencode(&camera_aim_assist_presets())
        );
        assert_eq!(
            CAMERA_AIM_ASSIST_PRESETS,
            reencode(&camera_aim_assist_presets()).as_slice()
        );
        assert_eq!(capture("00048-TrimDataPacket.bin"), reencode(&trim_data()));
        assert_eq!(TRIM_DATA, reencode(&trim_data()).as_slice());
        assert!(!camera_presets().camera_presets.presets.is_empty());
        assert!(!trim_data().trim_material_list.is_empty());
    }
}
