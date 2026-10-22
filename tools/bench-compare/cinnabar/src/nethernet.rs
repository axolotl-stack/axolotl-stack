//! Measurement-only use of the pinned Apache-2.0 NetherNet public APIs.
use crate::{Row, measure, payload};
use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use nethernet::connection::IceMode;
use nethernet::protocol::packet::discovery::{self, MessagePacket, Packets};
use nethernet::protocol::webrtc::candidate::{format_ice_candidate, parse_ice_candidate};
use nethernet::protocol::{Message, MessageSegment, Signal};
use nethernet_tokio::signaling::caller::server::{
    CallerServerSignaling, CallerServerSignalingConfig,
};
use nethernet_tokio::signaling::caller::{CallerSignaling, CallerSignalingConfig, SignalDelivery};
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
use tokio::task::JoinHandle;

const SIZES: [usize; 4] = [32, 512, 4096, 65536];
const SEGMENT_BYTES: usize = 16383;
const CANDIDATE: &str = "candidate:1 1 UDP 2130706431 127.0.0.1 50000 typ host generation 0 ufrag bench network-id 0 network-cost 0";
const OFFER: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na=setup:actpass\r\n";

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

// Dropping the owner aborts and drops the bridge driver and socket, including on errors.
struct Bridge(JoinHandle<Result<()>>);
impl Drop for Bridge {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn client_bridge(stream: TcpStream) -> Result<(CallerSignaling, Bridge)> {
    stream.set_nodelay(true)?;
    let (signaling, mut driver) = CallerSignaling::new("2".into(), "1".into(), signaling_config())?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                request = driver.recv() => {
                    let Some(request) = request else { break };
                    let mut line = serde_json::to_vec(&request.signal().to_string())?;
                    line.push(b'\n');
                    writer.write_all(&line).await?;
                    request.finish(Ok(SignalDelivery::ServiceAccepted));
                }
                line = lines.next_line() => {
                    let Some(line) = line? else { break };
                    let wire: String = serde_json::from_str(&line)?;
                    driver.try_deliver("1", &wire)?;
                }
            }
        }
        Ok(())
    });
    Ok((signaling, Bridge(task)))
}

async fn serve_connection(stream: TcpStream, size: usize, count: usize) -> Result<()> {
    stream.set_nodelay(true)?;
    let (signaling, mut driver) = CallerServerSignaling::new(
        "1".into(),
        CallerServerSignalingConfig {
            signaling: signaling_config(),
            ..Default::default()
        },
    )?;
    // Subscribe before admitting an offer from the socket.
    let mut server = NetherServer::bind_with(signaling, connection_config()).await?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut bridge = Bridge(tokio::spawn(async move {
        loop {
            tokio::select! {
                request = driver.recv() => {
                    let Some(request) = request else { break };
                    let mut line = serde_json::to_vec(&request.signal().to_string())?;
                    line.push(b'\n');
                    writer.write_all(&line).await?;
                    request.finish(Ok(SignalDelivery::ServiceAccepted));
                }
                line = lines.next_line() => {
                    let Some(line) = line? else { break };
                    let wire: String = serde_json::from_str(&line)?;
                    driver.try_deliver("2", &wire)?;
                }
            }
        }
        Ok(())
    }));
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
    // Signaling EOF acknowledges the client's validated final echo, outside timing.
    (&mut bridge.0).await??;
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
        let (signaling, bridge) = client_bridge(stream).await?;
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
        // Close the signaling socket only after validating the final received payload.
        drop(bridge);
        let mut retired = client.close();
        while retired.reliable.recv().await?.is_some() {}
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
    use super::message_count;

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
}
