//! The feature IR rejects unknown data, and the generated decoration order is
//! consistent with every biome's feature lists.

#[allow(dead_code)]
#[path = "../codegen/parser/feature.rs"]
mod feature;

use feature::{ConfiguredFeature, PlacedFeatureJson};
use unastar_noise::features::generated::{
    BIOME_NAMES, NETHER_FEATURES, OVERWORLD_BIOMES, OVERWORLD_FEATURES, PLACED_NAMES,
};

#[test]
fn unknown_feature_type_is_rejected() {
    let err = serde_json::from_str::<ConfiguredFeature>(
        r#"{"type":"minecraft:not_a_feature","config":{}}"#,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("not_a_feature"), "{err}");
}

#[test]
fn unknown_placement_modifier_is_rejected() {
    let err = serde_json::from_str::<PlacedFeatureJson>(
        r#"{"feature":"minecraft:ore_iron","placement":[{"type":"minecraft:not_a_modifier"}]}"#,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("not_a_modifier"), "{err}");
}

#[test]
fn unknown_config_field_is_rejected() {
    let err = serde_json::from_str::<ConfiguredFeature>(
        r#"{"type":"minecraft:ore","config":{"targets":[],"size":3,"discard_chance_on_air_exposure":0.0,"bogus":1}}"#,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("bogus"), "{err}");
}

#[test]
fn inline_object_errors_keep_their_cause() {
    // A placed-feature reference may be a string or an inline object; a bad
    // inline object must report the inner problem, not "no variant matched".
    let err = serde_json::from_str::<PlacedFeatureJson>(
        r#"{"feature":{"type":"minecraft:simple_block","config":{"to_place":{"type":"minecraft:nope"}}},"placement":[]}"#,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("nope"), "{err}");
}

/// Java's sorted step lists must keep each biome's relative feature order:
/// that is what makes per-biome decoration order match vanilla.
#[test]
fn step_order_respects_every_biome_list() {
    for (dim, data) in [
        ("overworld", &OVERWORLD_FEATURES),
        ("nether", &NETHER_FEATURES),
    ] {
        for (b, steps) in data.biome_steps.iter().enumerate() {
            for (s, idx) in steps.iter().enumerate() {
                assert!(
                    idx.windows(2).all(|w| w[0] < w[1]),
                    "{dim} {} step {s}: indices not strictly increasing",
                    BIOME_NAMES[b]
                );
                for &i in idx.iter() {
                    assert!((i as usize) < data.steps[s].len());
                }
            }
        }
    }
}

#[test]
fn overworld_biomes_decorate_only_overworld_features() {
    let plains = BIOME_NAMES
        .iter()
        .position(|b| *b == "minecraft:plains")
        .unwrap();
    let nether = BIOME_NAMES
        .iter()
        .position(|b| *b == "minecraft:nether_wastes")
        .unwrap();
    assert!(OVERWORLD_BIOMES & (1u128 << plains) != 0);
    assert!(OVERWORLD_BIOMES & (1u128 << nether) == 0);
    let ores: Vec<&str> = OVERWORLD_FEATURES.biome_steps[plains][6]
        .iter()
        .map(|&i| PLACED_NAMES[OVERWORLD_FEATURES.steps[6][i as usize] as usize])
        .collect();
    assert!(ores.contains(&"minecraft:ore_diamond"), "{ores:?}");
    assert!(ores.contains(&"minecraft:ore_coal_upper"), "{ores:?}");
    assert!(
        OVERWORLD_FEATURES.biome_steps[nether]
            .iter()
            .all(|s| s.is_empty())
    );
}
