//! Example: NetherNet Client
//!
//! This example demonstrates how to connect to a NetherNet server
//! and print received packets.
//!
//! Run with: `cargo run --example client`

use std::sync::Arc;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_nethernet::{Message, NetherNetStream, Signal, Signaling};

/// A simple mock signaling implementation for local testing.
/// In production, this would be backed by RakNet or WebSocket.
struct MockSignaling {
    network_id: String,
    outbound_tx: mpsc::Sender<Signal>,
}

#[async_trait::async_trait]
impl Signaling for MockSignaling {
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        println!("[SIGNAL OUT] {} -> {}", signal.typ, signal.network_id);
        let _ = self.outbound_tx.send(signal).await;
        Ok(())
    }

    fn network_id(&self) -> String {
        self.network_id.clone()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tokio_nethernet=debug".parse()?),
        )
        .init();

    println!("=== NetherNet Client Example ===\n");

    // Create signaling channels
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Signal>(100);
    let (_inbound_tx, _inbound_rx) = mpsc::channel::<Signal>(100);

    let signaling = Arc::new(MockSignaling {
        network_id: "client-67890".to_string(),
        outbound_tx,
    });

    // Log outgoing signals
    tokio::spawn(async move {
        while let Some(signal) = outbound_rx.recv().await {
            println!("[SIGNAL OUT] {} -> {}", signal.typ, signal.network_id);
        }
    });

    println!("Connecting to server: listener-12345...\n");

    // Connect to server
    let (mut stream, signal_pusher) =
        NetherNetStream::connect("listener-12345".to_string(), signaling).await?;

    println!("✓ Connected!\n");

    // Route incoming signals (in a real app)
    let _pusher = signal_pusher;

    // Send a test message
    println!("Sending test message...");
    stream
        .send(Message::reliable(Bytes::from("Hello from client!")))
        .await?;
    println!("✓ Message sent\n");

    // Read responses
    println!("Waiting for responses...\n");
    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                let channel = if msg.reliable {
                    "reliable"
                } else {
                    "unreliable"
                };
                println!(
                    "[RECV {}] {} bytes: {:?}",
                    channel,
                    msg.buffer.len(),
                    String::from_utf8_lossy(&msg.buffer)
                );
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    println!("Disconnected.");
    Ok(())
}
