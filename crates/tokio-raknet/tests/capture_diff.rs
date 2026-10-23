//! Capture diff: tokio-raknet `Vanilla` against Mojang RakNet wire captures.
//!
//! `tests/captures/bds-1.26.51` holds loopback captures of Bedrock Dedicated
//! Server 1.26.51 (Mojang RakNet, server side) recorded by
//! `tools/client-diff/raknet/capture.ps1` through the recording relay of
//! `examples/raknet_capture`. Each test replays the scenario against a
//! `RaknetListener` with `TransmissionPolicy::Vanilla` through the same relay
//! and fault model, reduces both captures to behavioural features and
//! compares the server side. Wire layouts must match exactly; timing within
//! `TOLERANCE` (the relay adds latency and Windows timers overshoot by up to
//! a 15.6 ms scheduler quantum). The client in the BDS captures is
//! gophertunnel/go-raknet and here `RaknetStream` (Vanilla), so features
//! that depend on the client's ACK clock (window growth after the first
//! round, keepalive timing) are not compared.
//!
//! Captures of the real Minecraft client (`tests/captures/real-client`,
//! recorded with `tools/client-diff/real-client/capture.ps1`) are compared
//! with `RaknetStream` by `real_client_captures_match_vanilla_client`.

// The capture tool's modules: the report and CLI helpers are unused here.
#[allow(dead_code)]
#[path = "../examples/raknet_capture/analysis.rs"]
mod analysis;
#[path = "../examples/raknet_capture/probe.rs"]
mod probe;
#[allow(dead_code)]
#[path = "../examples/raknet_capture/relay.rs"]
mod relay;
#[path = "../examples/raknet_capture/wire.rs"]
mod wire;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::time::timeout;
use tokio_raknet::session::TransmissionPolicy;
use tokio_raknet::transport::{Message, RaknetListenerConfig, RaknetStreamConfig};
use tokio_raknet::{RaknetListener, RaknetStream};

use analysis::{Side, Stats, analyze, intervals};
use relay::{Faults, Relay};
use wire::{Dir, Record};

/// Timing tolerance: relay forwarding plus a Windows timer quantum and
/// scheduling noise under a parallel test run.
const TOLERANCE: f64 = 45.0;

fn captures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

fn bds(scenario: &str) -> Vec<Record> {
    let path = captures().join(format!("bds-1.26.51/{scenario}.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    wire::read_capture(&text).unwrap().1
}

/// A Vanilla listener. tokio's per-IP ping/Request1 rate limiter (a local
/// safety bound BDS does not have) is off: every probe comes from 127.0.0.1.
async fn vanilla_listener() -> RaknetListener {
    RaknetListener::bind_with_config(
        "127.0.0.1:0".parse().unwrap(),
        RaknetListenerConfig {
            transmission_policy: TransmissionPolicy::Vanilla,
            ping_rate_limit_per_ip: 0,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

fn payload(len: usize) -> Vec<u8> {
    let mut bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    bytes[0] = 0xfe;
    bytes
}

/// One BDS-like session through the relay: the server sends a `big`-byte
/// message (BDS's first chunk batch was 222 fragments, ~300 KB at MTU 1400)
/// and a small message every 50 ms (game ticks); the client sends a 4 KB
/// "login" and small messages, stays `hold` and then drops its stream.
async fn session(faults: Faults, hold: Duration, client_mtu: u16, big: usize) -> Vec<Record> {
    let mut listener = vanilla_listener().await;
    let upstream = listener.local_addr();
    let relay = Relay::start("127.0.0.1:0".parse().unwrap(), upstream, faults, None)
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        let mut conn = timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("no connection")
            .expect("listener closed");
        conn.send_encoded(Message::new(payload(big))).await.unwrap();
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if conn.send_encoded(Message::new(payload(21))).await.is_err() {
                        break;
                    }
                }
                received = conn.recv_message() => if !matches!(received, Some(Ok(_))) { break },
            }
        }
    });
    let mut client = timeout(
        Duration::from_secs(10),
        RaknetStream::connect_with_config(
            relay.address,
            RaknetStreamConfig {
                mtu: client_mtu,
                ..Default::default()
            },
        ),
    )
    .await
    .expect("connect timed out")
    .unwrap();
    let started = Instant::now();
    client
        .send_encoded(Message::new(payload(4000)))
        .await
        .unwrap();
    loop {
        let message = timeout(Duration::from_secs(10), client.recv_message())
            .await
            .expect("big message not delivered")
            .unwrap()
            .unwrap();
        if message.payload.len() + 1 == big {
            break;
        }
    }
    let mut pinger = tokio::time::interval(Duration::from_millis(500));
    while started.elapsed() < hold {
        tokio::select! {
            _ = pinger.tick() => client.send_encoded(Message::new(payload(91))).await.unwrap(),
            received = client.recv_message() => { received.unwrap().unwrap(); }
        }
    }
    drop(client);
    tokio::time::sleep(Duration::from_millis(500)).await;
    server.abort();
    relay.finish().await
}

