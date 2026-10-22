use anyhow::{Result, ensure};
use bytes::{Bytes, BytesMut};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use jolyne::proto::{
    batch::{BatchCodec, BatchCompression, BatchEncoding, decode_batch_raw, encode_batch_raw},
    crypto::BatchCipher,
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
    frames: Vec<Bytes>,
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
        let packets = decode_batch_raw(&mut batch.clone(), false, usize::MAX)?;
        ensure!(
            packets.len() == fixture.packets.len(),
            "{}: packet count differs",
            fixture.name
        );
        for (packet, id) in packets.iter().zip(&fixture.packets) {
            ensure!(
                u32::from(packet.id()) == *id,
                "{}: packet ID differs",
                fixture.name
            );
        }
        let frames = packets
            .iter()
            .map(|packet| {
                let frame = packet.inner_frame();
                let mut remaining = frame.as_ref();
                valentine::protocol::wire::read_var_u32(&mut remaining).unwrap();
                frame.slice(frame.len() - remaining.len()..)
            })
            .collect();
        cases.push(Case {
            name: fixture.name,
            batch,
            packets,
            frames,
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

/// Raw-packet framing with DEFLATE level 6, no threshold, `0xFE` prefix.
fn raw_encoding(enabled: bool) -> BatchEncoding {
    BatchEncoding {
        enabled,
        algorithm: BatchCompression::Deflate,
        level: 6,
        threshold: 0,
        prefix: true,
    }
}

fn validate(case: &Case) -> Result<()> {
    let encoded = encode_batch_raw(&case.packets, raw_encoding(false))?;
    ensure!(
        encoded == case.batch,
        "{}: raw framing differs from corpus",
        case.name
    );
    let mut encoded = encode_batch_raw(&case.packets, raw_encoding(true))?;
    ensure!(
        encoded.starts_with(&[0xfe, 0]),
        "expected negotiated deflate marker"
    );
    let decoded = decode_batch_raw(&mut encoded, true, usize::MAX)?;
    ensure!(
        encode_batch_raw(&decoded, raw_encoding(false))? == case.batch,
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
        let mut codec = BatchCodec::default();
        let mut output = BytesMut::new();
        for (enabled, algorithm, encode_op, decode_op) in [
            (
                false,
                BatchCompression::None,
                "framing_encode",
                "framing_decode",
            ),
            (
                true,
                BatchCompression::Deflate,
                "deflate_encode",
                "deflate_decode",
            ),
            (
                true,
                BatchCompression::Snappy,
                "snappy_encode",
                "snappy_decode",
            ),
        ] {
            let settings = BatchEncoding {
                enabled,
                algorithm,
                level: 6,
                threshold: 0,
                prefix: true,
            };
            codec.encode_frames_into(&case.frames, settings, &mut output)?;
            let encoded = output.clone().freeze();
            let mut decoded = Vec::new();
            codec.decode_with(encoded.clone(), enabled, 4 * 1024 * 1024, true, |frame| {
                decoded.push(Bytes::copy_from_slice(frame));
                Ok(())
            })?;
            ensure!(
                decoded == case.frames,
                "{}: frame roundtrip differs",
                case.name
            );
            eprintln!(
                "{}",
                serde_json::json!({
                    "area": "batch_sizes", "name": case.name, "operation": encode_op,
                    "implementation": "rust-owned", "plain_bytes": case.batch.len(),
                    "encoded_bytes": encoded.len()
                })
            );
            crate::measure(
                "batch",
                &case.name,
                encode_op,
                "rust-owned",
                case.batch.len(),
                iterations,
                || {
                    codec
                        .encode_frames_into(black_box(&case.frames), settings, &mut output)
                        .unwrap();
                    black_box(&output);
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
                    codec
                        .decode_with(
                            black_box(encoded.clone()),
                            enabled,
                            4 * 1024 * 1024,
                            true,
                            |frame| {
                                black_box(frame);
                                Ok(())
                            },
                        )
                        .unwrap();
                },
            )?;
        }
        let plain = &case.batch[1..];
        let owned = || {
            let mut input = BytesMut::with_capacity(plain.len() + 8);
            input.extend_from_slice(plain);
            input
        };
        let mut encryptor = BatchCipher::new([0x42; 32], [0x42; 12]);
        let mut verifier = BatchCipher::new([0x42; 32], [0x42; 12]);
        crate::measure_prepared(
            &case.name,
            "encrypt",
            plain.len(),
            iterations,
            owned,
            |mut input| {
                encryptor.encrypt(black_box(&mut input), 0).unwrap();
                input
            },
            |mut output| {
                verifier.decrypt(&mut output, 0).unwrap();
                assert_eq!(output.as_ref(), plain);
            },
        )?;
        let mut producer = BatchCipher::new([0x42; 32], [0x42; 12]);
        let mut decryptor = BatchCipher::new([0x42; 32], [0x42; 12]);
        crate::measure_prepared(
            &case.name,
            "decrypt",
            plain.len(),
            iterations,
            || {
                let mut input = owned();
                producer.encrypt(&mut input, 0).unwrap();
                input
            },
            |mut input| {
                decryptor.decrypt(black_box(&mut input), 0).unwrap();
                input
            },
            |output| assert_eq!(output.as_ref(), plain),
        )?;
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
        assert!(decode_batch_raw(&mut truncated, false, usize::MAX).is_err());
    }
}
