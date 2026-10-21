use anyhow::{Result, ensure};
use bytes::Bytes;
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use jolyne::{
    batch::{decode_batch_raw, encode_batch_raw},
    raw::RawPacket,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    hint::black_box,
    io::{Read, Write},
    path::Path,
};

#[derive(Deserialize)]
struct Fixture {
    name: String,
    file: String,
    set: String,
    sha256: String,
    packets: Vec<u32>,
}

struct Case {
    name: String,
    batch: Bytes,
    packets: Vec<RawPacket>,
}

fn load() -> Result<Vec<Case>> {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../valentine_gen/protocolgen/testdata/bench");
    let manifest: Vec<Fixture> =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
    let mut cases = Vec::new();
    for fixture in manifest.into_iter().filter(|entry| entry.set == "batch") {
        let batch = Bytes::from(std::fs::read(dir.join(&fixture.file))?);
        ensure!(
            format!("{:x}", Sha256::digest(&batch)) == fixture.sha256,
            "{}: corpus digest mismatch",
            fixture.name
        );
        ensure!(
            batch.first() == Some(&0xfe),
            "{}: invalid marker",
            fixture.name
        );
        let packets = decode_batch_raw(&mut batch.clone(), false, None)?;
        ensure!(
            packets.len() == fixture.packets.len(),
            "{}: packet count differs",
            fixture.name
        );
        for (packet, id) in packets.iter().zip(&fixture.packets) {
            ensure!(
                packet.header.id as u32 == *id,
                "{}: packet ID differs",
                fixture.name
            );
        }
        cases.push(Case {
            name: fixture.name,
            batch,
            packets,
        });
    }
    ensure!(cases.len() == 4, "expected four batch fixtures");
    Ok(cases)
}

fn compress(plain: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(plain)?;
    Ok(encoder.finish()?)
}

fn decompress(encoded: &[u8]) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();
    DeflateDecoder::new(encoded).read_to_end(&mut decoded)?;
    Ok(decoded)
}

fn validate(case: &Case) -> Result<()> {
    let encoded = encode_batch_raw(&case.packets, false, 6, 0, true)?;
    ensure!(
        encoded == case.batch,
        "{}: raw framing differs from corpus",
        case.name
    );
    let mut encoded = encode_batch_raw(&case.packets, true, 6, 0, true)?;
    ensure!(
        encoded.starts_with(&[0xfe, 0]),
        "expected negotiated deflate marker"
    );
    let decoded = decode_batch_raw(&mut encoded, true, None)?;
    ensure!(
        encode_batch_raw(&decoded, false, 6, 0, true)? == case.batch,
        "{}: compressed raw roundtrip differs",
        case.name
    );
    let plain = &case.batch[1..];
    ensure!(
        decompress(&compress(plain)?)? == plain,
        "raw DEFLATE roundtrip differs"
    );
    Ok(())
}

pub fn run(iterations: usize) -> Result<()> {
    for case in load()? {
        validate(&case)?;
        for (enabled, encode_op, decode_op) in [
            (false, "framing_encode", "framing_decode"),
            (true, "deflate_encode", "deflate_decode"),
        ] {
            let encoded = encode_batch_raw(&case.packets, enabled, 6, 0, true)?;
            crate::measure(
                "batch",
                &case.name,
                encode_op,
                "rust-owned",
                case.batch.len(),
                iterations,
                || {
                    black_box(
                        encode_batch_raw(black_box(&case.packets), enabled, 6, 0, true).unwrap(),
                    );
                },
            )?;
            crate::measure(
                "batch",
                &case.name,
                decode_op,
                "rust-owned",
                case.batch.len(),
                iterations,
                || {
                    black_box(decode_batch_raw(&mut encoded.clone(), enabled, None).unwrap());
                },
            )?;
        }
        let plain = &case.batch[1..];
        let compressed = compress(plain)?;
        crate::measure(
            "batch",
            &case.name,
            "compress",
            "flate2-rust",
            plain.len(),
            iterations,
            || {
                black_box(compress(black_box(plain)).unwrap());
            },
        )?;
        crate::measure(
            "batch",
            &case.name,
            "decompress",
            "flate2-rust",
            plain.len(),
            iterations,
            || {
                black_box(decompress(black_box(&compressed)).unwrap());
            },
        )?;
        for operation in ["encrypt", "decrypt"] {
            eprintln!(
                "{}",
                serde_json::json!({
                    "area": "coverage", "name": case.name, "operation": operation,
                    "implementation": "rust-owned", "status": "unavailable",
                    "reason": "isolated checksum/encryption helpers are private; public transport also frames and sends"
                })
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_four_corpus_batches_preserve_exact_framing() {
        let cases = load().unwrap();
        assert_eq!(cases.len(), 4);
        for case in cases {
            validate(&case).unwrap();
        }
    }

    #[test]
    fn raw_decode_rejects_truncated_frame() {
        let mut truncated = Bytes::from_static(&[0xfe, 5, 1]);
        assert!(decode_batch_raw(&mut truncated, false, None).is_err());
    }
}
