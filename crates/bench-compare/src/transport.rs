use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::time::timeout;
use tokio_raknet::{
    RaknetListener, RaknetStream,
    transport::{Message, RaknetListenerConfig, RaknetStreamConfig, TransmissionPolicy},
};

pub const SIZES: [usize; 4] = [32, 512, 4096, 65536];
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn payload(size: usize) -> Bytes {
    let mut bytes: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
    bytes[0] = 0xfe;
    Bytes::from(bytes)
}

fn count_for_size(size: usize, count: usize) -> Result<usize> {
    if size < 4096 {
        return Ok(count);
    }
    match std::env::var("BENCH_SPLIT_COUNT") {
        Ok(value) => {
            let count = value
                .parse()
                .context("BENCH_SPLIT_COUNT must be an integer")?;
            ensure!(count > 0, "BENCH_SPLIT_COUNT must be positive");
            Ok(count)
        }
        Err(std::env::VarError::NotPresent) => Ok(count),
        Err(error) => Err(error.into()),
    }
}

/// `BENCH_RAKNET_POLICY=vanilla|optimized` selects the transmission policy of
/// both peers. Unset keeps the library defaults (Vanilla client, Optimized
/// listener).
fn policy_override() -> Result<Option<TransmissionPolicy>> {
    match std::env::var("BENCH_RAKNET_POLICY") {
        Ok(value) => match value.as_str() {
            "vanilla" => Ok(Some(TransmissionPolicy::Vanilla)),
            "optimized" => Ok(Some(TransmissionPolicy::Optimized)),
            _ => anyhow::bail!("BENCH_RAKNET_POLICY must be `vanilla` or `optimized`"),
        },
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn bind(address: SocketAddr) -> Result<RaknetListener> {
    let mut config = RaknetListenerConfig::default();
    if let Some(policy) = policy_override()? {
        config.transmission_policy = policy;
    }
    Ok(RaknetListener::bind_with_config(address, config).await?)
}

async fn connect(address: SocketAddr) -> Result<RaknetStream> {
    let mut config = RaknetStreamConfig::default();
    if let Some(policy) = policy_override()? {
        config.transmission_policy = policy;
    }
    Ok(RaknetStream::connect_with_config(address, config).await?)
}

pub async fn echo(mut listener: RaknetListener, count: usize) -> Result<()> {
    for size in SIZES {
        let count = count_for_size(size, count)?;
        let mut stream = timeout(TIMEOUT, listener.accept())
            .await
            .context("accept timeout")?
            .context("listener closed")?;
        let expected = payload(size);
        for _ in 0..=count {
            let received = timeout(TIMEOUT, stream.recv_message())
                .await
                .context("server receive timeout")?
                .context("peer closed")??;
            ensure!(
                received.id == expected[0] && received.payload.as_ref() == &expected[1..],
                "server payload mismatch ({size})"
            );
            timeout(TIMEOUT, stream.send_encoded(Message::new(expected.clone())))
                .await
                .context("server send timeout")??;
        }
        // Keep the session alive until the client has received the final echo
        // and closed it; dropping immediately can cancel queued transmission.
        let closed = timeout(TIMEOUT, stream.recv_message())
            .await
            .context("server close timeout")?;
        ensure!(
            closed.is_none() || matches!(closed, Some(Err(_))),
            "unexpected extra application message"
        );
    }
    Ok(())
}

pub async fn serve(address: SocketAddr, count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    let listener = timeout(TIMEOUT, bind(address)).await??;
    eprintln!("READY {}", listener.local_addr());
    echo(listener, count).await
}

pub async fn client(address: SocketAddr, count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    for size in SIZES {
        let count = count_for_size(size, count)?;
        // `setup`: the complete RakNet handshake (MTU discovery, Request2,
        // ConnectionRequest/Accepted, NewIncomingConnection) until the stream
        // is connected; one sample per fresh connection.
        let setup_started = Instant::now();
        let mut stream = timeout(TIMEOUT, connect(address))
            .await
            .context("connect timeout")??;
        let setup_seconds = setup_started.elapsed().as_secs_f64();
        let payload = payload(size);
        let mut samples = Vec::with_capacity(count);
        let mut started = Instant::now();
        for index in 0..=count {
            let tick = Instant::now();
            timeout(TIMEOUT, stream.send_encoded(Message::new(payload.clone())))
                .await
                .context("client send timeout")??;
            let reply = timeout(TIMEOUT, stream.recv_message())
                .await
                .context("client receive timeout")?
                .context("server closed")??;
            ensure!(
                reply.id == payload[0] && reply.payload.as_ref() == &payload[1..],
                "echo mismatch ({size})"
            );
            // Exclude the readiness echo and any prior connection teardown
            // from both throughput and per-message latency samples.
            if index == 0 {
                started = Instant::now();
                continue;
            }
            samples.push(tick.elapsed().as_secs_f64() * 1e6);
        }
        let seconds = started.elapsed().as_secs_f64();
        samples.sort_unstable_by(f64::total_cmp);
        let percentile = |percent: usize| {
            samples[(count * percent)
                .div_ceil(100)
                .saturating_sub(1)
                .min(count - 1)]
        };
        super::Row {
            area: "loopback",
            name: &size.to_string(),
            operation: "echo",
            implementation: "rust-owned",
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
        .emit()?;
        super::Row {
            area: "loopback",
            name: &size.to_string(),
            operation: "setup",
            implementation: "rust-owned",
            ns_per_op: setup_seconds * 1e9,
            allocs_per_op: None,
            bytes_per_op: None,
            payload_bytes: size,
            msgs_per_sec: 1.0 / setup_seconds,
            mb_per_sec: 0.0,
            p50_us: None,
            p99_us: None,
            max_us: None,
        }
        .emit()?;
        drop(stream);
    }
    Ok(())
}

pub async fn loopback(count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    let listener = bind("127.0.0.1:0".parse()?).await?;
    let address = listener.local_addr();
    tokio::try_join!(echo(listener, count), client(address, count))?;
    Ok(())
}
