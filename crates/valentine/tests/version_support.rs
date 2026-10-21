#[cfg(feature = "bedrock_1_26_51")]
#[test]
fn exposes_bedrock_1_26_51_with_protocol_2193() {
    use valentine::bedrock::version::v1_26_51;
    assert_eq!(v1_26_51::GAME_VERSION, "1.26.51");
    assert_eq!(v1_26_51::PROTOCOL_VERSION, 2193);
    assert_eq!(v1_26_51::INFO.minecraft_version, "1.26.51");
    assert_eq!(v1_26_51::INFO.protocol_version, 2193);
}