fn fragment_sizes(side: &Side) -> Vec<usize> {
    let mut sizes: Vec<usize> = side
        .splits
        .iter()
        .flat_map(|(_, parts)| parts.iter().rev().skip(1).copied())
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    sizes
}

/// Unreliable (occasional) pings: times.
fn occasional_pings(side: &Side) -> Vec<f64> {
    side.pings
        .iter()
        .filter(|&&(_, reliability)| reliability == 0)
        .map(|&(t, _)| t)
        .collect()
}

/// The Minecraft client joins the IPv4 port of the pong's advertisement, so
/// the real-client relay advertises itself there; BDS 1.26.51's empty
/// RakNet pong gets a BDS-shaped advertisement; the pong header stays byte
/// for byte.
#[test]
fn relay_advertises_itself_in_pongs() {
    let mut header = vec![0x1c];
    header.extend_from_slice(&7u64.to_be_bytes());
    header.extend_from_slice(&0x0102_0304_0506_0708u64.to_be_bytes());
    header.extend_from_slice(&[0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe]);
    header.extend_from_slice(&[0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78]);
    let pong = |advertisement: &str| {
        let mut pong = header.clone();
        pong.extend_from_slice(&(advertisement.len() as u16).to_be_bytes());
        pong.extend_from_slice(advertisement.as_bytes());
        pong
    };
    let rewrite = |bytes: &[u8]| relay::advertise_relay(bytes, "2193;1.26.51", 39140, 39141);
    let advertisement_of = |bytes: &[u8]| {
        assert_eq!(bytes[..33], header[..]);
        let len = u16::from_be_bytes([bytes[33], bytes[34]]) as usize;
        assert_eq!(bytes.len(), 35 + len);
        String::from_utf8(bytes[35..].to_vec()).unwrap()
    };

    // Port fields replaced, everything else kept.
    let advertisement =
        "MCPE;Dedicated Server;2193;1.26.51;0;10;12345;Bedrock level;Survival;1;39132;39133;0;";
    assert_eq!(
        advertisement_of(&rewrite(&pong(advertisement)).unwrap()),
        advertisement.replace(";39132;39133;", ";39140;39141;")
    );

    // BDS 1.26.51's 33-byte pong, or an empty advertisement: synthesised
    // with the pong's GUID.
    let synthesised = format!(
        "MCPE;Dedicated Server;2193;1.26.51;0;10;{};Bedrock level;Survival;1;39140;39141;0;",
        0x0102_0304_0506_0708u64
    );
    assert_eq!(advertisement_of(&rewrite(&header).unwrap()), synthesised);
    assert_eq!(advertisement_of(&rewrite(&pong("")).unwrap()), synthesised);

    // Not a pong, or an advertisement without port fields: forwarded as is.
    assert_eq!(rewrite(&header[..20]), None);
    let mut ping = pong(advertisement);
    ping[0] = 0x01;
    assert_eq!(rewrite(&ping), None);
    assert_eq!(rewrite(&pong("MCPE;motd;2193;1.26.51;0;10")), None);
}

/// End to end: a ping through the relay (`--advertise-relay`) returns the
/// server's advertisement with the relay's ports.
#[tokio::test]
async fn relay_pong_advertises_the_relay_port() {
    let listener = vanilla_listener().await;
    let relay = Relay::start(
        "127.0.0.1:0".parse().unwrap(),
        listener.local_addr(),
        Faults {
            advertise_relay: Some("2193;1.26.51".into()),
            ..Faults::default()
        },
        None,
    )
    .await
    .unwrap();
    let advertisement = tokio_raknet::transport::ping(relay.address).await.unwrap();
    let port = relay.address.port();
    let expected = String::from_utf8(listener.get_advertisement())
        .unwrap()
        .replace(";19132;19133", &format!(";{port};{}", port + 1));
    assert_eq!(std::str::from_utf8(&advertisement).unwrap(), expected);
    relay.finish().await;
}

