//! Network module - connection handling and sessions.

pub mod events;

use jolyne::BedrockStream;
use jolyne::stream::{Play, Server as ServerRole};
use jolyne::valentine::McpePacket;
use tokio::sync::{broadcast, mpsc};
use tracing::{info, trace, warn};

// Re-export types
pub use events::{NetworkEvent, SessionId};

/// Network loop: shuttle packets between network and main thread.
///
/// Runs a connected player's play-state transport until disconnect.
/// The caller owns the task lifecycle; this keeps handshake and play
/// handling in one connection task instead of bouncing through another spawn.
///
/// Uses manual flushing for efficient batching:
/// - `send_packet()` queues packets without sending
/// - `flush()` sends all queued packets as a single batch on tick
pub async fn run_network_loop(
    stream: BedrockStream<Play, ServerRole, jolyne::stream::transport::RakNetTransport>,
    session_id: SessionId,
    event_tx: mpsc::Sender<NetworkEvent>,
    mut outbound_rx: mpsc::Receiver<McpePacket>,
    mut tick_rx: broadcast::Receiver<()>,
) {
    let mut stream = stream;
    loop {
        tokio::select! {
            biased;

            // Priority 1: Tick signal - flush all buffered packets
            result = tick_rx.recv() => {
                match result {
                    Ok(()) => {
                        // Drain any remaining packets and queue them
                        while let Ok(packet) = outbound_rx.try_recv() {
                            if let Err(e) = stream.send_packet(packet).await {
                                tracing::error!(session_id, "Send failed (tick flush): {:?}", e);
                                return;
                            }
                        }
                        // Flush all queued packets as a single batch
                        if let Err(e) = stream.flush().await {
                            tracing::error!(session_id, "Flush failed: {:?}", e);
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        trace!(session_id, lagged = n, "Tick receiver lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Server shutting down
                        tracing::error!(session_id, "Tick receiver closed - server shutdown or sender dropped");
                        break;
                    }
                }
            }

            // Priority 2: Inbound packets from client
            result = stream.recv_packet_borrowed() => {
                match result {
                    Ok(packet) => {
                        let packet_args = stream.packet_args();
                        if let Err(e) = event_tx.try_send(NetworkEvent::Packet {
                            session_id,
                            packet_args,
                            packet: Box::new(packet),
                        }) {
                            match e {
                                mpsc::error::TrySendError::Full(_) => {
                                    tracing::warn!(
                                        session_id,
                                        "Inbound event channel full; closing noisy connection"
                                    );
                                }
                                mpsc::error::TrySendError::Closed(_) => {
                                    tracing::error!(session_id, "Main thread dropped event channel");
                                }
                            }
                            break;
                        }
                    }
                    Err(e) => {
                        // Log decode errors with more context
                        if let jolyne::JolyneError::Decode(decode_err) = &e {
                            tracing::error!(
                                session_id,
                                error = ?decode_err,
                                "Packet decode failed - connection closed. This may indicate a malformed packet from the client or a protocol mismatch."
                            );
                        } else {
                            tracing::error!(session_id, "Connection closed by client/error: {:?}", e);
                        }
                        break;
                    }
                }
            }

            // Priority 3: Queue outbound packets (batched flush on tick signal)
            Some(packet) = outbound_rx.recv() => {
                if let Err(e) = stream.send_packet(packet).await {
                    tracing::error!(session_id, "Send failed (immediate): {:?}", e);
                    break;
                }
                // Drain any other pending packets into buffer
                while let Ok(p) = outbound_rx.try_recv() {
                    if let Err(e) = stream.send_packet(p).await {
                        warn!(session_id, "Send failed: {:?}", e);
                        return;
                    }
                }
                // NO flush here - packets accumulate until tick signal for efficient batching
                // This reduces compression operations from N per tick to 1 per tick
            }
        }
    }

    // Final flush on disconnect
    let _ = stream.flush().await;
    let _ = event_tx
        .send(NetworkEvent::Disconnected { session_id })
        .await;
    info!(session_id, "Network task ended");
}
