//! Parity ratchets for the Java-style generator (decision D8).
//!
//! Each floor is the value measured when it was last raised; a change may only raise a
//! floor (or lower an error ceiling), in the same commit as the improvement. Metrics are
//! defined in `unastar::world::generator::parity`; print them (and the block histograms,
//! which stay report-only while decoration is missing) with
//! `cargo run -p unastar --example worldgen_parity_report`.
//!
//! The truth for the Java-style generator is the Java Edition reference world. The BDS
//! fixture only gates what both editions share: terrain shape (BDS behaves as Java's noise
//! router) and biome placement. BDS surface blocks come from Bedrock surface builders and
//! belong to the Bedrock-style generator's gates.

use std::path::Path;
use std::sync::Arc;

use unastar::world::generator::parity::{
    Fixture, ParityMetrics, ProtoFixture, measure, measure_proto,
};
use unastar::world::generator::{ChunkPipeline, JavaGenerator};

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

#[test]
fn java_style_vs_java_reference() {
    let fixture = "java_1_21_11_seed_12345.json";
    let m = metrics(fixture);
    assert_ratchets(
        fixture,
        &[
            ("terrain_exact", m.terrain_exact, 0.8036),
            ("terrain_within_2", m.terrain_within_2, 0.9008),
            ("-terrain_mean_abs_error", m.terrain_mean_abs_error, 1.3875),
            ("surface_exact", m.surface_exact, 0.8806),
            ("surface_block", m.surface_block, 0.8391),
            ("surface_biome", m.surface_biome, 0.9999),
        ],
    );
}

#[test]
fn java_style_terrain_and_biomes_vs_bds_reference() {
    let fixture = "bds_1_26_51_seed_12345.json";
    let m = metrics(fixture);
    assert_ratchets(
        fixture,
        &[
            ("terrain_exact", m.terrain_exact, 0.8459),
            ("terrain_within_2", m.terrain_within_2, 0.9428),
            ("-terrain_mean_abs_error", m.terrain_mean_abs_error, 0.7783),
            ("surface_biome", m.surface_biome, 0.9946),
        ],
    );
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
            ("block_exact", metrics.block_exact, 0.9801),
            ("class_exact", metrics.class_exact, 0.9812),
            ("biome_exact", metrics.biome_exact, 1.0),
        ],
    );
}
