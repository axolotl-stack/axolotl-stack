use client_diff::conformance::check;

/// A captured SetTime packet: header 0x0a, zigzag varint time 6000.
const SET_TIME: [u8; 3] = [0x0a, 0xe0, 0x5d];

#[test]
fn exact_packet_passes_every_codec_path() {
    let outcome = check(&SET_TIME);
    assert!(outcome.passed(), "{outcome:?}");
    assert!(outcome.borrowed_view);
}

#[test]
fn trailing_payload_bytes_fail_both_decoders() {
    let mut packet = SET_TIME.to_vec();
    packet.push(0x00);
    let outcome = check(&packet);
    assert!(!outcome.passed());
    assert!(
        outcome
            .owned_error
            .as_deref()
            .is_some_and(|error| error.contains("TrailingPacketBytes"))
    );
    assert!(
        outcome
            .borrowed_error
            .as_deref()
            .is_some_and(|error| error.contains("TrailingPacketBytes"))
    );
}
