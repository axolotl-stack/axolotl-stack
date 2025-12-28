//! Benchmarks for NetherNetStream operations.
//!
//! These benchmarks focus on message reassembly, fragmentation,
//! and the hot paths in stream processing.

use bytes::{Bytes, BytesMut};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::time::{Duration, Instant};
use tokio_nethernet::stream::{Message, NetherNetStreamConfig};

/// Test helper that mirrors the reassembly logic from NetherNetStream
struct ReassemblyBenchHarness {
    reassembly_buffer: Option<ReassemblyBuffer>,
    max_reassembly_size: usize,
    reassembly_timeout: Duration,
}

struct ReassemblyBuffer {
    expected_segments: u8,
    received_segments: u8,
    data: BytesMut,
    total_size: usize,
    started_at: Instant,
}

impl ReassemblyBenchHarness {
    fn new(max_size: usize, timeout: Duration) -> Self {
        Self {
            reassembly_buffer: None,
            max_reassembly_size: max_size,
            reassembly_timeout: timeout,
        }
    }

    fn handle_fragment(&mut self, data: Bytes, now: Instant) -> Option<Message> {
        if data.len() < 2 {
            return None;
        }

        let segments = data[0];
        let payload = data.slice(1..);

        if let Some(ref mut buf) = self.reassembly_buffer {
            if now.duration_since(buf.started_at) > self.reassembly_timeout {
                self.reassembly_buffer = None;
                return None;
            }

            if buf.total_size + payload.len() > self.max_reassembly_size {
                self.reassembly_buffer = None;
                return None;
            }

            if buf.expected_segments != segments {
                if buf.expected_segments > 0 && buf.expected_segments - 1 != segments {
                    self.reassembly_buffer = None;
                    return None;
                }
            }

            buf.expected_segments = segments;
            buf.data.extend_from_slice(&payload);
            buf.total_size += payload.len();
            buf.received_segments += 1;

            if segments == 0 {
                let full_data = std::mem::take(&mut buf.data).freeze();
                self.reassembly_buffer = None;
                return Some(Message::reliable(full_data));
            }
        } else {
            if segments == 0 {
                return Some(Message::reliable(payload));
            }

            if payload.len() > self.max_reassembly_size {
                return None;
            }

            self.reassembly_buffer = Some(ReassemblyBuffer {
                expected_segments: segments,
                received_segments: 1,
                data: BytesMut::from(payload.as_ref()),
                total_size: payload.len(),
                started_at: now,
            });
        }

        None
    }
}

fn benchmark_single_packet_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_packet_processing");

    for size in [64, 256, 1024, 4096, 8192] {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("no_reassembly", size),
            &size,
            |b, &size| {
                let mut harness =
                    ReassemblyBenchHarness::new(10 * 1024 * 1024, Duration::from_secs(30));
                let now = Instant::now();

                // segments=0 means single packet (no reassembly needed)
                let mut data = vec![0u8; size + 1];
                data[0] = 0; // segments = 0
                let data = Bytes::from(data);

                b.iter(|| harness.handle_fragment(black_box(data.clone()), now))
            },
        );
    }

    group.finish();
}

fn benchmark_fragmented_reassembly(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmented_reassembly");
    group.sample_size(50);

    // Simulate different total message sizes
    let message_sizes = [10_000, 100_000, 1_000_000];
    let fragment_size = 1000;

    for &total_size in &message_sizes {
        let num_fragments = (total_size + fragment_size - 1) / fragment_size;

        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_with_input(
            BenchmarkId::new("reassemble", total_size),
            &(total_size, num_fragments),
            |b, &(_total_size, num_fragments)| {
                let now = Instant::now();

                // Pre-create fragments
                let fragments: Vec<Bytes> = (0..num_fragments)
                    .rev()
                    .map(|i| {
                        let mut data = vec![0u8; fragment_size + 1];
                        data[0] = i as u8; // segments remaining
                        Bytes::from(data)
                    })
                    .collect();

                b.iter(|| {
                    let mut harness =
                        ReassemblyBenchHarness::new(10 * 1024 * 1024, Duration::from_secs(30));

                    for (idx, fragment) in fragments.iter().enumerate() {
                        let result = harness.handle_fragment(black_box(fragment.clone()), now);
                        if idx == fragments.len() - 1 {
                            assert!(result.is_some());
                        }
                    }
                })
            },
        );
    }

    group.finish();
}

