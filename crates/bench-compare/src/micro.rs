use anyhow::{Result, ensure};
use bytes::{Bytes, BytesMut};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use tokio_raknet::{
    protocol::{
        constants::DatagramFlags,
        datagram::{Datagram, DatagramPayload},
        encapsulated_packet::{EncapsulatedPacket, SplitInfo},
        packet::RaknetEncodable,
        reliability::Reliability,
        types::{DatagramHeader, EncapsulatedPacketHeader, Sequence24},
    },
    session::split_assembler::SplitAssembler,
};

fn packet(payload: Bytes) -> EncapsulatedPacket {
    EncapsulatedPacket {
        header: EncapsulatedPacketHeader::with_reliability(Reliability::ReliableOrdered),
        bit_length: (payload.len() * 8) as u16,
        reliable_index: Some(Sequence24::new(1)),
        sequence_index: None,
        ordering_index: Some(Sequence24::new(1)),
        ordering_channel: Some(0),
        split: None,
        payload,
    }
}
fn datagram(packet: EncapsulatedPacket) -> Datagram {
    Datagram {
        header: DatagramHeader {
            flags: DatagramFlags::VALID,
            sequence: Sequence24::new(1),
        },
        payload: DatagramPayload::EncapsulatedPackets(vec![packet]),
    }
}
fn fragments() -> [EncapsulatedPacket; 2] {
    std::array::from_fn(|index| {
        let mut p = packet(Bytes::from(vec![index as u8; 512]));
        p.header.is_split = true;
        p.split = Some(SplitInfo {
            count: 2,
            id: 1,
            index: index as u32,
        });
        p
    })
}
fn reassemble(parts: &[EncapsulatedPacket; 2]) -> EncapsulatedPacket {
    let mut assembler = SplitAssembler::new(Duration::from_secs(30), 512, 16);
    let now = Instant::now();
    assert!(
        assembler
            .add(parts[0].clone(), now)
            .expect("first split")
            .is_none()
    );
    assembler
        .add(parts[1].clone(), now)
        .expect("second split")
        .expect("complete split")
}
pub fn validate() -> Result<()> {
    for size in [32, 512, 4096] {
        let packet = packet(super::transport::payload(size));
        let mut out = BytesMut::new();
        packet.encode_raknet(&mut out)?;
        let mut cursor = out.freeze();
        ensure!(
            EncapsulatedPacket::decode_raknet(&mut cursor)? == packet && cursor.is_empty(),
            "encapsulated roundtrip"
        );
        let datagram = datagram(packet);
        let mut out = BytesMut::new();
        datagram.encode(&mut out)?;
        let encoded = out.freeze();
        let mut cursor = encoded.clone();
        let decoded = Datagram::decode(&mut cursor)?;
        ensure!(cursor.is_empty(), "datagram trailing bytes");
        let mut again = BytesMut::new();
        decoded.encode(&mut again)?;
        ensure!(
            again.as_ref() == encoded.as_ref(),
            "datagram reserialization"
        );
    }
    let assembled = reassemble(&fragments());
    ensure!(
        assembled.payload.len() == 1024
            && assembled.payload[..512] == [0; 512]
            && assembled.payload[512..] == [1; 512],
        "split content"
    );
    Ok(())
}
pub fn run(iterations: usize) -> Result<()> {
    validate()?;
    for size in [32, 512, 4096] {
        let p = packet(super::transport::payload(size));
        let name = format!("encapsulated_{size}");
        let mut out = BytesMut::with_capacity(p.size());
        p.encode_raknet(&mut out)?;
        let encoded = out.clone().freeze();
        super::measure(
            "micro",
            &name,
            "encode",
            "rust-owned",
            size,
            iterations,
            || {
                out.clear();
                p.encode_raknet(&mut out).unwrap();
                black_box(out.as_ref());
            },
        )?;
        super::measure(
            "micro",
            &name,
            "decode",
            "rust-owned",
            size,
            iterations,
            || {
                black_box(
                    EncapsulatedPacket::decode_raknet(&mut black_box(encoded.clone())).unwrap(),
                );
            },
        )?;
        let d = datagram(p);
        let name = format!("datagram_{size}");
        let mut out = BytesMut::with_capacity(d.size());
        d.encode(&mut out)?;
        let encoded = out.clone().freeze();
        super::measure(
            "micro",
            &name,
            "encode",
            "rust-owned",
            size,
            iterations,
            || {
                out.clear();
                d.encode(&mut out).unwrap();
                black_box(out.as_ref());
            },
        )?;
        super::measure(
            "micro",
            &name,
            "decode",
            "rust-owned",
            size,
            iterations,
            || {
                black_box(Datagram::decode(&mut black_box(encoded.clone())).unwrap());
            },
        )?;
    }
    let parts = fragments();
    super::measure(
        "micro",
        "split_2x512",
        "reassemble",
        "rust-owned",
        1024,
        iterations,
        || {
            black_box(reassemble(black_box(&parts)));
        },
    )
}
