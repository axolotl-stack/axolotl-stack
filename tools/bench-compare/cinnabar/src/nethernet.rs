//! Measurement-only use of the pinned Apache-2.0 NetherNet public APIs.
use crate::{Row, measure, payload};
use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use nethernet::connection::IceMode;
use nethernet::protocol::packet::discovery::{self, MessagePacket, Packets};
use nethernet::protocol::webrtc::candidate::{format_ice_candidate, parse_ice_candidate};
use nethernet::protocol::{Message, MessageSegment, Signal};
use nethernet_tokio::signaling::caller::server::{
    CallerServerSignaling, CallerServerSignalingConfig, CallerServerSignalingDriver,
};
use nethernet_tokio::signaling::caller::{
    CallerSignaling, CallerSignalingConfig, CallerSignalingDriver, SignalDelivery, SignalRequest,
};
use nethernet_tokio::transport::{
    ClientIceConfig, ConnectionConfig, NetherClient, NetherServer, ServerIceConfig,
};
use std::{
    hint::black_box,
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const SIZES: [usize; 4] = [32, 512, 4096, 65536];
const SEGMENT_BYTES: usize = 16383;
const CANDIDATE: &str = "candidate:1 1 UDP 2130706431 127.0.0.1 50000 typ host generation 0 ufrag bench network-id 0 network-cost 0";
const OFFER: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na=setup:actpass\r\n";
const BENCH_DONE: &str = "BENCH-DONE";
const BENCH_DONE_LINE: &[u8] = b"\"BENCH-DONE\"\n";

pub fn micro(iterations: usize) -> Result<()> {
    ensure!(iterations > 0, "iterations must be positive");
    for size in [32, 512, 4096, 65536, 1048576] {
        let body = payload(size);
        let name = format!("message_{size}");
        let segments: Vec<_> = Message::encode_segments(&body, SEGMENT_BYTES)?.collect();
        let mut checked = Message::new();
        let mut result = None;
        for wire in &segments {
            result = checked.add_segment(MessageSegment::decode(wire.clone())?)?;
        }
        ensure!(result.as_deref() == Some(body.as_ref()), "framing mismatch");
        measure(
            "nethernet_framing",
            &name,
            "encode",
            "cinnabar",
            size,
            iterations,
            || {
                for segment in Message::encode_segments(black_box(&body), SEGMENT_BYTES).unwrap() {
                    black_box(segment);
                }
            },
        )?;
        measure(
            "nethernet_framing",
            &name,
            "reassemble",
            "cinnabar",
            size,
            iterations,
            || {
                let mut message = Message::new();
                for segment in &segments {
                    black_box(
                        message
                            .add_segment(MessageSegment::decode(segment.clone()).unwrap())
                            .unwrap(),
                    );
                }
            },
        )?;
    }
    measure(
        "nethernet_framing",
        "invalid_count",
        "reassemble",
        "cinnabar",
        1,
        iterations,
        || {
            let mut message = Message::new();
            black_box(
                message
                    .add_segment(MessageSegment::new(255, Bytes::from_static(b"x")))
                    .unwrap_err(),
            );
        },
    )?;
    measure(
        "nethernet_framing",
        "invalid_sequence",
        "reassemble",
        "cinnabar",
        2,
        iterations,
        || {
            let mut message = Message::new();
            message
                .add_segment(MessageSegment::new(2, Bytes::from_static(b"x")))
                .unwrap();
            black_box(
                message
                    .add_segment(MessageSegment::new(0, Bytes::from_static(b"x")))
                    .unwrap_err(),
            );
        },
    )?;
    let oversized = payload(SEGMENT_BYTES * 255 + 1);
    measure(
        "nethernet_framing",
        "oversize",
        "encode",
        "cinnabar",
        oversized.len(),
        iterations,
        || {
            assert!(black_box(Message::encode_segments(&oversized, SEGMENT_BYTES)).is_err());
        },
    )?;
    let answer = OFFER.replace("actpass", "active");
    for (name, kind, data) in [
        ("connect_request", "CONNECTREQUEST", OFFER),
        ("connect_response", "CONNECTRESPONSE", answer.as_str()),
        ("candidate_add", "CANDIDATEADD", CANDIDATE),
        ("connect_error", "CONNECTERROR", "2"),
    ] {
        let wire = format!("{kind} 42 {data}");
        let signal = Signal::from_string(&wire, "1".into())?;
        ensure!(signal.to_string() == wire, "signal roundtrip mismatch");
        measure(
            "nethernet_signaling",
            name,
            "parse",
            "cinnabar",
            wire.len(),
            iterations,
            || {
                black_box(Signal::from_string(black_box(&wire), "1".into()).unwrap());
            },
        )?;
        measure(
            "nethernet_signaling",
            name,
            "format",
            "cinnabar",
            wire.len(),
            iterations,
            || {
                black_box(signal.to_string());
            },
        )?;
    }
    let candidate = parse_ice_candidate(CANDIDATE)?;
    measure(
        "nethernet_signaling",
        "ice_candidate",
        "parse",
        "cinnabar",
        CANDIDATE.len(),
        iterations,
        || {
            black_box(parse_ice_candidate(black_box(CANDIDATE)).unwrap());
        },
    )?;
    measure(
        "nethernet_signaling",
        "ice_candidate",
        "format",
        "cinnabar",
        CANDIDATE.len(),
        iterations,
        || {
            black_box(format_ice_candidate(0, &candidate, "bench"));
        },
    )?;
    let packet = Packets::Message(MessagePacket::new(2, "CONNECTERROR 42 2".into()));
    let wire = discovery::encode(&packet, 1)?;
    let (decoded, sender) = discovery::decode(&wire)?;
    ensure!(
        sender == 1
            && matches!(&decoded, Packets::Message(m) if m.recipient_id == 2 && m.data == "CONNECTERROR 42 2"),
        "discovery mismatch"
    );
    measure(
        "nethernet_discovery",
        "packet",
        "encode",
        "cinnabar",
        wire.len(),
        iterations,
        || {
            black_box(discovery::encode(&packet, 1).unwrap());
        },
    )?;
    measure(
        "nethernet_discovery",
        "packet",
        "decode",
        "cinnabar",
        wire.len(),
        iterations,
        || {
            black_box(discovery::decode(black_box(&wire)).unwrap());
        },
    )?;
    crate::coverage(
        "nethernet_framing_configured_limit",
        "cinnabar",
        "unavailable",
        "Message::with_maximum is crate-private; public oversize row exercises the 255-segment encoding limit instead",
    );
    Ok(())
}

fn signaling_config() -> CallerSignalingConfig {
    CallerSignalingConfig {
        ice_mode: IceMode::Full,
        require_offer_receipt: false,
        ..Default::default()
    }
}
fn connection_config() -> ConnectionConfig {
    let bind = "127.0.0.1:0".parse().unwrap();
    ConnectionConfig {
        attempts: 1,
        allow_unsigned_answers: true,
        allow_anonymous_answers: true,
        token_trust: Some(nethernet::identity::TokenTrust::Any),
        allow_anonymous_offers: true,
        server_trust: Some(nethernet::identity::TokenTrust::Any),
        client_ice: Some(ClientIceConfig {
            turn_streams: Vec::new(),
            turn_tls: None,
            ice: Default::default(),
            bind_addr: Some(bind),
        }),
        server_ice: Some(ServerIceConfig {
            turn_streams: Vec::new(),
            turn_tls: None,
            ice: Default::default(),
            bind_addr: bind,
        }),
        ..Default::default()
    }
}

/// The Cinnabar caller-signaling driver behind one benchmark TCP bridge.
trait SignalDriver: Send + 'static {
    fn recv(&mut self) -> impl Future<Output = Option<SignalRequest>> + Send;
    fn deliver(&self, wire: &str) -> Result<()>;
}

