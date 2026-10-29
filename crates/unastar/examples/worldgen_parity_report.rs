//! Print parity metrics against the edition's reference fixtures.
//!
//! `cargo run -p unastar --example worldgen_parity_report -- [--kind java|bedrock] [fixture.json...]`
//! (from the repo root; defaults to the BDS and Java seed-12345 fixtures). The same
//! metrics are ratcheted by `tests/worldgen_parity.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use unastar::world::generator::parity::{Fixture, ProtoFixture, measure, measure_proto};
use unastar::world::generator::{ChunkPipeline, Generator, JavaGenerator};

const DEFAULT_FIXTURES: [&str; 3] = [
    "crates/unastar/tests/fixtures/bds_1_26_51_seed_12345.json",
    "crates/unastar/tests/fixtures/java_1_21_11_seed_12345.json",
    "crates/unastar/tests/fixtures/java_1_21_11_seed_12345_proto.json",
];

fn main() {
    // Terrain generation needs a large stack in debug builds on Windows.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(run)
        .expect("spawn report thread")
        .join()
        .expect("report thread panicked");
}

fn run() {
    let mut args = std::env::args().skip(1);
    let mut kind = String::from("java");
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--kind" {
            kind = args.next().expect("--kind needs java or bedrock");
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    if paths.is_empty() {
        paths.extend(
            DEFAULT_FIXTURES
                .iter()
                .filter(|path| kind != "bedrock" || path.contains("bds_"))
                .map(PathBuf::from),
        );
    }
    for path in paths {
        if path.to_string_lossy().ends_with("_proto.json") {
            assert_eq!(
                kind, "java",
                "proto fixtures describe the Java pre-decoration stage"
            );
            report_proto(&path);
            continue;
        }
        let fixture = Fixture::load(&path).unwrap_or_else(|e| panic!("{e}"));
        let pipeline = ChunkPipeline::new(generator(&kind, fixture.seed));
        let started = std::time::Instant::now();
        let report = measure(&fixture, &pipeline);
        let m = report.metrics;
        println!(
            "## {} ({:?}, {} chunks, {:.1}s)",
            path.display(),
            fixture.edition,
            fixture.chunks.len(),
            started.elapsed().as_secs_f64()
        );
        println!("| metric | value |\n|---|---:|");
        println!("| columns | {} |", m.columns);
        println!("| terrain height exact | {:.1}% |", 100.0 * m.terrain_exact);
        println!(
            "| terrain height within 2 | {:.1}% |",
            100.0 * m.terrain_within_2
        );
        println!(
            "| terrain mean abs error | {:.2} |",
            m.terrain_mean_abs_error
        );
        println!("| surface height exact | {:.1}% |", 100.0 * m.surface_exact);
        println!("| surface block match | {:.1}% |", 100.0 * m.surface_block);
        println!("| surface biome match | {:.1}% |", 100.0 * m.surface_biome);
        println!("| histogram L1 | {:.3} |", m.histogram_l1);

        let mut confusion: Vec<_> = report
            .surface_confusion
            .iter()
            .filter(|((r, g), _)| r != g)
            .collect();
        confusion.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        println!("\nsurface mismatches (reference -> generated):");
        for ((reference, generated), n) in confusion.iter().take(15) {
            println!("  {reference} -> {generated}: {n}");
        }
        let mut rows: Vec<_> = report.histogram.iter().collect();
        rows.sort_by_key(|(_, (r, g))| std::cmp::Reverse((*r).max(*g)));
        println!("\nblock histogram (reference, generated):");
        for (name, (r, g)) in rows.iter().take(30) {
            println!("  {name}: {r} {g}");
        }
        println!();
    }
}

fn generator(kind: &str, seed: i64) -> Arc<dyn Generator> {
    match kind {
        "java" => Arc::new(JavaGenerator::new(seed)),
        #[cfg(feature = "bedrock-worldgen")]
        "bedrock" => Arc::new(unastar::world::generator::bedrock::BedrockGenerator::new(
            seed,
        )),
        #[cfg(not(feature = "bedrock-worldgen"))]
        "bedrock" => {
            panic!("--kind bedrock requires --features bedrock-worldgen and UNASTAR_BDS_DIR")
        }
        other => panic!("unknown --kind {other} (java, bedrock)"),
    }
}

fn report_proto(path: &std::path::Path) {
    let fixture = ProtoFixture::load(path).unwrap_or_else(|e| panic!("{e}"));
    let pipeline = ChunkPipeline::new(Arc::new(JavaGenerator::new(fixture.seed)));
    let report = measure_proto(&fixture, &pipeline);
    let m = report.metrics;
    println!(
        "## {} (pre-decoration, {} chunks)",
        path.display(),
        fixture.chunks.len()
    );
    println!(
        "Legacy Bedrock block-type projection only; properties and Java air variants are collapsed. Not canonical Java-state parity."
    );
    println!("| metric | value |\n|---|---:|");
    println!("| voxels | {} |", m.voxels);
    println!(
        "| projected block type exact | {:.2}% |",
        100.0 * m.block_exact
    );
    println!(
        "| class exact (air/water/lava/solid) | {:.2}% |",
        100.0 * m.class_exact
    );
    println!("| quart biome exact | {:.2}% |", 100.0 * m.biome_exact);
    let mut confusion: Vec<_> = report.confusion.iter().collect();
    confusion.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    println!("\nvoxel mismatches (reference -> generated):");
    for ((reference, generated), n) in confusion.iter().take(20) {
        println!("  {reference} -> {generated}: {n}");
    }
    println!();
}
