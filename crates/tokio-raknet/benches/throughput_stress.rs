//! Throughput and stress benchmarks for RakNet.
//!
//! These benchmarks simulate realistic Minecraft Bedrock workloads and
//! measure throughput under various conditions.

use bytes::{Bytes, BytesMut};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::time::Instant;
use tokio_raknet::protocol::{
    datagram::{Datagram, DatagramPayload},
    encapsulated_packet::{EncapsulatedPacket, SplitInfo},
    packet::RaknetPacket,
    reliability::Reliability,
    state::RakPriority,
    types::{DatagramHeader, EncapsulatedPacketHeader, Sequence24},
};
use tokio_raknet::session::{Session, SessionTunables};

/// Simulates typical Minecraft packet sizes
const MC_PACKET_SIZES: &[usize] = &[
    32,   // Small packets (pings, acks)
    128,  // Player movement
    512,  // Chunk data (small)
    1024, // Entity updates
    4096, // Large chunk data
];

fn make_packet(size: usize, reliability: Reliability, seq: u32) -> EncapsulatedPacket {
    EncapsulatedPacket {
        header: EncapsulatedPacketHeader {
            reliability,
            is_split: false,
            needs_bas: false,
        },
        bit_length: (size * 8) as u16,
        reliable_index: if reliability.is_reliable() {
            Some(Sequence24::new(seq))
        } else {
            None
        },
        sequence_index: if reliability.is_sequenced() {
            Some(Sequence24::new(seq))
        } else {
            None
        },
        ordering_index: if reliability.is_ordered() {
            Some(Sequence24::new(seq))
        } else {
            None
        },
        ordering_channel: if reliability.is_ordered() {
            Some(0)
        } else {
            None
        },
        split: None,
        payload: Bytes::from(vec![0xAB; size]),
    }
}

fn make_user_data_packet(size: usize) -> RaknetPacket {
    RaknetPacket::UserData {
        id: 0xFE,
        payload: Bytes::from(vec![0xAB; size]),
    }
}

fn make_split_packet(
    id: u16,
    index: u32,
    count: u32,
    size: usize,
    reliability: Reliability,
) -> EncapsulatedPacket {
    EncapsulatedPacket {
        header: EncapsulatedPacketHeader {
            reliability,
            is_split: true,
            needs_bas: false,
        },
        bit_length: (size * 8) as u16,
        reliable_index: if reliability.is_reliable() {
            Some(Sequence24::new(index))
        } else {
            None
        },
        sequence_index: None,
        ordering_index: None,
        ordering_channel: None,
        split: Some(SplitInfo { id, index, count }),
        payload: Bytes::from(vec![0xCD; size]),
    }
}

fn benchmark_throughput_encode_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_encode_decode");
    group.sample_size(100);

    for &size in MC_PACKET_SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        // Encode throughput
        group.bench_with_input(BenchmarkId::new("encode", size), &size, |b, &size| {
            let pkt = make_packet(size, Reliability::ReliableOrdered, 0);
            let datagram = Datagram {
                header: DatagramHeader {
                    flags: tokio_raknet::protocol::constants::DatagramFlags::VALID,
                    sequence: Sequence24::new(0),
                },
                payload: DatagramPayload::EncapsulatedPackets(vec![pkt]),
            };

            b.iter(|| {
                let mut buf = BytesMut::with_capacity(size + 64);
                black_box(&datagram).encode(&mut buf).unwrap();
                buf
            })
        });

        // Decode throughput
        group.bench_with_input(BenchmarkId::new("decode", size), &size, |b, &size| {
            let pkt = make_packet(size, Reliability::ReliableOrdered, 0);
            let datagram = Datagram {
                header: DatagramHeader {
                    flags: tokio_raknet::protocol::constants::DatagramFlags::VALID,
                    sequence: Sequence24::new(0),
                },
                payload: DatagramPayload::EncapsulatedPackets(vec![pkt]),
            };
            let mut buf = BytesMut::with_capacity(size + 64);
            datagram.encode(&mut buf).unwrap();
            let encoded = buf.freeze();

            b.iter(|| {
                let mut src = encoded.clone();
                Datagram::decode(&mut src).unwrap()
            })
        });
    }

    group.finish();
}

