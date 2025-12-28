//! Hot path benchmarks for Session - the critical path for packet processing.
//!
//! These benchmarks focus on the most frequently called operations during
//! a typical Minecraft Bedrock session.

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::time::Instant;
use tokio_raknet::protocol::{
    ack::{AckNackPayload, SequenceRange},
    datagram::{Datagram, DatagramPayload},
    encapsulated_packet::EncapsulatedPacket,
    packet::RaknetPacket,
    reliability::Reliability,
    state::RakPriority,
    types::{DatagramHeader, EncapsulatedPacketHeader, Sequence24},
};
use tokio_raknet::session::{Session, SessionTunables};

fn make_encapsulated_packet(payload_size: usize, reliability: Reliability) -> EncapsulatedPacket {
    EncapsulatedPacket {
        header: EncapsulatedPacketHeader {
            reliability,
            is_split: false,
            needs_bas: false,
        },
        bit_length: (payload_size * 8) as u16,
        reliable_index: if reliability.is_reliable() {
            Some(Sequence24::new(0))
        } else {
            None
        },
        sequence_index: if reliability.is_sequenced() {
            Some(Sequence24::new(0))
        } else {
            None
        },
        ordering_index: if reliability.is_ordered() {
            Some(Sequence24::new(0))
        } else {
            None
        },
        ordering_channel: if reliability.is_ordered() {
            Some(0)
        } else {
            None
        },
        split: None,
        payload: Bytes::from(vec![0u8; payload_size]),
    }
}

fn make_user_data_packet(payload_size: usize) -> RaknetPacket {
    RaknetPacket::UserData {
        id: 0xFE, // Game packet ID
        payload: Bytes::from(vec![0xAB; payload_size]),
    }
}

#[allow(dead_code)]
fn make_datagram(sequence: u32, packets: Vec<EncapsulatedPacket>) -> Datagram {
    Datagram {
        header: DatagramHeader {
            flags: tokio_raknet::protocol::constants::DatagramFlags::VALID,
            sequence: Sequence24::new(sequence),
        },
        payload: DatagramPayload::EncapsulatedPackets(packets),
    }
}

