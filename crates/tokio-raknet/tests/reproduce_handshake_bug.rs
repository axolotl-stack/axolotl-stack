use bytes::BytesMut;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tokio_raknet::RaknetListener;
use tokio_raknet::protocol::constants::DEFAULT_UNCONNECTED_MAGIC;
use tokio_raknet::protocol::packet::OpenConnectionRequest1;
use tokio_raknet::protocol::packet::OpenConnectionRequest2;
use tokio_raknet::protocol::packet::RaknetPacket;
use tokio_raknet::protocol::types::EoBPadding;

#[tokio::test]
async fn test_handshake_retry_bug() {
    // 1. Setup Server
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();
    println!("Server listening on {}", server_addr);

    // Spawn server loop to accept connections
    std::mem::drop(tokio::spawn(async move {
        if let Some(_conn) = listener.accept().await {
            println!("Server accepted connection");
            // Keep connection alive
            tokio::time::sleep(Duration::from_secs(2)).await;
        } else {
            panic!("Server failed to accept connection");
        }
    }));

    // 2. Setup Raw Client
    let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client_socket.connect(server_addr).await.unwrap();

    // 3. Send Request1
    let mut buf = BytesMut::new();
    let req1 = RaknetPacket::OpenConnectionRequest1(OpenConnectionRequest1 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        protocol_version: 11,
        padding: EoBPadding(900), // Request MTU ~900
    });
    req1.encode(&mut buf).unwrap();
    client_socket.send(&buf).await.unwrap();

    // 4. Receive Reply1
    let mut recv_buf = [0u8; 2048];
    let len = timeout(Duration::from_secs(1), client_socket.recv(&mut recv_buf))
        .await
        .expect("timeout req1")
        .unwrap();
    let mut slice = &recv_buf[..len];
    let reply1 = RaknetPacket::decode(&mut slice).unwrap();
    let cookie = match reply1 {
        RaknetPacket::OpenConnectionReply1(r) => r.cookie.unwrap(),
        _ => panic!("Expected Reply1, got something else"), // Debug not impl
    };

    // 5. Send Request2 (First Attempt)
    let mut buf = BytesMut::new();
    let req2 = RaknetPacket::OpenConnectionRequest2(OpenConnectionRequest2 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        server_addr,
        mtu: 900,
        cookie: Some(cookie),
        client_proof: false,
        client_guid: 12345,
    });
    req2.encode(&mut buf).unwrap();
    client_socket.send(&buf).await.unwrap();

    // 6. Receive Reply2
    let len = timeout(Duration::from_secs(1), client_socket.recv(&mut recv_buf))
        .await
        .expect("timeout req2")
        .unwrap();
    let mut slice = &recv_buf[..len];
    let reply2 = RaknetPacket::decode(&mut slice).unwrap();
    match reply2 {
        RaknetPacket::OpenConnectionReply2(_) => println!("Received Reply2 (Handshake Complete)"),
        _ => panic!("Expected Reply2"),
    }

    // 7. Send Request2 AGAIN (Simulate duplicate/retry)
    println!("Sending Request2 AGAIN (Retry)...");
    client_socket.send(&buf).await.unwrap();

    // 8. Expect Reply2 AGAIN
    // If the fix works, the server should resend Reply2.
    // If the bug exists, the server would have removed the session and ignored the packet (timeout).
    let len = timeout(Duration::from_secs(1), client_socket.recv(&mut recv_buf))
        .await
        .expect("timeout waiting for retry Reply2")
        .unwrap();
    let mut slice = &recv_buf[..len];
    let reply2_retry = RaknetPacket::decode(&mut slice).unwrap();
    match reply2_retry {
        RaknetPacket::OpenConnectionReply2(_) => println!("Received Retry Reply2 - Fix Verified!"),
        _ => panic!("Expected Retry Reply2"),
    }
}