/// LS-1..LS-5 (listener offline handshake): every offline exchange of the
/// BDS probe gets the same reply type and MTU/security fields.
#[tokio::test]
async fn offline_handshake_matches_bds_probe() {
    let listener = vanilla_listener().await;
    let out = std::env::temp_dir().join(format!(
        "tokio-raknet-probe-{}-{}.jsonl",
        std::process::id(),
        listener.local_addr().port()
    ));
    probe::run(
        listener.local_addr(),
        out.to_str().unwrap(),
        "tokio-raknet Vanilla",
        Duration::from_millis(40),
    )
    .await
    .unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    let ours = wire::read_capture(&text).unwrap().1;
    let reference = bds("probe");
    // Reply bodies: (direction, ID, MTU, security) where present.
    let shape = |records: &[Record]| -> Vec<(Dir, u8, Option<u16>, Option<bool>)> {
        records
            .iter()
            .map(|record| {
                let id = record.offline_id().expect("offline");
                let bytes = record.offline_bytes().unwrap();
                let (mtu, security) = match id {
                    0x06 => (
                        Some(u16::from_be_bytes([bytes[26], bytes[27]])),
                        Some(bytes[25] != 0),
                    ),
                    0x08 => (
                        Some(u16::from_be_bytes([
                            bytes[bytes.len() - 3],
                            bytes[bytes.len() - 2],
                        ])),
                        Some(bytes[bytes.len() - 1] != 0),
                    ),
                    0x19 => (Some(u16::from(bytes[1])), None),
                    _ => (None, None),
                };
                (record.dir, id, mtu, security)
            })
            .collect()
    };
    let (ours, reference) = (shape(&ours), shape(&reference));
    let differences: Vec<String> = ours
        .iter()
        .zip(&reference)
        .enumerate()
        .filter(|(_, (ours, reference))| ours != reference)
        .map(|(index, (ours, reference))| format!("#{index}: ours {ours:?}, bds {reference:?}"))
        .collect();
    assert!(
        differences.is_empty() && ours.len() == reference.len(),
        "{} vs {} records; first differences: {:#?}",
        ours.len(),
        reference.len(),
        &differences[..differences.len().min(6)]
    );
}