fn benchmark_session_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("session_tick");

    // Benchmark tick with empty session
    group.bench_function("tick_empty", |b| {
        let mut session = Session::new(1400);
        let now = Instant::now();
        b.iter(|| {
            black_box(session.on_tick(now));
        })
    });

    // Benchmark tick with pending ACKs
    group.bench_function("tick_with_pending_acks", |b| {
        b.iter_batched(
            || {
                let mut session = Session::new(1400);
                for i in 0..100 {
                    session.process_datagram_sequence(Sequence24::new(i));
                }
                session
            },
            |mut session| {
                let now = Instant::now();
                black_box(session.on_tick(now));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Benchmark tick with outgoing packets queued
    group.bench_function("tick_with_outgoing_packets", |b| {
        b.iter_batched(
            || {
                let mut session = Session::new(1400);
                for _ in 0..50 {
                    let pkt = make_user_data_packet(100);
                    session.queue_packet(pkt, Reliability::ReliableOrdered, 0, RakPriority::Normal);
                }
                session
            },
            |mut session| {
                let now = Instant::now();
                black_box(session.on_tick(now));
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn benchmark_queue_packet(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_packet");

    for payload_size in [64, 256, 1024, 4096] {
        group.throughput(Throughput::Bytes(payload_size as u64));

        group.bench_with_input(
            BenchmarkId::new("unreliable", payload_size),
            &payload_size,
            |b, &size| {
                let mut session = Session::new(1400);
                b.iter(|| {
                    let pkt = make_user_data_packet(size);
                    session.queue_packet(
                        black_box(pkt),
                        Reliability::Unreliable,
                        0,
                        RakPriority::Normal,
                    );
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("reliable", payload_size),
            &payload_size,
            |b, &size| {
                let mut session = Session::new(1400);
                b.iter(|| {
                    let pkt = make_user_data_packet(size);
                    session.queue_packet(
                        black_box(pkt),
                        Reliability::Reliable,
                        0,
                        RakPriority::Normal,
                    );
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("reliable_ordered", payload_size),
            &payload_size,
            |b, &size| {
                let mut session = Session::new(1400);
                b.iter(|| {
                    let pkt = make_user_data_packet(size);
                    session.queue_packet(
                        black_box(pkt),
                        Reliability::ReliableOrdered,
                        0,
                        RakPriority::Normal,
                    );
                })
            },
        );
    }

    group.finish();
}

fn benchmark_handle_ack_nack(c: &mut Criterion) {
    let mut group = c.benchmark_group("handle_ack_nack");

    // Benchmark handling small ACK payload
    group.bench_function("handle_ack_small", |b| {
        let payload = AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(0),
                end: Sequence24::new(10),
            }],
        };
        let mut session = Session::new(1400);
        b.iter(|| {
            session.handle_ack_payload(black_box(payload.clone()));
        })
    });

    // Benchmark handling large ACK payload (stress test for queue limits)
    group.bench_function("handle_ack_large", |b| {
        let ranges: Vec<SequenceRange> = (0..100)
            .map(|i| SequenceRange {
                start: Sequence24::new(i * 2),
                end: Sequence24::new(i * 2),
            })
            .collect();
        let payload = AckNackPayload { ranges };
        let mut session = Session::new(1400);
        b.iter(|| {
            session.handle_ack_payload(black_box(payload.clone()));
        })
    });

    // Benchmark NACK handling
    group.bench_function("handle_nack", |b| {
        let ranges: Vec<SequenceRange> = (0..50)
            .map(|i| SequenceRange {
                start: Sequence24::new(i),
                end: Sequence24::new(i),
            })
            .collect();
        let payload = AckNackPayload { ranges };
        let mut session = Session::new(1400);
        b.iter(|| {
            session.handle_nack_payload(black_box(payload.clone()));
        })
    });

    // Benchmark ACK queue overflow (security limit test)
    group.bench_function("handle_ack_overflow", |b| {
        let tunables = SessionTunables {
            max_incoming_ack_queue: 100,
            ..Default::default()
        };
        let ranges: Vec<SequenceRange> = (0..200)
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

fn benchmark_handle_data_payload(c: &mut Criterion) {
    let mut group = c.benchmark_group("handle_data_payload");

    // Single packet processing
    group.bench_function("single_unreliable", |b| {
        let pkt = make_encapsulated_packet(100, Reliability::Unreliable);
        let now = Instant::now();
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                let _ = session.handle_data_payload(black_box(vec![pkt.clone()]), now);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("single_reliable", |b| {
        let pkt = make_encapsulated_packet(100, Reliability::Reliable);
        let now = Instant::now();
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                let _ = session.handle_data_payload(black_box(vec![pkt.clone()]), now);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("single_reliable_ordered", |b| {
        let pkt = make_encapsulated_packet(100, Reliability::ReliableOrdered);
        let now = Instant::now();
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                let _ = session.handle_data_payload(black_box(vec![pkt.clone()]), now);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Batch packet processing (typical game tick)
    group.bench_function("batch_10_reliable_ordered", |b| {
        let packets: Vec<_> = (0..10)
            .map(|i| {
                let mut pkt = make_encapsulated_packet(100, Reliability::ReliableOrdered);
                pkt.reliable_index = Some(Sequence24::new(i));
                pkt.ordering_index = Some(Sequence24::new(i));
                pkt
            })
            .collect();
        let now = Instant::now();
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                let _ = session.handle_data_payload(black_box(packets.clone()), now);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn benchmark_sequence_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequence_processing");

    // In-order sequence processing
    group.bench_function("in_order_100", |b| {
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                for i in 0..100 {
                    session.process_datagram_sequence(Sequence24::new(i));
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Out-of-order sequence processing (with gaps)
    group.bench_function("out_of_order_with_gaps", |b| {
        b.iter_batched(
            || Session::new(1400),
            |mut session| {
                // Simulate out-of-order arrival
                for i in [0, 5, 2, 8, 1, 9, 3, 7, 4, 6].iter() {
                    session.process_datagram_sequence(Sequence24::new(*i));
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Wrap-around sequence processing
    group.bench_function("wrap_around", |b| {
        b.iter_batched(
            || {
                let mut session = Session::new(1400);
                // Start near wrap point
                session.process_datagram_sequence(Sequence24::new(0xFFFF00));
                session
            },
            |mut session| {
                for i in 0..256 {
                    session.process_datagram_sequence(Sequence24::new(0xFFFF00 + i + 1));
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_session_tick,
    benchmark_queue_packet,
    benchmark_handle_ack_nack,
    benchmark_handle_data_payload,
    benchmark_sequence_processing,
);
criterion_main!(benches);
