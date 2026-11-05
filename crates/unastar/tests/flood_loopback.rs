//! End to end over loopback RakNet: a client that floods packets faster than the
//! per-tick budget fills its own inbound queue and is disconnected with a reason it
//! can show, not by a silent close.
//!
//! Exclusive loopback socket run: take the native lock first
//! (`native-lock.sh acquire items ...`).

use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::RakNetTransport;
use jolyne::stream::{Client, Handshake};
use jolyne::valentine::types::EnumsConnectionDisconnectFailReason;
use jolyne::valentine::{AnimatePacket, McpePacket};
use jolyne::{BedrockStream, FlushMode, JolyneError};
use unastar::config::{SpawnLocation, SpawnRule, UnastarConfig};

const TIMEOUT: Duration = Duration::from_secs(20);

/// Packets sent in one batch: more than the 512-packet inbound queue holds, and under
/// jolyne's per-batch packet limit. The server decodes the whole batch at once, so the
/// queue overflows before a tick can drain it.
const FLOOD: usize = 700;

fn free_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn a_flooding_client_is_disconnected_with_a_reason() {
    let directory = tempfile::tempdir().unwrap();
    // The runtime keeps worlds, players and plugins below the working directory.
    std::env::set_current_dir(directory.path()).unwrap();
    let port = free_port();
    let mut config = UnastarConfig::default();
    config.server.bind_address = format!("127.0.0.1:{port}");
    config.server.online_mode = false;
    config.server.default_chunk_radius = 2;
    config.world = unastar::world::WorldConfig::void_spawn_platform(1);
    config.players.data_dir = directory.path().join("players");
    config.spawn_rules = vec![SpawnRule {
        always_at_location: true,
        location: Some(SpawnLocation::default()),
        ..SpawnRule::default()
    }];
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_stack_size(64 * 1024 * 1024)
        .build()
        .unwrap();
    runtime.block_on(async move {
        let mut server = unastar::server::UnastarServer::new(Arc::new(config))
            .await
            .unwrap();
        let _server_task =
            tokio::spawn(async move { server.run().await.map_err(|e| e.to_string()) });
        let address: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let mut attempts = 0;
        let client = loop {
            match BedrockStream::<Handshake, Client, RakNetTransport>::connect(address).await {
                Ok(client) => break client,
                Err(_) if attempts < 50 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => panic!("connect: {error}"),
            }
        };
        let (mut client, _game) = tokio::time::timeout(
            TIMEOUT,
            client.join(ClientHandshakeConfig::random(address, "flooder")),
        )
        .await
        .expect("join in time")
        .expect("join");

        // One batch: the client's default flush mode would send each packet as its own
        // RakNet message and overflow its own send queue first.
        client.set_flush_mode(FlushMode::Manual).await.unwrap();
        let animate = McpePacket::from(AnimatePacket::default());
        for _ in 0..FLOOD {
            client.send_packet(animate.clone()).await.unwrap();
        }
        client.flush().await.unwrap();

        let error = tokio::time::timeout(TIMEOUT, async {
            loop {
                if let Err(error) = client.recv_packet().await {
                    return error;
                }
            }
        })
        .await
        .expect("the flooder is disconnected in time");
        let JolyneError::Disconnected(disconnect) = error else {
            panic!("expected a Disconnect with a reason, got {error}");
        };
        assert_eq!(
            disconnect.reason,
            EnumsConnectionDisconnectFailReason::Kicked
        );
        assert!(
            disconnect
                .message
                .as_deref()
                .is_some_and(|message| message.contains("Too many packets")),
            "message: {:?}",
            disconnect.message
        );
    });
}
