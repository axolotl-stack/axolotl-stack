//! Every socket in these tests binds `127.0.0.1` (listeners explicitly,
//! clients through `client_bind_addr`): a socket on a wildcard or interface
//! address makes Windows Firewall prompt for each new test executable path.
//! Keep new tests on loopback.

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
use tokio_raknet::transport::{
    Message, RaknetListenerConfig, RaknetStreamConfig, TransmissionPolicy,
};
use tokio_raknet::{RaknetListener, RaknetStream};

fn default_system_addresses(peer: SocketAddr) -> Vec<SocketAddr> {
    vec![peer; 10]
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
        client_guid: (-12345i64) as u64,
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
async fn test_listener_high_performance_config() {
    let config = RaknetListenerConfig::high_performance();

    assert_eq!(config.max_pending_connections, 1024);
    assert_eq!(config.max_queued_reliable_bytes, 4 * 1024 * 1024);
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
    listener.set_advertisement(new_ad.clone()).unwrap();

    assert_eq!(listener.get_advertisement(), new_ad);
    assert!(listener.set_advertisement(vec![0; 32768]).is_err());
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

        // Client sees its local addr and server's peer addr. A loopback
        // server is dialled from loopback, never from a wildcard socket.
        assert!(client.local_addr().port() > 0);
        assert_eq!(
            client.local_addr().ip(),
            std::net::IpAddr::from([127, 0, 0, 1])
        );
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
            client_guid: (-12345i64) as u64,
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
async fn listener_acks_a_quiet_peer_without_application_or_socket_traffic() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let address = listener.local_addr();
    let raw = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    raw.connect(address).await.unwrap();
    raw_request2_handshake(&raw, address).await;
    send_online_control(
        &raw,
        RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid: (-12345i64) as u64,
            timestamp: RaknetTime(1),
            secure: false,
        }),
        0,
        0,
        None,
    )
    .await;
    send_online_control(
        &raw,
        RaknetPacket::NewIncomingConnection(NewIncomingConnection {
            server_address: address,
            system_addresses: default_system_addresses(address),
            request_timestamp: RaknetTime(2),
            accepted_timestamp: RaknetTime(1),
        }),
        1,
        1,
        Some(0),
    )
    .await;
    let mut stream = timeout(Duration::from_secs(1), listener.accept())
        .await
        .unwrap()
        .unwrap();
    for sequence in 2..7 {
        send_online_control(
            &raw,
            RaknetPacket::UserData {
                id: 0xfe,
                payload: Bytes::from_static(b"quiet peer"),
            },
            sequence,
            sequence,
            Some(sequence - 1),
        )
        .await;
        let message = stream.recv_message().await.unwrap().unwrap();
        assert_eq!(message.payload, Bytes::from_static(b"quiet peer"));
        // No application response and no further UDP input may drive the flush.
        // Repeated rounds cannot all hide behind the 100 ms maintenance tick.
        timeout(Duration::from_millis(20), async {
            let mut buffer = [0; 2048];
            loop {
                let len = raw.recv(&mut buffer).await.unwrap();
                if let Ok(Datagram {
                    payload: DatagramPayload::Ack(ack),
                    ..
                }) = Datagram::decode(&mut &buffer[..len])
                    && ack.ranges.iter().any(|range| {
                        range.start.value() <= sequence && range.end.value() >= sequence
                    })
                {
                    break;
                }
            }
        })
        .await
        .expect("ACK waited for maintenance instead of burst completion or its backstop");
    }
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
        client_guid: (-67890i64) as u64,
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
        ..Default::default()
    };
    let mut listener = RaknetListener::bind_with_config("127.0.0.1:0".parse().unwrap(), config)
        .await
        .expect("failed to bind listener");
    let server_addr = listener.local_addr();

    let half_open = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    half_open.connect(server_addr).await.unwrap();
    raw_request2_handshake(&half_open, server_addr).await;

    // Upstream handler.go:handleOpenConnectionRequest2 uses an absolute ten
    // second handshake deadline, independent of the session's stale threshold.
    // Transport uses std::time::Instant, so Tokio's paused clock cannot expire it.
    tokio::time::sleep(Duration::from_millis(10_500)).await;

    let accepted = tokio::spawn(async move {
        timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("accept timed out")
            .expect("listener closed unexpectedly")
    });

    let client = timeout(Duration::from_secs(5), RaknetStream::connect(server_addr))
        .await
        .expect("connect timed out")
        .expect("connect failed after half-open handshake deadline");

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

