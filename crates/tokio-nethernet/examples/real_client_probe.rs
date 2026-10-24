//! Loopback-only probe of the real Minecraft client's NetherNet behaviour.
//!
//! Every socket this program opens is bound to `127.0.0.1`, so it never
//! listens on a network interface and cannot raise a host-firewall prompt.
//! The client's LAN socket (`[::]:7551` / `0.0.0.0:7551`) accepts discovery
//! datagrams from any source, which gives two loopback paths (see
//! `crates/tokio-nethernet/PARITY.md` and `tools/client-diff/nethernet/README.md`):
//!
//! - `host`: the client hosts a LAN world. A unicast discovery request to
//!   `127.0.0.1:7551` returns its server data; the probe dials it and records
//!   the vanilla answer, ICE outcome and first packets.
//! - `server`: the client browses LAN games. Unsolicited discovery responses
//!   to `127.0.0.1:7551` list the probe; joining it records the vanilla offer,
//!   trickled candidates, identity and first packets.
//! - `route`: checks that datagrams from `127.0.0.1` reach this machine's own
//!   interface addresses (the ICE pair the client needs), without binding them.
//!
//! Every event is one JSON line on stdout and in `--out <file>`.
//!
//! ```text
//! cargo run -p tokio-nethernet --features discovery --example real_client_probe -- host --out host.jsonl
//! ```

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio_nethernet::discovery::{DiscoveryListener, DiscoveryListenerConfig, ServerData};
use tokio_nethernet::identity::{IdentityError, TokenVerifier};
use tokio_nethernet::{
    Message, NetherNetDialer, NetherNetDialerConfig, NetherNetListener, NetherNetListenerConfig,
    NetherNetStream, Signal, Signaling, SignalingChannel, TransportConfig,
};

/// The client's fixed LAN discovery port.
const CLIENT_LAN: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7551);
/// Bedrock 1.26.51 protocol (RequestNetworkSettings and the server data).
const PROTOCOL: i32 = 2193;

/// JSON-lines event log shared by every task.
#[derive(Clone)]
struct Log {
    started: Instant,
    file: Arc<parking_lot::Mutex<std::fs::File>>,
}

impl Log {
    fn create(path: &str) -> anyhow::Result<Self> {
        Ok(Self {
            started: Instant::now(),
            file: Arc::new(parking_lot::Mutex::new(
                std::fs::File::create(path).with_context(|| format!("create {path}"))?,
            )),
        })
    }

    fn event(&self, event: &str, mut fields: Value) {
        use std::io::Write;
        fields["t_ms"] = json!(self.started.elapsed().as_millis() as u64);
        fields["event"] = json!(event);
        let line = fields.to_string();
        println!("{line}");
        let mut file = self.file.lock();
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

/// Refuses to continue unless `addr` is a loopback address.
fn require_loopback(what: &str, addr: SocketAddr) -> anyhow::Result<()> {
    if !addr.ip().is_loopback() {
        bail!("{what} bound {addr}: the probe must bind loopback only");
    }
    Ok(())
}

fn transport() -> TransportConfig {
    let config = TransportConfig::loopback_v4();
    assert!(!config.resolve_mdns, "mDNS would bind 0.0.0.0:5353");
    config
}

/// Records every signal sent and received through a discovery listener.
struct Recording {
    inner: Arc<DiscoveryListener>,
    log: Log,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
}

impl Recording {
    async fn new(inner: Arc<DiscoveryListener>, log: Log) -> anyhow::Result<Self> {
        let mut inbound = inner
            .take_signal_receiver()
            .await
            .context("discovery signal receiver already taken")?;
        let (forward, receiver) = mpsc::channel(128);
        let record = log.clone();
        tokio::spawn(async move {
            while let Some(signal) = inbound.recv().await {
                record.event(
                    "signal_in",
                    json!({"from": signal.network_id, "wire": redact(&signal)}),
                );
                if forward.send(signal).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            inner,
            log,
            receiver: Mutex::new(Some(receiver)),
        })
    }
}

#[async_trait::async_trait]
impl Signaling for Recording {
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        self.log.event(
            "signal_out",
            json!({"to": signal.network_id, "wire": redact(&signal)}),
        );
        let result = self.inner.signal(signal).await;
        if let Err(error) = &result {
            self.log
                .event("signal_out_error", json!({"error": error.to_string()}));
        }
        result
    }
    fn network_id(&self) -> String {
        Signaling::network_id(&*self.inner)
    }
    fn disable_trickle_ice(&self) -> bool {
        self.inner.disable_trickle_ice()
    }
    fn requires_identity(&self) -> bool {
        self.inner.requires_identity()
    }
}

#[async_trait::async_trait]
impl SignalingChannel for Recording {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.receiver.lock().await.take()
    }
}

/// Accepts any peer token and records its header and claims (never the
/// signature, so the log cannot replay it): the probe observes, it does not
/// authenticate. Fingerprint binding is still checked by the crate.
struct RecordToken(Log);

impl TokenVerifier for RecordToken {
    fn verify<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), IdentityError>> + Send + 'a>> {
        use base64::Engine;
        let decode = |part: Option<&str>| {
            part.and_then(|part| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(part.trim_end_matches('='))
                    .ok()
            })
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        };
        let mut parts = token.split('.');
        let header = decode(parts.next());
        let claims = decode(parts.next());
        self.0
            .event("peer_token", json!({"header": header, "claims": claims}));
        Box::pin(async { Ok(()) })
    }
}

