use bytes::{Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tokio_raknet::RaknetError;
use tokio_raknet::protocol::constants::{DEFAULT_UNCONNECTED_MAGIC, DatagramFlags};
use tokio_raknet::protocol::datagram::{Datagram, DatagramPayload};
use tokio_raknet::protocol::encapsulated_packet::EncapsulatedPacket;
use tokio_raknet::protocol::packet::{
    ConnectionRequest, NewIncomingConnection, OpenConnectionRequest1, OpenConnectionRequest2,
    RaknetPacket,
};
use tokio_raknet::protocol::reliability::Reliability;
use tokio_raknet::protocol::state::RakPriority;
use tokio_raknet::protocol::types::{
    DatagramHeader, EncapsulatedPacketHeader, EoBPadding, RaknetTime, Sequence24,
};
use tokio_raknet::session::{QueuePacketError, manager::SessionError};
use tokio_raknet::transport::{Message, RaknetListenerConfig, RaknetStreamConfig};
use tokio_raknet::{RaknetListener, RaknetStream};

fn default_system_addresses(peer: SocketAddr) -> [SocketAddr; 10] {
    [peer; 10]
}

async fn recv_packet(socket: &UdpSocket) -> RaknetPacket {
    let mut recv_buf = [0u8; 2048];
    let len = timeout(Duration::from_secs(1), socket.recv(&mut recv_buf))
        .await
        .expect("timed out waiting for packet")
        .expect("failed to receive packet");
    let mut slice = &recv_buf[..len];
    RaknetPacket::decode(&mut slice).expect("failed to decode packet")
}

async fn raw_request2_handshake(socket: &UdpSocket, server_addr: SocketAddr) {
    let req1 = RaknetPacket::OpenConnectionRequest1(OpenConnectionRequest1 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        protocol_version: 11,
        padding: EoBPadding(900),
    });
    let mut buf = BytesMut::new();
    req1.encode(&mut buf).unwrap();
    socket.send(&buf).await.unwrap();

    let cookie = match recv_packet(socket).await {
        RaknetPacket::OpenConnectionReply1(reply) => reply.cookie.expect("cookie"),
        other => panic!("expected reply1, got {other:?}"),
    };

    let req2 = RaknetPacket::OpenConnectionRequest2(OpenConnectionRequest2 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        server_addr,
        mtu: 900,
        cookie: Some(cookie),
        client_proof: false,
        client_guid: 12345,
    });
    buf.clear();
    req2.encode(&mut buf).unwrap();
    socket.send(&buf).await.unwrap();

    match recv_packet(socket).await {
        RaknetPacket::OpenConnectionReply2(_) => {}
        other => panic!("expected reply2, got {other:?}"),
    }
}

async fn send_online_control(
    socket: &UdpSocket,
    packet: RaknetPacket,
    sequence: u32,
    reliable_index: u32,
    ordering_index: Option<u32>,
) {
    let mut payload = BytesMut::new();
    packet.encode(&mut payload).unwrap();

    let reliability = if ordering_index.is_some() {
        Reliability::ReliableOrdered
    } else {
        Reliability::Reliable
    };

    let encap = EncapsulatedPacket {
        header: EncapsulatedPacketHeader::with_reliability(reliability),
        bit_length: (payload.len() as u16) * 8,
        reliable_index: Some(Sequence24::new(reliable_index)),
        sequence_index: None,
        ordering_index: ordering_index.map(Sequence24::new),
        ordering_channel: ordering_index.map(|_| 0),
        split: None,
        payload: payload.freeze(),
    };

    let dgram = Datagram {
        header: DatagramHeader {
            flags: DatagramFlags::VALID,
            sequence: Sequence24::new(sequence),
        },
        payload: DatagramPayload::EncapsulatedPackets(vec![encap]),
    };

    let mut buf = BytesMut::new();
    dgram.encode(&mut buf).unwrap();
    socket.send(&buf).await.unwrap();
}

fn assert_invalid_channel_error(error: RaknetError) {
    match error {
        RaknetError::Session(SessionError::Queue(QueuePacketError::InvalidOrderingChannel {
            ..
        })) => {}
        RaknetError::Session(SessionError::InvalidState {
            msg: "ordering channel out of range",
            ..
        }) => {}
        other => panic!("expected invalid ordering channel error, got {other:?}"),
    }
}

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