/// MTU-*, HS-11, DG-1/2, REL-11, KA-1, KA-6, ACK-1, CC-1: the baseline BDS
/// session (16 s idle, gophertunnel disconnects) against the same session
/// shape on a Vanilla listener.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn baseline_session_matches_bds() {
    let reference = analyze(&bds("baseline"));
    let ours = analyze(
        &session(
            Faults {
                mtu: Some(1492),
                ..Faults::default()
            },
            Duration::from_millis(5600),
            1492,
            296_000,
        )
        .await,
    );
    let (bds_server, server) = (&reference.server, &ours.server);

    // MTU-*: the 1492-byte probe is answered with BDS's 1400 cap.
    let reply1 =
        |offline: &analysis::Offline| offline.reply1.iter().map(|r| r.1).collect::<Vec<_>>();
    assert_eq!(reply1(&ours.offline), reply1(&reference.offline));
    // HS-11: ReliableOrdered CRA with 20 system addresses and index 0.
    assert_eq!(server.accepted, bds_server.accepted);
    assert_eq!(server.accepted, Some((3, 20, 0)));
    // REL-11/DG-1: fragments of MTU - 60 bytes in 1364-byte datagrams, and
    // several messages per datagram.
    assert_eq!(fragment_sizes(server), fragment_sizes(bds_server));
    assert_eq!(server.max_split_datagram, bds_server.max_split_datagram);
    assert!(server.max_datagram <= bds_server.max_split_datagram);
    assert!(server.frames_per_datagram.keys().any(|&n| n > 1));
    // DG-2/CC-1: the update that starts the first split message sends its
    // fragments in slow start (0x04); every datagram after the first one of
    // that update is a continuous send (0x8c). How many fit the window
    // depends on how much the window grew before, i.e. on timing.
    assert_eq!(bds_server.first_split_round, vec![0x84, 0x8c]);
    let round = &server.first_split_round;
    assert!(!round.is_empty() && round[0] & 0x84 == 0x84, "{round:x?}");
    assert!(round[1..].iter().all(|&flags| flags == 0x8c), "{round:x?}");
    // KA-1: two unreliable pings at connect, then every 5 s.
    for side in [server, bds_server] {
        let pings = occasional_pings(side);
        assert!(pings.len() >= 3, "{pings:?}");
        assert!(pings[1] - pings[0] < TOLERANCE, "connect pings {pings:?}");
        for interval in &intervals(&pings[1..]) {
            assert!(
                (interval - 5000.0).abs() <= TOLERANCE,
                "ping interval {interval} in {pings:?}"
            );
        }
    }
    // ACK-1: once the server has an RTT sample, ACKs wait at least the
    // 10 ms SYN and leave within an update period of it.
    let reference_acks = Stats::of(&bds_server.ack_delays_after_rtt);
    let acks = Stats::of(&server.ack_delays_after_rtt);
    assert!(
        reference_acks.min >= 9.0 && reference_acks.max <= 25.0,
        "{reference_acks}"
    );
    assert!(acks.n > 0 && acks.min >= 9.0, "{acks}");
    assert!(
        acks.p50 <= reference_acks.max + TOLERANCE,
        "{acks} vs {reference_acks}"
    );
    // KA-6: the client's notification is ACKed; no echo, no further data.
    for side in [server, bds_server] {
        assert!(side.disconnects.is_empty());
        assert!(side.peer_disconnect_acked.is_some(), "0x15 not ACKed");
        assert_eq!(side.data_after_peer_disconnect, 0);
    }
    assert_eq!(ours.client.disconnects.first().map(|d| d.1), Some(3));
}

/// ACK-3/ACK-6 (5% loss capture): NAKs leave as soon as the datagram that
/// reveals a gap arrives, and NAKed messages are resent at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loss_recovery_matches_bds() {
    let reference = analyze(&bds("loss-5"));
    let ours = analyze(
        &session(
            Faults {
                mtu: Some(1492),
                // A lost client datagram (server NAKs) and lost server
                // datagrams (client NAKs, server resends).
                drop_app_c2s: vec![2, 5],
                drop_app_s2c: vec![3, 40, 41],
                ..Faults::default()
            },
            Duration::from_millis(1500),
            1492,
            296_000,
        )
        .await,
    );
    let bds_latency = Stats::of(&reference.server.nak_latency);
    assert!(bds_latency.n > 0 && bds_latency.max <= 1.0, "{bds_latency}");
    let latency = Stats::of(&ours.server.nak_latency);
    assert!(latency.n > 0 && latency.max <= TOLERANCE, "{latency}");

    let bds_resend = Stats::of(&reference.server.resend_after_nak);
    assert!(bds_resend.n > 0 && bds_resend.max <= 1.0, "{bds_resend}");
    let resend = Stats::of(&ours.server.resend_after_nak);
    assert!(resend.n > 0 && resend.max <= TOLERANCE, "{resend}");
}

/// KA-2/CC-2 (`client-silence`): when the client goes silent, BDS keeps
/// resending every RTO without backoff and drops the connection silently
/// 10 s after the last datagram it received.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn silent_client_is_dropped_after_ten_seconds_like_bds() {
    let reference = analyze(&bds("client-silence"));
    let mut listener = vanilla_listener().await;
    let relay = Relay::start(
        "127.0.0.1:0".parse().unwrap(),
        listener.local_addr(),
        Faults {
            blackhole_c2s_after: Some(Duration::from_millis(1500)),
            ..Faults::default()
        },
        None,
    )
    .await
    .unwrap();
    let server = tokio::spawn(async move {
        let mut conn = listener.accept().await.unwrap();
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        loop {
            tick.tick().await;
            if conn.send_encoded(Message::new(payload(21))).await.is_err() {
                break;
            }
        }
    });
    let client = RaknetStream::connect(relay.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(13_000)).await;
    drop(client);
    server.abort();
    let ours = analyze(&relay.finish().await);

    for (name, side) in [("bds", &reference.server), ("tokio", &ours.server)] {
        let silence = side.last_ms - side.last_peer_delivered_ms;
        assert!(
            (9_900.0..=10_000.0 + TOLERANCE * 2.0).contains(&silence),
            "{name}: last datagram {silence} ms after the client's last one"
        );
        assert!(side.disconnects.is_empty(), "{name}: dropped silently");
        let gaps: Vec<f64> = side
            .resends
            .iter()
            .filter(|resend| resend.t_ms > side.last_peer_delivered_ms + 3000.0)
            .map(|resend| resend.after_ms)
            .collect();
        let gaps = Stats::of(&gaps);
        assert!(gaps.n > 0, "{name}: no resends to the silent client");
        assert!(
            gaps.max <= gaps.min * 1.5 + TOLERANCE,
            "{name}: resend interval grows (backoff): {gaps}"
        );
    }
}

