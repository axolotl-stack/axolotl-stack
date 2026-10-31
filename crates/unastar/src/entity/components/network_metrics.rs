//! Recent transport RTT observations, bound to the exact live connection.

use super::PlayerSession;
use bevy_ecs::prelude::*;
use jolyne::valentine::McpePacket;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{Sender, WeakSender};

/// A measured transport estimate and when the network pump observed it.
/// `observed_at` is the read time, not an invented underlying probe timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkRttSample {
    pub round_trip_time: Duration,
    pub observed_at: Instant,
}

/// Read-only network metrics for one player connection. Missing/unsupported RTT,
/// expired observations and a replaced transport yield `None`, never a default zero.
#[derive(Component, Clone)]
pub struct PlayerNetworkMetrics {
    session_id: u64,
    outbound: WeakSender<McpePacket>,
    latest: Arc<RwLock<Option<NetworkRttSample>>>,
}

impl std::fmt::Debug for PlayerNetworkMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerNetworkMetrics")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl PlayerNetworkMetrics {
    /// Read a sufficiently recent observation for the same live session.
    pub fn sample(&self, session: &PlayerSession, max_age: Duration) -> Option<NetworkRttSample> {
        self.sample_at(session, Instant::now(), max_age)
    }

    /// Read at an explicit monotonic time; future, stale and unrelated samples
    /// are rejected. This keeps freshness decisions deterministic in tests.
    pub fn sample_at(
        &self,
        session: &PlayerSession,
        now: Instant,
        max_age: Duration,
    ) -> Option<NetworkRttSample> {
        if !self.matches(session) {
            return None;
        }
        let sample = (*self.latest.read().ok()?)?;
        (now.checked_duration_since(sample.observed_at)? <= max_age).then_some(sample)
    }

    /// Bind a read handle and its single pump-owned producer without retaining
    /// the outbound sender or keeping a departed transport alive.
    pub(crate) fn bind(
        session_id: u64,
        outbound: &Sender<McpePacket>,
    ) -> (Self, NetworkMetricsPublisher) {
        let latest = Arc::new(RwLock::new(None));
        (
            Self {
                session_id,
                outbound: outbound.downgrade(),
                latest: latest.clone(),
            },
            NetworkMetricsPublisher { latest },
        )
    }

    /// Require both logical session identity and the original transport channel.
    pub(crate) fn matches(&self, session: &PlayerSession) -> bool {
        self.session_id == session.session_id
            && self
                .outbound
                .upgrade()
                .is_some_and(|source| source.same_channel(&session.outbound_tx))
    }
}

/// Owned exclusively by the actual network pump; native applications only read.
pub(crate) struct NetworkMetricsPublisher {
    latest: Arc<RwLock<Option<NetworkRttSample>>>,
}

impl NetworkMetricsPublisher {
    /// Publish the actual transport report, preserving unsupported/unmeasured None.
    pub(crate) fn observe(&self, rtt: Option<Duration>, now: Instant) {
        if let Ok(mut latest) = self.latest.write() {
            *latest = rtt.map(|round_trip_time| NetworkRttSample {
                round_trip_time,
                observed_at: now,
            });
        }
    }

    /// Mark a disconnected or cancelled pump as having no current measurement.
    pub(crate) fn clear(&self) {
        if let Ok(mut latest) = self.latest.write() {
            *latest = None;
        }
    }
}

impl Drop for NetworkMetricsPublisher {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One synthetic session with an actual bounded outbound transport channel.
    fn session() -> (PlayerSession, tokio::sync::mpsc::Receiver<McpePacket>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        (PlayerSession::new(7, "Peer".into(), None, None, tx), rx)
    }