#[tokio::test]
async fn test_zero_length_datagram_does_not_break_listener() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let probe = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    probe.connect(server_addr).await.unwrap();
    probe.send(&[]).await.unwrap();

    let server_handle = tokio::spawn(async move {
        timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("accept timed out")
            .expect("listener closed unexpectedly");
    });

    let client = timeout(Duration::from_secs(5), RaknetStream::connect(server_addr))
        .await
        .expect("connect timed out")
        .expect("connect failed");
    drop(client);

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_listener_waits_for_new_incoming_connection_before_accept() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client_socket.connect(server_addr).await.unwrap();

    raw_request2_handshake(&client_socket, server_addr).await;

    send_online_control(
        &client_socket,
        RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid: 12345,
            timestamp: RaknetTime(1),
            secure: false,
        }),
        0,
        0,
        None,
    )
    .await;

    let not_yet = timeout(Duration::from_millis(250), listener.accept()).await;
    assert!(
        not_yet.is_err(),
        "listener accepted before NewIncomingConnection"
    );

    send_online_control(
        &client_socket,
        RaknetPacket::NewIncomingConnection(NewIncomingConnection {
            server_address: server_addr,
            system_addresses: default_system_addresses(server_addr),
            request_timestamp: RaknetTime(2),
            accepted_timestamp: RaknetTime(1),
        }),
        1,
        1,
        Some(0),
    )
    .await;

    timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("accept timed out after NewIncomingConnection")
        .expect("listener closed unexpectedly");
}

#[tokio::test]
async fn test_forged_new_incoming_connection_does_not_reach_accept_queue() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client_socket.connect(server_addr).await.unwrap();

    raw_request2_handshake(&client_socket, server_addr).await;

    send_online_control(
        &client_socket,
        RaknetPacket::NewIncomingConnection(NewIncomingConnection {
            server_address: server_addr,
            system_addresses: default_system_addresses(server_addr),
            request_timestamp: RaknetTime(2),
            accepted_timestamp: RaknetTime(1),
        }),
        0,
        0,
        Some(0),
    )
    .await;

    let forged_accept = timeout(Duration::from_millis(250), listener.accept()).await;
    assert!(
        forged_accept.is_err(),
        "listener accepted a forged NewIncomingConnection without ConnectionRequest"
    );
}

#[tokio::test]
async fn test_request2_rechecks_max_connections() {
    let config = RaknetListenerConfig {
        max_connections: 1,
        ..Default::default()
    };
    let mut listener = RaknetListener::bind_with_config("127.0.0.1:0".parse().unwrap(), config)
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let accepted = tokio::spawn(async move {
        let conn = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("accept timed out")
            .expect("listener closed unexpectedly");
        tokio::time::sleep(Duration::from_millis(750)).await;
        conn
    });

    let second = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    second.connect(server_addr).await.unwrap();

    let req1 = RaknetPacket::OpenConnectionRequest1(OpenConnectionRequest1 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        protocol_version: 11,
        padding: EoBPadding(900),
    });
    let mut buf = BytesMut::new();
    req1.encode(&mut buf).unwrap();
    second.send(&buf).await.unwrap();

    let cookie = match recv_packet(&second).await {
        RaknetPacket::OpenConnectionReply1(reply) => reply.cookie.expect("cookie"),
        other => panic!("expected reply1, got {other:?}"),
    };

    let client = RaknetStream::connect(server_addr)
        .await
        .expect("failed to establish first connection");

    let req2 = RaknetPacket::OpenConnectionRequest2(OpenConnectionRequest2 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        server_addr,
        mtu: 900,
        cookie: Some(cookie),
        client_proof: false,
        client_guid: 67890,
    });
    buf.clear();
    req2.encode(&mut buf).unwrap();
    second.send(&buf).await.unwrap();

    match recv_packet(&second).await {
        RaknetPacket::NoFreeIncomingConnections(_) => {}
        other => panic!("expected NoFreeIncomingConnections, got {other:?}"),
    }

    drop(client);
    accepted.await.unwrap();
}

#[tokio::test]
async fn test_half_open_request2_session_times_out_and_frees_slot() {
    let config = RaknetListenerConfig {
        max_connections: 1,
        session_stale: Duration::from_millis(100),
        session_timeout: Duration::from_secs(5),
        ..Default::default()
    };
    let mut listener = RaknetListener::bind_with_config("127.0.0.1:0".parse().unwrap(), config)
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let half_open = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    half_open.connect(server_addr).await.unwrap();
    raw_request2_handshake(&half_open, server_addr).await;

    tokio::time::sleep(Duration::from_millis(250)).await;

    let accepted = tokio::spawn(async move {
        timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("accept timed out")
            .expect("listener closed unexpectedly")
    });

    let client = timeout(Duration::from_secs(5), RaknetStream::connect(server_addr))
        .await
        .expect("connect timed out")
        .expect("connect failed after half-open stale timeout");

    drop(client);
    accepted.await.unwrap();
}

