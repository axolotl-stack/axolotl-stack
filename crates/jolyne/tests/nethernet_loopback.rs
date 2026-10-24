//! End-to-end over real NetherNet (WebRTC data channels on IPv4 loopback,
//! in-process signaling): the listener admits a client, both run the whole
//! join, and a packet crosses each way.

#![cfg(all(feature = "server", feature = "client", feature = "nethernet"))]

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use jolyne::stream::NetherNetTransport;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::valentine::{McpePacketName, TextPacket};
use jolyne::{
    BedrockListener, BedrockListenerConfig, BedrockStream, Client, ClientConfig, WorldTemplate,
};
use p384::SecretKey;
use tokio::sync::{Mutex, mpsc};
use tokio_nethernet::{
    NetherNetDialer, NetherNetDialerConfig, NetherNetListener, NetherNetListenerConfig, Signal,
    Signaling, SignalingChannel, TransportConfig,
};

/// One side of an in-memory signaling pipe.
struct Local {
    id: &'static str,
    sender: mpsc::Sender<Signal>,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
}

#[async_trait]
impl Signaling for Local {
    async fn signal(&self, mut signal: Signal) -> anyhow::Result<()> {
        signal.network_id = self.id.into();
        self.sender.send(signal).await?;
        Ok(())
    }
    fn network_id(&self) -> String {
        self.id.into()
    }
    fn disable_trickle_ice(&self) -> bool {
        true
    }
    fn requires_identity(&self) -> bool {
        false
    }
}

#[async_trait]
impl SignalingChannel for Local {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.receiver.lock().await.take()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_join_over_nethernet_loopback() {
    let (to_server, server_rx) = mpsc::channel(16);
    let (to_client, client_rx) = mpsc::channel(16);
    let raw = NetherNetListener::bind_with_signaling(
        Local {
            id: "1",
            sender: to_client,
            receiver: Mutex::new(Some(server_rx)),
        },
        NetherNetListenerConfig {
            transport: TransportConfig::loopback_v4(),
            allow_identityless_client: true,
            ..Default::default()
        },
    );
    // DTLS encrypts NetherNet; Bedrock encryption stays off, as on retail.
    let mut listener = BedrockListener::from_listener(
        raw,
        BedrockListenerConfig {
            online_mode: false,
            encryption_enabled: false,
            ..Default::default()
        },
    );
    let dialer = NetherNetDialer::connect_with_signaling(
        Local {
            id: "2",
            sender: to_server,
            receiver: Mutex::new(Some(client_rx)),
        },
        NetherNetDialerConfig {
            transport: TransportConfig::loopback_v4(),
            allow_identityless_server: true,
            ..Default::default()
        },
    );

    let key = SecretKey::random(&mut rand::thread_rng());
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.unwrap();
        let (mut play, _) = stream
            .accept_join_sequence(&WorldTemplate::default(), &key)
            .await
            .unwrap();
        while !play
            .recv_packet_raw()
            .await
            .unwrap()
            .is(McpePacketName::TextPacket)
        {}
        play.send_packet(TextPacket::default().into())
            .await
            .unwrap();
        // Dropping the listener closes its connections: keep it with `play`.
        (play, listener)
    });

    let client = async {
        let stream = dialer.dial("1".into()).await.unwrap();
        let handshake = BedrockStream::<_, Client, _>::from_transport(
            NetherNetTransport::new(stream),
            ClientConfig::default(),
        )
        .await
        .unwrap();
        let label = SocketAddr::from(([127, 0, 0, 1], 0));
        let (mut client, _) = handshake
            .join(ClientHandshakeConfig::random(label, "Nether"))
            .await
            .unwrap();
        client
            .send_packet(TextPacket::default().into())
            .await
            .unwrap();
        while !client
            .recv_packet_raw()
            .await
            .unwrap()
            .is(McpePacketName::TextPacket)
        {}
        client
    };
    let (_client, server) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(client, server)
    })
    .await
    .expect("join over NetherNet loopback timed out");
    let (server, _listener) = server.unwrap();
    assert!(server.metrics().packets_in > 0);
}
