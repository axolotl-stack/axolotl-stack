//! End-to-end jolyne join + echo over RakNet loopback.
//!
//! One process runs a jolyne listener and a jolyne client. Each round dials,
//! completes the full join (settings, offline login, ECDH encryption, packs,
//! StartGame, spawn) and then echoes `count` Text packets through the
//! server's typed receive/send path. Rows: `join/raknet/join` (dial → play)
//! and `join/raknet/echo` (one Text round trip). The first round is a
//! discarded warm-up.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use jolyne::config::CompressionConfig;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::valentine::{McpePacket, McpePacketData, TextPacket};
use jolyne::{BedrockListener, BedrockListenerConfig, BedrockStream, WorldTemplate};
use p384::SecretKey;
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(30);

fn config() -> BedrockListenerConfig {
    BedrockListenerConfig {
        online_mode: false,
        compression: CompressionConfig {
            threshold: 256,
            ..CompressionConfig::default()
        },
        ..BedrockListenerConfig::default()
    }
}

async fn serve(
    mut listener: BedrockListener<tokio_raknet::transport::RaknetListener>,
) -> Result<()> {
    let template = Arc::new(WorldTemplate::default());
    let key = SecretKey::random(&mut rand::thread_rng());
    loop {
        let stream = listener.accept().await?;
        let (template, key) = (template.clone(), key.clone());
        tokio::spawn(async move {
            let Ok((mut play, _)) = stream.accept_join_sequence(&template, &key).await else {
                return;
            };
            while let Ok(packet) = play.recv_packet().await {
                if matches!(packet.data, McpePacketData::TextPacket(_))
                    && play.send_packet(packet).await.is_err()
                {
                    return;
                }
            }
        });
    }
}

fn percentile(samples: &[f64], percent: usize) -> f64 {
    samples[(samples.len() * percent)
        .div_ceil(100)
        .saturating_sub(1)
        .min(samples.len() - 1)]
}

fn emit(operation: &str, mut samples: Vec<f64>) -> Result<()> {
    samples.sort_unstable_by(f64::total_cmp);
    let mean_us = samples.iter().sum::<f64>() / samples.len() as f64;
    super::Row {
        area: "join",
        name: "raknet",
        operation,
        implementation: "rust-owned",
        ns_per_op: mean_us * 1e3,
        allocs_per_op: None,
        bytes_per_op: None,
        payload_bytes: 0,
        msgs_per_sec: 1e6 / mean_us,
        mb_per_sec: 0.0,
        p50_us: Some(percentile(&samples, 50)),
        p99_us: Some(percentile(&samples, 99)),
        max_us: samples.last().copied(),
    }
    .emit()
}

/// Runs `joins` measured joins, each followed by `count` echoes.
pub async fn loopback(joins: usize, count: usize) -> Result<()> {
    ensure!(joins > 0 && count > 0, "joins and count must be positive");
    let listener = BedrockListener::raknet()
        .addr("127.0.0.1:0")
        .config(config())
        .bind()
        .await?;
    let addr: SocketAddr = listener.local_addr().context("listener address")?;
    let server = tokio::spawn(serve(listener));
    let text = McpePacket::from(TextPacket::default());
    let mut join_samples = Vec::with_capacity(joins);
    let mut echo_samples = Vec::with_capacity(joins * count);
    for round in 0..=joins {
        let started = Instant::now();
        let stream = timeout(TIMEOUT, BedrockStream::connect(addr)).await??;
        let (mut play, _) = timeout(
            TIMEOUT,
            stream.join(ClientHandshakeConfig::random(addr, "Bench")),
        )
        .await??;
        let joined = started.elapsed();
        for _ in 0..count {
            let tick = Instant::now();
            play.send_packet(text.clone()).await?;
            loop {
                let packet = timeout(TIMEOUT, play.recv_packet()).await??;
                if matches!(packet.data, McpePacketData::TextPacket(_)) {
                    break;
                }
            }
            if round > 0 {
                echo_samples.push(tick.elapsed().as_secs_f64() * 1e6);
            }
        }
        if round > 0 {
            join_samples.push(joined.as_secs_f64() * 1e6);
        }
    }
    server.abort();
    emit("join", join_samples)?;
    emit("echo", echo_samples)
}
