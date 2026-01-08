use bytes::Bytes;
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use tokio_raknet::{RaknetListener, RaknetStream};
use tokio_raknet::transport::{Message, RaknetListenerConfig, RaknetStreamConfig};
use tokio_raknet::protocol::reliability::Reliability;
use tokio_raknet::protocol::state::RakPriority;

#[tokio::test]
async fn test_basic_handshake_and_exchange() {
    // 1. Bind a server to a random port
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let local_addr = listener.local_addr();

    println!("Server listening on {}", local_addr);

    // 2. Spawn the server accept loop
    let server_handle = tokio::spawn(async move {
        // Accept one connection
        let mut conn = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("timeout waiting for connection")
            .expect("listener closed unexpectedly");

        println!("Server accepted connection from {}", conn.peer_addr());

        // Wait for a packet
        let packet = conn
            .next()
            .await
            .expect("connection closed")
            .expect("Failed to read.");
        assert_eq!(packet, Bytes::from_static(b"hello server"));

        // Send a reply
        conn.send_encoded("hello client".as_bytes()).await.unwrap();
    });

    // 3. Client connects to the server
    let client_handle = tokio::spawn(async move {
        // Give server a moment to bind (though not strictly needed with await)
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut client = RaknetStream::connect(local_addr)
            .await
            .expect("failed to connect to server");

        println!("Client connected!");

        // Send a message
        client
            .send_encoded("hello server".as_bytes())
            .await
            .unwrap();

        // Wait for reply
        let reply = timeout(Duration::from_secs(2), client.next())
            .await
            .expect("timeout waiting for reply")
            .expect("connection closed")
            .expect("Failed to read as well");

        assert_eq!(reply, Bytes::from_static(b"hello client"));
    });

    // 4. Wait for both to finish
    let (server_res, client_res) = tokio::join!(server_handle, client_handle);
    server_res.unwrap();
    client_res.unwrap();
}

#[tokio::test]
async fn test_listener_config_defaults() {
    let config = RaknetListenerConfig::default();

    assert_eq!(config.max_connections, 1024);
    assert_eq!(config.max_pending_connections, 256);
    assert_eq!(config.max_mtu, 1400);
    assert_eq!(config.session_timeout, Duration::from_secs(10));
    assert_eq!(config.max_ordering_channels, 16); // MAXIMUM_ORDERING_CHANNELS
}

#[tokio::test]
async fn test_listener_high_performance_config() {
    let config = RaknetListenerConfig::high_performance();

    assert_eq!(config.max_pending_connections, 1024);
    assert_eq!(config.max_queued_reliable_bytes, 4 * 1024 * 1024);
}

#[tokio::test]
async fn test_stream_config_defaults() {
    let config = RaknetStreamConfig::default();

    assert_eq!(config.mtu, 1400);
    assert_eq!(config.connection_timeout, Duration::from_secs(10));
    assert_eq!(config.session_timeout, Duration::from_secs(10));
    assert_eq!(config.max_ordering_channels, 16); // MAXIMUM_ORDERING_CHANNELS
}

#[tokio::test]
async fn test_listener_set_advertisement() {
    let listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");

    // Default advertisement
    let default_ad = listener.get_advertisement();
    assert!(!default_ad.is_empty());

    // Set new advertisement
    let new_ad = b"Custom Advertisement".to_vec();
    listener.set_advertisement(new_ad.clone());

    assert_eq!(listener.get_advertisement(), new_ad);
}

#[tokio::test]
async fn test_listener_local_addr() {
    let listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");

    let addr = listener.local_addr();
    assert!(addr.port() > 0);
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
}

#[tokio::test]
async fn test_message_builder_integration() {
    // Test the Message builder pattern with transport layer
    let msg = Message::new(vec![0xFE, 0x01, 0x02, 0x03])
        .reliability(Reliability::Reliable)
        .channel(1)
        .priority(RakPriority::High);

    assert_eq!(msg.buffer.as_ref(), &[0xFE, 0x01, 0x02, 0x03]);
    assert_eq!(msg.reliability, Reliability::Reliable);
    assert_eq!(msg.channel, 1);
    assert_eq!(msg.priority, RakPriority::High);
}

#[tokio::test]
async fn test_bind_with_custom_config() {
    let config = RaknetListenerConfig {
        max_connections: 100,
        max_mtu: 1200,
        advertisement: b"Test Server".to_vec(),
        ..Default::default()
    };

    let listener = RaknetListener::bind_with_config("127.0.0.1:0".parse().unwrap(), config)
        .await
        .expect("failed to bind with config");

    // Verify advertisement was set
    assert_eq!(listener.get_advertisement(), b"Test Server".to_vec());
}

#[tokio::test]
async fn test_multiple_messages_exchange() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let local_addr = listener.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("timeout")
            .expect("accept failed");

        // Receive and echo back multiple messages
        for i in 0..3 {
            let packet = timeout(Duration::from_secs(2), conn.next())
                .await
                .expect("timeout")
                .expect("closed")
                .expect("read error");

            let expected = format!("message {}", i);
            assert_eq!(packet.as_ref(), expected.as_bytes());

            let reply = format!("echo {}", i);
            conn.send_encoded(reply.into_bytes()).await.unwrap();
        }
    });

    let client_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut client = RaknetStream::connect(local_addr)
            .await
            .expect("connect failed");

        // Send and receive multiple messages
        for i in 0..3 {
            let msg = format!("message {}", i);
            client.send_encoded(msg.into_bytes()).await.unwrap();

            let reply = timeout(Duration::from_secs(2), client.next())
                .await
                .expect("timeout")
                .expect("closed")
                .expect("read error");

            let expected = format!("echo {}", i);
            assert_eq!(reply.as_ref(), expected.as_bytes());
        }
    });

    let (server_res, client_res) = tokio::join!(server_handle, client_handle);
    server_res.unwrap();
    client_res.unwrap();
}

#[tokio::test]
async fn test_stream_addresses() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let server_handle = tokio::spawn(async move {
        let conn = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("timeout")
            .expect("accept failed");

        // Server sees its local addr and client's peer addr
        assert_eq!(conn.local_addr(), server_addr);
        assert!(conn.peer_addr().port() > 0);
    });

    let client_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;

        let client = RaknetStream::connect(server_addr)
            .await
            .expect("connect failed");

        // Client sees its local addr and server's peer addr
        assert!(client.local_addr().port() > 0);
        assert_eq!(client.peer_addr(), server_addr);
    });

    let (server_res, client_res) = tokio::join!(server_handle, client_handle);
    server_res.unwrap();
    client_res.unwrap();
}
