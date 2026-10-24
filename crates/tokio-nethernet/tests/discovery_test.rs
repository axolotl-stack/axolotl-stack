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
        ..Default::default()
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
        ..Default::default()
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

/// Exercise discovery, signal routing, SCTP negotiation and fragmented payloads
/// with default configurations: like BDS 1.26.51 on LAN, neither side needs an
/// `a=identity` (go-nethernet's defaults reject identityless peers).
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
    // Loopback only: binding interface addresses would raise a host-firewall prompt.
    let mut listener = NetherNetListener::bind_with_signaling(
        server,
        NetherNetListenerConfig {
            transport: tokio_nethernet::TransportConfig::loopback_v4(),
            ..Default::default()
        },
    );
    let dialer = NetherNetDialer::connect_with_signaling(
        client,
        NetherNetDialerConfig {
            transport: tokio_nethernet::TransportConfig::loopback_v4(),
            ..Default::default()
        },
    );
    let (mut client, mut server) = timeout(Duration::from_secs(10), async {
        tokio::try_join!(dialer.dial("333333".into()), listener.accept())
    })
    .await
    .expect("discovery connection deadline")
    .expect("discovery negotiation");
    assert!(
        client.remote_identity().is_none() && server.remote_identity().is_none(),
        "vanilla LAN answers and offers carry no identity by default"
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

/// Delegates to a client `DiscoveryListener` (including its trickle policy)
/// and records every outbound signal; inbound ones are recorded by the test.
struct Recording {
    inner: std::sync::Arc<DiscoveryListener>,
    sent: std::sync::Arc<tokio::sync::Mutex<Vec<tokio_nethernet::Signal>>>,
    receiver: tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<tokio_nethernet::Signal>>>,
}

#[async_trait::async_trait]
impl tokio_nethernet::Signaling for Recording {
    async fn signal(&self, signal: tokio_nethernet::Signal) -> anyhow::Result<()> {
        self.sent.lock().await.push(signal.clone());
        self.inner.signal(signal).await
    }
    fn network_id(&self) -> String {
        tokio_nethernet::Signaling::network_id(&*self.inner)
    }
    fn disable_trickle_ice(&self) -> bool {
        tokio_nethernet::Signaling::disable_trickle_ice(&*self.inner)
    }
    fn requires_identity(&self) -> bool {
        false
    }
}

#[async_trait::async_trait]
impl tokio_nethernet::SignalingChannel for Recording {
    async fn take_signal_receiver(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<tokio_nethernet::Signal>> {
        self.receiver.lock().await.take()
    }
}

/// LAN negotiation is non-trickle both ways, as the 1.26.52 client and BDS
/// do: the offer and the answer carry every candidate inline and no
/// `CANDIDATEADD` is sent in either direction.
#[tokio::test]
async fn lan_offers_and_answers_carry_inline_candidates() {
    use tokio_nethernet::{NetherNetDialer, NetherNetDialerConfig, SignalType, TransportConfig};

    let server = DiscoveryListener::bind(
        "127.0.0.1:0",
        DiscoveryListenerConfig {
            network_id: 555555,
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
            network_id: 666666,
            broadcast_addr: server.local_addr().unwrap(),
            broadcast_interval: Duration::from_millis(20),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    timeout(Duration::from_secs(3), async {
        while !client.responses().await.contains_key(&555555) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("discovery response");

    let mut inbound = client.take_signal_receiver().await.unwrap();
    let (forward, receiver) = tokio::sync::mpsc::channel(128);
    let observed = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let record = observed.clone();
    tokio::spawn(async move {
        while let Some(signal) = inbound.recv().await {
            record.lock().await.push(signal.clone());
            if forward.send(signal).await.is_err() {
                break;
            }
        }
    });
    let mut listener = NetherNetListener::bind_with_signaling(
        server,
        NetherNetListenerConfig {
            allow_identityless_client: true,
            transport: TransportConfig::loopback_v4(),
            ..Default::default()
        },
    );
    let sent = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let dialer = NetherNetDialer::connect_with_signaling(
        Recording {
            inner: client,
            sent: sent.clone(),
            receiver: tokio::sync::Mutex::new(Some(receiver)),
        },
        NetherNetDialerConfig {
            transport: TransportConfig::loopback_v4(),
            allow_identityless_server: true,
            ..Default::default()
        },
    );
    timeout(Duration::from_secs(10), async {
        tokio::try_join!(dialer.dial("555555".into()), listener.accept())
    })
    .await
    .expect("LAN connection deadline")
    .expect("LAN negotiation");
    // Let any trickled candidate that would follow the descriptions arrive.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let received = observed.lock().await;
    let sent = sent.lock().await;
    for (typ, signals) in [
        (SignalType::Offer, &*sent),
        (SignalType::Answer, &*received),
    ] {
        let description = signals
            .iter()
            .find(|signal| signal.typ == typ)
            .expect("description");
        assert!(
            description.data.contains("a=candidate:"),
            "{typ} must carry its candidates: {}",
            description.data
        );
        assert!(
            signals
                .iter()
                .all(|signal| signal.typ != SignalType::Candidate),
            "LAN peers must not trickle: {signals:?}"
        );
    }
}

/// The client sends discovery requests only while browsing, never while it
/// announces a world; tokio used to request every interval regardless.
#[tokio::test]
async fn announcing_listener_sends_no_requests() {
    let probe = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let listener = DiscoveryListener::bind(
        "127.0.0.1:0",
        DiscoveryListenerConfig {
            network_id: 777777,
            broadcast_addr: probe.local_addr().unwrap(),
            broadcast_interval: Duration::from_millis(20),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut buf = vec![0; 2048];
    // Browsing: requests arrive at the configured destination.
    timeout(Duration::from_secs(2), probe.recv(&mut buf))
        .await
        .expect("browsing listener must request")
        .unwrap();
    listener.set_server_data(ServerData::default()).await;
    // A request already in flight may still land; then nothing more.
    tokio::time::sleep(Duration::from_millis(60)).await;
    while probe.try_recv(&mut buf).is_ok() {}
    assert!(
        timeout(Duration::from_millis(200), probe.recv(&mut buf))
            .await
            .is_err(),
        "announcing listener sent a discovery request"
    );
}
