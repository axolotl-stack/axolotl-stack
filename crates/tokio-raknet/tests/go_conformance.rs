//! Differential conformance against lunar-bedrock/go-raknet `6124f52` (the
//! `tools/bench-compare/go` pin). Opt-in because it needs Go:
//!
//! ```text
//! cargo test -p tokio-raknet --test go_conformance -- --ignored --nocapture
//! ```
//!
//! Each scenario runs one scripted client twice through a recording,
//! fault-injecting UDP relay in front of a go-raknet echo server: first the
//! go-raknet client (`bench-compare raknet-conformance-client`), then
//! `RaknetStream` with the default (Vanilla) policy. `Vanilla` follows Mojang
//! RakNet (see `PARITY.md` and `tests/capture_diff.rs`), so only the wire
//! behaviour both share must be equal: MTU probe sizes and cadence, Request2,
//! NewIncomingConnection (except the first system address), the
//! DisconnectNotification, datagram flags, the 10 ms ACK delay and NAKs for
//! revealed gaps. Where go-raknet differs from Mojang (ConnectionRequest
//! reliability, one frame per datagram, ReliableOrdered pings on a 500 ms
//! tick, fragment sizes, the first NIC system address), each side is checked
//! against its own documented behaviour.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_raknet::RaknetStream;
use tokio_raknet::protocol::datagram::{Datagram, DatagramPayload};
use tokio_raknet::protocol::encapsulated_packet::EncapsulatedPacket;
use tokio_raknet::protocol::reliability::Reliability;
use tokio_raknet::transport::Message;

/// Timer tolerance for phase comparisons: Windows timers can overshoot by a
/// scheduler quantum (about 15.6 ms) and the relay adds its own latency.
const TOLERANCE: Duration = Duration::from_millis(45);

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Go peer, built once per test process.
static HELPER: LazyLock<PathBuf> = LazyLock::new(|| {
    let exe = workspace_root().join("target").join(format!(
        "raknet-conformance{}",
        std::env::consts::EXE_SUFFIX
    ));
    let status = Command::new("go")
        .args(["build", "-o"])
        .arg(&exe)
        .arg(".")
        .current_dir(workspace_root().join("tools/bench-compare/go"))
        .status()
        .expect("Go is required for the conformance test");
    assert!(status.success(), "go build failed");
    exe
});

fn helper() -> &'static PathBuf {
    &HELPER
}

fn payload(size: usize) -> Bytes {
    let mut bytes: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
    bytes[0] = 0xfe;
    Bytes::from(bytes)
}

struct GoServer {
    child: Child,
    address: SocketAddr,
}

