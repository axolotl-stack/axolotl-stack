use crate::Row;
use anyhow::{Context, Result, bail, ensure};
use raknet_tokio::prelude::{
    Established, RakClient, RakPriority, RakReliability, RakServer, RakSession, RakSessionError,
    RunningServer,
};
use std::{
    error::Error,
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant},
};
use tokio::time::timeout;

const SIZES: [usize; 4] = [32, 512, 4096, 65536];
const TIMEOUT: Duration = Duration::from_secs(30);

fn payload(size: usize) -> Box<[u8]> {
    let mut bytes: Box<[u8]> = (0..size).map(|index| (index % 251) as u8).collect();
    bytes[0] = 0xfe;
    bytes
}

fn count_for_size(size: usize, count: usize) -> Result<usize> {
    if size < 4096 {
        return Ok(count);
    }
    match std::env::var("BENCH_SPLIT_COUNT") {
        Ok(value) => {
            let count = value
                .parse::<usize>()
                .context("BENCH_SPLIT_COUNT must be an integer")?;
            ensure!(count > 0, "BENCH_SPLIT_COUNT must be positive");
            Ok(count)
        }
        Err(std::env::VarError::NotPresent) => Ok(count),
        Err(error) => Err(error.into()),
    }
}

async fn send(session: &RakSession<Established>, bytes: Box<[u8]>) -> Result<()> {
    // send admits and drives the automatic-flush actor; it is not a delivery ACK.
    timeout(
        TIMEOUT,
        session.send(bytes, RakReliability::ReliableOrdered, RakPriority::Normal),
    )
    .await
    .context("send timeout")??;
    Ok(())
}

async fn echo(server: &mut RakServer<RunningServer>, count: usize) -> Result<()> {
    for size in SIZES {
        let count = count_for_size(size, count)?;
        let mut session = timeout(TIMEOUT, server.accept())
            .await
            .context("server accept timeout")??;
        let expected = payload(size);
        for index in 0..=count {
            let received = timeout(TIMEOUT, session.recv::<Box<[u8]>>())
                .await
                .context("server receive timeout")??;
            ensure!(
                received == expected,
                "server payload mismatch: size {size}, message {index}"
            );
            send(&session, received).await?;
        }
        // Keep the owner alive until the peer has read the final echo and closed.
        // Closing immediately after send could cut off queued reliable data.
        match timeout(TIMEOUT, session.recv::<Box<[u8]>>())
            .await
            .context("server close timeout")?
        {
            Err(RakSessionError::Closed) => {}
            Err(error) => return Err(error.into()),
            Ok(_) => bail!("unexpected application message after final echo ({size})"),
        }
    }
    Ok(())
}

