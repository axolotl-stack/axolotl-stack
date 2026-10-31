//! Network module - connection handling and sessions.

pub mod events;

use jolyne::BedrockStream;
use jolyne::stream::{Play, Server as ServerRole};
use jolyne::valentine::{EnumsPacketViolationSeverity, McpePacket, McpePacketArgs, McpePacketData};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, trace, warn};

// Re-export types
pub use events::{NetworkEvent, SessionId};

fn terminates_connection(packet: &McpePacket) -> bool {
    match &packet.data {
        McpePacketData::DisconnectPacket(_) => true,
        McpePacketData::PacketViolationWarningPacket(warning) => {
            warning.violation_severity == EnumsPacketViolationSeverity::Terminatingconnection
        }
        _ => false,
    }
}

/// Network loop: shuttle packets between network and main thread.
///
/// Runs a connected player's play-state transport until disconnect.
/// The caller owns the task lifecycle; this keeps handshake and play
/// handling in one connection task instead of bouncing through another spawn.
///
/// Uses manual flushing for efficient batching:
/// - `send_packet()` queues packets without sending
/// - `flush()` sends all queued packets as a single batch on tick
pub async fn run_network_loop<T: jolyne::stream::Transport>(
    stream: BedrockStream<Play, ServerRole, T>,
    session_id: SessionId,
    event_tx: mpsc::Sender<NetworkEvent>,
    outbound_rx: mpsc::Receiver<McpePacket>,
    tick_rx: broadcast::Receiver<()>,
) {
    run_network_loop_with_metrics(stream, session_id, event_tx, outbound_rx, tick_rx, None).await;
}

/// Run the same play pump with optional read-only transport observation attached.
pub(crate) async fn run_network_loop_with_metrics<T: jolyne::stream::Transport>(
    stream: BedrockStream<Play, ServerRole, T>,
    session_id: SessionId,
    event_tx: mpsc::Sender<NetworkEvent>,
    mut outbound_rx: mpsc::Receiver<McpePacket>,
    mut tick_rx: broadcast::Receiver<()>,
    metrics: Option<crate::entity::components::network_metrics::NetworkMetricsPublisher>,
) {
    let mut stream = stream;
    if let Some(metrics) = &metrics {
        metrics.observe(stream.latency(), std::time::Instant::now());
    }
    // An inbound failure the client should be told about (e.g. a streamed
    // spawn's `Timeout(Spawn)` or runtime ID mismatch).
    let mut failure = None;
    'network: loop {
        tokio::select! {
            biased;

            // Priority 1: Tick signal - flush all buffered packets
            result = tick_rx.recv() => {
                match result {
                    Ok(()) => {
                        // Drain any remaining packets and queue them
                        while let Ok(packet) = outbound_rx.try_recv() {
                            let terminating = terminates_connection(&packet);
                            if let Err(e) = stream.send_packet(packet).await {
                                tracing::error!(session_id, "Send failed (tick flush): {:?}", e);
                                break 'network;
                            }
                            if terminating {
                                break 'network;
                            }
                        }
                        // Flush all queued packets as a single batch
                        if let Err(e) = stream.flush().await {
                            tracing::error!(session_id, "Flush failed: {:?}", e);
                            break 'network;
                        }
                        if let Some(metrics) = &metrics {
                            metrics.observe(stream.latency(), std::time::Instant::now());
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
                        let packet_args = McpePacketArgs;
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
                    Err(jolyne::JolyneError::Disconnected(disconnect)) => {
                        info!(
                            session_id,
                            reason = ?disconnect.reason,
                            message = disconnect.message.as_deref().unwrap_or(""),
                            "Client disconnected"
                        );
                        break;
                    }
                    // Save and Quit closes the RakNet connection without a
                    // Bedrock Disconnect: a normal leave, not an error.
                    Err(jolyne::JolyneError::ConnectionClosed) => {
                        info!(session_id, "Client closed the connection");
                        break;
                    }
                    Err(e) => {
                        tracing::error!(session_id, "Connection failed: {e}");
                        failure = Some(e);
                        break;
                    }
                }
            }

            // Priority 3: Queue outbound packets (batched flush on tick signal)
            Some(packet) = outbound_rx.recv() => {
                let terminating = terminates_connection(&packet);
                if let Err(e) = stream.send_packet(packet).await {
                    tracing::error!(session_id, "Send failed (immediate): {:?}", e);
                    break;
                }
                if terminating {
                    break 'network;
                }
                // Drain any other pending packets into buffer
                while let Ok(p) = outbound_rx.try_recv() {
                    let terminating = terminates_connection(&p);
                    if let Err(e) = stream.send_packet(p).await {
                        warn!(session_id, "Send failed: {:?}", e);
                        break 'network;
                    }
                    if terminating {
                        break 'network;
                    }
                }
                // NO flush here - packets accumulate until tick signal for efficient batching
                // This reduces compression operations from N per tick to 1 per tick
            }
        }
    }

    if let Some(metrics) = &metrics {
        metrics.clear();
    }
    // Close drains the terminal packet and earlier buffered output, never
    // packets queued after it. Every failure path also reaches this epilogue,
    // with the failure's reasoned `Disconnect` when it has one.
    let reason = failure
        .as_ref()
        .and_then(|e| Some((e.disconnect_reason()?, e.disconnect_message())));
    let closed = match reason {
        Some((reason, message)) => stream.disconnect(reason, message).await,
        None => stream.close().await,
    };
    if let Err(e) = closed {
        trace!(session_id, "Closing connection failed: {e}");
    }
    let _ = event_tx
        .send(NetworkEvent::Disconnected { session_id })
        .await;
    info!(session_id, "Network task ended");
}
