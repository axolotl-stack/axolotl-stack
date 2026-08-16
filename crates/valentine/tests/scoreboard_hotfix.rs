#![cfg(feature = "bedrock_1_26_44")]

use valentine::bedrock::{
    codec::BedrockCodec,
    version::v1_26_44::{RemoveScore, ScoreboardId},
};

fn remove_score(objective_name: Option<Option<&str>>) -> RemoveScore {
    RemoveScore {
        action: "remove".to_string(),
        scoreboard_id: ScoreboardId { scoreboard_id: 7 },
        objective_name: objective_name.map(|value| value.map(str::to_string)),
    }
}

#[test]
fn remove_score_preserves_both_1_26_44_optional_markers() {
    let cases = [
        (None, vec![6, b'r', b'e', b'm', b'o', b'v', b'e', 14, 0]),
        (
            Some(None),
            vec![6, b'r', b'e', b'm', b'o', b'v', b'e', 14, 1, 0],
        ),
        (
            Some(Some("obj")),
            vec![
                6, b'r', b'e', b'm', b'o', b'v', b'e', 14, 1, 1, 3, b'o', b'b', b'j',
            ],
        ),
    ];

    for (objective_name, expected) in cases {
        let value = remove_score(objective_name);
        let mut encoded = Vec::new();
        value.encode(&mut encoded).expect("encode RemoveScore");
        assert_eq!(encoded, expected);

        let mut input = expected.as_slice();
        let decoded = RemoveScore::decode(&mut input, ()).expect("decode RemoveScore");
        assert_eq!(decoded, value);
        assert!(input.is_empty(), "RemoveScore left trailing bytes");
    }
}
