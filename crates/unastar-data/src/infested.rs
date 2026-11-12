//! The blocks a silverfish hides in, as measured on BDS 1.26.51.
//!
//! `data/measured/infested_blocks.json` records, per block a silverfish was
//! left in (a 1 x 1 pit cut into that block, three runs each,
//! `sweep.py silverfish --items blocks`), the infested block it turned the
//! block into, or that it never merged. BDS keeps the pairing in code, so it
//! is a measured fact (method and hash in `data/measured/manifest.json`,
//! family `infested_blocks`), as is the actor a broken infested block lets
//! out.

use std::sync::LazyLock;

use serde::Deserialize;

/// A block a silverfish merges into and the infested block it becomes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InfestedPair {
    /// The plain block (`minecraft:stone`).
    pub host: String,
    /// The block it becomes with a silverfish inside.
    pub infested: String,
}

#[derive(Deserialize)]
struct Table {
    releases: String,
    pairs: Vec<InfestedPair>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/measured/infested_blocks.json"))
        .expect("measured infested blocks decode")
});

/// Every host block with its infested block, in the measured file's order.
pub fn infested_pairs() -> &'static [InfestedPair] {
    &TABLE.pairs
}

/// The actor a broken infested block lets out (`minecraft:silverfish`).
pub fn released_actor() -> &'static str {
    &TABLE.releases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_measured_host_has_one_infested_block() {
        let pairs = infested_pairs();
        assert!(
            pairs
                .iter()
                .any(|p| p.host == "minecraft:stone" && p.infested == "minecraft:infested_stone")
        );
        assert_eq!(released_actor(), "minecraft:silverfish");
        for pair in pairs {
            assert_eq!(
                pairs.iter().filter(|other| other.host == pair.host).count(),
                1,
                "{} is listed twice",
                pair.host
            );
        }
    }
}
