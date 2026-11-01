/// Convert a legacy actor body after validating it with its original codec.
/// The current layout adds one trailing optional passenger field.
pub(crate) fn legacy_actor(bytes: &[u8]) -> jolyne::valentine::AddActorPacket {
    use valentine::bedrock::codec::BedrockCodec;
    assert_eq!(bytes[0], 0x0d);
    let mut legacy = &bytes[1..];
    valentine::bedrock::version::v1_26_51::AddActorPacket::decode(&mut legacy, ()).unwrap();
    assert!(legacy.is_empty());
    let mut current = bytes[1..].to_vec();
    current.push(0); // No passenger-of-block state in the legacy layout.
    let mut input = current.as_slice();
    let packet = jolyne::valentine::AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    packet
}