fn benchmark_message_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_creation");

    for size in [64, 256, 1024, 4096] {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("reliable", size), &size, |b, &size| {
            let data = Bytes::from(vec![0xAB; size]);
            b.iter(|| Message::reliable(black_box(data.clone())))
        });

        group.bench_with_input(BenchmarkId::new("unreliable", size), &size, |b, &size| {
            let data = Bytes::from(vec![0xAB; size]);
            b.iter(|| Message::unreliable(black_box(data.clone())))
        });
    }

    group.finish();
}

fn benchmark_config_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_creation");

    group.bench_function("default_config", |b| {
        b.iter(|| NetherNetStreamConfig::default())
    });

    group.finish();
}

fn benchmark_security_limits(c: &mut Criterion) {
    let mut group = c.benchmark_group("security_limits");

    // Benchmark size limit rejection (attack vector mitigation)
    group.bench_function("reject_oversized_fragment", |b| {
        let mut harness = ReassemblyBenchHarness::new(1024, Duration::from_secs(30));
        let now = Instant::now();

        // Create fragment that exceeds limit
        let mut data = vec![0u8; 2048];
        data[0] = 1; // segments = 1 (more to come)
        let data = Bytes::from(data);

        b.iter(|| harness.handle_fragment(black_box(data.clone()), now))
    });

    // Benchmark size limit during reassembly
    group.bench_function("reject_oversized_during_reassembly", |b| {
        let now = Instant::now();

        b.iter_batched(
            || {
                let mut harness = ReassemblyBenchHarness::new(1024, Duration::from_secs(30));
                // Add first fragment (512 bytes + header)
                let mut frag1 = vec![0u8; 513];
                frag1[0] = 1;
                harness.handle_fragment(Bytes::from(frag1), now);
                harness
            },
            |mut harness| {
                // Try to add second fragment that exceeds limit
                let mut frag2 = vec![0u8; 600];
                frag2[0] = 0;
                harness.handle_fragment(black_box(Bytes::from(frag2)), now)
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Benchmark invalid sequence rejection
    group.bench_function("reject_invalid_sequence", |b| {
        let now = Instant::now();

        b.iter_batched(
            || {
                let mut harness =
                    ReassemblyBenchHarness::new(10 * 1024 * 1024, Duration::from_secs(30));
                // Start with segments = 5
                let mut frag1 = vec![0u8; 100];
                frag1[0] = 5;
                harness.handle_fragment(Bytes::from(frag1), now);
                harness
            },
            |mut harness| {
                // Send invalid sequence (should be 4, sending 10)
                let mut frag2 = vec![0u8; 100];
                frag2[0] = 10;
                harness.handle_fragment(black_box(Bytes::from(frag2)), now)
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn benchmark_high_throughput_scenario(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_throughput");
    group.sample_size(20);

    // Simulate high packet rate (100 packets)
    group.bench_function("100_single_packets", |b| {
        let now = Instant::now();

        let packets: Vec<Bytes> = (0..100)
            .map(|_| {
                let mut data = vec![0u8; 257]; // 256 bytes payload + 1 byte header
                data[0] = 0; // single packet
                Bytes::from(data)
            })
            .collect();

        b.iter(|| {
            let mut harness =
                ReassemblyBenchHarness::new(10 * 1024 * 1024, Duration::from_secs(30));
            for pkt in packets.iter() {
                let _ = harness.handle_fragment(black_box(pkt.clone()), now);
            }
        })
    });

    // Simulate mixed single/fragmented traffic
    group.bench_function("mixed_traffic", |b| {
        let now = Instant::now();

        // Create a mix of single packets and fragmented packets
        let mut all_fragments = Vec::new();

        // 50 single packets
        for _ in 0..50 {
            let mut data = vec![0u8; 129];
            data[0] = 0;
            all_fragments.push(Bytes::from(data));
        }

        // 10 fragmented messages (5 fragments each)
        for _ in 0..10 {
            for seg in (0..5).rev() {
                let mut data = vec![0u8; 201];
                data[0] = seg;
                all_fragments.push(Bytes::from(data));
            }
        }

        b.iter(|| {
            let mut harness =
                ReassemblyBenchHarness::new(10 * 1024 * 1024, Duration::from_secs(30));
            for fragment in all_fragments.iter() {
                let _ = harness.handle_fragment(black_box(fragment.clone()), now);
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_single_packet_processing,
    benchmark_fragmented_reassembly,
    benchmark_message_creation,
    benchmark_config_creation,
    benchmark_security_limits,
    benchmark_high_throughput_scenario,
);
criterion_main!(benches);
