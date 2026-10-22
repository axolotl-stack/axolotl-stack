use std::time::{Duration, Instant};

use bytes::BytesMut;
use tokio::net::UdpSocket;
use tokio::time::{self, Interval, MissedTickBehavior};

use crate::protocol::packet::RaknetPacket;
use crate::session::TransmissionPolicy;
use crate::session::manager::ManagedSession;
use crate::session::policy::VANILLA_TICK_INTERVAL;
use crate::transport::ReceivedMessage;

/// Optimized listener maintenance period.
const TICK_INTERVAL_MS: u64 = 100;

/// Listener maintenance interval: 100 ms for Optimized; Vanilla runs every
/// session's Mojang update cycle every 10 ms (`VANILLA_TICK_INTERVAL`).
pub fn new_tick_interval(policy: TransmissionPolicy) -> Interval {
    let period = if policy.is_vanilla() {
        VANILLA_TICK_INTERVAL
    } else {
        Duration::from_millis(TICK_INTERVAL_MS)
    };
    let mut tick = time::interval(period);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tick
}

/// Flushes any pending maintenance and outbound datagrams for a managed session.
#[tracing::instrument(skip_all, fields(peer= %peer.to_string()), level = "trace")]
pub async fn flush_managed(
    managed: &mut ManagedSession,
    socket: &UdpSocket,
    peer: std::net::SocketAddr,
    now: Instant,
    run_tick: bool,
    out: &mut BytesMut,
) {
    if run_tick {
        for d in managed.on_tick(now) {
            tracing::trace!("send_tick_datagram");
            out.clear();
            if let Err(e) = d.encode(out) {
                tracing::error!(error = ?e, "failed to encode tick datagram - dropping");
                continue;
            }
            let _ = socket.send_to(out.as_ref(), peer).await;
        }
    }

    while let Some(d) = managed.build_datagram(now) {
        tracing::trace!("send_datagram");
        out.clear();
        if let Err(e) = d.encode(out) {
            tracing::error!(error = ?e, "failed to encode datagram - dropping");
            continue;
        }
        let _ = socket.send_to(out.as_ref(), peer).await;
    }
}

/// Convert a batch of decoded session packets into application messages
/// (ID byte + payload) with transport metadata.
pub fn into_received_messages(pkts: Vec<crate::session::IncomingPacket>) -> Vec<ReceivedMessage> {
    let mut out = Vec::new();
    for pkt in pkts {
        if let RaknetPacket::UserData { id, payload } = pkt.packet {
            out.push(ReceivedMessage {
                id,
                payload,
                reliability: pkt.reliability,
                channel: pkt.ordering_channel.unwrap_or(0),
            });
        }
    }
    out
}
