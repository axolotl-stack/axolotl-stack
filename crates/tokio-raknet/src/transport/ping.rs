//! Unconnected ping (server discovery), go-raknet `dial.go:Ping`.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use tokio::net::UdpSocket;

use crate::RaknetError;
use crate::protocol::{
    constants::{DEFAULT_UNCONNECTED_MAGIC, MAXIMUM_MTU_SIZE},
    packet::{DecodeError, RaknetPacket, UnconnectedPing},
    types::{RaknetTime, raknet_start_time},
};

/// go-raknet `dial.go:Ping` default timeout.
pub const DEFAULT_PING_TIMEOUT: Duration = Duration::from_secs(5);

/// Send an unconnected ping and return the server's pong data (the Bedrock
/// MOTD string), timing out after five seconds like go-raknet `Ping`.
pub async fn ping(server: SocketAddr) -> Result<Bytes, RaknetError> {
    ping_with_timeout(server, DEFAULT_PING_TIMEOUT).await
}

/// go-raknet `Dialer.PingContext`: one 33-byte `UnconnectedPing` carrying a
/// fresh random client GUID (as the Mojang client's per-connect
/// `resetMyGUID`), then the first datagram from the server must be an
/// `UnconnectedPong`.
pub async fn ping_with_timeout(
    server: SocketAddr,
    timeout: Duration,
) -> Result<Bytes, RaknetError> {
    tokio::time::timeout(timeout, async {
        let socket = UdpSocket::bind(if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .await?;
        let ping = RaknetPacket::UnconnectedPing(UnconnectedPing {
            ping_time: RaknetTime(
                Instant::now()
                    .saturating_duration_since(raknet_start_time())
                    .as_millis() as u64,
            ),
            magic: DEFAULT_UNCONNECTED_MAGIC,
            client_guid: Some(super::handshake::random_guid()),
        });
        let mut out = BytesMut::with_capacity(33);
        ping.encode(&mut out)?;
        socket.send_to(&out, server).await?;

        let mut buffer = vec![0u8; usize::from(MAXIMUM_MTU_SIZE)];
        loop {
            let (len, from) = socket.recv_from(&mut buffer).await?;
            if from != server {
                continue;
            }
            let mut slice = &buffer[..len];
            return match RaknetPacket::decode(&mut slice) {
                Ok(RaknetPacket::UnconnectedPong(pong)) => {
                    Ok(pong.advertisement.0.unwrap_or_default())
                }
                Ok(other) => Err(DecodeError::UnknownId(other.id()).into()),
                Err(error) => Err(error.into()),
            };
        }
    })
    .await
    .map_err(|_| {
        RaknetError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("ping timed out after {timeout:?}"),
        ))
    })?
}
