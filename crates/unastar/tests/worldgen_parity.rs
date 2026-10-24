//! Parity ratchets for the Java-style generator (decision D8).
//!
//! Each floor is the value measured when it was last raised; a change may only raise a
//! floor (or lower an error ceiling), in the same commit as the improvement. Metrics are
//! defined in `unastar::world::generator::parity`; print them with
//! `cargo run -p unastar --example worldgen_parity_report`.

use std::path::Path;
use std::sync::Arc;

use unastar::world::generator::parity::{
    Fixture, ParityMetrics, ProtoFixture, measure, measure_proto,
};
use unastar::world::generator::{ChunkPipeline, JavaGenerator};

/// Minimum rates (and maximum errors) a generator must keep.
struct Ratchet {
    terrain_exact: f64,
    terrain_within_2: f64,
    max_terrain_mean_abs_error: f64,
    surface_exact: f64,
    surface_block: f64,
    surface_biome: f64,
    max_histogram_l1: f64,
}

fn check(fixture: &str, ratchet: Ratchet) {
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
    let ParityMetrics {
        terrain_exact,
        terrain_within_2,
        terrain_mean_abs_error,
        surface_exact,
        surface_block,
        surface_biome,
        histogram_l1,
        ..
    } = metrics;
    let mut failures = Vec::new();
    let mut floor = |name: &str, value: f64, min: f64| {
        if value < min {
            failures.push(format!("{name} {value:.4} fell below its floor {min:.4}"));
        }
    };
    floor("terrain_exact", terrain_exact, ratchet.terrain_exact);
    floor(
        "terrain_within_2",
        terrain_within_2,
        ratchet.terrain_within_2,
    );
    floor("surface_exact", surface_exact, ratchet.surface_exact);
    floor("surface_block", surface_block, ratchet.surface_block);
    floor("surface_biome", surface_biome, ratchet.surface_biome);
    let mut ceiling = |name: &str, value: f64, max: f64| {
        if value > max {
            failures.push(format!("{name} {value:.4} rose above its ceiling {max:.4}"));
        }
    };
    ceiling(
        "terrain_mean_abs_error",
        terrain_mean_abs_error,
        ratchet.max_terrain_mean_abs_error,
    );
    ceiling("histogram_l1", histogram_l1, ratchet.max_histogram_l1);
    assert!(failures.is_empty(), "{fixture}: {}", failures.join("; "));
}

#[test]
fn java_style_vs_java_reference() {
    check(
        "java_1_21_11_seed_12345.json",
        Ratchet {
            terrain_exact: 0.7659,
            terrain_within_2: 0.8619,
            max_terrain_mean_abs_error: 2.4451,
            surface_exact: 0.8442,
            surface_block: 0.7286,
            surface_biome: 0.0,
            max_histogram_l1: 0.4543,
        },
    );
}

#[test]
fn java_style_vs_bds_reference() {
    check(
        "bds_1_26_51_seed_12345.json",
        Ratchet {
            terrain_exact: 0.8439,
            terrain_within_2: 0.9233,
            max_terrain_mean_abs_error: 1.3262,
            surface_exact: 0.8806,
            surface_block: 0.8128,
            surface_biome: 0.0,
            max_histogram_l1: 0.3724,
        },
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
    let floors = [
        ("block_exact", metrics.block_exact, 0.9606),
        ("class_exact", metrics.class_exact, 0.9743),
        ("biome_exact", metrics.biome_exact, 0.0),
    ];
    for (name, value, floor) in floors {
        assert!(
            value >= floor,
            "proto {name} {value:.4} fell below its floor {floor:.4}"
        );
    }
}
