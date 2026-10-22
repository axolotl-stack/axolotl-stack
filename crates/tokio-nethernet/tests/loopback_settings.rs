use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_nethernet::{
    Message, NetherNetDialer, NetherNetDialerConfig, NetherNetListener, NetherNetListenerConfig,
    Signal, SignalType, Signaling, SignalingChannel, TransportConfig,
};

struct Local {
    id: &'static str,
    sender: mpsc::Sender<Signal>,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
    observed: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Signaling for Local {
    async fn signal(&self, mut signal: Signal) -> anyhow::Result<()> {
        if matches!(signal.typ, SignalType::Offer | SignalType::Answer) {
            self.observed.lock().await.push(signal.data.clone());
        }
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

#[tokio::test]
async fn custom_settings_restrict_both_peers_to_ipv4_loopback() {
    let settings = TransportConfig::loopback_v4();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let (to_server, server_rx) = mpsc::channel(16);
    let (to_client, client_rx) = mpsc::channel(16);
    let mut server = NetherNetListener::bind_with_signaling(
        Local {
            id: "1",
            sender: to_client,
            receiver: Mutex::new(Some(server_rx)),
            observed: observed.clone(),
        },
        NetherNetListenerConfig {
            transport: settings.clone(),
            allow_identityless_client: true,
            ..Default::default()
        },
    );
    let dialer = NetherNetDialer::connect_with_signaling(
        Local {
            id: "2",
            sender: to_server,
            receiver: Mutex::new(Some(client_rx)),
            observed: observed.clone(),
        },
        NetherNetDialerConfig {
            transport: settings,
            allow_identityless_server: true,
            ..Default::default()
        },
    );
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let (client, accepted) = tokio::join!(dialer.dial("1".into()), server.accept());
        let mut client = client.unwrap();
        let mut accepted = accepted.unwrap();
        client
            .send(Message::reliable(bytes::Bytes::from_static(
                b"loopback settings",
            )))
            .await
            .unwrap();
        assert_eq!(
            accepted.next().await.unwrap().unwrap().buffer,
            "loopback settings"
        );
    })
    .await
    .expect("loopback negotiation timed out");
    let sdps = observed.lock().await;
    assert_eq!(sdps.len(), 2, "offer and answer must both be observed");
    for sdp in sdps.iter() {
        let addresses: Vec<_> = sdp
            .lines()
            .filter(|line| line.starts_with("a=candidate:"))
            .map(|line| line.split_whitespace().nth(4).expect("candidate IP"))
            .collect();
        assert!(
            !addresses.is_empty(),
            "configured peers must advertise a usable candidate"
        );
        assert!(
            addresses.iter().all(|ip| *ip == "127.0.0.1"),
            "unexpected candidates: {addresses:?}"
        );
    }
}
