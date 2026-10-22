//! NETHERNET-03 regression: an independent WebRTC peer with pion v4's SRTP
//! preference.
//!
//! df-mc/go-nethernet uses pion, whose `constants.go:defaultSrtpProtectionProfiles`
//! offers SRTP_AEAD_AES_256_GCM first. With webrtc-rs 0.12 that profile was
//! keyed with AES-128 inside webrtc-srtp 0.14 and panicked, so SCTP never
//! started and dials timed out. The remote here is a raw webrtc-rs 0.12
//! answerer (an independent stack) offering pion's exact profile list as DTLS
//! client. Our DTLS server must answer with a profile both sides can key
//! (AES-128-GCM): webrtc-rs 0.12 itself still panics on AES-256-GCM.

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use tokio_nethernet::{
    Message, NetherNetDialer, NetherNetDialerConfig, NetherNetListener, NetherNetListenerConfig,
    Signal, Signaling, SignalingChannel, TransportConfig, signal_type,
};
use webrtc::api::APIBuilder;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::dtls::extension::extension_use_srtp::SrtpProtectionProfile;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

struct Pipe {
    id: &'static str,
    sender: mpsc::Sender<Signal>,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
}

#[async_trait]
impl Signaling for Pipe {
    async fn signal(&self, mut signal: Signal) -> anyhow::Result<()> {
        // The transport delivers the sender's network ID, not the destination.
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
impl SignalingChannel for Pipe {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.receiver.lock().await.take()
    }
}

fn loopback_settings() -> SettingEngine {
    let mut settings = SettingEngine::default();
    settings.set_include_loopback_candidate(true);
    settings.set_ip_filter(Box::new(|ip| ip.is_loopback() && ip.is_ipv4()));
    settings
}

/// Answers one NetherNet offer and echoes every frame verbatim (header included).
/// The reply sender is returned: closing signaling would end the pending dial.
async fn pion_profile_answerer(
    mut offers: mpsc::Receiver<Signal>,
    replies: mpsc::Sender<Signal>,
) -> (
    Arc<webrtc::peer_connection::RTCPeerConnection>,
    mpsc::Sender<Signal>,
) {
    let offer = offers.recv().await.expect("offer");
    assert_eq!(offer.typ, signal_type::OFFER);
    let mut settings = loopback_settings();
    settings.set_srtp_protection_profiles(vec![
        SrtpProtectionProfile::Srtp_Aead_Aes_256_Gcm,
        SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm,
        SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_80,
    ]);
    let pc = Arc::new(
        APIBuilder::new()
            .with_setting_engine(settings)
            .build()
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap(),
    );
    pc.on_data_channel(Box::new(|channel| {
        let echo = channel.clone();
        channel.on_message(Box::new(move |message| {
            let echo = echo.clone();
            Box::pin(async move {
                let _ = echo.send(&message.data).await;
            })
        }));
        Box::pin(async {})
    }));
    pc.set_remote_description(RTCSessionDescription::offer(offer.data).unwrap())
        .await
        .unwrap();
    let answer = pc.create_answer(None).await.unwrap();
    let mut gathered = pc.gathering_complete_promise().await;
    pc.set_local_description(answer).await.unwrap();
    gathered.recv().await;
    let mut sdp = pc.local_description().await.unwrap().sdp;
    if !sdp.contains("a=max-message-size:") {
        sdp.push_str("a=max-message-size:65536\r\n");
    }
    replies
        .send(Signal {
            typ: signal_type::ANSWER.into(),
            connection_id: offer.connection_id,
            data: sdp,
            network_id: "pion".into(),
        })
        .await
        .unwrap();
    (pc, replies)
}

#[tokio::test]
async fn dialer_connects_to_peer_preferring_aes_256_gcm_srtp() {
    let (to_peer, peer_rx) = mpsc::channel(16);
    let (to_dialer, dialer_rx) = mpsc::channel(16);
    let dialer = NetherNetDialer::connect_with_signaling(
        Pipe {
            id: "dialer",
            sender: to_peer,
            receiver: Mutex::new(Some(dialer_rx)),
        },
        NetherNetDialerConfig {
            transport: TransportConfig::loopback_v4(),
            allow_identityless_server: true,
            connection_timeout: Duration::from_secs(10),
            ..Default::default()
        },
    );
    let peer = tokio::spawn(pion_profile_answerer(peer_rx, to_dialer));
    let mut stream = dialer
        .dial("pion".into())
        .await
        .expect("dial must complete with a keyable SRTP profile");
    let payload = bytes::Bytes::from_static(b"nethernet interop");
    stream
        .send(Message::reliable(payload.clone()))
        .await
        .unwrap();
    let echoed = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("echo timed out")
        .expect("stream ended")
        .unwrap();
    assert!(echoed.reliable);
    assert_eq!(echoed.buffer, payload);
    stream.close().await.unwrap();
    let (pc, _signaling) = peer.await.unwrap();
    pc.close().await.unwrap();
}

/// Our listener answering an independent webrtc-rs 0.12 offerer that opens
/// the two NetherNet channels like libwebrtc clients do.
#[tokio::test]
async fn listener_accepts_independent_webrtc_rs_offerer() {
    let (to_listener, listener_rx) = mpsc::channel(16);
    let (to_peer, mut peer_rx) = mpsc::channel(16);
    let mut listener = NetherNetListener::bind_with_signaling(
        Pipe {
            id: "listener",
            sender: to_peer,
            receiver: Mutex::new(Some(listener_rx)),
        },
        NetherNetListenerConfig {
            transport: TransportConfig::loopback_v4(),
            allow_identityless_client: true,
            ..Default::default()
        },
    );
    let pc = Arc::new(
        APIBuilder::new()
            .with_setting_engine(loopback_settings())
            .build()
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap(),
    );
    let reliable = pc
        .create_data_channel("ReliableDataChannel", None)
        .await
        .unwrap();
    pc.create_data_channel(
        "UnreliableDataChannel",
        Some(RTCDataChannelInit {
            ordered: Some(false),
            max_retransmits: Some(0),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let (opened_tx, mut opened_rx) = mpsc::channel(1);
    reliable.on_open(Box::new(move || {
        Box::pin(async move {
            let _ = opened_tx.send(()).await;
        })
    }));
    let (frames_tx, mut frames_rx) = mpsc::channel(4);
    reliable.on_message(Box::new(move |message| {
        let frames_tx = frames_tx.clone();
        Box::pin(async move {
            let _ = frames_tx.send(message.data).await;
        })
    }));
    let offer = pc.create_offer(None).await.unwrap();
    let mut gathered = pc.gathering_complete_promise().await;
    pc.set_local_description(offer).await.unwrap();
    gathered.recv().await;
    let mut sdp = pc.local_description().await.unwrap().sdp;
    if !sdp.contains("a=max-message-size:") {
        sdp.push_str("a=max-message-size:65536\r\n");
    }
    to_listener
        .send(Signal {
            typ: signal_type::OFFER.into(),
            connection_id: 7,
            data: sdp,
            network_id: "peer".into(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        let answer = peer_rx.recv().await.unwrap();
        assert_eq!(answer.typ, signal_type::ANSWER);
        pc.set_remote_description(RTCSessionDescription::answer(answer.data).unwrap())
            .await
            .unwrap();
        let mut accepted = listener.accept().await.unwrap();
        opened_rx.recv().await.unwrap();
        // One NetherNet segment: countdown byte 0, then the payload.
        reliable
            .send(&bytes::Bytes::from_static(b"\0hello"))
            .await
            .unwrap();
        assert_eq!(
            accepted.next().await.unwrap().unwrap().buffer.as_ref(),
            b"hello"
        );
        accepted
            .send(Message::reliable(bytes::Bytes::from_static(b"back")))
            .await
            .unwrap();
        assert_eq!(frames_rx.recv().await.unwrap().as_ref(), b"\0back");
    })
    .await
    .expect("independent offerer exchange timed out");
    pc.close().await.unwrap();
}
