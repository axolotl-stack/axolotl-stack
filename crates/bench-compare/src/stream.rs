//! Jolyne's whole stream path without a network: each batch fixture is sent
//! through a `BedrockTransport` over a self-loop transport (send queue = receive
//! queue) and every packet is received again. Rows (`area = "stream"`):
//!
//! - `raw_deflate_encrypted`: `send_batch_raw` + `recv_packet_raw` per packet
//!   with DEFLATE 6 (threshold 256) and AES-CTR, the proxy path;
//! - `typed_deflate_encrypted`: `send_batch` of owned packets + owned
//!   `recv_packet` decode, the server path.
//!
//! Both directions use one connection's send and receive ciphers, which start
//! from the same key and IV, so the stream decrypts its own output.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::{Result, ensure};
use bytes::Bytes;
use jolyne::config::ConnectionOptions;
use jolyne::proto::batch::{BatchCompression, decode_batch_raw};
use jolyne::stream::BedrockTransport;
use jolyne::stream::transport::{Datagram, Framing, Transmit, Transport};
use jolyne::valentine::McpePacket;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug)]
struct Never;

impl std::fmt::Display for Never {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unreachable")
    }
}

impl std::error::Error for Never {}

#[derive(Default)]
struct SelfLoop {
    queue: VecDeque<Bytes>,
}

impl Transport for SelfLoop {
    type Error = Never;
    const FRAMING: Framing = Framing::RakNet;

    fn poll_send_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Never>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(mut self: Pin<&mut Self>, message: Transmit) -> Result<(), Never> {
        self.queue.push_back(message.payload);
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Never>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Never>> {
        Poll::Ready(Ok(()))
    }
    fn poll_recv(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Datagram, Never>>> {
        Poll::Ready(
            self.queue
                .pop_front()
                .map(|bytes| Ok(Datagram::Contiguous(bytes))),
        )
    }
    fn peer_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
    }
}

#[derive(Deserialize)]
struct Fixture {
    name: String,
    file: String,
    set: String,
}

fn link() -> Result<BedrockTransport<SelfLoop>> {
    link_with(256, true)
}

fn link_with(threshold: u16, encrypted: bool) -> Result<BedrockTransport<SelfLoop>> {
    let mut link = BedrockTransport::new(
        SelfLoop::default(),
        ConnectionOptions {
            unknown_packets: jolyne::raw::UnknownPacketPolicy::Passthrough,
            max_packets_per_outbound_batch: 4096,
            ..ConnectionOptions::default()
        },
    );
    link.connection_mut()
        .enable_compression(BatchCompression::Deflate, 6, threshold)?;
    if encrypted {
        link.connection_mut()
            .enable_encryption([0x42; 32], [0x42; 12])?;
    }
    Ok(link)
}

pub fn run(iterations: usize) -> Result<()> {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../valentine_gen/protocolgen/testdata/bench");
    let manifest: Vec<Fixture> =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
    for fixture in manifest.into_iter().filter(|entry| entry.set == "batch") {
        let mut batch = Bytes::from(std::fs::read(dir.join(&fixture.file))?);
        let payload = batch.len();
        let raw = decode_batch_raw(&mut batch, false, usize::MAX)?;
        let typed: Vec<McpePacket> = raw
            .iter()
            .cloned()
            .map(|packet| packet.decode())
            .collect::<Result<_, _>>()?;
        let count = raw.len();

        for (operation, threshold, encrypted) in [
            ("raw_deflate", 256, false),
            ("raw_encrypted", u16::MAX, true),
        ] {
            let mut stream = link_with(threshold, encrypted)?;
            super::measure(
                "stream",
                &fixture.name,
                operation,
                "rust-owned",
                payload,
                iterations,
                || {
                    futures::executor::block_on(async {
                        stream.send_batch_raw(&raw, true).await.unwrap();
                        for _ in 0..count {
                            std::hint::black_box(stream.recv_packet_raw().await.unwrap());
                        }
                    })
                },
            )?;
        }

        let mut stream = link()?;
        super::measure(
            "stream",
            &fixture.name,
            "raw_deflate_encrypted",
            "rust-owned",
            payload,
            iterations,
            || {
                futures::executor::block_on(async {
                    stream.send_batch_raw(&raw, true).await.unwrap();
                    for _ in 0..count {
                        std::hint::black_box(stream.recv_packet_raw().await.unwrap());
                    }
                })
            },
        )?;

        let mut stream = link()?;
        let mut received = 0;
        super::measure(
            "stream",
            &fixture.name,
            "typed_deflate_encrypted",
            "rust-owned",
            payload,
            iterations,
            || {
                futures::executor::block_on(async {
                    stream.send_batch(&typed, true).await.unwrap();
                    for _ in 0..count {
                        std::hint::black_box(stream.recv_packet().await.unwrap());
                        received += 1;
                    }
                })
            },
        )?;
        ensure!(received > 0, "no packets received");
    }
    Ok(())
}
