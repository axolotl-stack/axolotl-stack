//! An actual encoded play pump closes even when its peer ignores the terminal notice.

use super::*;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{Datagram, Framing, Transmit, Transport};
use jolyne::stream::{BedrockStream, Client, Handshake, Server};
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

/// Bounded framed bytes preserve the actual protocol driver, encryption and codec paths.
struct Pipe {
    tx: PollSender<Transmit>,
    rx: mpsc::Receiver<Transmit>,
}
#[derive(Debug, thiserror::Error)]
#[error("test transport closed")]
struct Closed;
impl Transport for Pipe {
    type Error = Closed;
    const FRAMING: Framing = Framing::RakNet;
    fn poll_send_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Closed>> {
        self.tx.poll_reserve(cx).map_err(|_| Closed)
    }
    fn start_send(mut self: Pin<&mut Self>, message: Transmit) -> Result<(), Closed> {
        self.tx.send_item(message).map_err(|_| Closed)
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Closed>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Closed>> {
        self.tx.close();
        Poll::Ready(Ok(()))
    }
    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Datagram, Closed>>> {
        self.rx
            .poll_recv(cx)
            .map(|message| message.map(|message| Ok(Datagram::Contiguous(message.payload))))
    }
    fn peer_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 19132))
    }
}
/// Pair two independent bounded transports without any socket, account service or application process.
fn pair() -> (Pipe, Pipe) {
    let (a_tx, a_rx) = mpsc::channel(64);
    let (b_tx, b_rx) = mpsc::channel(64);
    (
        Pipe {
            tx: PollSender::new(a_tx),
            rx: b_rx,
        },
        Pipe {
            tx: PollSender::new(b_tx),
            rx: a_rx,
        },
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn component_request_closes_the_real_pump_before_the_peer_reads_and_preserves_quit_policy() {
    tokio::time::timeout(Duration::from_secs(10),async {
        let (server_io,client_io)=pair();
        let server=tokio::spawn(async move {let key=p384::SecretKey::random(&mut rand::thread_rng());BedrockStream::<Handshake,Server,_>::from_transport(server_io,Arc::new(jolyne::BedrockListenerConfig{online_mode:false,..Default::default()})).accept_join_sequence(&jolyne::WorldTemplate::default(),&key).await.unwrap().0});
        let mut login=ClientHandshakeConfig::random(SocketAddr::from((Ipv4Addr::LOCALHOST,19132)),"Player");login.uuid=uuid::Uuid::from_u128(42);
        let client=BedrockStream::<Handshake,Client,_>::from_transport(client_io,jolyne::ClientConfig::default()).await.unwrap();let (mut client,_)=client.join(login).await.unwrap();let mut server=server.await.unwrap();
        assert_eq!(server.identity().uuid(),uuid::Uuid::from_u128(42));assert!(server.identity().xuid().is_none());server.set_flush_mode(jolyne::FlushMode::Manual).await.unwrap();
        let mut h=Harness::with_runtime(close_command(false,false),String::new(),true,server.runtime_id() as i64).await;
        h.world_mut().resource_mut::<crate::server::game::player_events::PlayerEventHooks>().register(Retain);
        let (_dummy,dummy_rx)=mpsc::channel(1);let outbound=std::mem::replace(&mut h.packets,dummy_rx);
        let (events,mut inbound)=mpsc::channel(64);let (ticks,tick_rx)=tokio::sync::broadcast::channel(16);
        let pump=tokio::spawn(crate::network::run_network_loop(server,42,events,outbound,tick_rx));
        client.send_packet(CommandRequestPacket{command:"/notice".into(),origin:types::CommandOriginDatajson{type_:"player".into(),request_id:"network-42".into(),..Default::default()},is_internal:false,version:"latest".into()}.into()).await.unwrap();client.flush().await.unwrap();
        loop {if let crate::network::NetworkEvent::Packet{session_id,packet_args,packet}=inbound.recv().await.unwrap() && packet.packet_id()==McpePacketName::CommandRequestPacket {assert_eq!(session_id,42);h.router.route_packet(session_id,packet_args,*packet);h.router.tick().await.unwrap();break;}}
        // A terminal request can already have flushed and closed the pump.
        // A missing tick receiver is valid; the checks below still require its
        // real departure, transport completion and exact terminal notice.
        let _ = ticks.send(());
        // Deliberately do not read or acknowledge the terminal notice yet.
        loop {if matches!(inbound.recv().await.unwrap(),crate::network::NetworkEvent::Disconnected{session_id:42}){break;}}
        pump.await.unwrap();
        let error=loop {match client.recv_packet_borrowed().await {Ok(_)=>(),Err(error)=>break error}};
        assert!(matches!(error,jolyne::JolyneError::Disconnected(ref detail) if detail.reason==EnumsConnectionDisconnectFailReason::Kicked && detail.message.as_deref()==Some("Example")));
        assert!(h.router.prepare_disconnect(42));h.router.despawn_player(42);
        assert!(h.world().get::<PlayerSession>(h.player).is_none());assert!(h.world().get::<crate::server::game::player_events::DetachedPlayer>(h.player).is_some());
    }).await.unwrap();
}
