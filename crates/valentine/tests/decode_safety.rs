#![cfg(feature = "bedrock_1_26_51")]

use bytes::{BufMut, Bytes, BytesMut};
use valentine::bedrock::codec::{BedrockCodec, MAX_RECURSIVE_VALUE_DEPTH};
use valentine::bedrock::error::DecodeError;
use valentine::bedrock::version::v1_26_51::*;

fn nested_value(depth: usize, map: bool) -> Bytes {
    let mut bytes = BytesMut::new();
    for _ in 0..depth {
        bytes.put_i32_le(if map { 6 } else { 5 });
        bytes.put_u8(1); // one child
        if map {
            bytes.put_u8(0);
        } // empty map key
    }
    bytes.put_i32_le(0); // None leaf
    bytes.freeze()
}

#[test]
fn recursive_values_reject_excessive_depth_and_restore_budget() {
    for map in [false, true] {
        let mut wire = nested_value(MAX_RECURSIVE_VALUE_DEPTH * 2, map);
        assert!(matches!(
            CerealDynamicValue::decode(&mut wire, ()),
            Err(DecodeError::RecursiveDepthExceeded { .. })
        ));
        let mut shallow = nested_value(2, map);
        let value = CerealDynamicValue::decode(&mut shallow, ()).unwrap();
        let mut encoded = BytesMut::new();
        value.encode(&mut encoded).unwrap();
        assert_eq!(encoded.freeze(), nested_value(2, map));
    }
}

#[test]
fn borrowed_packet_decode_enforces_recursive_budget() {
    let mut body = BytesMut::new();
    body.put_u8(1); // updates count
    body.put_u8(1); // DataStoreChange
    body.put_u8(0); // empty store name
    body.put_u8(0); // empty property
    body.put_u32_le(0); // update count
    body.extend_from_slice(&nested_value(MAX_RECURSIVE_VALUE_DEPTH * 2, false));
    let mut payload = BytesMut::new();
    valentine::protocol::wire::write_var_u32(&mut payload, 330);
    payload.extend_from_slice(&body);
    let mut frame = BytesMut::new();
    valentine::protocol::wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    // Recursive unions use the deferred owned fallback; parsing the frame alone
    // deliberately retains raw bytes without decoding that payload.
    let borrowed = BorrowedMcpePacket::decode_inner(&mut frame.freeze()).unwrap();
    assert!(matches!(
        borrowed.into_owned(McpePacketArgs),
        Err(DecodeError::RecursiveDepthExceeded { .. })
    ));
}

#[test]
fn owned_and_borrowed_arrays_reject_impossible_lengths() {
    let mut wire = BytesMut::new();
    wire.put_u32_le(1); // image width
    wire.put_u32_le(1); // image height
    valentine::protocol::wire::write_var_u32(&mut wire, u32::MAX);
    wire.put_u8(42);
    let wire = wire.freeze();
    assert!(matches!(
        SkinImage::decode(&mut wire.clone(), ()),
        Err(DecodeError::ArrayLengthExceeded { .. })
    ));
    assert!(matches!(
        SkinImageView::decode(&mut wire.clone()),
        Err(DecodeError::ArrayLengthExceeded { .. })
    ));
}
