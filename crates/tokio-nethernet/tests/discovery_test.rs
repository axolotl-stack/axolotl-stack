//! Integration tests for the LAN discovery module.

#![cfg(feature = "discovery")]

use std::time::Duration;

use tokio::time::timeout;
use tokio_nethernet::discovery::{
    DiscoveryListener, DiscoveryListenerConfig, ServerData, TransportLayer,
};
use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig};

/// Test that discovery packets can be encoded and decoded correctly.
#[tokio::test]
async fn test_discovery_packet_roundtrip() {
    use tokio_nethernet::discovery::ServerData;

    let data = ServerData {
        server_name: "Test Server".into(),
        level_name: "Test World".into(),
        player_count: 5,
        max_player_count: 20,
        game_type: 1,
        editor_world: false,
        hardcore: false,
        transport_layer: TransportLayer::NetherNet,
        connection_type: 4,
        ..Default::default()
    };

    let encoded = data.encode();
    let decoded = ServerData::decode(&encoded).expect("decode should succeed");

    assert_eq!(decoded.server_name, data.server_name);
    assert_eq!(decoded.level_name, data.level_name);
    assert_eq!(decoded.player_count, data.player_count);
    assert_eq!(decoded.max_player_count, data.max_player_count);
}

/// Test that two DiscoveryListeners can see each other's broadcasts.
#[tokio::test]
async fn test_discovery_broadcast() {
    // Server on port 7551
    let server_config = DiscoveryListenerConfig {
        network_id: 111111,
        broadcast_addr: "127.0.0.1:17552".parse().unwrap(), // Broadcast to client port
        broadcast_interval: Duration::from_millis(100),
        address_timeout: Duration::from_secs(5),
    };
    let server = DiscoveryListener::bind("127.0.0.1:17551", server_config)
        .await
        .expect("server bind");

    // Set server data
    server
        .set_server_data(ServerData {
            server_name: "Test Server".into(),
            level_name: "Test World".into(),
            player_count: 1,
            max_player_count: 10,
            ..Default::default()
        })
        .await;

    // Client on a different port, broadcasting to server
    let client_config = DiscoveryListenerConfig {
        network_id: 222222,
        broadcast_addr: "127.0.0.1:17551".parse().unwrap(), // Broadcast to server port
        broadcast_interval: Duration::from_millis(100),
        address_timeout: Duration::from_secs(5),
    };
    let client = DiscoveryListener::bind("127.0.0.1:17552", client_config)
        .await
        .expect("client bind");

    // Wait for discovery
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check if client discovered the server
    let responses = client.responses().await;
    println!("Client discovered {} servers", responses.len());

    let data = responses.get(&111111).expect("server must be discovered");
    let decoded = ServerData::decode(data).expect("responses contain decoded application bytes");
    assert_eq!(decoded.server_name, "Test Server");
}

/// Exercise discovery, signal routing, SCTP negotiation and fragmented payloads.
#[tokio::test]
async fn test_discovery_connect_flow() {
    use bytes::Bytes;
    use futures::{SinkExt, StreamExt};
    use tokio_nethernet::{NetherNetDialer, NetherNetDialerConfig};

    let server = DiscoveryListener::bind(
        "127.0.0.1:0",
        DiscoveryListenerConfig {
            network_id: 333333,
            broadcast_addr: "127.0.0.1:9".parse().unwrap(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    server.set_server_data(ServerData::default()).await;
    let client = DiscoveryListener::bind(
        "127.0.0.1:0",
        DiscoveryListenerConfig {
            network_id: 444444,
            broadcast_addr: server.local_addr().unwrap(),
            broadcast_interval: Duration::from_millis(20),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    timeout(Duration::from_secs(3), async {
        while !client.responses().await.contains_key(&333333) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("discovery response");
    let mut listener = NetherNetListener::bind_with_signaling(
        server,
        NetherNetListenerConfig {
            allow_identityless_client: true,
            ..Default::default()
        },
    );
    let dialer = NetherNetDialer::connect_with_signaling(client, NetherNetDialerConfig::default());
    let (mut client, mut server) = timeout(Duration::from_secs(10), async {
        tokio::try_join!(dialer.dial("333333".into()), listener.accept())
    })
    .await
    .expect("discovery connection deadline")
    .expect("discovery negotiation");
    assert!(
        client.remote_identity().is_some(),
        "default listener must issue a signed server identity"
    );
    let payload = Bytes::from(vec![0x5a; 140_000]);
    client
        .send(tokio_nethernet::Message::reliable(payload.clone()))
        .await
        .unwrap();
    let received = timeout(Duration::from_secs(5), server.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(received.buffer, payload);
    assert!(received.reliable);
}
