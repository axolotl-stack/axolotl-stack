use anyhow::{Context, Result, ensure};
use bytes::{Buf, Bytes, BytesMut};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{hint::black_box, path::Path};
use valentine::bedrock::borrowed::BorrowedReader;
use valentine::{
    bedrock::version::v1_26_51::{
        BorrowedMcpePacket, BorrowedMcpePacketData, GameHeader, McpePacketArgs, McpePacketData,
    },
    protocol::wire,
};

#[derive(Deserialize)]
struct Entry {
    name: String,
    file: String,
    sha256: String,
    packets: Vec<u32>,
}

pub struct Case {
    pub name: String,
    pub frames: Vec<Bytes>,
    pub packets: Vec<(GameHeader, McpePacketData)>,
    pub batch_entries: Bytes,
    pub payload_bytes: usize,
    pub borrowed_supported: bool,
}

pub fn load() -> Result<Vec<Case>> {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../valentine_gen/protocolgen/testdata/bench");
    let manifest: Vec<Entry> = serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
    let mut cases = Vec::new();
    for entry in manifest {
        let batch = Bytes::from(std::fs::read(dir.join(&entry.file))?);
        ensure!(
            format!("{:x}", Sha256::digest(&batch)) == entry.sha256,
            "{}: corpus digest mismatch",
            entry.name
        );
        ensure!(
            batch.first() == Some(&0xfe),
            "{}: missing batch marker",
            entry.name
        );
        let mut rest = batch.slice(1..);
        let mut frames = Vec::new();
        let mut payload_bytes = 0;
        while rest.has_remaining() {
            let start = rest.clone();
            let length = wire::read_var_u32(&mut rest)? as usize;
            let prefix = start.len() - rest.len();
            ensure!(length <= rest.len(), "{}: truncated frame", entry.name);
            frames.push(start.slice(..prefix + length));
            rest.advance(length);
            payload_bytes += length;
        }
        ensure!(
            frames.len() == entry.packets.len() && !frames.is_empty(),
            "{}: manifest packet count differs",
            entry.name
        );
        let validated = (|| -> Result<Case> {
            let mut packets = Vec::new();
            let mut borrowed_supported = true;
            let mut borrowed_error = None;
            for frame in &frames {
                let mut cursor = BorrowedReader::new(frame, frame.len());
                let (header, packet) =
                    McpePacketData::decode_inner_reader(&mut cursor, McpePacketArgs)
                        .context("typed owned decode")?;
                ensure!(!cursor.has_remaining(), "trailing owned frame");
                let mut encoded = BytesMut::new();
                packet.encode_inner_bytes_mut(
                    &mut encoded,
                    header.from_subclient,
                    header.to_subclient,
                )?;
                let mut again = encoded.clone().freeze();
                let decoded = McpePacketData::decode_inner_bytes(&mut again, McpePacketArgs)?;
                ensure!(
                    !again.has_remaining() && decoded == (header, packet.clone()),
                    "typed encode roundtrip differs"
                );
                let mut canonical = BytesMut::new();
                decoded.1.encode_inner_bytes_mut(
                    &mut canonical,
                    decoded.0.from_subclient,
                    decoded.0.to_subclient,
                )?;
                ensure!(encoded == canonical, "unstable reserialization");
                ensure!(
                    encoded.as_ref() == frame.as_ref(),
                    "typed encoding differs from committed corpus"
                );
                let borrowed_result = (|| -> Result<()> {
                    let mut cursor = BorrowedReader::new(frame, frame.len());
                    let (borrowed, remaining) =
                        BorrowedMcpePacket::decode_inner_reader(&mut cursor)?;
                    ensure!(
                        !matches!(&borrowed.data, BorrowedMcpePacketData::Raw { .. }),
                        "public decoder returns Raw"
                    );
                    ensure!(
                        remaining == 0 && !cursor.has_remaining(),
                        "trailing borrowed payload"
                    );
                    let owned = borrowed.into_owned(McpePacketArgs)?;
                    ensure!(
                        owned.data == packet && owned.header == header,
                        "borrowed typed value differs"
                    );
                    Ok(())
                })();
                if let Err(error) = borrowed_result {
                    borrowed_supported = false;
                    borrowed_error = Some(format!("{error:#}"));
                }
                packets.push((header, packet));
            }
            eprintln!(
                "{}",
                serde_json::json!({
                    "area": "coverage", "name": entry.name, "implementation": "rust-borrowed",
                    "status": if borrowed_supported { "supported_decode" } else { "unavailable" },
                    "reason": borrowed_error, "encode": "unavailable: no public borrowed encoder"
                })
            );
            if frames.len() > 1 {
                payload_bytes = batch.len();
            }
            let batch_entries = if frames.len() > 1 {
                batch
            } else {
                batch.slice(1..)
            };
            Ok(Case {
                name: entry.name.clone(),
                frames,
                packets,
                payload_bytes,
                borrowed_supported,
                batch_entries,
            })
        })();
        match validated {
            Ok(case) => {
                eprintln!(
                    "{}",
                    serde_json::json!({"area": "coverage", "name": entry.name, "implementation": "rust-owned", "status": "supported_decode_encode"})
                );
                cases.push(case);
            }
            Err(error) => {
                for implementation in ["rust-owned", "rust-borrowed"] {
                    eprintln!(
                        "{}",
                        serde_json::json!({"area": "coverage", "name": entry.name, "implementation": implementation, "status": "unavailable", "reason": format!("{error:#}")})
                    );
                }
            }
        }
    }
    ensure!(
        !cases.is_empty(),
        "no corpus cases support validated typed roundtrip"
    );
    Ok(cases)
}

