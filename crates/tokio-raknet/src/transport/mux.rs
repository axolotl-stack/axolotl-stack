use std::time::{Duration, Instant};

use bytes::{BufMut, BytesMut};
use tokio::net::UdpSocket;
use tokio::time::{self, Interval, MissedTickBehavior};

use crate::protocol::packet::RaknetPacket;
use crate::session::manager::ManagedSession;
use crate::transport::ReceivedMessage;

const TICK_INTERVAL_MS: u64 = 20;

pub fn new_tick_interval() -> Interval {
    let mut tick = time::interval(Duration::from_millis(TICK_INTERVAL_MS));
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
) {
    if run_tick {
        for d in managed.on_tick(now) {
            tracing::trace!("send_tick_datagram");
            let mut out = BytesMut::new();
            d.encode(&mut out).expect("Bad datagram in queue.");
            let _ = socket.send_to(&out, peer).await;
        }
    }

    while let Some(d) = managed.build_datagram(now) {
        tracing::trace!("send_datagram");
        let mut out = BytesMut::new();
        d.encode(&mut out).expect("Bad datagram in queue.");
        let _ = socket.send_to(&out, peer).await;
    }
}

/// Convert a batch of decoded session packets into application messages
/// (ID byte + payload) with transport metadata.
pub fn into_received_messages(pkts: Vec<crate::session::IncomingPacket>) -> Vec<ReceivedMessage> {
    let mut out = Vec::new();
    for pkt in pkts {
        if let RaknetPacket::UserData { id, payload } = pkt.packet {
            let mut buf = BytesMut::with_capacity(1 + payload.len());
            buf.put_u8(id);
            buf.extend_from_slice(&payload);
            out.push(ReceivedMessage {
                buffer: buf.freeze(),
                reliability: pkt.reliability,
                channel: pkt.ordering_channel.unwrap_or(0),
            });
        }
    }
    out
}