/// Bind a listener whose sessions send every message immediately (Vanilla has
/// no congestion window), for tests that push thousands of small messages.
async fn vanilla_listener() -> RaknetListener {
    RaknetListener::bind_with_config(
        "127.0.0.1:0".parse().unwrap(),
        RaknetListenerConfig {
            transmission_policy: TransmissionPolicy::Vanilla,
            max_queued_reliable_bytes: 16 * 1024 * 1024,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

/// Send `count` small messages in bursts small enough for loopback socket buffers.
async fn send_paced(stream: &mut RaknetStream, count: usize) {
    for index in 0..count {
        stream.send_encoded(vec![0xfe, 1]).await.unwrap();
        if index % 64 == 63 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

#[tokio::test]
async fn full_client_receive_queue_notifies_server_of_disconnect() {
    let mut listener = vanilla_listener().await;
    let address = listener.local_addr();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        // One more than go-raknet's 4096-message application queue.
        send_paced(&mut stream, 4097).await;
        // This is shorter than the inactivity timeout of an idle peer: the
        // client must send a disconnect, not simply abandon its UDP task.
        let result = timeout(Duration::from_secs(4), stream.recv_message())
            .await
            .expect("client abandoned full receive queue without disconnect");
        assert!(matches!(
            result,
            Some(Err(RaknetError::Disconnected(_)))
                | Some(Err(RaknetError::ConnectionClosed))
                | None
        ));
    });
    let client = RaknetStream::connect(address).await.unwrap();
    // Deliberately retain but never drain the client's 4096-message queue.
    server.await.unwrap();
    drop(client);
}

/// go-raknet `conn.go:newConn` buffers up to 4096 unread messages. The client
/// queue previously held 128, so a burst of 129 unread messages disconnected.
#[tokio::test]
async fn client_buffers_4096_unread_messages() {
    let mut listener = vanilla_listener().await;
    let address = listener.local_addr();
    let (sent_tx, sent_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        send_paced(&mut stream, 4096).await;
        sent_tx.send(()).unwrap();
        // Keep the session open until the client has read everything.
        let _ = timeout(Duration::from_secs(10), stream.recv_message()).await;
    });
    let mut client = RaknetStream::connect(address).await.unwrap();
    sent_rx.await.unwrap();
    // Let every message arrive before the application starts reading.
    tokio::time::sleep(Duration::from_millis(500)).await;
    for index in 0..4096 {
        let message = timeout(Duration::from_secs(5), client.recv_message())
            .await
            .unwrap_or_else(|_| panic!("message {index} missing"))
            .unwrap()
            .unwrap();
        assert_eq!(message.id, 0xfe);
    }
    drop(client);
    server.await.unwrap();
}

/// go-raknet `Dialer.Ping`: the pong data of the listener is returned.
#[tokio::test]
async fn ping_returns_listener_advertisement() {
    let listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    listener.set_advertisement(b"MCPE;parity".to_vec()).unwrap();
    let data = tokio_raknet::transport::ping(listener.local_addr())
        .await
        .unwrap();
    assert_eq!(&data[..], b"MCPE;parity");

    // Nothing answers: the ping times out instead of hanging.
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let error = tokio_raknet::transport::ping_with_timeout(
        silent.local_addr().unwrap(),
        Duration::from_millis(100),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, RaknetError::Io(error) if error.kind() == std::io::ErrorKind::TimedOut)
    );
}

/// `RaknetListener::guid` is the GUID carried by the listener's pongs.
#[tokio::test]
async fn listener_guid_matches_unconnected_pong() {
    let listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut ping = BytesMut::new();
    RaknetPacket::UnconnectedPing(tokio_raknet::protocol::packet::UnconnectedPing {
        ping_time: RaknetTime(1),
        magic: DEFAULT_UNCONNECTED_MAGIC,
        client_guid: Some(5),
    })
    .encode(&mut ping)
    .unwrap();
    socket.send_to(&ping, listener.local_addr()).await.unwrap();
    let RaknetPacket::UnconnectedPong(pong) = recv_packet(&socket).await else {
        panic!("expected UnconnectedPong");
    };
    assert_eq!(pong.server_guid, listener.guid());
}

/// `RaknetStream::rtt` is `None` until an acknowledgement sample exists and
/// then reflects the session estimate, for client and accepted streams and
/// both policies.
#[tokio::test]
async fn stream_rtt_is_published_after_acknowledgements() {
    for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
        let mut listener = RaknetListener::bind_with_config(
            "127.0.0.1:0".parse().unwrap(),
            RaknetListenerConfig {
                transmission_policy: policy,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let address = listener.local_addr();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            loop {
                match timeout(Duration::from_secs(5), stream.recv_message()).await {
                    Ok(Some(Ok(message))) => {
                        let mut echo = vec![message.id];
                        echo.extend_from_slice(&message.payload);
                        stream.send_encoded(echo).await.unwrap();
                    }
                    _ => return stream.rtt(),
                }
            }
        });
        let mut client = RaknetStream::connect_with_config(
            address,
            RaknetStreamConfig {
                transmission_policy: policy,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        for _ in 0..5 {
            client.send_encoded(vec![0xfe, 1, 2]).await.unwrap();
            client.recv_message().await.unwrap().unwrap();
            // Vanilla acknowledges on its 100 ms tick.
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
        let rtt = client.rtt().expect("client RTT after acknowledged sends");
        assert!(rtt < Duration::from_secs(1), "{policy:?}: {rtt:?}");
        drop(client);
        let server_rtt = server.await.unwrap();
        assert!(server_rtt.is_some(), "{policy:?}: server stream RTT");
    }
}
