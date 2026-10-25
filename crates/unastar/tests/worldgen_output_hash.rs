//! Optimization gate: exact generated chunk bytes, not statistical vanilla parity.
//!
//! SHA-256 covers coordinates, all 24 network subchunks (framed by length),
//! disk biome/heightmap bytes and network biome bytes in fixture request order.
//! Goldens must be captured before an optimization; correctness changes require
//! separate evidence and an explicit golden update, never a performance exception.

#![cfg(any(feature = "java-worldgen", feature = "bedrock-worldgen"))]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use unastar::world::ChunkPos;
use unastar::world::generator::{ChunkPipeline, Generator};

#[derive(Deserialize)]
struct Fixture {
    seed: i64,
    chunks: Vec<Position>,
}
#[derive(Deserialize)]
struct Position {
    x: i32,
    z: i32,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ChunkHash {
    x: i32,
    z: i32,
    sha256: String,
}

fn verify(kind: &str, make_generator: fn(i64) -> Arc<dyn Generator>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixture: Fixture =
        serde_json::from_slice(&std::fs::read(root.join("bds_1_26_51_seed_12345.json")).unwrap())
            .unwrap();
    let actual = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let pipeline = ChunkPipeline::new(make_generator(fixture.seed));
            fixture
                .chunks
                .iter()
                .map(|p| {
                    let chunk = pipeline.generate(ChunkPos::new(p.x, p.z));
                    let mut digest = Sha256::new();
                    digest.update(p.x.to_le_bytes());
                    digest.update(p.z.to_le_bytes());
                    for y in -4..20 {
                        let bytes = chunk.encode_subchunk(y).unwrap();
                        digest.update((bytes.len() as u64).to_le_bytes());
                        digest.update(bytes);
                    }
                    for bytes in [chunk.encode_biomes_disk(), chunk.encode_biomes()] {
                        digest.update((bytes.len() as u64).to_le_bytes());
                        digest.update(bytes);
                    }
                    ChunkHash {
                        x: p.x,
                        z: p.z,
                        sha256: format!("{:x}", digest.finalize()),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap()
        .join()
        .unwrap();
    // Emitted on failure with --nocapture to review an explicitly authorized update.
    println!(
        "{kind} exact fixture hashes: {}",
        serde_json::to_string_pretty(&actual).unwrap()
    );
    let path = root.join(format!("worldgen_output_hash_{kind}.json"));
    let expected: Vec<ChunkHash> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(actual, expected, "{kind}: generated chunk bytes changed");
}

#[cfg(feature = "java-worldgen")]
#[test]
fn java_fixture_chunk_bytes_are_unchanged() {
    verify("java", |seed| {
        Arc::new(unastar::world::generator::JavaGenerator::new(seed))
    });
}

#[cfg(feature = "bedrock-worldgen")]
#[test]
fn bedrock_fixture_chunk_bytes_are_unchanged() {
    verify("bedrock", |seed| {
        Arc::new(unastar::world::generator::bedrock::BedrockGenerator::new(
            seed,
        ))
    });
}
