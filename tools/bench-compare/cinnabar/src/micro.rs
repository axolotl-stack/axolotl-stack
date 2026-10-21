use anyhow::{Result, ensure};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, BatchPhase, CompressionAlgorithm, CompressionSettings,
    DecodeOutcome,
    encryption::{BatchDecryptor, BatchEncryptor},
};
use bytes::{Bytes, BytesMut};
use raknet::{
    prelude::RakReliability,
    protocol::{
        codec::RakCodec,
        packets::{ack::Ack, frame_set::FrameSet},
        types::frame::Frame,
    },
};
use std::hint::black_box;

fn wire<T: RakCodec + PartialEq + std::fmt::Debug>(
    name: &str,
    value: &T,
    size: usize,
    iterations: usize,
) -> Result<()> {
    let mut out = Vec::with_capacity(value.size_hint());
    value.serialize(&mut out)?;
    let encoded = out.clone();
    let mut cursor = encoded.as_slice();
    ensure!(
        T::deserialize(&mut cursor)? == *value && cursor.is_empty(),
        "{name}: wire roundtrip differs"
    );
    super::measure(
        "micro",
        name,
        "encode",
        "cinnabar",
        size,
        iterations,
        || {
            out.clear();
            black_box(value).serialize(&mut out).unwrap();
            black_box(out.as_slice());
        },
    )?;
    super::measure(
        "micro",
        name,
        "decode",
        "cinnabar",
        size,
        iterations,
        || {
            black_box(T::deserialize(&mut black_box(encoded.as_slice())).unwrap());
        },
    )
}

fn decode<S: BatchPhase>(codec: &BatchCodec<S>, bytes: Bytes) -> Vec<Bytes> {
    match codec.decode(bytes).unwrap() {
        DecodeOutcome::Packets { packets, .. } => packets.collect(),
        outcome => panic!("validated batch dropped: {outcome:?}"),
    }
}
fn batch<S: BatchPhase>(
    codec: &BatchCodec<S>,
    case: &super::codec::Corpus,
    encode: &str,
    decode_label: &str,
    iterations: usize,
) -> Result<()> {
    let mut encoded = codec.encode(&case.packets)?;
    ensure!(encoded.len() == 1, "benchmark batch unexpectedly split");
    let input = encoded[0].clone();
    ensure!(
        decode(codec, input.clone()) == case.packets,
        "batch roundtrip differs"
    );
    if encode == "framing_encode" {
        ensure!(input == case.batch, "raw framing differs from corpus");
    }
    super::measure(
        "batch",
        &case.name,
        encode,
        "cinnabar",
        case.batch.len(),
        iterations,
        || {
            codec
                .encode_into(black_box(&case.packets), &mut encoded)
                .unwrap();
            black_box(&encoded);
        },
    )?;
    super::measure(
        "batch",
        &case.name,
        decode_label,
        "cinnabar",
        case.batch.len(),
        iterations,
        || {
            // Consume every packet view, but do not add an adapter-owned collection.
            match codec.decode(black_box(input.clone())).unwrap() {
                DecodeOutcome::Packets { packets, .. } => {
                    for packet in packets {
                        black_box(packet);
                    }
                }
                outcome => panic!("validated batch dropped: {outcome:?}"),
            }
        },
    )
}
fn cipher(case: &super::codec::Corpus, iterations: usize) -> Result<()> {
    let plain = case.batch.slice(1..);
    let owned = || {
        let mut bytes = BytesMut::with_capacity(plain.len() + 8);
        bytes.extend_from_slice(&plain);
        bytes.freeze()
    };
    let mut encryptor = BatchEncryptor::new([0x42; 32]);
    let mut verifier = BatchDecryptor::new([0x42; 32]);
    super::measure_prepared(
        &case.name,
        "encrypt",
        plain.len(),
        iterations,
        owned,
        |input| black_box(encryptor.encrypt(black_box(input)).unwrap()),
        |output| assert_eq!(verifier.decrypt(output).unwrap(), plain),
    )?;
    let mut producer = BatchEncryptor::new([0x42; 32]);
    let mut decryptor = BatchDecryptor::new([0x42; 32]);
    super::measure_prepared(
        &case.name,
        "decrypt",
        plain.len(),
        iterations,
        || producer.encrypt(owned()).unwrap(),
        |input| black_box(decryptor.decrypt(black_box(input)).unwrap()),
        |output| assert_eq!(output, plain),
    )
}

pub fn run(iterations: usize) -> Result<()> {
    for size in [32, 512, 4096] {
        let mut frame = Frame::new(
            RakReliability::ReliableOrdered,
            super::payload(size).to_vec().into_boxed_slice(),
        );
        frame.reliable_index = 1;
        frame.order_index = 1;
        wire(&format!("encapsulated_{size}"), &frame, size, iterations)?;
        wire(
            &format!("datagram_{size}"),
            &FrameSet::new(1, vec![frame], false, false, false),
            size,
            iterations,
        )?;
    }
    let ack = Ack::new((1..=8).chain([16]).chain(32..=40).collect(), false);
    let mut ack_bytes = Vec::new();
    ack.serialize(&mut ack_bytes)?;
    ensure!(
        ack_bytes
            == [
                0xc0, 0, 3, 0, 1, 0, 0, 8, 0, 0, 1, 16, 0, 0, 0, 32, 0, 0, 40, 0, 0,
            ],
        "ACK bytes differ from the canonical three-range datagram"
    );
    wire("ack_3_ranges", &ack, 18 * 3, iterations)?;
    super::coverage(
        "split_2x512",
        "cinnabar",
        "unavailable",
        "split reassembly lives in private session implementation; no isolated public assembler API, including test-support",
    );
    for case in super::codec::corpus()?
        .into_iter()
        .filter(|case| case.packets.len() > 1)
    {
        let codec = BatchCodec::new(BatchHeader::RakNet, BatchLimits::default());
        batch(
            &codec,
            &case,
            "framing_encode",
            "framing_decode",
            iterations,
        )?;
        for (algorithm, encode, decode) in [
            (
                CompressionAlgorithm::Deflate,
                "deflate_encode",
                "deflate_decode",
            ),
            (
                CompressionAlgorithm::Snappy,
                "snappy_encode",
                "snappy_decode",
            ),
        ] {
            let codec = BatchCodec::new(BatchHeader::RakNet, BatchLimits::default()).negotiate(
                CompressionSettings {
                    algorithm,
                    threshold: 0,
                },
            );
            batch(&codec, &case, encode, decode, iterations)?;
        }
        super::coverage(
            &case.name,
            "cinnabar",
            "unavailable",
            "isolated compress/decompress: root compression module is private; deflate/snappy rows include public BatchCodec framing",
        );
        cipher(&case, iterations)?;
    }
    Ok(())
}
