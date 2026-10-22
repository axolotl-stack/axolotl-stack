//! TURN relay and server-reflexive gathering against an independent TURN
//! server (webrtc-rs `turn` 0.9) on loopback. The dialer uses the relay-only
//! ICE policy, so every ICE check, DTLS record and SCTP packet it exchanges
//! must traverse the allocation (permissions, Send/Data indications and
//! ChannelData), as Xbox Live joins do through Microsoft's TURN relays.

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, mpsc};
use tokio_nethernet::{
    IceServer, Message, NetherNetDialer, NetherNetDialerConfig, NetherNetListener,
    NetherNetListenerConfig, Signal, SignalType, Signaling, SignalingChannel, TransportConfig,
};
use webrtc::turn::auth::{AuthHandler, generate_auth_key};
use webrtc::turn::relay::relay_static::RelayAddressGeneratorStatic;
use webrtc::turn::server::Server;
use webrtc::turn::server::config::{ConnConfig, ServerConfig};
use webrtc::util::vnet::net::Net;

const REALM: &str = "nethernet";

struct Auth(Vec<u8>);

impl AuthHandler for Auth {
    fn auth_handle(
        &self,
        username: &str,
        _: &str,
        _: SocketAddr,
    ) -> Result<Vec<u8>, webrtc::turn::Error> {
        if username == "user" {
            Ok(self.0.clone())
        } else {
            Err(webrtc::turn::Error::ErrFakeErr)
        }
    }
}

async fn turn_server() -> (Server, SocketAddr) {
    let conn = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = conn.local_addr().unwrap();
    let server = Server::new(ServerConfig {
        conn_configs: vec![ConnConfig {
            conn,
            relay_addr_generator: Box::new(RelayAddressGeneratorStatic {
                relay_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
                address: "127.0.0.1".to_owned(),
                net: Arc::new(Net::new(None)),
            }),
        }],
        realm: REALM.to_owned(),
        auth_handler: Arc::new(Auth(generate_auth_key("user", REALM, "pass"))),
        channel_bind_timeout: Duration::from_secs(0),
        alloc_close_notify: None,
    })
    .await
    .unwrap();
    (server, addr)
}

struct Local {
    id: &'static str,
    sender: mpsc::Sender<Signal>,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
    observed: Arc<Mutex<Vec<String>>>,
    trickle: bool,
}

#[async_trait]
impl Signaling for Local {
    async fn signal(&self, mut signal: Signal) -> anyhow::Result<()> {
        if matches!(signal.typ, SignalType::Offer | SignalType::Candidate) && self.id == "2" {
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
        !self.trickle
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

/// Dials through a fresh TURN server; returns the dialer's signaled offer
/// and trickled candidates, and the negotiation time.
async fn dial_through_turn(
    relay_only: bool,
    trickle: bool,
    urls: impl Fn(SocketAddr) -> Vec<String>,
) -> (Vec<String>, Duration) {
    let (server, turn) = turn_server().await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let (to_server, server_rx) = mpsc::channel(16);
    let (to_client, client_rx) = mpsc::channel(16);
    let mut listener = NetherNetListener::bind_with_signaling(
        Local {
            id: "1",
            sender: to_client,
            receiver: Mutex::new(Some(server_rx)),
            observed: observed.clone(),
            trickle,
        },
        NetherNetListenerConfig {
            transport: TransportConfig::loopback_v4(),
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
            trickle,
        },
        NetherNetDialerConfig {
            transport: TransportConfig {
                relay_only,
                ..TransportConfig::loopback_v4()
            },
            ice_servers: vec![IceServer {
                username: "user".into(),
                password: "pass".into(),
                urls: urls(turn),
            }],
            allow_identityless_server: true,
            ..Default::default()
        },
    );
    let started = std::time::Instant::now();
    let elapsed = tokio::time::timeout(Duration::from_secs(20), async {
        let (dialed, accepted) = tokio::join!(dialer.dial("1".into()), listener.accept());
        let mut dialed = dialed.expect("dial through TURN");
        let mut accepted = accepted.expect("accept through TURN");
        let elapsed = started.elapsed();
        let large = bytes::Bytes::from((0..100_000).map(|n| n as u8).collect::<Vec<_>>());
        dialed.send(Message::reliable(large.clone())).await.unwrap();
        assert_eq!(accepted.next().await.unwrap().unwrap().buffer, large);
        accepted
            .send(Message::reliable(bytes::Bytes::from_static(
                b"relayed reply",
            )))
            .await
            .unwrap();
        assert_eq!(
            dialed.next().await.unwrap().unwrap().buffer.as_ref(),
            b"relayed reply"
        );
        elapsed
    })
    .await
    .expect("relayed negotiation timed out");
    server.close().await.unwrap();
    let signals = observed.lock().await.clone();
    (signals, elapsed)
}

fn candidates(signals: &[String]) -> Vec<String> {
    signals
        .iter()
        .flat_map(|signal| signal.lines())
        .filter_map(|line| line.trim().strip_prefix("a=").or(Some(line.trim())))
        .filter(|line| line.starts_with("candidate:"))
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn relay_only_dialer_exchanges_messages_through_turn() {
    let (signals, _) = dial_through_turn(true, false, |turn| {
        vec![format!("turn:{turn}?transport=udp")]
    })
    .await;
    // The non-trickle offer carried only the relay candidate.
    let candidates = candidates(&signals);
    assert_eq!(candidates.len(), 1, "{candidates:?}");
    assert!(
        candidates[0].contains(" typ relay raddr "),
        "{}",
        candidates[0]
    );
}

/// Xbox credentials list `stun:` and `turn:` for one host:port. Both answers
/// come from the same address, so they must reach the right client: a
/// misrouted response would stall gathering until its 5 s timeout.
#[tokio::test]
async fn same_address_stun_and_turn_gather_without_timeout() {
    let (signals, elapsed) = dial_through_turn(false, false, |turn| {
        vec![format!("stun:{turn}"), format!("turn:{turn}?transport=udp")]
    })
    .await;
    let candidates = candidates(&signals);
    assert!(
        candidates.iter().any(|c| c.contains(" typ relay ")),
        "{candidates:?}"
    );
    assert!(
        candidates.iter().any(|c| c.contains(" typ host ")),
        "{candidates:?}"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "gathering stalled: {elapsed:?}"
    );
}

/// Trickle mode resolves ICE servers after the offer and trickles the relay
/// candidate as CANDIDATEADD.
#[tokio::test]
async fn trickled_relay_candidate_connects_relay_only_dialer() {
    let (signals, _) = dial_through_turn(true, true, |turn| {
        vec![format!("turn:{turn}?transport=udp")]
    })
    .await;
    assert!(
        !signals[0].contains("a=candidate:"),
        "offer must not inline candidates"
    );
    let candidates = candidates(&signals[1..]);
    assert_eq!(candidates.len(), 1, "{candidates:?}");
    assert!(candidates[0].contains(" typ relay "), "{}", candidates[0]);
}
