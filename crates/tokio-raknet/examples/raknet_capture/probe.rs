//! Offline-handshake probe: asks a server how it answers
//! `OpenConnectionRequest1`/`2` of chosen sizes, unknown protocol versions
//! and unconnected pings. Every case uses a fresh loopback socket (a fresh
//! client address) and is recorded like relay traffic.

use std::io::Write;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;

use super::wire::{self, Action, Dir, Record};

const MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];
const GUID: i64 = -0x0123_4567_89ab_cdef;

struct Probe {
    start: Instant,
    out: std::io::BufWriter<std::fs::File>,
    count: usize,
}

impl Probe {
    fn log(&mut self, dir: Dir, bytes: &[u8]) -> std::io::Result<()> {
        let record = Record {
            t_us: self.start.elapsed().as_micros() as u64,
            dir,
            len: bytes.len(),
            action: Action::Fwd,
            body: wire::parse(bytes),
        };
        serde_json::to_writer(&mut self.out, &record)?;
        self.out.write_all(b"\n")?;
        self.count += 1;
        Ok(())
    }

    async fn exchange(
        &mut self,
        socket: &UdpSocket,
        request: &[u8],
        wait: Duration,
    ) -> std::io::Result<Vec<Vec<u8>>> {
        self.log(Dir::C2s, request)?;
        socket.send(request).await?;
        let mut replies = Vec::new();
        let deadline = tokio::time::Instant::now() + wait;
        let mut buf = vec![0u8; 4096];
        while let Ok(received) = tokio::time::timeout_at(deadline, socket.recv(&mut buf)).await {
            let Ok(len) = received else { break };
            self.log(Dir::S2c, &buf[..len])?;
            replies.push(buf[..len].to_vec());
        }
        Ok(replies)
    }
}

fn request1(mtu: usize, protocol: u8) -> Vec<u8> {
    let mut bytes = vec![0x05];
    bytes.extend_from_slice(&MAGIC);
    bytes.push(protocol);
    bytes.resize(mtu - wire::UDP_IP_OVERHEAD, 0);
    bytes
}

/// `(cookie, mtu)` of an `OpenConnectionReply1`.
fn reply1(bytes: &[u8]) -> Option<(Option<u32>, u16)> {
    if bytes.first() != Some(&0x06) || bytes.len() < 28 {
        return None;
    }
    let security = bytes[25] != 0;
    let at = if security { 30 } else { 26 };
    let cookie = security.then(|| u32::from_be_bytes(bytes[26..30].try_into().unwrap()));
    Some((cookie, u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?)))
}

fn request2(server: SocketAddr, cookie: Option<u32>, mtu: u16, guid: i64) -> Vec<u8> {
    let mut bytes = vec![0x07];
    bytes.extend_from_slice(&MAGIC);
    if let Some(cookie) = cookie {
        bytes.extend_from_slice(&cookie.to_be_bytes());
        bytes.push(0);
    }
    let SocketAddr::V4(v4) = server else {
        unreachable!("loopback IPv4 server")
    };
    bytes.push(4);
    bytes.extend(v4.ip().octets().iter().map(|b| !b));
    bytes.extend_from_slice(&v4.port().to_be_bytes());
    bytes.extend_from_slice(&mtu.to_be_bytes());
    bytes.extend_from_slice(&guid.to_be_bytes());
    bytes
}

async fn fresh(server: SocketAddr) -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    socket.connect(server).await?;
    Ok(socket)
}

/// Runs every probe case against `server` and writes the capture.
pub async fn run(
    server: SocketAddr,
    out: &str,
    server_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = std::io::BufWriter::new(std::fs::File::create(out)?);
    let meta = wire::Meta {
        kind: "meta".into(),
        version: 1,
        scenario: "offline-probe".into(),
        server: server_name.into(),
        client: "raknet_capture probe".into(),
        faults: "none".into(),
    };
    serde_json::to_writer(&mut writer, &meta)?;
    writer.write_all(b"\n")?;
    let mut probe = Probe {
        start: Instant::now(),
        out: writer,
        count: 0,
    };
    let wait = Duration::from_millis(500);

    // Unconnected ping: 0x01, time, magic, GUID.
    let socket = fresh(server).await?;
    let mut ping = vec![0x01];
    ping.extend_from_slice(&1234u64.to_be_bytes());
    ping.extend_from_slice(&MAGIC);
    ping.extend_from_slice(&GUID.to_be_bytes());
    probe.exchange(&socket, &ping, wait).await?;

    // Request1 sizes: Reply1 MTU as a function of the request size.
    for mtu in [1500usize, 1492, 1464, 1400, 1300, 1200, 1000, 576, 400, 300, 100] {
        let socket = fresh(server).await?;
        probe.exchange(&socket, &request1(mtu, 11), wait).await?;
    }
    // Unsupported protocol versions.
    for protocol in [10u8, 12] {
        let socket = fresh(server).await?;
        probe.exchange(&socket, &request1(1492, protocol), wait).await?;
    }
    // Request2 MTU handling relative to the Reply1 MTU (1492-byte Request1).
    // Each case uses its own GUID: BDS answers a GUID it already has a
    // connection for with AlreadyConnected.
    for (case, mtu) in [1492u16, 1400, 1200, 576, 400, 399, 300, 1500, 1600, 2000]
        .into_iter()
        .enumerate()
    {
        let socket = fresh(server).await?;
        let replies = probe.exchange(&socket, &request1(1492, 11), wait).await?;
        let Some((cookie, _)) = replies.iter().find_map(|reply| reply1(reply)) else {
            continue;
        };
        let guid = GUID - case as i64;
        probe
            .exchange(&socket, &request2(server, cookie, mtu, guid), wait)
            .await?;
    }
    // Request2 without a preceding Request1.
    let socket = fresh(server).await?;
    probe
        .exchange(&socket, &request2(server, None, 1400, GUID - 100), wait)
        .await?;
    // A repeated Request2, a Request1 after the connection exists, and the
    // same GUID from another address.
    let socket = fresh(server).await?;
    let replies = probe.exchange(&socket, &request1(1200, 11), wait).await?;
    if let Some((cookie, mtu)) = replies.iter().find_map(|reply| reply1(reply)) {
        let request = request2(server, cookie, mtu, GUID - 101);
        probe.exchange(&socket, &request, wait).await?;
        probe.exchange(&socket, &request, wait).await?;
        probe.exchange(&socket, &request1(1200, 11), wait).await?;
        let other = fresh(server).await?;
        probe.exchange(&other, &request, wait).await?;
    }
    probe.out.flush()?;
    eprintln!("probe: {} records -> {out}", probe.count);
    Ok(())
}