async fn exchange(session: RakSession<Established>, size: usize, count: usize) -> Result<()> {
    let mut session = session;
    let expected = payload(size);
    let mut samples = Vec::with_capacity(count);
    let mut started = Instant::now();
    for index in 0..=count {
        let tick = Instant::now();
        // The public API consumes Box<[u8]>; copying here is its required ownership cost.
        send(&session, expected.clone()).await?;
        let received = timeout(TIMEOUT, session.recv::<Box<[u8]>>())
            .await
            .context("client receive timeout")??;
        ensure!(
            received == expected,
            "echo payload mismatch: size {size}, message {index}"
        );
        if index == 0 {
            started = Instant::now();
        } else {
            samples.push(tick.elapsed().as_secs_f64() * 1e6);
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    samples.sort_unstable_by(f64::total_cmp);
    let percentile = |percent: usize| {
        samples[(count * percent)
            .div_ceil(100)
            .saturating_sub(1)
            .min(count - 1)]
    };
    // Closing consumes send permission; draining recv waits for the actor's terminal
    // state while the RakClient socket owner remains alive. Neither is timed above.
    let mut closing = session.close()?;
    match timeout(TIMEOUT, closing.recv::<Box<[u8]>>())
        .await
        .context("client close timeout")?
    {
        Err(RakSessionError::Closed) => {}
        Err(error) => return Err(error.into()),
        Ok(_) => bail!("unexpected application message during client close ({size})"),
    }
    Row {
        area: "loopback",
        name: &size.to_string(),
        operation: "echo",
        implementation: "cinnabar",
        ns_per_op: seconds * 1e9 / count as f64,
        allocs_per_op: None,
        bytes_per_op: None,
        payload_bytes: size,
        msgs_per_sec: count as f64 / seconds,
        mb_per_sec: count as f64 * size as f64 / seconds / 1e6,
        p50_us: Some(percentile(50)),
        p99_us: Some(percentile(99)),
        max_us: samples.last().copied(),
    }
    .emit()?;
    Ok(())
}

async fn client(address: SocketAddr, count: usize) -> Result<()> {
    for size in SIZES {
        let count = count_for_size(size, count)?;
        // A fresh socket owner as well as session prevents cross-size state reuse.
        let client = timeout(TIMEOUT, RakClient::new(|_| {}).start())
            .await
            .context("client start timeout")??;
        let result = async {
            let session = timeout(TIMEOUT, client.connect(address))
                .await
                .context("client connect timeout")??;
            exchange(session, size, count).await
        }
        .await;
        let stopped = timeout(TIMEOUT, client.stop()).await;
        result.with_context(|| format!("client size {size}"))?;
        stopped.context("client stop timeout")?;
    }
    Ok(())
}

async fn start_server(address: SocketAddr) -> Result<RakServer<RunningServer>> {
    Ok(timeout(
        TIMEOUT,
        RakServer::new(address, |config| {
            config.max_connections = 1;
        })
        .start(),
    )
    .await
    .context("server start timeout")??)
}

pub async fn run(mode: &str, address: &str, count: usize) -> Result<(), Box<dyn Error>> {
    run_inner(mode, address, count).await.map_err(Into::into)
}

async fn run_inner(mode: &str, address: &str, count: usize) -> Result<()> {
    ensure!(count > 0, "count must be positive");
    // Validate split counts before starting either endpoint.
    count_for_size(4096, count)?;
    match mode {
        "client" => {
            let address = timeout(TIMEOUT, tokio::net::lookup_host(address))
                .await
                .context("address resolution timeout")??
                .next()
                .context("address resolved to no endpoints")?;
            client(address, count).await?;
        }
        "serve" | "loopback" => {
            let address = if mode == "loopback" {
                // Upstream exposes neither local_addr nor socket adoption. Reserve
                // an ephemeral address, then bind it through its public constructor.
                // A competing bind is an explicit start error, never a silent retry.
                let reservation = UdpSocket::bind("127.0.0.1:0")?;
                reservation.local_addr()?
            } else {
                timeout(TIMEOUT, tokio::net::lookup_host(address))
                    .await
                    .context("address resolution timeout")??
                    .next()
                    .context("address resolved to no endpoints")?
            };
            let mut server = start_server(address).await?;
            let result = if mode == "serve" {
                eprintln!("READY {address}");
                echo(&mut server, count).await
            } else {
                tokio::try_join!(echo(&mut server, count), client(address, count)).map(|_| ())
            };
            let stopped = timeout(TIMEOUT, server.stop()).await;
            result?;
            stopped.context("server stop timeout")?;
        }
        _ => bail!("unknown transport mode: {mode}"),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_matches_cross_implementation_contract() {
        for size in SIZES {
            let bytes = payload(size);
            assert_eq!(bytes.len(), size);
            assert_eq!(bytes[0], 0xfe);
            for (index, byte) in bytes.iter().enumerate().skip(1) {
                assert_eq!(*byte, (index % 251) as u8);
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn loopback_roundtrips_all_sizes() {
        run("loopback", "127.0.0.1:0", 2).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_zero_count_and_unknown_mode() {
        assert!(run("loopback", "127.0.0.1:0", 0).await.is_err());
        assert!(run("unknown", "127.0.0.1:0", 1).await.is_err());
    }
}