    /// A real Jolyne transport implementation with a controlled measurement report.
    struct ReportedLatency(Option<Duration>);
    impl jolyne::stream::Transport for ReportedLatency {
        type Error = std::io::Error;
        const FRAMING: jolyne::stream::transport::Framing =
            jolyne::stream::transport::Framing::RakNet;
        fn poll_send_ready(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn start_send(
            self: std::pin::Pin<&mut Self>,
            _: jolyne::stream::transport::Transmit,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_recv(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Result<jolyne::stream::transport::Datagram, Self::Error>>>
        {
            std::task::Poll::Pending
        }
        fn peer_addr(&self) -> std::net::SocketAddr {
            ([127, 0, 0, 1], 19132).into()
        }
        fn latency(&self) -> Option<Duration> {
            self.0
        }
    }

    #[test]
    fn jolyne_transport_observations_retain_optional_measured_rtt_exactly() {
        let (session, _rx) = session();
        let (metrics, publisher) = PlayerNetworkMetrics::bind(7, &session.outbound_tx);
        let now = Instant::now();
        for measured in [None, Some(Duration::from_micros(12_345))] {
            let link = jolyne::stream::BedrockTransport::new(
                ReportedLatency(measured),
                jolyne::config::ConnectionOptions::default(),
            );
            publisher.observe(link.latency(), now);
            assert_eq!(
                metrics
                    .sample_at(&session, now, Duration::from_secs(1))
                    .map(|sample| sample.round_trip_time),
                measured
            );
        }
    }

    #[test]
    fn queued_metrics_bind_only_to_the_current_native_entity_and_transport() {
        use crate::server::game::{GameServer, ServerConfig, SessionEntityMap};
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let (session, _rx) = session();
        let (old, publisher) = PlayerNetworkMetrics::bind(7, &session.outbound_tx);
        let actor = server
            .ecs
            .world_mut()
            .spawn((super::super::Player, session))
            .id();
        server
            .ecs
            .world_mut()
            .resource_mut::<SessionEntityMap>()
            .insert(7, actor);
        assert!(server.attach_network_metrics(7, old.clone()));
        let (new_tx, _new_rx) = tokio::sync::mpsc::channel(8);
        let replacement = PlayerSession::new(7, "Peer".into(), None, None, new_tx);
        let (new, new_publisher) = PlayerNetworkMetrics::bind(7, &replacement.outbound_tx);
        server.ecs.world_mut().entity_mut(actor).insert(replacement);
        assert!(!server.attach_network_metrics(7, old));
        assert!(server.attach_network_metrics(7, new));
        let now = Instant::now();
        new_publisher.observe(Some(Duration::from_millis(4)), now);
        drop(publisher);
        let world = server.ecs.world();
        let session = world.get::<PlayerSession>(actor).unwrap();
        assert_eq!(
            world
                .get::<PlayerNetworkMetrics>(actor)
                .unwrap()
                .sample_at(session, now, Duration::from_secs(1))
                .unwrap()
                .round_trip_time,
            Duration::from_millis(4)
        );
    }

    #[test]
    fn reports_only_measured_fresh_values_and_clears_unsupported_or_departed_pumps() {
        let (session, _rx) = session();
        let (metrics, publisher) = PlayerNetworkMetrics::bind(7, &session.outbound_tx);
        let now = Instant::now();
        let ttl = Duration::from_secs(2);
        assert_eq!(metrics.sample_at(&session, now, ttl), None);
        publisher.observe(Some(Duration::from_millis(37)), now);
        assert_eq!(
            metrics
                .sample_at(&session, now + ttl, ttl)
                .unwrap()
                .round_trip_time,
            Duration::from_millis(37)
        );
        assert_eq!(
            metrics.sample_at(&session, now + ttl + Duration::from_nanos(1), ttl),
            None
        );
        assert_eq!(
            metrics.sample_at(&session, now - Duration::from_nanos(1), ttl),
            None
        );
        publisher.observe(None, now);
        assert_eq!(metrics.sample_at(&session, now, ttl), None);
        publisher.observe(Some(Duration::from_millis(12)), now);
        drop(publisher);
        assert_eq!(metrics.sample_at(&session, now, ttl), None);
    }

    #[test]
    fn same_number_replacement_channels_never_inherit_metrics_or_keep_senders_alive() {
        let (mut session, _rx) = session();
        let (metrics, publisher) = PlayerNetworkMetrics::bind(7, &session.outbound_tx);
        assert_eq!(session.outbound_tx.strong_count(), 1);
        let now = Instant::now();
        publisher.observe(Some(Duration::from_millis(8)), now);
        session.session_id = 8;
        assert_eq!(
            metrics.sample_at(&session, now, Duration::from_secs(1)),
            None
        );
        session.session_id = 7;
        let (tx, _new_rx) = tokio::sync::mpsc::channel(8);
        session.outbound_tx = tx;
        assert_eq!(
            metrics.sample_at(&session, now, Duration::from_secs(1)),
            None
        );
    }
}