impl Case {
    pub fn decode_owned(&self) {
        let mut cursor =
            BorrowedReader::new(black_box(&self.batch_entries), self.batch_entries.len());
        if self.frames.len() > 1 {
            assert_eq!(cursor.get_u8(), 0xfe);
        }
        while cursor.has_remaining() {
            black_box(
                McpePacketData::decode_inner_reader(&mut cursor, McpePacketArgs)
                    .expect("validated owned decode"),
            );
        }
    }
    pub fn decode_borrowed(&self) {
        let mut cursor =
            BorrowedReader::new(black_box(&self.batch_entries), self.batch_entries.len());
        if self.frames.len() > 1 {
            assert_eq!(cursor.get_u8(), 0xfe);
        }
        while cursor.has_remaining() {
            let (packet, remaining) = BorrowedMcpePacket::decode_inner_reader(&mut cursor)
                .expect("validated borrowed decode");
            assert_eq!(remaining, 0, "validated borrowed trailing bytes");
            black_box(packet);
        }
    }
    pub fn encode(&self, out: &mut BytesMut) {
        out.clear();
        if self.frames.len() > 1 {
            out.extend_from_slice(&[0xfe]);
        }
        for (header, packet) in &self.packets {
            black_box(packet)
                .encode_inner_bytes_mut(out, header.from_subclient, header.to_subclient)
                .expect("validated encode");
        }
        black_box(out.as_ref());
    }
    pub fn decode_operation(&self) -> &'static str {
        if self.frames.len() > 1 {
            "decode_batch"
        } else {
            "decode"
        }
    }
    pub fn encode_operation(&self) -> &'static str {
        if self.frames.len() > 1 {
            "encode_batch"
        } else {
            "encode"
        }
    }
}

pub fn run(iterations: usize) -> Result<()> {
    for case in load()? {
        super::measure(
            "codec",
            &case.name,
            case.decode_operation(),
            "rust-owned",
            case.payload_bytes,
            iterations,
            || case.decode_owned(),
        )?;
        if case.borrowed_supported {
            super::measure(
                "codec",
                &case.name,
                case.decode_operation(),
                "rust-borrowed",
                case.payload_bytes,
                iterations,
                || case.decode_borrowed(),
            )?;
        }
        let mut out = BytesMut::with_capacity(case.batch_entries.len());
        super::measure(
            "codec",
            &case.name,
            case.encode_operation(),
            "rust-owned",
            case.payload_bytes,
            iterations,
            || case.encode(&mut out),
        )?;
    }
    Ok(())
}