impl Drop for GoServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_go_server() -> GoServer {
    use std::io::BufRead;
    let mut child = Command::new(helper())
        .args(["raknet-conformance-serve", "-address", "127.0.0.1:0"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(child.stderr.take().unwrap()).lines();
    let address = loop {
        let line = lines.next().expect("go server exited").unwrap();
        if let Some(address) = line.strip_prefix("READY ") {
            break address.trim().parse().unwrap();
        }
    };
    // Keep draining the server log so it never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    GoServer { child, address }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    ToServer,
    ToClient,
}

#[derive(Clone, Debug)]
struct Record {
    at: Duration,
    direction: Direction,
    bytes: Vec<u8>,
    dropped: bool,
}

/// Faults applied by the relay.
#[derive(Clone, Default)]
struct Faults {
    /// Drop client datagrams longer than this (a smaller path MTU).
    max_to_server: Option<usize>,
    /// Drop these (0-based) application datagrams towards the server.
    drop_to_server_app: Vec<usize>,
    /// Drop these application datagrams towards the client.
    drop_to_client_app: Vec<usize>,
    /// Hold this application datagram towards the client until the next data
    /// datagram towards the client has been forwarded (reordering). ACKs do
    /// not release it: go-raknet's congestion window may keep the next data
    /// datagram back until the server receives an ACK, and an ACK passing the
    /// held datagram reorders nothing.
    hold_to_client_app: Option<usize>,
}

/// A data datagram carrying application (not RakNet control) frames.
fn is_application(bytes: &[u8]) -> bool {
    if bytes
        .first()
        .is_none_or(|&b| b & 0x80 == 0 || b & 0x60 != 0)
    {
        return false;
    }
    let Ok(datagram) = Datagram::decode(&mut &bytes[..]) else {
        return false;
    };
    match datagram.payload {
        DatagramPayload::EncapsulatedPackets(frames) => frames.iter().any(|frame| {
            frame.split.is_some() || frame.payload.first().is_some_and(|&id| id >= 0x80)
        }),
        _ => false,
    }
}

struct Relay {
    address: SocketAddr,
    records: Arc<Mutex<Vec<Record>>>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Relay {
    async fn start(server: SocketAddr, faults: Faults) -> Self {
        let front = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let back = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        back.connect(server).await.unwrap();
        let address = front.local_addr().unwrap();
        let records = Arc::new(Mutex::new(Vec::new()));
        let (stop, mut stopped) = oneshot::channel();
        let log = records.clone();
        let task = tokio::spawn(async move {
            let start = Instant::now();
            let mut client = None;
            let (mut to_server_app, mut to_client_app) = (0usize, 0usize);
            let mut held: Option<Vec<u8>> = None;
            let mut front_buf = vec![0u8; 2048];
            let mut back_buf = vec![0u8; 2048];
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    received = front.recv_from(&mut front_buf) => {
                        let Ok((len, from)) = received else { continue };
                        client = Some(from);
                        let bytes = front_buf[..len].to_vec();
                        let mut dropped = faults.max_to_server.is_some_and(|max| len > max);
                        if is_application(&bytes) {
                            dropped |= faults.drop_to_server_app.contains(&to_server_app);
                            to_server_app += 1;
                        }
                        if !dropped {
                            let _ = back.send(&bytes).await;
                        }
                        log.lock().push(Record {
                            at: start.elapsed(),
                            direction: Direction::ToServer,
                            bytes,
                            dropped,
                        });
                    }
                    received = back.recv(&mut back_buf) => {
                        let Ok(len) = received else { continue };
                        let Some(client) = client else { continue };
                        let bytes = back_buf[..len].to_vec();
                        let mut dropped = false;
                        let mut hold = false;
                        if is_application(&bytes) {
                            dropped = faults.drop_to_client_app.contains(&to_client_app);
                            hold = faults.hold_to_client_app == Some(to_client_app);
                            to_client_app += 1;
                        }
                        log.lock().push(Record {
                            at: start.elapsed(),
                            direction: Direction::ToClient,
                            bytes: bytes.clone(),
                            dropped,
                        });
                        if dropped {
                            continue;
                        }
                        if hold {
                            held = Some(bytes);
                            continue;
                        }
                        let _ = front.send_to(&bytes, client).await;
                        if bytes[0] & 0xe0 == 0x80
                            && let Some(held) = held.take()
                        {
                            let _ = front.send_to(&held, client).await;
                        }
                    }
                }
            }
        });
        Self {
            address,
            records,
            stop: Some(stop),
            task,
        }
    }

    async fn finish(mut self) -> Vec<Record> {
        let _ = self.stop.take().unwrap().send(());
        self.task.await.unwrap();
        std::mem::take(&mut *self.records.lock())
    }

    fn saw_disconnect(&self) -> bool {
        self.records.lock().iter().any(|record| {
            record.direction == Direction::ToServer
                && frames(&record.bytes)
                    .iter()
                    .any(|f| f.payload.as_ref() == [0x15])
        })
    }
}

fn frames(bytes: &[u8]) -> Vec<EncapsulatedPacket> {
    if bytes
        .first()
        .is_none_or(|&b| b & 0x80 == 0 || b & 0x60 != 0)
    {
        return Vec::new();
    }
    match Datagram::decode(&mut &bytes[..]) {
        Ok(Datagram {
            payload: DatagramPayload::EncapsulatedPackets(frames),
            ..
        }) => frames,
        _ => Vec::new(),
    }
}

struct Script {
    sizes: &'static [usize],
    hold: Duration,
}

async fn run_go(server: SocketAddr, faults: Faults, script: &Script) -> Vec<Record> {
    let relay = Relay::start(server, faults).await;
    let sizes = script
        .sizes
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut child = tokio::process::Command::new(helper())
        .args(["raknet-conformance-client", "-address"])
        .arg(relay.address.to_string())
        .args(["-sizes", &sizes, "-hold"])
        .arg(format!("{}ms", script.hold.as_millis()))
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let status = timeout(Duration::from_secs(40), child.wait())
        .await
        .expect("go client timed out")
        .unwrap();
    assert!(status.success(), "go client failed");
    tokio::time::sleep(Duration::from_millis(300)).await;
    relay.finish().await
}

async fn run_tokio(server: SocketAddr, faults: Faults, script: &Script) -> Vec<Record> {
    let relay = Relay::start(server, faults).await;
    let mut stream = timeout(
        Duration::from_secs(15),
        RaknetStream::connect(relay.address),
    )
    .await
    .expect("tokio connect timed out")
    .unwrap();
    for &size in script.sizes {
        let want = payload(size);
        stream
            .send_encoded(Message::new(want.clone()))
            .await
            .unwrap();
        let got = timeout(Duration::from_secs(10), stream.recv_message())
            .await
            .expect("echo timed out")
            .unwrap()
            .unwrap();
        assert_eq!(got.id, want[0]);
        assert_eq!(got.payload.as_ref(), &want[1..]);
    }
    tokio::time::sleep(script.hold).await;
    drop(stream);
    let deadline = Instant::now() + Duration::from_secs(8);
    while !relay.saw_disconnect() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    relay.finish().await
}

/// An application frame: reliability, split (count, index), payload length.
type AppFrame = (Reliability, Option<(u32, u32)>, usize);

/// Timing-independent and timing features of one client trace.
#[derive(Debug, Default)]
struct Features {
    /// Request1 (UDP payload length, send time) before discovery ended.
    probes: Vec<(usize, Duration)>,
    /// Request2 with cookie and GUID masked.
    request2: Vec<Vec<u8>>,
    /// When the ConnectionRequest passed the relay.
    t0: Duration,
    connection_request: Option<(Reliability, usize)>,
    /// NewIncomingConnection with the relay address and timestamps masked.
    new_incoming: Option<(Reliability, Vec<u8>)>,
    data_flags: Vec<u8>,
    frames_per_datagram: Vec<usize>,
    /// First transmissions of application frames.
    app_frames: Vec<AppFrame>,
    /// Message indices seen more than once: (first send, resend).
    resends: Vec<(Duration, Duration)>,
    acks: Vec<Duration>,
    /// Each ACK's delay after the last data datagram forwarded to the client.
    ack_delays: Vec<Duration>,
    pings: Vec<(Duration, Reliability)>,
    /// NAK records relative to the first NAKed sequence.
    naks: Vec<Vec<(u32, u32)>>,
    disconnect: Option<(Reliability, Vec<u8>)>,
    /// MTU carried by the first Request2.
    mtu: usize,
}

fn features(records: &[Record]) -> Features {
    let mut out = Features::default();
    let mut first_sent = std::collections::HashMap::new();
    let mut reply1_seen = false;
    let mut last_data_to_client = None;
    for record in records {
        if record.direction == Direction::ToClient {
            if record.bytes.first() == Some(&0x06) && !record.dropped {
                reply1_seen = true;
            }
            if !record.dropped && record.bytes.first().is_some_and(|&b| b & 0xe0 == 0x80) {
                last_data_to_client = Some(record.at);
            }
            continue;
        }
        let bytes = &record.bytes;
        match bytes[0] {
            0x05 if !reply1_seen => out.probes.push((bytes.len(), record.at)),
            0x07 => {
                // [magic][cookie, proof]? [IPv4 address][MTU][GUID]: mask the
                // GUID, the relay port and the per-address cookie.
                let mut masked = bytes.clone();
                let len = masked.len();
                masked[len - 8..].fill(0);
                masked[len - 12..len - 10].fill(0);
                if len > 1 + 16 + 7 + 2 + 8 {
                    masked[17..21].fill(0);
                }
                if out.mtu == 0 {
                    out.mtu = usize::from(u16::from_be_bytes([bytes[len - 10], bytes[len - 9]]));
                }
                out.request2.push(masked);
            }
            0xc0 => {
                out.acks.push(record.at);
                if let Some(last) = last_data_to_client {
                    out.ack_delays.push(record.at - last);
                }
            }
            0xa0 => {
                if let Ok(Datagram {
                    payload: DatagramPayload::Nak(nak),
                    ..
                }) = Datagram::decode(&mut &bytes[..])
                {
                    let base = nak.ranges[0].start.value();
                    out.naks.push(
                        nak.ranges
                            .iter()
                            .map(|r| (r.start.value() - base, r.end.value() - base))
                            .collect(),
                    );
                }
            }
            flags if flags & 0x80 != 0 => {
                let frames = frames(bytes);
                out.data_flags.push(flags);
                out.frames_per_datagram.push(frames.len());
                for frame in frames {
                    if let Some(index) = frame.reliable_index {
                        if let Some(&first) = first_sent.get(&index.value()) {
                            out.resends.push((first, record.at));
                            continue;
                        }
                        first_sent.insert(index.value(), record.at);
                    }
                    let id = frame.payload[0];
                    if frame.split.is_some() || id >= 0x80 {
                        out.app_frames.push((
                            frame.header.reliability,
                            frame.split.as_ref().map(|s| (s.count, s.index)),
                            frame.payload.len(),
                        ));
                        continue;
                    }
                    match id {
                        0x09 => {
                            out.t0 = record.at;
                            out.connection_request =
                                Some((frame.header.reliability, frame.payload.len()));
                        }
                        0x13 => {
                            // Mask the relay address and both timestamps; the
                            // first system address is compared separately.
                            let mut masked = frame.payload.to_vec();
                            masked[1..8].fill(0);
                            let len = masked.len();
                            masked[len - 16..].fill(0);
                            out.new_incoming = Some((frame.header.reliability, masked));
                        }
                        0x00 => out.pings.push((record.at, frame.header.reliability)),
                        0x15 => {
                            out.disconnect =
                                Some((frame.header.reliability, frame.payload.to_vec()))
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Distance of `offset` from the nearest multiple of `period`.
fn phase_error(offset: Duration, period: Duration) -> Duration {
    let rem = offset.as_nanos() % period.as_nanos();
    Duration::from_nanos(rem.min(period.as_nanos() - rem) as u64)
}

/// The first of the NIC's 20 system addresses (after the ID and the 7-byte
/// IPv4 server address) and the NIC with that address masked.
fn split_first_system_address(nic: &(Reliability, Vec<u8>)) -> ([u8; 7], (Reliability, Vec<u8>)) {
    let (reliability, bytes) = nic;
    let first = bytes[8..15].try_into().unwrap();
    let mut masked = bytes.clone();
    masked[8..15].fill(0);
    (first, (*reliability, masked))
}

fn assert_common(go: &Features, rust: &Features, scenario: &str) {
    // Shared by go-raknet and Mojang RakNet.
    assert_eq!(rust.request2, go.request2, "{scenario}: Request2 bytes");
    // HS-7: same NIC except the first system address: go writes its local
    // socket address (`handler.go:284`, `addr.go:11`), the Vanilla client
    // leaves all 20 unassigned.
    let (go_first, go_nic) = split_first_system_address(go.new_incoming.as_ref().unwrap());
    let (rust_first, rust_nic) = split_first_system_address(rust.new_incoming.as_ref().unwrap());
    assert_eq!(rust_nic, go_nic, "{scenario}: NewIncomingConnection");
    assert_eq!(
        rust_first,
        [4, 0xff, 0xff, 0xff, 0xff, 0, 0],
        "{scenario}/tokio: NIC"
    );
    assert_eq!(
        &go_first[..5],
        &[4, !127, !0, !0, !1],
        "{scenario}/go: NIC 127.0.0.1"
    );
    assert_eq!(
        rust.disconnect, go.disconnect,
        "{scenario}: DisconnectNotification"
    );
    // HS-4: go writes the request ReliableOrdered, Mojang RELIABLE.
    assert_eq!(
        go.connection_request.map(|c| c.0),
        Some(Reliability::ReliableOrdered)
    );
    assert_eq!(
        rust.connection_request.map(|c| c.0),
        Some(Reliability::Reliable)
    );
    // REL-5/REL-11: go keeps the reliability and splits at
    // `MTU - 28 - 14 - 10`; Mojang sends fragments reliably, at most
    // `MTU - 60` bytes each.
    for &(reliability, split, len) in &rust.app_frames {
        if split.is_some() {
            assert!(reliability.is_reliable(), "{scenario}: {reliability:?}");
            assert!(len <= rust.mtu - 60, "{scenario}: tokio fragment {len}");
        }
    }
    for &(_, split, len) in &go.app_frames {
        if split.is_some_and(|(count, index)| index + 1 < count) {
            assert_eq!(len, go.mtu - 28 - 14 - 10, "{scenario}: go fragment");
        }
    }
    for (name, features) in [("go", go), ("tokio", rust)] {
        // DG-2: Mojang's flags, 0x80 plus 0x04 in slow start and 0x08 for a
        // continuous send (go: `conn.go:1078-1087`).
        assert!(
            features
                .data_flags
                .iter()
                .all(|&flags| flags & !0x0c == 0x80),
            "{scenario}/{name}: flags {:?}",
            features.data_flags
        );
        // ACK-1: ACKs leave at once until an ACK/RTT sample exists, then
        // 10 ms after the oldest unacknowledged datagram (go: `conn.go:66-69,
        // 317-326`), so never later than 10 ms after the newest one.
        for &delay in &features.ack_delays {
            assert!(
                delay <= Duration::from_millis(10) + TOLERANCE,
                "{scenario}/{name}: ACK {delay:?} after the last datagram"
            );
        }
    }
    // DG-1 (go-raknet): one frame per datagram (`conn.go:1077-1091`).
    assert!(
        go.frames_per_datagram.iter().all(|&n| n == 1),
        "{scenario}/go: one frame per datagram"
    );
    // KA-1 (go-raknet): ReliableOrdered pings every fifth tick of the 100 ms
    // connection ticker (`conn.go:281-282`).
    for &(ping, reliability) in &go.pings {
        assert_eq!(reliability, Reliability::ReliableOrdered);
        let offset = ping - go.t0;
        assert!(
            offset >= Duration::from_millis(500) - TOLERANCE
                && phase_error(offset, Duration::from_millis(500)) <= TOLERANCE,
            "{scenario}/go: ping off the 500 ms grid at {offset:?}"
        );
    }
    // KA-1 (Mojang): two unreliable pings right after connecting.
    let pings: Vec<_> = rust.pings.iter().take(2).collect();
    assert_eq!(pings.len(), 2, "{scenario}/tokio: connect pings");
    assert!(
        pings
            .iter()
            .all(|&&(at, reliability)| reliability == Reliability::Unreliable
                && at - rust.t0 < Duration::from_millis(200)),
        "{scenario}/tokio: {pings:?}"
    );
    eprintln!(
        "{scenario}: go acks {} pings {} naks {:?} resends {:?}; tokio acks {} pings {} naks {:?} resends {:?}",
        go.acks.len(),
        go.pings.len(),
        go.naks,
        go.resends,
        rust.acks.len(),
        rust.pings.len(),
        rust.naks,
        rust.resends
    );
}

/// MTU-1/2/3/4/8/12, HS-2/4/7, DG-1/2, ACK-1, KA-1, KA-5: path MTU 1200.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Go; run with --ignored"]
async fn mtu_discovery_with_1200_byte_path() {
    let server = start_go_server();
    let faults = Faults {
        max_to_server: Some(1200 - 28),
        ..Default::default()
    };
    let script = Script {
        sizes: &[32, 2000],
        hold: Duration::from_millis(1200),
    };
    let go = features(&run_go(server.address, faults.clone(), &script).await);
    let rust = features(&run_tokio(server.address, faults, &script).await);
    let sizes = |f: &Features| f.probes.iter().map(|p| p.0).collect::<Vec<_>>();
    assert_eq!(sizes(&rust), sizes(&go), "probe sizes");
    assert_eq!(sizes(&go), [1464, 1464, 1464, 1464, 1172]);
    // Consecutive probes 500 ms apart. go-raknet keeps a 500 ms ticker grid;
    // Mojang schedules each probe 500 ms after the previous send, so its
    // lateness accumulates (the real client's probes are 501-508 ms apart,
    // PARITY.md MTU-2) and absolute offsets from the first probe would drift
    // out of tolerance.
    for features in [&go, &rust] {
        for (index, pair) in features.probes.windows(2).enumerate() {
            let gap = pair[1].1 - pair[0].1;
            assert!(
                gap.abs_diff(Duration::from_millis(500)) <= TOLERANCE,
                "probe {} {gap:?} after probe {index}",
                index + 1
            );
        }
    }
    // Request2 carries the MTU the 1200-byte path allowed.
    let request2 = &go.request2[0];
    assert_eq!(
        &request2[request2.len() - 10..request2.len() - 8],
        &1200u16.to_be_bytes()
    );
    assert_common(&go, &rust, "mtu");
}

/// ACK-3/ACK-6/CC-2: lost application datagrams in both directions are
/// recovered by both clients, through the server's NAK or the RTO; both
/// clients NAK a lost server datagram as soon as a later one reveals it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Go; run with --ignored"]
async fn loss_recovery_matches_go() {
    let server = start_go_server();
    let faults = Faults {
        drop_to_server_app: vec![1],
        drop_to_client_app: vec![2],
        ..Default::default()
    };
    let script = Script {
        sizes: &[32, 32, 4096, 32],
        hold: Duration::from_millis(700),
    };
    let go = features(&run_go(server.address, faults.clone(), &script).await);
    let rust = features(&run_tokio(server.address, faults, &script).await);
    assert_common(&go, &rust, "loss");
    for (name, features) in [("go", &go), ("tokio", &rust)] {
        assert!(
            !features.resends.is_empty(),
            "{name}: dropped datagram not resent"
        );
    }
    // ACK-3: both NAK a gap as soon as a later datagram reveals it (go:
    // `conn.go:705-711`).
    for (name, features) in [("go", &go), ("tokio", &rust)] {
        assert!(
            !features.naks.is_empty(),
            "{name}: lost server datagram not NAKed"
        );
    }
}

/// REL-6/8/9: an out-of-order split fragment is reassembled by both clients
/// (the echo checks delivery); both NAK the revealed gap (ACK-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Go; run with --ignored"]
async fn reordered_split_delivery_matches_go() {
    let server = start_go_server();
    let faults = Faults {
        hold_to_client_app: Some(2),
        ..Default::default()
    };
    let script = Script {
        sizes: &[32, 4096, 32],
        hold: Duration::from_millis(700),
    };
    let go = features(&run_go(server.address, faults.clone(), &script).await);
    let rust = features(&run_tokio(server.address, faults, &script).await);
    assert_common(&go, &rust, "reorder");
    for (name, features) in [("go", &go), ("tokio", &rust)] {
        assert!(!features.naks.is_empty(), "{name}: reordered gap not NAKed");
    }
}
