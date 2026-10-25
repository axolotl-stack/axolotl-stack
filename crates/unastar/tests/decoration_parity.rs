//! Decoration parity of the Java-style generator against the Java 1.21.11
//! reference world (decision D8): block counts of feature- and structure-made
//! blocks over the fixture chunks.
//!
//! For every tracked block with at least `MIN_COUNT` blocks in the Java fixture
//! the score is `min(ours, java) / max(ours, java)`; the group score is the mean.
//! Floors only go up (raise them in the commit that improves them). Print the
//! per-block table with `cargo test -p unastar --features java-worldgen --test
//! decoration_parity -- --nocapture`.

#![cfg(feature = "java-worldgen")]

use std::path::Path;
use std::sync::Arc;

use unastar::world::generator::parity::{Fixture, measure};
use unastar::world::generator::{ChunkPipeline, JavaGenerator};

const MIN_COUNT: u64 = 20;

const ORES: &[&str] = &[
    "minecraft:coal_ore",
    "minecraft:deepslate_coal_ore",
    "minecraft:iron_ore",
    "minecraft:deepslate_iron_ore",
    "minecraft:copper_ore",
    "minecraft:deepslate_copper_ore",
    "minecraft:gold_ore",
    "minecraft:deepslate_gold_ore",
    "minecraft:redstone_ore",
    "minecraft:deepslate_redstone_ore",
    "minecraft:lapis_ore",
    "minecraft:deepslate_lapis_ore",
    "minecraft:diamond_ore",
    "minecraft:deepslate_diamond_ore",
    "minecraft:emerald_ore",
];

const BLOBS: &[&str] = &[
    "minecraft:granite",
    "minecraft:diorite",
    "minecraft:andesite",
    "minecraft:tuff",
    "minecraft:gravel",
    "minecraft:dirt",
    "minecraft:clay",
    "minecraft:sand",
];

const VEGETATION: &[&str] = &[
    "minecraft:short_grass",
    "minecraft:tall_grass",
    "minecraft:fern",
    "minecraft:oak_log",
    "minecraft:oak_leaves",
    "minecraft:birch_log",
    "minecraft:birch_leaves",
    "minecraft:spruce_log",
    "minecraft:spruce_leaves",
    "minecraft:jungle_log",
    "minecraft:jungle_leaves",
    "minecraft:dark_oak_log",
    "minecraft:dark_oak_leaves",
    "minecraft:acacia_log",
    "minecraft:acacia_leaves",
    "minecraft:dandelion",
    "minecraft:poppy",
    "minecraft:bush",
    "minecraft:kelp",
    "minecraft:kelp_plant",
    "minecraft:seagrass",
    "minecraft:tall_seagrass",
    "minecraft:sugar_cane",
    "minecraft:pumpkin",
    "minecraft:snow",
    "minecraft:ice",
];

struct Group {
    score: f64,
    tracked: usize,
}

fn group(
    name: &str,
    blocks: &[&str],
    hist: &std::collections::BTreeMap<String, (u64, u64)>,
) -> Group {
    let mut sum = 0.0;
    let mut tracked = 0;
    println!("{name}:");
    for b in blocks {
        let (java, ours) = hist.get(*b).copied().unwrap_or((0, 0));
        if java < MIN_COUNT {
            continue;
        }
        let s = java.min(ours) as f64 / java.max(ours) as f64;
        println!("  {b:<40} java {java:>7} ours {ours:>7}  {s:.3}");
        sum += s;
        tracked += 1;
    }
    Group {
        score: if tracked == 0 {
            1.0
        } else {
            sum / tracked as f64
        },
        tracked,
    }
}

#[test]
fn decoration_block_counts_vs_java_reference() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java_1_21_11_seed_12345.json");
    let hist = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let fixture = Fixture::load(&path).unwrap();
            let pipeline = ChunkPipeline::new(Arc::new(JavaGenerator::new(fixture.seed)));
            measure(&fixture, &pipeline).histogram
        })
        .unwrap()
        .join()
        .unwrap();
    let ores = group("ores", ORES, &hist);
    let blobs = group("blobs", BLOBS, &hist);
    let vegetation = group("vegetation", VEGETATION, &hist);
    println!(
        "ores {:.4} ({}), blobs {:.4} ({}), vegetation {:.4} ({})",
        ores.score, ores.tracked, blobs.score, blobs.tracked, vegetation.score, vegetation.tracked
    );
    let floors = [
        ("ores", ores.score, 0.9730),
        ("blobs", blobs.score, 0.9677),
        ("vegetation", vegetation.score, 0.8535),
    ];
    let failures: Vec<String> = floors
        .iter()
        .filter(|(_, v, f)| v < f)
        .map(|(n, v, f)| format!("{n} {v:.4} fell below its floor {f:.4}"))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("; "));
}
