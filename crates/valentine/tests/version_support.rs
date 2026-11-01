#[cfg(feature = "bedrock_1_26_51")]
#[test]
fn exposes_bedrock_1_26_51_with_protocol_2193() {
    use valentine::bedrock::version::v1_26_51;
    assert_eq!(v1_26_51::GAME_VERSION, "1.26.51");
    assert_eq!(v1_26_51::PROTOCOL_VERSION, 2193);
    assert_eq!(v1_26_51::INFO.minecraft_version, "1.26.51");
    assert_eq!(v1_26_51::INFO.protocol_version, 2193);
}

#[cfg(feature = "bedrock_1_26_60")]
#[test]
fn exposes_bedrock_1_26_60_with_protocol_2225() {
    use valentine::bedrock::version::v1_26_60;
    assert_eq!(v1_26_60::GAME_VERSION, "1.26.60");
    assert_eq!(v1_26_60::PROTOCOL_VERSION, 2225);
    assert_eq!(v1_26_60::INFO.protocol_version, 2225);
}
