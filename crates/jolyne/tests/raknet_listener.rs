//! End-to-end over real RakNet loopback: listener admission (`max_players`),
//! status advertisement and a full client join.

#![cfg(all(feature = "server", feature = "client", feature = "raknet"))]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use jolyne::status::StaticStatus;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::valentine::{PlayStatusPacketStatus, TextPacket};
use jolyne::{BedrockListener, BedrockListenerConfig, BedrockStream, JolyneError, WorldTemplate};
use p384::SecretKey;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listener_advertises_status_and_enforces_max_players() {
    let mut listener = BedrockListener::raknet()
        .addr("127.0.0.1:0")
        .config(BedrockListenerConfig {
            online_mode: false,
            max_players: 1,
            status: Some(Arc::new(StaticStatus::new("Test MOTD", "Sub"))),
            ..Default::default()
        })
        .bind()
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let pong = String::from_utf8(listener.inner().get_advertisement()).unwrap();
    assert!(
        pong.starts_with("MCPE;Test MOTD;2193;1.26.51;0;1;"),
        "{pong}"
    );

    let key = SecretKey::random(&mut rand::thread_rng());
    let server = tokio::spawn(async move {
        let mut joined = Vec::new();
        for _ in 0..2 {
            let stream = listener.accept().await.unwrap();
            let key = key.clone();
            joined.push(tokio::spawn(async move {
                stream
                    .accept_join_sequence(&WorldTemplate::default(), &key)
                    .await
                    .map(|(play, _)| play)
            }));
        }
        let first = joined.remove(0).await.unwrap();
        let second = joined.remove(0).await.unwrap();
        let pong = String::from_utf8(listener.inner().get_advertisement()).unwrap();
        (first, second, pong)
    });

    let join = |name: &'static str| async move {
        let stream = BedrockStream::connect(addr).await?;
        stream.join(ClientHandshakeConfig::random(addr, name)).await
    };
    let (mut first, _) = join("First").await.unwrap();
    first
        .send_packet(TextPacket::default().into())
        .await
        .unwrap();
    let refused = join("Second").await.map(|_| ()).unwrap_err();
    assert!(
        matches!(
            refused,
            JolyneError::LoginRefused(PlayStatusPacketStatus::LoginfailedServerfullsubclient)
        ),
        "{refused}"
    );

    let (first_server, second_server, pong) = tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap();
    let mut first_server = first_server.unwrap();
    assert!(matches!(
        second_server.map(|_| ()),
        Err(JolyneError::Refused(_))
    ));
    assert!(
        pong.contains(";1;1;"),
        "player count refreshed on join: {pong}"
    );
    loop {
        let packet = first_server.recv_packet_raw().await.unwrap();
        if packet.is(jolyne::valentine::McpePacketName::TextPacket) {
            break;
        }
    }
}