#[tokio::test]
async fn test_pre_accept_user_data_flood_does_not_block_listener() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let raw = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    raw.connect(server_addr).await.unwrap();
    raw_request2_handshake(&raw, server_addr).await;

    for i in 0..160 {
        send_online_control(
            &raw,
            RaknetPacket::UserData {
                id: 0x8e,
                payload: Bytes::from_static(b"pre-accept"),
            },
            i,
            i,
            None,
        )
        .await;
    }

    let accepted = tokio::spawn(async move {
        timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("accept timed out")
            .expect("listener closed unexpectedly")
    });

    let client = timeout(Duration::from_secs(5), RaknetStream::connect(server_addr))
        .await
        .expect("connect timed out")
        .expect("connect failed after pre-accept flood");

    drop(client);
    accepted.await.unwrap();
}

#[tokio::test]
async fn test_accept_queue_full_connection_is_retried_after_capacity_returns() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let mut clients = Vec::new();
    for _ in 0..33 {
        let client = timeout(Duration::from_secs(5), RaknetStream::connect(server_addr))
            .await
            .expect("connect timed out")
            .expect("connect failed");
        clients.push(client);
    }

    let mut accepted = Vec::new();
    for index in 0..33 {
        let conn = timeout(Duration::from_secs(2), listener.accept())
            .await
            .unwrap_or_else(|_| panic!("accept timed out at connection {index}"))
            .expect("listener closed unexpectedly");
        accepted.push(conn);
    }

    assert_eq!(clients.len(), 33);
    assert_eq!(accepted.len(), 33);
}

#[tokio::test]
async fn test_client_invalid_channel_queue_error_reaches_app() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let server_handle = tokio::spawn(async move {
        let conn = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("accept timed out")
            .expect("listener closed unexpectedly");
        tokio::time::sleep(Duration::from_millis(500)).await;
        conn
    });

    let mut client = RaknetStream::connect(server_addr)
        .await
        .expect("connect failed");
    client
        .send(Message::new(vec![0x8e, 0x01]).channel(99))
        .await
        .expect("mpsc send should succeed");

    let error = timeout(Duration::from_secs(2), client.recv_message())
        .await
        .expect("timed out waiting for queue error")
        .expect("client stream closed")
        .expect_err("expected invalid channel error");
    assert_invalid_channel_error(error);

    drop(client);
    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_server_invalid_channel_queue_error_reaches_app() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("accept timed out")
            .expect("listener closed unexpectedly");
        conn.send(Message::new(vec![0x8e, 0x01]).channel(99))
            .await
            .expect("mpsc send should succeed");

        let error = timeout(Duration::from_secs(2), conn.recv_message())
            .await
            .expect("timed out waiting for queue error")
            .expect("server stream closed")
            .expect_err("expected invalid channel error");
        assert_invalid_channel_error(error);
    });

    let client = RaknetStream::connect(server_addr)
        .await
        .expect("connect failed");

    server_handle.await.unwrap();
    drop(client);
}

#[tokio::test]
async fn test_stream_connect_timeout_is_enforced() {
    let inert_server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = inert_server.local_addr().unwrap();

    let start = Instant::now();
    let err = RaknetStream::connect_with_config(
        server_addr,
        RaknetStreamConfig {
            connection_timeout: Duration::from_millis(150),
            mtu: 900,
            ..Default::default()
        },
    )
    .await
    .err()
    .expect("connect should time out against inert UDP peer");
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(1), "elapsed: {elapsed:?}");
    match err {
        tokio_raknet::RaknetError::Io(io_err) => {
            assert_eq!(io_err.kind(), std::io::ErrorKind::TimedOut);
        }
        other => panic!("expected timeout io error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_recv_message_exposes_id_and_payload_without_repacking() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("failed to bind listener");
    let local_addr = listener.local_addr();

    let server_handle = tokio::spawn(async move {
        let mut conn = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("timeout")
            .expect("accept failed");

        conn.recv_message()
            .await
            .expect("connection closed")
            .expect("recv_message failed")
    });

    let client_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut client = RaknetStream::connect(local_addr)
            .await
            .expect("connect failed");
        client
            .send_encoded(vec![0x8e, 0xaa, 0xbb, 0xcc])
            .await
            .unwrap();
    });

    client_handle.await.unwrap();
    let received = server_handle.await.unwrap();

    assert_eq!(received.id, 0x8e);
    assert_eq!(received.payload.as_ref(), &[0xaa, 0xbb, 0xcc]);
}
