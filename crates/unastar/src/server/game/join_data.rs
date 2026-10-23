//! Pre-`StartGame` packets captured from BDS 1.26.51.
//!
//! The client requires `JigsawStructureData` before `StartGame`; BDS also
//! sends `VoxelShapes` there. Both payloads are vendored verbatim under
//! `data/bds-1.26.51/` (provenance and refresh steps in its README).

use bytes::Bytes;
use jolyne::valentine::{JigsawStructureDataPacket, VoxelShapesPacket};
use valentine::bedrock::codec::{BedrockCodec, Nbt};

const JIGSAW_STRUCTURE_DATA: &[u8] =
    include_bytes!("../../../data/bds-1.26.51/jigsaw_structure_data.nbt");
const VOXEL_SHAPES: &[u8] = include_bytes!("../../../data/bds-1.26.51/voxel_shapes.bin");

/// BDS's jigsaw structure definitions. The vendored bytes are checked to be
/// exactly one network-NBT compound by the tests below.
pub(crate) fn jigsaw_structures() -> JigsawStructureDataPacket {
    JigsawStructureDataPacket {
        jigsaw_structure_data_tag: Nbt(Bytes::from_static(JIGSAW_STRUCTURE_DATA)),
    }
}

/// BDS's named voxel shapes.
///
/// # Panics
/// If the vendored payload does not decode, which the tests below rule out.
pub(crate) fn voxel_shapes() -> VoxelShapesPacket {
    let mut buf = VOXEL_SHAPES;
    let shapes = VoxelShapesPacket::decode(&mut buf, ()).expect("vendored VoxelShapes decodes");
    assert!(buf.is_empty(), "vendored VoxelShapes has trailing bytes");
    shapes
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
}
