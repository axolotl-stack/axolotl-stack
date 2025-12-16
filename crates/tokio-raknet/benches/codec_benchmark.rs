use bytes::{Bytes, BytesMut};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use tokio_raknet::protocol::{
    datagram::{Datagram, DatagramPayload},
    encapsulated_packet::EncapsulatedPacket,
    reliability::Reliability,
    types::{DatagramHeader, EncapsulatedPacketHeader, Sequence24},
};

fn benchmark_datagram_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("datagram_io");

    let enc_packet = EncapsulatedPacket {
        header: EncapsulatedPacketHeader {
            reliability: Reliability::ReliableOrdered,
            is_split: false,
            needs_bas: false,
        },
        bit_length: 80, // 10 bytes * 8
        reliable_index: Some(Sequence24::new(1)),
        sequence_index: None,
        ordering_index: Some(Sequence24::new(1)),
        ordering_channel: Some(0),
        split: None,
        payload: Bytes::from_static(b"0123456789"),
    };

    let datagram = Datagram {
        header: DatagramHeader {
            flags: tokio_raknet::protocol::constants::DatagramFlags::VALID,
            sequence: Sequence24::new(123),
        },
        payload: DatagramPayload::EncapsulatedPackets(vec![enc_packet.clone()]),
    };

    group.bench_function("encode_single_encap", |b| {
        b.iter(|| {
            let mut buf = BytesMut::with_capacity(1024);
            black_box(&datagram).encode(&mut buf).unwrap();
            buf
        })
    });

    // Benchmark decoding
    let mut buf = BytesMut::with_capacity(1024);
    datagram.encode(&mut buf).unwrap();
    let encoded_bytes = buf.freeze();

    group.bench_function("decode_single_encap", |b| {
        b.iter(|| {
            let mut src = encoded_bytes.clone();
            Datagram::decode(&mut src).unwrap()
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_datagram_encode);
criterion_main!(benches);
