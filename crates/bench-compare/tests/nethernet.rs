use bench_compare::nethernet::{CANDIDATE, signal_fixtures};
use tokio_nethernet::{Signal, parse_ice_candidate};

#[test]
fn signaling_fixtures_roundtrip_without_losing_sdp_newlines() {
    for (_, text) in signal_fixtures() {
        let signal = Signal::parse(&text, "2".into()).unwrap();
        assert_eq!(signal.to_string(), text);
        let encoded = serde_json::to_string(&text).unwrap();
        assert!(!encoded.contains('\n'));
        assert_eq!(serde_json::from_str::<String>(&encoded).unwrap(), text);
        assert_eq!(signal.connection_id, 42);
    }
}

#[test]
fn candidate_fixture_is_publicly_parseable() {
    assert!(parse_ice_candidate(CANDIDATE).is_ok());
}
