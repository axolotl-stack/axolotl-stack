use bench_compare::transport;

#[test]
fn transport_payload_matches_cross_language_contract() {
    for size in transport::SIZES {
        let payload = transport::payload(size);
        assert_eq!(payload.len(), size);
        assert_eq!(payload[0], 0xfe);
        for (index, byte) in payload.iter().enumerate().skip(1) {
            assert_eq!(*byte, (index % 251) as u8);
        }
    }
}
