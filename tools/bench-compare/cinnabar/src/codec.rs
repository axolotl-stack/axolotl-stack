use anyhow::{Result, ensure};
use bytes::Bytes;
use contract::{
    packets::{Decoded, Packet},
    wire::{Frames, Limits, PacketHeader, Peer},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{hint::black_box, path::Path};

#[derive(Deserialize)]
struct Entry {
    name: String,
    file: String,
    sha256: String,
    sender: String,
    packets: Vec<u32>,
}
pub struct Corpus {
    pub name: String,
    pub batch: Bytes,
    pub packets: Vec<Bytes>,
    sender: Peer,
}
pub fn corpus() -> Result<Vec<Corpus>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates/valentine_gen/protocolgen/testdata/bench");
    let entries: Vec<Entry> = serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
    let mut cases = Vec::new();
    for entry in entries {
        let batch = Bytes::from(std::fs::read(dir.join(entry.file))?);
        ensure!(
            format!("{:x}", Sha256::digest(&batch)) == entry.sha256,
            "{}: corpus digest differs",
            entry.name
        );
        let frames =
            Frames::from_batch(batch.clone(), usize::MAX)?.collect::<Result<Vec<_>, _>>()?;
        ensure!(
            frames.len() == entry.packets.len() && !frames.is_empty(),
            "corpus packet count differs"
        );
        let mut packets = Vec::new();
        for (frame, id) in frames.iter().zip(entry.packets) {
            ensure!(frame.id() == id, "corpus packet ID differs");
            let bytes = frame.frame();
            let prefix = bytes
                .iter()
                .position(|b| b & 0x80 == 0)
                .ok_or_else(|| anyhow::anyhow!("invalid length"))?
                + 1;
            packets.push(bytes.slice(prefix..));
        }
        let sender = match entry.sender.as_str() {
            "server" => Peer::Server,
            "client" => Peer::Client,
            other => anyhow::bail!("unknown sender {other}"),
        };
        cases.push(Corpus {
            name: entry.name,
            batch,
            packets,
            sender,
        });
    }
    Ok(cases)
}

pub fn run(iterations: usize) -> Result<()> {
    let limits = Limits::default();
    let mut supported = 0;
    for case in corpus()? {
        let batch = case.packets.len() > 1;
        let input = if batch {
            case.batch.clone()
        } else {
            case.batch.slice(1..)
        };
        let validated = (|| -> Result<Vec<(PacketHeader, Packet)>> {
            let mut packets = Vec::new();
            for frame in Frames::from_batch(case.batch.clone(), usize::MAX)? {
                let frame = frame?;
                let Decoded::Known(packet) =
                    Packet::decode_framed(frame.frame(), case.sender, &limits)?
                else {
                    anyhow::bail!("unknown packet is not typed codec coverage")
                };
                let header = frame.header();
                let mut encoded = Vec::new();
                packet.encode_frame(
                    case.sender,
                    header.sender_subclient,
                    header.target_subclient,
                    &mut encoded,
                )?;
                ensure!(
                    encoded.as_slice() == frame.frame().as_ref(),
                    "typed reserialization differs from corpus"
                );
                ensure!(
                    Packet::decode_framed(&Bytes::from(encoded), case.sender, &limits)?
                        == Decoded::Known(packet.clone()),
                    "typed roundtrip differs"
                );
                packets.push((header, packet));
            }
            Ok(packets)
        })();
        let packets = match validated {
            Ok(packets) => packets,
            Err(error) => {
                super::coverage(&case.name, "contract", "unavailable", &format!("{error:#}"));
                continue;
            }
        };
        supported += 1;
        super::coverage(
            &case.name,
            "contract",
            "supported_decode_encode",
            "protocolgen rust-contract; not bedrock-net packet codec",
        );
        let size = if batch {
            case.batch.len()
        } else {
            case.packets[0].len()
        };
        super::measure(
            "codec",
            &case.name,
            if batch { "decode_batch" } else { "decode" },
            "contract",
            size,
            iterations,
            || {
                if batch {
                    for frame in Frames::from_batch(black_box(input.clone()), usize::MAX).unwrap() {
                        black_box(
                            Packet::decode_frame(&frame.unwrap(), case.sender, &limits).unwrap(),
                        );
                    }
                } else {
                    black_box(
                        Packet::decode_framed(black_box(&input), case.sender, &limits).unwrap(),
                    );
                }
            },
        )?;
        let mut out = Vec::with_capacity(input.len());
        super::measure(
            "codec",
            &case.name,
            if batch { "encode_batch" } else { "encode" },
            "contract",
            size,
            iterations,
            || {
                out.clear();
                if batch {
                    out.push(0xfe);
                }
                for (header, packet) in &packets {
                    black_box(packet)
                        .encode_frame(
                            case.sender,
                            header.sender_subclient,
                            header.target_subclient,
                            &mut out,
                        )
                        .unwrap();
                }
                black_box(out.as_slice());
            },
        )?;
    }
    ensure!(
        supported > 0,
        "no corpus cases support validated contract roundtrip"
    );
    Ok(())
}