impl SignalDriver for CallerSignalingDriver {
    fn recv(&mut self) -> impl Future<Output = Option<SignalRequest>> + Send {
        CallerSignalingDriver::recv(self)
    }
    fn deliver(&self, wire: &str) -> Result<()> {
        Ok(self.try_deliver("1", wire)?)
    }
}

impl SignalDriver for CallerServerSignalingDriver {
    fn recv(&mut self) -> impl Future<Output = Option<SignalRequest>> + Send {
        CallerServerSignalingDriver::recv(self)
    }
    fn deliver(&self, wire: &str) -> Result<()> {
        Ok(self.try_deliver("2", wire)?)
    }
}

// Dropping the owner aborts and drops the bridge driver and socket, including on errors.
struct Bridge {
    task: JoinHandle<Result<()>>,
    completion: oneshot::Receiver<()>,
    commands: mpsc::Sender<oneshot::Sender<()>>,
}

impl Bridge {
    /// Bridges `driver` over the benchmark TCP signaling socket.
    ///
    /// Two-phase BENCH-DONE, as the Rust and Go bridges: the task keeps
    /// reading and writing until this side has written its marker and seen the
    /// peer's. The driver ending is not completion (`NetherClient::connect_with`
    /// drops its signaling handle once connected, which ends the driver at an
    /// arbitrary time), and EOF before the peer's marker is an error.
    fn spawn(stream: TcpStream, mut driver: impl SignalDriver) -> Result<Self> {
        stream.set_nodelay(true)?;
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let (completed, completion) = oneshot::channel();
        let (commands, mut controls) = mpsc::channel::<oneshot::Sender<()>>(1);
        let task = tokio::spawn(async move {
            let mut completed = Some(completed);
            let mut driver_open = true;
            let mut reading = true;
            loop {
                tokio::select! {
                    command = controls.recv() => {
                        let Some(sent) = command else { break };
                        writer.write_all(BENCH_DONE_LINE).await?;
                        let _ = sent.send(());
                    }
                    request = driver.recv(), if driver_open => {
                        let Some(request) = request else {
                            driver_open = false;
                            continue;
                        };
                        let mut line = serde_json::to_vec(&request.signal().to_string())?;
                        line.push(b'\n');
                        writer.write_all(&line).await?;
                        request.finish(Ok(SignalDelivery::ServiceAccepted));
                    }
                    line = lines.next_line(), if reading => {
                        let Some(line) = line? else {
                            ensure!(completed.is_none(), "signaling closed before peer BENCH-DONE");
                            reading = false;
                            continue;
                        };
                        let wire: String = serde_json::from_str(&line)?;
                        if wire == BENCH_DONE {
                            if let Some(completed) = completed.take() {
                                let _ = completed.send(());
                            }
                            continue;
                        }
                        // Late signals after the driver ended have no recipient.
                        if driver_open {
                            driver.deliver(&wire)?;
                        }
                    }
                }
            }
            Ok(())
        });
        Ok(Self {
            task,
            completion,
            commands,
        })
    }

