//! Example: NetherNet Listener (Server)
//!
//! This example demonstrates how to create a NetherNet server that accepts
//! incoming connections and prints received packets.
//!
//! Run with: `cargo run --example listener`

use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig, Signal, Signaling};

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

    println!("=== NetherNet Listener Example ===\n");

    // Create signaling channels
    // In a real app, these would be connected to your signaling transport
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Signal>(100);
    let (_inbound_tx, mut inbound_rx) = mpsc::channel::<Signal>(100);

    let signaling = Arc::new(MockSignaling {
        network_id: "listener-12345".to_string(),
        outbound_tx,
    });

    // Create the listener
    let (mut listener, signal_pusher) =
        NetherNetListener::new(signaling, NetherNetListenerConfig::default());

    println!("Listener created with network_id: listener-12345");
    println!("Waiting for incoming connections...\n");

    // Route incoming signals to the listener
    let pusher = signal_pusher.clone();
    tokio::spawn(async move {
        while let Some(signal) = inbound_rx.recv().await {
            println!("[SIGNAL IN] {} from {}", signal.typ, signal.network_id);
            let _ = pusher.send(signal).await;
        }
    });

    // Log outgoing signals (in a real app, send these over the network)
    tokio::spawn(async move {
        while let Some(signal) = outbound_rx.recv().await {
            println!("[SIGNAL OUT] {} -> {}", signal.typ, signal.network_id);
            // In production: send signal over network to remote peer
        }
    });

    // Accept connections
    loop {
        match listener.accept().await {
            Ok(mut stream) => {
                println!("\n✓ New connection established!");

                tokio::spawn(async move {
                    println!("Reading packets from stream...\n");

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
                                eprintln!("Error receiving message: {}", e);
                                break;
                            }
                        }
                    }

                    println!("Connection closed.");
                });
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
