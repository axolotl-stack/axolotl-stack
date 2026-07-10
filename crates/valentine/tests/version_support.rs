use valentine::bedrock::version::v1_26_30;

#[test]
fn exposes_bedrock_1_26_30() {
    assert_eq!(v1_26_30::GAME_VERSION, "1.26.30");
    assert_eq!(v1_26_30::PROTOCOL_VERSION, 1001);
    assert_eq!(v1_26_30::INFO.minecraft_version, "1.26.30");
    assert_eq!(v1_26_30::INFO.protocol_version, 1001);
}
