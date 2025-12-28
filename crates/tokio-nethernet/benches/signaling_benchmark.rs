//! Benchmarks for signaling operations in tokio-nethernet.
//!
//! These benchmarks cover the hot paths in ICE candidate parsing,
//! signal parsing, and validation.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use tokio_nethernet::signaling::{
    Signal, SignalErrorCode, format_ice_candidate, parse_ice_candidate,
};

const VALID_HOST_CANDIDATE: &str = "candidate:1 1 udp 2130706431 192.168.1.100 54321 typ host generation 0 ufrag abc123 network-id 1 network-cost 0";

const VALID_RELAY_CANDIDATE: &str = "candidate:2 1 udp 1677721855 203.0.113.50 19302 typ relay raddr 192.168.1.100 rport 54321 generation 0 ufrag xyz network-id 1 network-cost 0";

const VALID_IPV6_CANDIDATE: &str =
    "candidate:1 1 udp 2130706431 2001:db8::1 54321 typ host generation 0 ufrag abc";

fn benchmark_parse_ice_candidate(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_ice_candidate");

    group.bench_function("host_candidate", |b| {
        b.iter(|| parse_ice_candidate(black_box(VALID_HOST_CANDIDATE)))
    });

    group.bench_function("relay_candidate", |b| {
        b.iter(|| parse_ice_candidate(black_box(VALID_RELAY_CANDIDATE)))
    });

    group.bench_function("ipv6_candidate", |b| {
        b.iter(|| parse_ice_candidate(black_box(VALID_IPV6_CANDIDATE)))
    });

    // Benchmark rejection of invalid candidates (security hot path)
    group.bench_function("invalid_ip_rejection", |b| {
        let invalid = "candidate:1 1 udp 2130706431 999.999.999.999 54321 typ host";
        b.iter(|| parse_ice_candidate(black_box(invalid)))
    });

    group.bench_function("invalid_protocol_rejection", |b| {
        let invalid = "candidate:1 1 sctp 2130706431 192.168.1.100 54321 typ host";
        b.iter(|| parse_ice_candidate(black_box(invalid)))
    });

    group.bench_function("too_long_rejection", |b| {
        let long = format!(
            "candidate:1 1 udp 2130706431 192.168.1.100 54321 typ host {}",
            "x".repeat(2000)
        );
        b.iter(|| parse_ice_candidate(black_box(&long)))
    });

    group.finish();
}

fn benchmark_format_ice_candidate(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_ice_candidate");

    group.bench_function("host_candidate", |b| {
        b.iter(|| {
            format_ice_candidate(
                black_box(1),
                black_box("abc123"),
                black_box("udp"),
                black_box(2130706431),
                black_box("192.168.1.100"),
                black_box(54321),
                black_box("host"),
                black_box(None),
                black_box(None),
                black_box("ufrag123"),
            )
        })
    });

    group.bench_function("relay_candidate", |b| {
        b.iter(|| {
            format_ice_candidate(
                black_box(1),
                black_box("abc123"),
                black_box("udp"),
                black_box(1677721855),
                black_box("203.0.113.50"),
                black_box(19302),
                black_box("relay"),
                black_box(Some("192.168.1.100")),
                black_box(Some(54321)),
                black_box("ufrag123"),
            )
        })
    });

    group.finish();
}

fn benchmark_signal_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("signal_parse");

    // Typical connect request with SDP
    let connect_request = format!(
        "CONNECTREQUEST 12345678901234 v=0\r\no=- {} 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0 1\r\na=ice-ufrag:{}\r\na=ice-pwd:{}\r\n",
        "1234567890", "abcd1234", "password123456789012"
    );

    group.bench_function("connect_request", |b| {
        b.iter(|| {
            Signal::parse(
                black_box(&connect_request),
                black_box("network123".to_string()),
            )
        })
    });

    // Short signal
    let short_signal = "CANDIDATEADD 123 candidate:1 1 udp 2130706431 192.168.1.1 5000 typ host";
    group.bench_function("candidate_add", |b| {
        b.iter(|| Signal::parse(black_box(short_signal), black_box("network123".to_string())))
    });

    // Error signal
    let error_signal = "CONNECTERROR 123 2";
    group.bench_function("error_signal", |b| {
        b.iter(|| Signal::parse(black_box(error_signal), black_box("network123".to_string())))
    });

    // Invalid signal (rejection path)
    let invalid_signal = "INVALID notanumber data";
    group.bench_function("invalid_rejection", |b| {
        b.iter(|| {
            Signal::parse(
                black_box(invalid_signal),
                black_box("network123".to_string()),
            )
        })
    });

    group.finish();
}

fn benchmark_signal_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("signal_creation");

    group.bench_function("error_signal", |b| {
        b.iter(|| {
            Signal::error(
                black_box(12345678901234),
                black_box("network123".to_string()),
                black_box(SignalErrorCode::NegotiationTimeout),
            )
        })
    });

    group.bench_function("signal_display", |b| {
        let signal = Signal {
            typ: "CONNECTREQUEST".to_string(),
            connection_id: 12345678901234,
            data: "v=0\r\no=- 1234567890 2 IN IP4 127.0.0.1\r\n".to_string(),
            network_id: "network123".to_string(),
        };
        b.iter(|| format!("{}", black_box(&signal)))
    });

    group.finish();
}

fn benchmark_batch_candidate_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_candidate_parsing");

    // Simulate parsing multiple candidates (typical ICE gathering)
    let candidates = vec![
        "candidate:1 1 udp 2130706431 192.168.1.100 54321 typ host generation 0 ufrag abc",
        "candidate:2 1 udp 2130706430 192.168.1.100 54322 typ host generation 0 ufrag abc",
        "candidate:3 1 udp 1694498815 203.0.113.50 19302 typ srflx raddr 192.168.1.100 rport 54321 generation 0 ufrag abc",
        "candidate:4 1 udp 1677721855 203.0.113.100 3478 typ relay raddr 203.0.113.50 rport 19302 generation 0 ufrag abc",
    ];

    group.throughput(Throughput::Elements(candidates.len() as u64));

    group.bench_function("parse_4_candidates", |b| {
        b.iter(|| {
            for candidate in candidates.iter() {
                let _ = parse_ice_candidate(black_box(candidate));
            }
        })
    });

    // Larger batch (stress test)
    let large_batch: Vec<String> = (0..100)
        .map(|i| {
            format!(
                "candidate:{} 1 udp {} 192.168.1.{} {} typ host generation 0 ufrag abc",
                i,
                2130706431 - i,
                i % 256,
                50000 + i
            )
        })
        .collect();

    group.throughput(Throughput::Elements(100));

    group.bench_function("parse_100_candidates", |b| {
        b.iter(|| {
            for candidate in large_batch.iter() {
                let _ = parse_ice_candidate(black_box(candidate));
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_parse_ice_candidate,
    benchmark_format_ice_candidate,
    benchmark_signal_parse,
    benchmark_signal_creation,
    benchmark_batch_candidate_parsing,
);
criterion_main!(benches);
