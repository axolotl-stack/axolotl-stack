use bytes::Bytes;
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use tokio_raknet::{RaknetListener, RaknetStream};

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
