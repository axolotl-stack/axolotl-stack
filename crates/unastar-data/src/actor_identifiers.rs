//! AvailableActorIdentifiers facts captured from BDS 1.26.51 by TOOLING.
//! All five fields and capture row order are retained; actor definitions do
//! not invent runtime ids, built-in rows or spawn-egg flags.

use serde::Deserialize;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorIdentifier {
    pub rid: u32,
    pub id: String,
    pub bid: String,
    pub hasspawnegg: u8,
    pub summonable: u8,
}

#[derive(Deserialize)]
struct Artifact {
    bds_version: String,
    protocol: u32,
    entries: Vec<ActorIdentifier>,
}

static VANILLA: LazyLock<Vec<ActorIdentifier>> = LazyLock::new(|| {
    let artifact: Artifact = serde_json::from_str(include_str!("../output/actor_identifiers.json"))
        .expect("captured actor identifier data");
    assert_eq!(artifact.bds_version, "1.26.51");
    assert_eq!(artifact.protocol, 2193);
    let mut ids = HashSet::new();
    for row in &artifact.entries {
        assert!(ids.insert(&row.id), "duplicate actor identifier {}", row.id);
        assert!(
            row.hasspawnegg <= 1 && row.summonable <= 1,
            "invalid actor flags for {}",
            row.id
        );
        assert!(
            i32::try_from(row.rid).is_ok(),
            "actor runtime id exceeds packet range"
        );
    }
    artifact.entries
});

/// Rows in BDS capture order, not identifier or runtime-id order.
pub fn vanilla() -> &'static [ActorIdentifier] {
    &VANILLA
}
