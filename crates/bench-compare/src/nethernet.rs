//! Measurement-only adapter over public tokio-nethernet APIs.
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::{
    hint::black_box,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, tcp::OwnedWriteHalf},
    sync::{Mutex, mpsc, watch},
    time::timeout,
};
use tokio_nethernet::{
    Message, NetherNetDialer, NetherNetDialerConfig, NetherNetListener, NetherNetListenerConfig,
    NetherNetStream, Signal, Signaling, SignalingChannel, format_ice_candidate,
    parse_ice_candidate,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const IMPLEMENTATION: &str = "rust-owned";
pub const CANDIDATE: &str = "candidate:1 1 UDP 2130706431 127.0.0.1 50000 typ host generation 0 ufrag bench network-id 0 network-cost 0";
const SDP: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na=setup:";

fn loopback_settings() -> webrtc::api::setting_engine::SettingEngine {
    let mut settings = webrtc::api::setting_engine::SettingEngine::default();
    settings.set_include_loopback_candidate(true);
    settings.set_ip_filter(Box::new(|ip| ip.is_loopback() && ip.is_ipv4()));
    settings.set_network_types(vec![webrtc::ice::network_type::NetworkType::Udp4]);
    settings
}
pub fn signal_fixtures() -> [(&'static str, String); 4] {
    [
        (
            "connect_request",
            format!("CONNECTREQUEST 42 {SDP}actpass\r\n"),
        ),
        (
            "connect_response",
            format!("CONNECTRESPONSE 42 {SDP}active\r\n"),
        ),
        ("candidate_add", format!("CANDIDATEADD 42 {CANDIDATE}")),
        ("connect_error", "CONNECTERROR 42 2".into()),
    ]
}

pub fn micro(iterations: usize) -> Result<()> {
    ensure!(iterations > 0, "iterations must be positive");
    for (name, text) in signal_fixtures() {
        let signal = Signal::parse(&text, "2".into())?;
        ensure!(signal.to_string() == text, "signal roundtrip mismatch");
        super::measure(
            "nethernet_signaling",
            name,
            "parse",
            IMPLEMENTATION,
            text.len(),
            iterations,
            || {
                black_box(Signal::parse(black_box(&text), "2".into()).unwrap());
            },
        )?;
        super::measure(
            "nethernet_signaling",
            name,
            "format",
            IMPLEMENTATION,
            text.len(),
            iterations,
            || {
                black_box(black_box(&signal).to_string());
            },
        )?;
    }
    parse_ice_candidate(CANDIDATE)?;
    let format_candidate = || {
        format_ice_candidate(
            0,
            "1",
            "UDP",
            2130706431,
            "127.0.0.1",
            50000,
            "host",
            None,
            None,
            "bench",
        )
    };
    ensure!(format_candidate() == CANDIDATE, "candidate format mismatch");
    super::measure(
        "nethernet_signaling",
        "ice_candidate",
        "parse",
        IMPLEMENTATION,
        CANDIDATE.len(),
        iterations,
        || {
            black_box(parse_ice_candidate(black_box(CANDIDATE)).unwrap());
        },
    )?;
    super::measure(
        "nethernet_signaling",
        "ice_candidate",
        "format",
        IMPLEMENTATION,
        CANDIDATE.len(),
        iterations,
        || {
            black_box(format_candidate());
        },
    )?;
    for (name, reason) in [
        (
            "nethernet_framing",
            "segment_count, send_message and ReassemblyState are private; encode/reassemble including invalid counts and oversize have no public isolated API",
        ),
        (
            "nethernet_discovery",
            "discovery::packet is private; encrypted/HMAC encode/decode functions are not publicly re-exported",
        ),
    ] {
        eprintln!(
            "{}",
            serde_json::json!({"area":"coverage", "status":"unavailable", "name":name, "implementation":IMPLEMENTATION, "reason":reason})
        );
    }
    Ok(())
}

/// Benchmark-local signaling only. Application bytes still use real ICE/DTLS/SCTP.
struct Bridge {
    writer: Mutex<OwnedWriteHalf>,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
    ended: watch::Receiver<bool>,
    reader_task: tokio::task::AbortHandle,
    local_id: &'static str,
}

impl Bridge {
    fn new(socket: TcpStream, server: bool) -> Result<Arc<Self>> {
        socket.set_nodelay(true)?;
        let (reader, writer) = socket.into_split();
        let (sender, receiver) = mpsc::channel(128);
        let (ended_tx, ended) = watch::channel(false);
        let remote_id = if server { "2" } else { "1" };
        let task = tokio::spawn(async move {
            let result: Result<()> = async {
                let mut lines = BufReader::new(reader).lines();
                while let Some(line) = lines.next_line().await? {
                    let text: String = serde_json::from_str(&line)?;
                    let signal = Signal::parse(&text, remote_id.into())?;
                    if sender.send(signal).await.is_err() {
                        break;
                    }
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                eprintln!("NetherNet signaling bridge: {error:#}");
            }
            let _ = ended_tx.send(true);
        });
        Ok(Arc::new(Self {
            writer: Mutex::new(writer),
            receiver: Mutex::new(Some(receiver)),
            ended,
            reader_task: task.abort_handle(),
            local_id: if server { "1" } else { "2" },
        }))
    }

    async fn shutdown(&self) -> Result<()> {
        self.writer.lock().await.shutdown().await?;
        Ok(())
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

#[async_trait]
impl Signaling for Bridge {
    async fn signal(&self, signal: Signal) -> Result<()> {
        let mut encoded = serde_json::to_vec(&signal.to_string())?;
        encoded.push(b'\n');
        self.writer.lock().await.write_all(&encoded).await?;
        Ok(())
    }
    fn network_id(&self) -> String {
        self.local_id.into()
    }
    fn disable_trickle_ice(&self) -> bool {
        true
    }
    fn requires_identity(&self) -> bool {
        false
    }
    async fn closed(&self) {
        let mut ended = self.ended.clone();
        while !*ended.borrow_and_update() {
            if ended.changed().await.is_err() {
                break;
            }
        }
    }
}

#[async_trait]
impl SignalingChannel for Bridge {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.receiver.lock().await.take()
    }
}

fn count_for_size(size: usize, count: usize) -> Result<usize> {
    let count = if size >= 4096 {
        match std::env::var("BENCH_SPLIT_COUNT") {
            Ok(value) => value
                .parse()
                .context("BENCH_SPLIT_COUNT must be an integer")?,
            Err(std::env::VarError::NotPresent) => 10,
            Err(error) => return Err(error.into()),
        }
    } else {
        count
    };
    ensure!(count > 0, "message count must be positive");
    Ok(count)
}

async fn echo(listener: TcpListener, count: usize) -> Result<()> {
    for size in super::transport::SIZES {
        let count = count_for_size(size, count)?;
        let (socket, _) = timeout(TIMEOUT, listener.accept())
            .await
            .context("signaling accept timeout")??;
        let bridge = Bridge::new(socket, true)?;
        let mut listener = NetherNetListener::bind_with_signaling(
            bridge.clone(),
            NetherNetListenerConfig {
                allow_identityless_client: true,
                setting_engine: Some(loopback_settings()),
                ..Default::default()
            },
        );
        let mut stream = timeout(TIMEOUT, listener.accept())
            .await
            .context("SCTP accept timeout")??;
        let expected = super::transport::payload(size);
        for _ in 0..=count {
            let message = timeout(TIMEOUT, stream.next())
                .await
                .context("server receive timeout")?
                .context("client closed")??;
            ensure!(
                message.reliable && message.buffer == expected,
                "server payload mismatch ({size})"
            );
            timeout(TIMEOUT, stream.send(message))
                .await
                .context("server send timeout")??;
        }
        // Client shuts down signaling only after validating the final echo.
        timeout(TIMEOUT, bridge.closed())
            .await
            .context("client shutdown timeout")?;
        drop(stream);
        bridge.shutdown().await?;
    }
    Ok(())
}

pub async fn serve(address: SocketAddr, count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    let listener = TcpListener::bind(address).await?;
    eprintln!("READY {}", listener.local_addr()?);
    echo(listener, count).await
}

async fn exchange(stream: &mut NetherNetStream, payload: &bytes::Bytes) -> Result<()> {
    timeout(TIMEOUT, stream.send(Message::reliable(payload.clone())))
        .await
        .context("client send timeout")??;
    let reply = timeout(TIMEOUT, stream.next())
        .await
        .context("client receive timeout")?
        .context("server closed")??;
    ensure!(
        reply.reliable && reply.buffer == *payload,
        "echo payload mismatch ({})",
        payload.len()
    );
    Ok(())
}

fn emit(size: usize, operation: &str, seconds: f64, mut samples: Vec<f64>) -> Result<()> {
    samples.sort_unstable_by(f64::total_cmp);
    let count = samples.len();
    let percentile = |percent: usize| {
        samples[(count * percent)
            .div_ceil(100)
            .saturating_sub(1)
            .min(count - 1)]
    };
    super::Row {
        area: "nethernet_loopback",
        name: &size.to_string(),
        operation,
        implementation: IMPLEMENTATION,
        ns_per_op: seconds * 1e9 / count as f64,
        allocs_per_op: None,
        bytes_per_op: None,
        payload_bytes: size,
        msgs_per_sec: count as f64 / seconds,
        mb_per_sec: count as f64 * size as f64 / seconds / 1e6,
        p50_us: Some(percentile(50)),
        p99_us: Some(percentile(99)),
        max_us: Some(samples[count - 1]),
    }
    .emit()
}

pub async fn client(address: SocketAddr, count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    for size in super::transport::SIZES {
        let count = count_for_size(size, count)?;
        let payload = super::transport::payload(size);
        let mut samples = Vec::with_capacity(count);
        let setup = Instant::now();
        let socket = timeout(TIMEOUT, TcpStream::connect(address))
            .await
            .context("signaling connect timeout")??;
        let bridge = Bridge::new(socket, false)?;
        let dialer = NetherNetDialer::connect_with_signaling(
            bridge.clone(),
            NetherNetDialerConfig {
                allow_identityless_server: true,
                setting_engine: Some(loopback_settings()),
                ..Default::default()
            },
        );
        let mut stream = timeout(TIMEOUT, dialer.dial("1".into()))
            .await
            .context("SCTP connect timeout")??;
        let setup_seconds = setup.elapsed().as_secs_f64();
        exchange(&mut stream, &payload).await?;
        let started = Instant::now();
        for _ in 0..count {
            let tick = Instant::now();
            exchange(&mut stream, &payload).await?;
            samples.push(tick.elapsed().as_secs_f64() * 1e6);
        }
        let seconds = started.elapsed().as_secs_f64();
        emit(size, "setup", setup_seconds, vec![setup_seconds * 1e6])?;
        emit(size, "echo", seconds, samples)?;
        drop(stream);
        drop(dialer);
        bridge.shutdown().await?;
    }
    Ok(())
}

pub async fn loopback(count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::try_join!(echo(listener, count), client(address, count))?;
    Ok(())
}