    async fn complete(&mut self, client: bool) -> Result<()> {
        let handshake = async {
            if !client {
                (&mut self.completion)
                    .await
                    .context("signaling closed before client BENCH-DONE")?;
            }
            let (sent, received) = oneshot::channel();
            self.commands
                .send(sent)
                .await
                .context("signaling bridge closed before sending BENCH-DONE")?;
            received.await.context("BENCH-DONE write failed")?;
            if client {
                (&mut self.completion)
                    .await
                    .context("signaling closed before server BENCH-DONE")?;
            }
            Ok(())
        };
        tokio::pin!(handshake);
        tokio::time::timeout(Duration::from_secs(60), async {
            tokio::select! {
                biased;
                result = &mut self.task => {
                    // The task only ends early on an error (EOF before the
                    // peer's marker included).
                    result.context("signaling bridge task failed")??;
                    handshake.await
                }
                result = &mut handshake => result,
            }
        })
        .await
        .context("BENCH-DONE handshake timed out")?
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn client_bridge(stream: TcpStream) -> Result<(CallerSignaling, Bridge)> {
    let (signaling, driver) = CallerSignaling::new("2".into(), "1".into(), signaling_config())?;
    Ok((signaling, Bridge::spawn(stream, driver)?))
}

async fn serve_connection(stream: TcpStream, size: usize, count: usize) -> Result<()> {
    let (signaling, driver) = CallerServerSignaling::new(
        "1".into(),
        CallerServerSignalingConfig {
            signaling: signaling_config(),
            ..Default::default()
        },
    )?;
    // Subscribe before admitting an offer from the socket.
    let mut server = NetherServer::bind_with(signaling, connection_config()).await?;
    let mut bridge = Bridge::spawn(stream, driver)?;
    let mut accepted = server.accept().await?;
    let expected = payload(size);
    for _ in 0..=count {
        let data = accepted
            .reliable
            .recv()
            .await?
            .context("peer closed before echo completed")?;
        ensure!(data == expected, "server echo payload mismatch");
        accepted.session.send(data).await?;
    }
    // The last echo send has drained before acknowledging the client's completion.
    bridge.complete(false).await?;
    drop(accepted.close());
    server.close().await?;
    Ok(())
}

fn rounds(size: usize, count: usize) -> Result<usize> {
    let configured = match std::env::var("BENCH_SPLIT_COUNT") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    message_count(size, count, configured.as_deref())
}

fn message_count(size: usize, count: usize, configured: Option<&str>) -> Result<usize> {
    let count = if size < 4096 {
        count
    } else {
        configured
            .map(str::parse)
            .transpose()
            .context("invalid BENCH_SPLIT_COUNT")?
            .unwrap_or(10)
    };
    ensure!(count > 0, "message count must be positive");
    Ok(count)
}

async fn serve(listener: TcpListener, count: usize) -> Result<()> {
    for size in SIZES {
        let (stream, _) = listener.accept().await?;
        tokio::time::timeout(
            Duration::from_secs(60),
            serve_connection(stream, size, rounds(size, count)?),
        )
        .await??;
    }
    Ok(())
}

/// Retirement drains the reliable channel to its end. Every echo was verified
/// and both BENCH-DONE markers were exchanged before it starts, so a peer that
/// closed first (go-nethernet and tokio-nethernet abort SCTP on close, as pion
/// does) ends it as well instead of failing the run.
fn retirement_finished<T>(received: nethernet_tokio::Result<Option<T>>) -> Result<bool> {
    match received {
        Ok(Some(_)) => Ok(false),
        Ok(None) | Err(nethernet_tokio::NetherError::ConnectionClosed) => Ok(true),
        Err(error) => Err(error.into()),
    }
}

fn emit(operation: &str, size: usize, samples: &mut [f64], total_ns: f64) -> Result<()> {
    samples.sort_by(f64::total_cmp);
    let ns = total_ns / samples.len() as f64;
    Row {
        area: "nethernet_loopback",
        name: &size.to_string(),
        operation,
        implementation: "cinnabar",
        ns_per_op: ns,
        allocs_per_op: None,
        bytes_per_op: None,
        payload_bytes: size,
        msgs_per_sec: 1e9 / ns,
        mb_per_sec: size as f64 * 1e3 / ns,
        p50_us: Some(samples[(samples.len() * 50).div_ceil(100) - 1] / 1000.0),
        p99_us: Some(
            samples[((samples.len() * 99).div_ceil(100) - 1).min(samples.len() - 1)] / 1000.0,
        ),
        max_us: Some(samples[samples.len() - 1] / 1000.0),
    }
    .emit()
}

async fn client(address: SocketAddr, count: usize) -> Result<()> {
    for size in SIZES {
        let body = payload(size);
        let start = Instant::now();
        let stream = TcpStream::connect(address).await?;
        let (signaling, mut bridge) = client_bridge(stream).await?;
        let mut client =
            NetherClient::connect_with(signaling, "1".into(), connection_config()).await?;
        let setup = start.elapsed().as_secs_f64() * 1e9;
        client.send(body.clone()).await?;
        ensure!(
            client.recv().await?.as_deref() == Some(body.as_ref()),
            "readiness echo mismatch"
        );
        let count = rounds(size, count)?;
        let mut samples = Vec::with_capacity(count);
        let total = Instant::now();
        for _ in 0..count {
            let start = Instant::now();
            client.send(body.clone()).await?;
            let received = client.recv().await?.context("peer closed before echo")?;
            ensure!(received == body, "echo payload mismatch");
            samples.push(start.elapsed().as_secs_f64() * 1e9);
        }
        let total_ns = total.elapsed().as_secs_f64() * 1e9;
        // Keep signaling and RTC alive until the server's final echo send has drained.
        bridge.complete(true).await?;
        let mut retired = client.close();
        while !retirement_finished(retired.reliable.recv().await)? {}
        drop(bridge);
        emit("setup", size, &mut [setup], setup)?;
        emit("echo", size, &mut samples, total_ns)?;
    }
    Ok(())
}

pub async fn run(mode: &str, address: &str, count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    let address: SocketAddr = address.parse()?;
    ensure!(
        address.ip().is_loopback(),
        "signaling endpoint must be loopback"
    );
    match mode {
        "serve" => {
            let listener = TcpListener::bind(address).await?;
            eprintln!("READY {}", listener.local_addr()?);
            serve(listener, count).await
        }
        "client" => tokio::time::timeout(Duration::from_secs(240), client(address, count)).await?,
        "loopback" => {
            let listener = TcpListener::bind(address).await?;
            let address = listener.local_addr()?;
            tokio::time::timeout(Duration::from_secs(240), async {
                tokio::try_join!(serve(listener, count), client(address, count))?;
                Ok(())
            })
            .await?
        }
        _ => anyhow::bail!("unknown NetherNet mode {mode}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Bridge, SignalDriver, SignalRequest, message_count, retirement_finished};
    use nethernet_tokio::NetherError;
    use tokio::net::{TcpListener, TcpStream};

    /// A driver whose signaling handle is already gone, as after
    /// `NetherClient::connect_with` returns.
    struct EndedDriver;

    impl SignalDriver for EndedDriver {
        async fn recv(&mut self) -> Option<SignalRequest> {
            None
        }
        fn deliver(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    async fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (client, server) = tokio::join!(TcpStream::connect(address), listener.accept());
        (client.unwrap(), server.unwrap().0)
    }

    /// Regression: the bridge task used to stop when the driver ended, so a
    /// marker arriving afterwards was lost ("signaling closed before server
    /// BENCH-DONE"). Both markers must still be exchanged.
    #[tokio::test]
    async fn markers_are_exchanged_after_the_signal_driver_ended() {
        let (client, server) = tcp_pair().await;
        let mut client = Bridge::spawn(client, EndedDriver).unwrap();
        let mut server = Bridge::spawn(server, EndedDriver).unwrap();
        // Let both tasks observe the ended driver before any marker.
        tokio::task::yield_now().await;
        let (client, server) = tokio::join!(client.complete(true), server.complete(false));
        client.unwrap();
        server.unwrap();
    }

    /// Regression: go-nethernet/tokio-nethernet servers abort SCTP right after
    /// their marker, so the Cinnabar client's retirement saw ConnectionClosed
    /// and failed cinnabar-to-go/rust interop after every echo had passed.
    #[test]
    fn retirement_ends_on_peer_close_but_not_on_other_errors() {
        assert!(!retirement_finished(Ok(Some(()))).unwrap());
        assert!(retirement_finished::<()>(Ok(None)).unwrap());
        assert!(retirement_finished::<()>(Err(NetherError::ConnectionClosed)).unwrap());
        assert!(
            retirement_finished::<()>(Err(NetherError::Io(std::io::ErrorKind::Other.into())))
                .is_err()
        );
    }

    /// EOF is never completion: a peer that closes without its marker fails
    /// the handshake.
    #[tokio::test]
    async fn eof_before_the_peer_marker_is_an_error() {
        let (client, server) = tcp_pair().await;
        let mut client = Bridge::spawn(client, EndedDriver).unwrap();
        drop(server);
        assert!(client.complete(true).await.is_err());
    }

    #[test]
    fn split_counts_match_other_peers_when_small_count_changes() {
        for small_count in [1, 20, 100, 200] {
            assert_eq!(message_count(32, small_count, None).unwrap(), small_count);
            for size in [4096, 65536] {
                assert_eq!(message_count(size, small_count, None).unwrap(), 10);
                assert_eq!(message_count(size, small_count, Some("7")).unwrap(), 7);
            }
        }
        assert!(message_count(4096, 20, Some("0")).is_err());
        assert!(message_count(4096, 20, Some("invalid")).is_err());
    }

    #[tokio::test]
    async fn local_caller_signaling_echoes_all_payload_boundaries_before_retirement() {
        // The actual client/server routines compare every reliable payload byte,
        // including readiness and the final 64 KiB echo before the marker handshake.
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            super::run("loopback", "127.0.0.1:0", 1),
        )
        .await
        .expect("local caller echo timed out")
        .expect("local caller echo or retirement failed");
    }
}
