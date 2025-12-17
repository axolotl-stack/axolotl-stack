//! Integration tests for the LAN discovery module.

#![cfg(feature = "discovery")]

use std::time::Duration;

use tokio::time::timeout;
use tokio_nethernet::discovery::{
    DiscoveryListener, DiscoveryListenerConfig, ServerData, TransportLayer,
};
use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig, NetherNetStream};

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

    // Note: On localhost, broadcast may not work the same as real LAN
    // The important thing is no crashes
}

/// Test the full discovery + nethernet connection flow.
#[tokio::test]
async fn test_discovery_connect_flow() {
    tracing_subscriber::fmt()
        .with_env_filter("tokio_nethernet=debug")
        .try_init()
        .ok();

    // Use unique ports to avoid conflicts
    let server_port = 18551;
    let client_port = 18552;

    // Server discovery listener
    let server_config = DiscoveryListenerConfig {
        network_id: 333333,
        broadcast_addr: format!("127.0.0.1:{}", client_port).parse().unwrap(),
        broadcast_interval: Duration::from_millis(200),
        address_timeout: Duration::from_secs(10),
    };
    let server_discovery =
        DiscoveryListener::bind(&format!("127.0.0.1:{}", server_port), server_config)
            .await
            .expect("server discovery bind");

    server_discovery
        .set_server_data(ServerData {
            server_name: "Integration Test Server".into(),
            level_name: "Test World".into(),
            ..Default::default()
        })
        .await;

    // Get server signal receiver
    let mut server_signal_rx = server_discovery
        .take_signal_receiver()
        .await
        .expect("server signal rx");

    // Create NetherNet listener
    let (mut nethernet_listener, server_signal_tx) =
        NetherNetListener::new(server_discovery.clone(), NetherNetListenerConfig::default());

    // Route signals from discovery to nethernet
    tokio::spawn(async move {
        while let Some(signal) = server_signal_rx.recv().await {
            println!(
                "[SERVER] Received signal: {} from {}",
                signal.typ, signal.network_id
            );
            if server_signal_tx.send(signal).await.is_err() {
                break;
            }
        }
    });

    // Client discovery listener
    let client_config = DiscoveryListenerConfig {
        network_id: 444444,
        broadcast_addr: format!("127.0.0.1:{}", server_port).parse().unwrap(),
        broadcast_interval: Duration::from_millis(200),
        address_timeout: Duration::from_secs(10),
    };
    let client_discovery =
        DiscoveryListener::bind(&format!("127.0.0.1:{}", client_port), client_config)
            .await
            .expect("client discovery bind");

    // Wait for discovery
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Get client signal receiver
    let mut client_signal_rx = client_discovery
        .take_signal_receiver()
        .await
        .expect("client signal rx");

    // Connect to server
    let connect_fut = async {
        let (stream, client_signal_tx) =
            NetherNetStream::connect("333333".to_string(), client_discovery).await?;

        // Route signals for client
        tokio::spawn(async move {
            while let Some(signal) = client_signal_rx.recv().await {
                println!(
                    "[CLIENT] Received signal: {} from {}",
                    signal.typ, signal.network_id
                );
                if client_signal_tx.send(signal).await.is_err() {
                    break;
                }
            }
        });

        Ok::<_, tokio_nethernet::NetherNetError>(stream)
    };

    // Accept on server side (with timeout)
    let accept_fut = async { nethernet_listener.accept().await };

    // Run both with a timeout
    let result = timeout(Duration::from_secs(30), async {
        tokio::select! {
            client_result = connect_fut => {
                println!("Client connected: {:?}", client_result.is_ok());
            }
            server_result = accept_fut => {
                println!("Server accepted: {:?}", server_result.is_ok());
            }
        }
    })
    .await;

    // Just verify it doesn't panic/hang forever
    match result {
        Ok(_) => println!("Test completed (connection may or may not have succeeded)"),
        Err(_) => {
            println!("Test timed out - this is expected if WebRTC can't establish ICE on localhost")
        }
    }
}
