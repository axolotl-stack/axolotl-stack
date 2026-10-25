//! Structure start parity of the Java-style generator against the Java 1.21.11
//! reference world (decision D8). The fixture
//! (`examples/java_structure_reference.rs`) lists every chunk the reference
//! world saved at or past `structure_starts` and the starts originating in it.
//!
//! Metrics over those chunks: recall (Java starts we also start, same
//! structure and chunk), precision (our starts Java also has) and the share of
//! matched starts whose whole bounding box is identical. Floors only go up.

#![cfg(feature = "java-worldgen")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use unastar::world::generator::JavaGenerator;

#[derive(Deserialize)]
struct StartFixture {
    seed: i64,
    chunks: Vec<[i32; 2]>,
    starts: Vec<Start>,
}

#[derive(Deserialize)]
struct Start {
    structure: String,
    chunk: [i32; 2],
    bbox: [i32; 6],
}

#[test]
fn structure_starts_vs_java_reference() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/java_1_21_11_seed_12345_structures.json");
    let fixture: StartFixture =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (recall, precision, bbox_exact) = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let generator = JavaGenerator::new(fixture.seed);
            let java: BTreeMap<(String, i32, i32), [i32; 6]> = fixture
                .starts
                .iter()
                .map(|s| ((s.structure.clone(), s.chunk[0], s.chunk[1]), s.bbox))
                .collect();
            let mut ours: BTreeMap<(String, i32, i32), [i32; 6]> = BTreeMap::new();
            for [x, z] in &fixture.chunks {
                for s in generator.structure_starts(*x, *z).iter() {
                    // Java's saved box is the pieces' box (no terrain-adaptation inflation).
                    let mut bb = s.pieces[0].bbox();
                    for p in &s.pieces[1..] {
                        bb.encapsulate(&p.bbox());
                    }
                    ours.insert(
                        (s.def().name.to_string(), *x, *z),
                        [bb.min_x, bb.min_y, bb.min_z, bb.max_x, bb.max_y, bb.max_z],
                    );
                }
            }
            let jk: BTreeSet<_> = java.keys().collect();
            let ok: BTreeSet<_> = ours.keys().collect();
            let matched: Vec<_> = jk.intersection(&ok).collect();
            for k in jk.difference(&ok) {
                println!("missing  {k:?} {:?}", java[*k]);
            }
            for k in ok.difference(&jk) {
                println!("extra    {k:?} {:?}", ours[*k]);
            }
            let exact = matched
                .iter()
                .filter(|k| {
                    let same = java[***k] == ours[***k];
                    if !same {
                        println!("bbox     {k:?} java {:?} ours {:?}", java[***k], ours[***k]);
                    }
                    same
                })
                .count();
            let recall = matched.len() as f64 / jk.len().max(1) as f64;
            let precision = matched.len() as f64 / ok.len().max(1) as f64;
            let bbox_exact = exact as f64 / matched.len().max(1) as f64;
            println!(
                "java {} ours {} matched {} recall {recall:.4} precision {precision:.4} bbox exact {bbox_exact:.4}",
                jk.len(),
                ok.len(),
                matched.len()
            );
            (recall, precision, bbox_exact)
        })
        .unwrap()
        .join()
        .unwrap();
    let floors = [
        ("recall", recall, 1.0),
        ("precision", precision, 1.0),
        ("bbox_exact", bbox_exact, 0.8965),
    ];
    let failures: Vec<String> = floors
        .iter()
        .filter(|(_, v, f)| v < f)
        .map(|(n, v, f)| format!("{n} {v:.4} fell below its floor {f:.4}"))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("; "));
}