fn benchmark_high_packet_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_packet_rate");
    group.sample_size(50);

    // Simulate 100 packets per tick (typical busy server)
    group.bench_function("100_packets_per_tick", |b| {
        let packets: Vec<_> = (0..100)
            .map(|i| make_packet(128, Reliability::ReliableOrdered, i))
            .collect();
        let now = Instant::now();

        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                // Receive 100 packets
                let _ = session.handle_data_payload(black_box(packets.clone()), now);
                // Process tick
                black_box(session.on_tick(now));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Simulate sending 100 packets per tick using queue_packet
    group.bench_function("queue_100_packets", |b| {
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                for _ in 0..100 {
                    let pkt = make_user_data_packet(128);
                    session.queue_packet(pkt, Reliability::ReliableOrdered, 0, RakPriority::Normal);
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Simulate mixed traffic (typical game scenario)
    group.bench_function("mixed_traffic_tick", |b| {
        let now = Instant::now();

        b.iter_batched(
            || {
                let mut session = Session::new(1400);
                // Queue outgoing packets
                for _ in 0..50 {
                    let pkt = make_user_data_packet(128);
                    session.queue_packet(pkt, Reliability::ReliableOrdered, 0, RakPriority::Normal);
                }
                // Process some incoming sequences
                for i in 0..50 {
                    session.process_datagram_sequence(Sequence24::new(i));
                }
                session
            },
            |mut session| {
                // Tick should process both incoming ACKs and outgoing packets
                black_box(session.on_tick(now));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn benchmark_split_reassembly_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("split_reassembly_throughput");
    group.sample_size(50);

    // Simulate reassembling a 64KB chunk (typical Minecraft chunk data)
    let chunk_sizes = [16 * 1024, 64 * 1024, 256 * 1024];

    for chunk_size in chunk_sizes {
        let part_size = 1000; // Typical MTU-based split size
        let num_parts = (chunk_size + part_size - 1) / part_size;

        group.throughput(Throughput::Bytes(chunk_size as u64));

        group.bench_with_input(
            BenchmarkId::new("reassemble", chunk_size),
            &(chunk_size, part_size, num_parts),
            |b, &(_chunk_size, part_size, num_parts)| {
                let now = Instant::now();

                b.iter_batched(
                    || Session::new(1400),
                    |mut session| {
                        let packets: Vec<_> = (0..num_parts)
                            .map(|i| {
                                make_split_packet(
                                    0,
                                    i as u32,
                                    num_parts as u32,
                                    part_size,
                                    Reliability::Reliable,
                                )
                            })
                            .collect();
                        let _ = session.handle_data_payload(black_box(packets), now);
                    },
                    criterion::BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn benchmark_ordering_channel_stress(c: &mut Criterion) {
    let mut group = c.benchmark_group("ordering_channel_stress");
    group.sample_size(50);

    // Multiple ordering channels (typical Minecraft uses several)
    group.bench_function("8_channels_100_packets", |b| {
        let now = Instant::now();

        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                let packets: Vec<_> = (0..100)
                    .map(|i| {
                        let channel = (i % 8) as u8;
                        let mut pkt = make_packet(64, Reliability::ReliableOrdered, i);
                        pkt.ordering_channel = Some(channel);
                        pkt.ordering_index = Some(Sequence24::new(i / 8));
                        pkt
                    })
                    .collect();
                let _ = session.handle_data_payload(black_box(packets), now);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Out-of-order delivery simulation
    group.bench_function("out_of_order_delivery", |b| {
        let now = Instant::now();

        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                // Simulate packets arriving out of order
                let order = [5, 2, 8, 0, 9, 1, 7, 3, 6, 4];
                for &i in order.iter() {
                    let mut pkt = make_packet(64, Reliability::ReliableOrdered, i);
                    pkt.ordering_index = Some(Sequence24::new(i));
                    let _ = session.handle_data_payload(black_box(vec![pkt]), now);
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn benchmark_reliable_tracker_stress(c: &mut Criterion) {
    let mut group = c.benchmark_group("reliable_tracker_stress");

    // Simulate high reliable packet rate
    group.bench_function("1000_reliable_packets", |b| {
        let now = Instant::now();

        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                for i in 0..1000 {
                    let pkt = make_packet(64, Reliability::Reliable, i);
                    let _ = session.handle_data_payload(black_box(vec![pkt]), now);
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Simulate duplicate detection (attackers may send duplicates)
    group.bench_function("duplicate_detection", |b| {
        let now = Instant::now();

        b.iter_batched(
            || {
                let mut session = Session::new(1400);
                // Pre-populate with some packets
                for i in 0..100 {
                    let pkt = make_packet(64, Reliability::Reliable, i);
                    let _ = session.handle_data_payload(vec![pkt], now);
                }
                session
            },
            |mut session| {
                // Try to process duplicates (should be rejected)
                for i in 0..100 {
                    let pkt = make_packet(64, Reliability::Reliable, i);
                    let _ = session.handle_data_payload(black_box(vec![pkt]), now);
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn benchmark_memory_pressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_pressure");
    group.sample_size(20);

    // Simulate many concurrent split packets (memory pressure test)
    group.bench_function("concurrent_splits", |b| {
        let now = Instant::now();
        let tunables = SessionTunables {
            max_concurrent_splits: 100,
            ..Default::default()
        };

        b.iter_batched(
            || Session::with_tunables(1400, tunables.clone()),
            |mut session| {
                // Start 50 different split packets (each incomplete)
                for split_id in 0..50 {
                    let pkt = make_split_packet(split_id, 0, 10, 100, Reliability::Reliable);
                    let _ = session.handle_data_payload(black_box(vec![pkt]), now);
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Simulate large ACK/NACK queue (potential DoS vector)
    group.bench_function("large_ack_queue_processing", |b| {
        use tokio_raknet::protocol::ack::{AckNackPayload, SequenceRange};

        let tunables = SessionTunables {
            max_incoming_ack_queue: 1000,
            ..Default::default()
        };
        let ranges: Vec<_> = (0..1000)
            .map(|i| SequenceRange {
                start: Sequence24::new(i),
                end: Sequence24::new(i),
            })
            .collect();
        let payload = AckNackPayload { ranges };

        b.iter_batched(
            || Session::with_tunables(1400, tunables.clone()),
            |mut session| {
                session.handle_ack_payload(black_box(payload.clone()));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_throughput_encode_decode,
    benchmark_high_packet_rate,
    benchmark_split_reassembly_throughput,
    benchmark_ordering_channel_stress,
    benchmark_reliable_tracker_stress,
    benchmark_memory_pressure,
);
criterion_main!(benches);
