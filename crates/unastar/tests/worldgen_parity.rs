//! Parity ratchets for the Java-style generator (decision D8).
//!
//! Each floor is the value measured when it was last raised; a change may only raise a
//! floor (or lower an error ceiling), in the same commit as the improvement. Metrics are
//! defined in `unastar::world::generator::parity`; print them (and the block histograms)
//! with
//! `cargo run -p unastar --example worldgen_parity_report`.
//!
//! The truth for the Java-style generator is the Java Edition reference world. The BDS
//! fixture only gates what both editions share, biome placement: BDS surface blocks come
//! from Bedrock surface builders and its caves from Bedrock carver features, so BDS terrain
//! and surface heights belong to the Bedrock-style generator's gates (its noise router is
//! shared, which `examples/worldgen_parity_report.rs` still shows).

use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "java-worldgen")]
use unastar::world::generator::parity::{Fixture, ParityMetrics, measure};
use unastar::world::generator::parity::{ProtoFixture, measure_proto};
use unastar::world::generator::{ChunkPipeline, JavaGenerator};

#[cfg(feature = "java-worldgen")]
fn metrics(fixture: &str) -> ParityMetrics {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let metrics = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let fixture = Fixture::load(&path).unwrap();
            let pipeline = ChunkPipeline::new(Arc::new(JavaGenerator::new(fixture.seed)));
            measure(&fixture, &pipeline).metrics
        })
        .unwrap()
        .join()
        .unwrap();
    println!("{fixture}: {metrics:#?}");
    metrics
}

/// `(name, value, floor)` must have `value >= floor`; `(name, value, ceiling)` with a
/// leading `-` in the name must have `value <= ceiling`.
fn assert_ratchets(fixture: &str, checks: &[(&str, f64, f64)]) {
    let failures: Vec<String> = checks
        .iter()
        .filter_map(|&(name, value, bound)| {
            if let Some(name) = name.strip_prefix('-') {
                (value > bound)
                    .then(|| format!("{name} {value:.4} rose above its ceiling {bound:.4}"))
            } else {
                (value < bound)
                    .then(|| format!("{name} {value:.4} fell below its floor {bound:.4}"))
            }
        })
        .collect();
    assert!(failures.is_empty(), "{fixture}: {}", failures.join("; "));
}

/// Finished chunks include structures, whose templates need `java-worldgen`.
#[cfg(feature = "java-worldgen")]
#[test]
fn java_style_vs_java_reference() {
    let fixture = "java_1_21_11_seed_12345.json";
    let m = metrics(fixture);
    assert_ratchets(
        fixture,
        &[
            ("terrain_exact", m.terrain_exact, 0.9484),
            ("terrain_within_2", m.terrain_within_2, 0.9847),
            ("-terrain_mean_abs_error", m.terrain_mean_abs_error, 0.2551),
            ("surface_exact", m.surface_exact, 0.9223),
            ("surface_block", m.surface_block, 0.9138),
            ("-histogram_l1", m.histogram_l1, 0.0124),
            ("surface_biome", m.surface_biome, 1.0),
        ],
    );
}

/// Biome placement is shared by both editions (BDS places Java's biomes); terrain height
/// is not gated here because BDS carves with its own cave carver features.
#[cfg(feature = "java-worldgen")]
#[test]
fn java_style_biomes_vs_bds_reference() {
    let fixture = "bds_1_26_51_seed_12345.json";
    let m = metrics(fixture);
    assert_ratchets(fixture, &[("surface_biome", m.surface_biome, 0.9946)]);
}

/// Exact pre-decoration voxels (noise, aquifers, ore veins, surface, carvers) and quart
/// biomes against the undecorated Java world.
#[test]
fn java_style_protos_vs_java_reference() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/java_1_21_11_seed_12345_proto.json");
    let metrics = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let fixture = ProtoFixture::load(&path).unwrap();
            let pipeline = ChunkPipeline::new(Arc::new(JavaGenerator::new(fixture.seed)));
            measure_proto(&fixture, &pipeline).metrics
        })
        .unwrap()
        .join()
        .unwrap();
    println!("proto: {metrics:#?}");
    assert_ratchets(
        "java_1_21_11_seed_12345_proto.json",
        &[
            ("block_exact", metrics.block_exact, 0.9948),
            ("class_exact", metrics.class_exact, 0.9954),
            ("biome_exact", metrics.biome_exact, 1.0),
        ],
    );
}

/// World spawn of the Java reference world (seed 12345, `level.dat` `spawn.pos`). Java
/// picks the same spawn with and without features, so the undecorated generator must hit
/// it exactly.
/// `JavaGenerator` always loads the structure tables, which need `java-worldgen`.
#[cfg(feature = "java-worldgen")]
#[test]
fn java_style_spawn_matches_java_reference() {
    let spawn = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| ChunkPipeline::new(Arc::new(JavaGenerator::new(12345))).find_spawn())
        .unwrap()
        .join()
        .unwrap();
    assert_eq!((spawn.x, spawn.y, spawn.z), (96, 136, -32));
}