/// The real Minecraft client's RakNet (Mojang, client side) against
/// `RaknetStream` with the Vanilla policy, for every capture recorded with
/// `tools/client-diff/real-client/capture.ps1`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_client_captures_match_vanilla_client() {
    let directory = captures().join("real-client");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|e| panic!("{directory:?}: {e}"))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no captures in {directory:?}");
    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        let (meta, records) = wire::read_capture(&text).unwrap();
        let reference = analyze(&records);
        // The relay MTU of the capture, recovered from its fault description.
        let mtu = meta
            .faults
            .split("mtu: Some(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .and_then(|mtu| mtu.parse::<usize>().ok());
        // 200 fragments of `MTU - 60` (BDS caps the MTU at 1400): within
        // the 512-part split bound (RAKNET-01) at every path MTU.
        let big = 200 * (mtu.unwrap_or(1492).min(1400) - 60);
        let ours = analyze(
            &session(
                Faults {
                    mtu,
                    ..Faults::default()
                },
                Duration::from_millis(6000),
                1492,
                big,
            )
            .await,
        );
        let name = path.display();
        let (client, real) = (&ours.client, &reference.client);
        // MTU-1/2/3: probe sizes and the 500 ms cadence.
        let probes = |offline: &analysis::Offline| -> Vec<usize> {
            offline.request1.iter().map(|r| r.1).collect()
        };
        let reference_probes = probes(&reference.offline);
        let our_probes = probes(&ours.offline);
        let n = reference_probes.len().min(our_probes.len());
        assert_eq!(
            our_probes[..n],
            reference_probes[..n],
            "{name}: probe sizes"
        );
        for offline in [&ours.offline, &reference.offline] {
            let times: Vec<f64> = offline.request1.iter().map(|r| r.0).collect();
            for gap in intervals(&times) {
                assert!(
                    (gap - 500.0).abs() <= TOLERANCE,
                    "{name}: probe cadence {gap}"
                );
            }
        }
        // HS-4: ConnectionRequest reliability; DG-2: flags in use.
        assert_eq!(
            client.reliability.get(&0x09),
            real.reliability.get(&0x09),
            "{name}: ConnectionRequest"
        );
        assert_eq!(
            client.reliability.get(&0x13),
            real.reliability.get(&0x13),
            "{name}: NewIncomingConnection"
        );
        // ACK-1: the client's ACK delay once it has an RTT sample.
        let (ours_acks, real_acks) = (
            Stats::of(&client.ack_delays_after_rtt),
            Stats::of(&real.ack_delays_after_rtt),
        );
        assert!(
            (ours_acks.p50 - real_acks.p50).abs() <= TOLERANCE,
            "{name}: ACK delay {ours_acks} vs {real_acks}"
        );
        // KA-1: occasional ping period.
        let ping_gaps = |side: &Side| Stats::of(&intervals(&occasional_pings(side)[1..]));
        let (ours_pings, real_pings) = (ping_gaps(client), ping_gaps(real));
        assert!(
            (ours_pings.p50 - real_pings.p50).abs() <= TOLERANCE,
            "{name}: ping period {ours_pings} vs {real_pings}"
        );
        // REL-11: fragment size.
        let (ours_fragments, real_fragments) = (fragment_sizes(client), fragment_sizes(real));
        if !real_fragments.is_empty() {
            assert_eq!(ours_fragments, real_fragments, "{name}: fragment sizes");
        }
    }
}