/// Signal text with the `a=identity` value (which embeds the account's
/// multiplayer JWT) replaced by its length; the token's claims are logged
/// separately by [`RecordToken`].
fn redact(signal: &Signal) -> String {
    signal
        .to_string()
        .split("\r\n")
        .map(|line| match line.strip_prefix("a=identity:") {
            Some(value) => format!("a=identity:<redacted {} bytes>", value.len()),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn hex_prefix(bytes: &[u8], limit: usize) -> String {
    hex::encode(&bytes[..bytes.len().min(limit)])
}

/// Logs every message until `idle` passes without one or the stream ends.
async fn drain(log: &Log, stream: &mut NetherNetStream, idle: Duration) {
    loop {
        match tokio::time::timeout(idle, stream.next()).await {
            Ok(Some(Ok(message))) => log.event(
                "message_in",
                json!({
                    "reliable": message.reliable,
                    "len": message.buffer.len(),
                    "hex_prefix": hex_prefix(&message.buffer, 96),
                }),
            ),
            Ok(Some(Err(error))) => {
                log.event("stream_error", json!({"error": error.to_string()}));
                return;
            }
            Ok(None) => {
                log.event("stream_end", json!({}));
                return;
            }
            Err(_) => {
                log.event("idle", json!({"seconds": idle.as_secs()}));
                return;
            }
        }
    }
}

/// One uncompressed batch (the batch before NetworkSettings has no compression
/// prefix and NetherNet batches have no 0xfe header): varuint32 length, then the packet.
fn batch(packet: &[u8]) -> Bytes {
    let mut out = Vec::with_capacity(packet.len() + 5);
    let mut length = packet.len() as u32;
    loop {
        let byte = (length & 0x7f) as u8;
        length >>= 7;
        if length == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out.extend_from_slice(packet);
    Bytes::from(out)
}

/// RequestNetworkSettings (id 193 as varuint32) with the big-endian protocol.
fn request_network_settings() -> Bytes {
    let mut packet = vec![0xc1, 0x01];
    packet.extend_from_slice(&PROTOCOL.to_be_bytes());
    batch(&packet)
}

/// NetworkSettings (id 143): threshold 0, algorithm 0xffff (none), no throttle.
fn network_settings() -> Bytes {
    let mut packet = vec![0x8f, 0x01];
    packet.extend_from_slice(&0u16.to_le_bytes());
    packet.extend_from_slice(&0xffffu16.to_le_bytes());
    packet.push(0);
    packet.push(0);
    packet.extend_from_slice(&0f32.to_le_bytes());
    batch(&packet)
}

async fn bind_discovery(
    log: &Log,
    broadcast_addr: SocketAddr,
) -> anyhow::Result<Arc<DiscoveryListener>> {
    let discovery = DiscoveryListener::bind(
        "127.0.0.1:0",
        DiscoveryListenerConfig {
            broadcast_addr,
            ..Default::default()
        },
    )
    .await?;
    let local = discovery.local_addr()?;
    require_loopback("discovery", local)?;
    log.event(
        "discovery_bound",
        json!({"local": local.to_string(), "network_id": Signaling::network_id(&*discovery)}),
    );
    Ok(discovery)
}

/// The client hosts a LAN world; the probe discovers and dials it.
async fn host(log: Log, wait: Duration) -> anyhow::Result<()> {
    let discovery = bind_discovery(&log, CLIENT_LAN).await?;
    let deadline = Instant::now() + wait;
    let (host_id, data) = loop {
        if let Some((id, data)) = discovery.responses().await.into_iter().next() {
            break (id, data);
        }
        if Instant::now() > deadline {
            log.event("no_host", json!({"waited_s": wait.as_secs()}));
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    log.event(
        "host_discovered",
        json!({
            "network_id": host_id,
            "server_data_hex": hex::encode(&data),
            "decoded": ServerData::decode(&data).map(|d| format!("{d:?}")),
        }),
    );
    let signaling = Recording::new(discovery, log.clone()).await?;
    let dialer = NetherNetDialer::connect_with_signaling(
        signaling,
        NetherNetDialerConfig {
            transport: transport(),
            allow_identityless_server: true,
            verify_server_token: Some(Arc::new(RecordToken(log.clone()))),
            connection_timeout: Duration::from_secs(30),
            ..Default::default()
        },
    );
    let started = Instant::now();
    let mut stream = match dialer.dial(host_id.to_string()).await {
        Ok(stream) => stream,
        Err(error) => {
            log.event(
                "dial_failed",
                json!({"error": error.to_string(), "after_ms": started.elapsed().as_millis() as u64}),
            );
            return Ok(());
        }
    };
    log.event(
        "connected",
        json!({
            "after_ms": started.elapsed().as_millis() as u64,
            "remote_identity": stream.remote_identity().map(|identity| identity.domain.clone()),
        }),
    );
    stream
        .send(Message::reliable(request_network_settings()))
        .await?;
    log.event("message_out", json!({"packet": "RequestNetworkSettings"}));
    drain(&log, &mut stream, Duration::from_secs(15)).await;
    let closing = Instant::now();
    let closed = stream.close().await;
    log.event(
        "closed",
        json!({"result": format!("{closed:?}"), "ms": closing.elapsed().as_millis() as u64}),
    );
    Ok(())
}

/// The client browses LAN games; the probe announces itself and accepts.
async fn server(log: Log, wait: Duration) -> anyhow::Result<()> {
    // Announcing listeners send no requests; the address is never used.
    let discovery = bind_discovery(&log, SocketAddr::from(([127, 0, 0, 1], 9))).await?;
    discovery
        .set_server_data(
            ServerData::builder()
                .name("NETHERNET-TRUTH probe")
                .level("loopback")
                .players(0, 10)
                .build(),
        )
        .await;
    let announcer = discovery.clone();
    let announce_log = log.clone();
    let announce = tokio::spawn(async move {
        let mut reported = false;
        loop {
            if let Err(error) = announcer.send_server_data_to(CLIENT_LAN).await
                && !reported
            {
                announce_log.event("announce_error", json!({"error": error.to_string()}));
                reported = true;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    let signaling = Recording::new(discovery, log.clone()).await?;
    let mut listener = NetherNetListener::bind_with_signaling(
        signaling,
        NetherNetListenerConfig {
            transport: transport(),
            allow_identityless_client: true,
            verify_client_token: Some(Arc::new(RecordToken(log.clone()))),
            connection_timeout: Duration::from_secs(30),
            ..Default::default()
        },
    );
    log.event(
        "announcing",
        json!({"to": CLIENT_LAN.to_string(), "wait_s": wait.as_secs()}),
    );
    let accepted = tokio::time::timeout(wait, listener.accept()).await;
    announce.abort();
    let mut stream = match accepted {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            log.event("accept_failed", json!({"error": error.to_string()}));
            return Ok(());
        }
        Err(_) => {
            log.event("no_join", json!({"waited_s": wait.as_secs()}));
            return Ok(());
        }
    };
    log.event(
        "accepted",
        json!({"remote_identity": stream.remote_identity().map(|identity| identity.domain.clone())}),
    );
    match tokio::time::timeout(Duration::from_secs(15), stream.next()).await {
        Ok(Some(Ok(first))) => {
            log.event(
                "message_in",
                json!({
                    "reliable": first.reliable,
                    "len": first.buffer.len(),
                    "hex_prefix": hex_prefix(&first.buffer, 96),
                }),
            );
            stream.send(Message::reliable(network_settings())).await?;
            log.event("message_out", json!({"packet": "NetworkSettings"}));
            drain(&log, &mut stream, Duration::from_secs(15)).await;
        }
        other => log.event("no_first_message", json!({"result": format!("{other:?}")})),
    }
    let closing = Instant::now();
    let closed = stream.close().await;
    log.event(
        "closed",
        json!({"result": format!("{closed:?}"), "ms": closing.elapsed().as_millis() as u64}),
    );
    Ok(())
}

/// Sends one datagram from 127.0.0.1 to each local IPv4 address's discard
/// port: an ICMP port-unreachable (WSAECONNRESET on the next receive) proves
/// the datagram was delivered locally, i.e. 127.0.0.1 can reach the address
/// the client's ICE sockets are bound to. Nothing but 127.0.0.1 is bound.
async fn route(log: Log) -> anyhow::Result<()> {
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    require_loopback("route probe", socket.local_addr()?)?;
    let mut targets = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    for interface in if_addrs::get_if_addrs()? {
        if let if_addrs::IfAddr::V4(v4) = interface.addr
            && !v4.ip.is_loopback()
            && !v4.ip.is_link_local()
        {
            targets.push(IpAddr::V4(v4.ip));
        }
    }
    let mut buf = [0u8; 16];
    for ip in targets {
        let target = SocketAddr::new(ip, 9);
        let sent = socket.send_to(b"probe", target).await;
        let outcome = match sent {
            Err(error) => format!("send failed: {error}"),
            Ok(_) => match tokio::time::timeout(Duration::from_millis(500), socket.recv(&mut buf))
                .await
            {
                Err(_) => "no reply (dropped or filtered)".to_owned(),
                Ok(Ok(_)) => "unexpected reply".to_owned(),
                Ok(Err(error)) if error.raw_os_error() == Some(10054) => {
                    "delivered (port unreachable)".to_owned()
                }
                Ok(Err(error)) => format!("receive error: {error}"),
            },
        };
        log.event(
            "route",
            json!({"to": target.to_string(), "outcome": outcome}),
        );
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    let option = |name: &str| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|index| args.get(index + 1))
            .cloned()
    };
    let out = option("--out").unwrap_or_else(|| format!("nethernet-probe-{mode}.jsonl"));
    let wait = Duration::from_secs(
        option("--wait")
            .map(|value| value.parse())
            .transpose()
            .context("--wait takes seconds")?
            .unwrap_or(180),
    );
    if std::env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .init();
    }
    let log = Log::create(&out)?;
    log.event("start", json!({"mode": mode, "out": out}));
    match mode {
        "host" => host(log.clone(), wait).await?,
        "server" => server(log.clone(), wait).await?,
        "route" => route(log.clone()).await?,
        _ => bail!("usage: real_client_probe <host|server|route> [--out FILE] [--wait SECONDS]"),
    }
    log.event("done", json!({}));
    Ok(())
}
