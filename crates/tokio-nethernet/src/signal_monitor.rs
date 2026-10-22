//! Signal-level monitoring for detecting WebRTC signaling attacks.
//!
//! This module provides detection for:
//! - CONNECTERROR injection (attackers sending error signals)
//! - Duplicate CONNECTRESPONSE (race condition attacks)
//! - ICE candidate injection from unexpected sources
//! - SDP manipulation (invalid/malformed SDPs)
//!
//! # Attack Vectors Detected
//!
//! ## CONNECTERROR 13 (FailedToSetRemoteDescription)
//! This error occurs when the client can't apply the SDP answer.
//! Possible causes:
//! - Attacker injected a fake/invalid CONNECTRESPONSE first
//! - SDP was modified in transit
//! - ICE candidates were injected that conflict with negotiation
//!
//! ## Session Hijacking via Signaling
//! The Xbox signaling WebSocket uses nethernet_id for routing.
//! If someone gains access to your signaling channel, they can:
//! - Send fake error signals to disconnect clients
//! - Race to send responses before the real server
//! - Inject ICE candidates pointing to attacker TURN servers

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::signaling::{Signal, SignalType};

/// Local monitoring memory limits, not NetherNet protocol constants.
const MAX_TRACKED_ENTRIES: usize = 1024;
const MAX_NETWORKS_PER_CONNECTION: usize = 16;
const MAX_NETWORK_ID_BYTES: usize = 256;

/// Configuration for signal monitoring.
#[derive(Debug, Clone)]
pub struct SignalMonitorConfig {
    /// Whether monitoring is enabled.
    pub enabled: bool,
    /// Time window to track signals (older signals are cleaned up).
    pub tracking_window: Duration,
    /// Maximum number of errors before alerting.
    pub error_threshold: u32,
    /// Whether to log detailed signal info.
    pub verbose: bool,
    /// Expected TURN server URLs (others are suspicious).
    pub expected_turn_urls: Vec<String>,
}

impl Default for SignalMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tracking_window: Duration::from_secs(60),
            error_threshold: 3,
            verbose: false,
            expected_turn_urls: vec![
                // Microsoft/Minecraft TURN servers
                "turn.franchise.minecraft-services.net".to_string(),
                "relay.communication.microsoft.com".to_string(),
            ],
        }
    }
}

/// A detected security anomaly in signaling.
#[derive(Debug, Clone)]
pub struct SignalAnomaly {
    /// Type of anomaly detected.
    pub anomaly_type: AnomalyType,
    /// Connection ID involved.
    pub connection_id: u64,
    /// Network ID (remote XUID) involved.
    pub network_id: String,
    /// Human-readable description.
    pub description: String,
    /// When the anomaly was detected.
    pub timestamp: Instant,
    /// Severity level (1-5, 5 being most severe).
    pub severity: u8,
}

/// Types of signaling anomalies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnomalyType {
    /// Received an error signal (CONNECTERROR).
    ErrorReceived(i32),
    /// Multiple responses for the same connection.
    DuplicateResponse,
    /// ICE candidate from unexpected network.
    UnexpectedCandidate,
    /// ICE candidate with suspicious TURN server.
    SuspiciousTurnServer,
    /// Signal from unknown/unexpected network ID.
    UnknownNetworkId,
    /// Rapid signal flood (possible DoS).
    SignalFlood,
    /// SDP appears malformed or truncated.
    MalformedSdp,
}

impl std::fmt::Display for AnomalyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnomalyType::ErrorReceived(code) => write!(f, "CONNECTERROR (code {})", code),
            AnomalyType::DuplicateResponse => write!(f, "Duplicate Response"),
            AnomalyType::UnexpectedCandidate => write!(f, "Unexpected Candidate"),
            AnomalyType::SuspiciousTurnServer => write!(f, "Suspicious TURN Server"),
            AnomalyType::UnknownNetworkId => write!(f, "Unknown Network ID"),
            AnomalyType::SignalFlood => write!(f, "Signal Flood"),
            AnomalyType::MalformedSdp => write!(f, "Malformed SDP"),
        }
    }
}

/// Tracks connection state for monitoring.
#[derive(Debug)]
struct ConnectionState {
    /// Whether we've sent a CONNECTRESPONSE.
    response_sent: bool,
    /// Whether we've received a CONNECTREQUEST.
    request_received: bool,
    /// Network IDs that have sent signals for this connection.
    seen_network_ids: HashSet<String>,
    /// Number of candidates received.
    candidate_count: u32,
    /// Number of errors received.
    error_count: u32,
    /// When this connection was first seen.
    first_seen: Instant,
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            response_sent: false,
            request_received: false,
            seen_network_ids: HashSet::new(),
            candidate_count: 0,
            error_count: 0,
            first_seen: Instant::now(),
        }
    }
}

/// Signal monitor for detecting signaling-layer attacks.
pub struct SignalMonitor {
    config: SignalMonitorConfig,
    /// Per-connection tracking.
    connections: RwLock<HashMap<u64, ConnectionState>>,
    /// Recent anomalies.
    anomalies: RwLock<Vec<SignalAnomaly>>,
    /// Signal counts per network_id for flood detection.
    signal_counts: RwLock<HashMap<String, (u32, Instant)>>,
    /// Statistics.
    stats: RwLock<MonitorStats>,
}

/// Statistics from signal monitoring.
#[derive(Debug, Default, Clone)]
pub struct MonitorStats {
    /// Total signals processed.
    pub signals_processed: u64,
    /// Total anomalies detected.
    pub anomalies_detected: u64,
    /// CONNECTERROR signals received.
    pub errors_received: u64,
    /// Suspicious TURN servers seen.
    pub suspicious_turn_count: u64,
    /// Network IDs seen sending signals.
    pub unique_network_ids: HashSet<String>,
}

impl SignalMonitor {
    /// Create a new signal monitor.
    pub fn new(config: SignalMonitorConfig) -> Self {
        Self {
            config,
            connections: RwLock::new(HashMap::new()),
            anomalies: RwLock::new(Vec::new()),
            signal_counts: RwLock::new(HashMap::new()),
            stats: RwLock::new(MonitorStats::default()),
        }
    }

    /// Create a new signal monitor wrapped in Arc with automatic periodic cleanup.
    /// The cleanup task runs every `tracking_window` duration and removes stale data.
    pub fn new_with_cleanup(config: SignalMonitorConfig) -> std::sync::Arc<Self> {
        let monitor = std::sync::Arc::new(Self::new(config.clone()));

        if config.enabled {
            let monitor_weak = std::sync::Arc::downgrade(&monitor);
            let cleanup_interval = config.tracking_window.max(Duration::from_millis(1));

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(cleanup_interval);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    interval.tick().await;

                    // Use weak reference to allow monitor to be dropped
                    if let Some(monitor) = monitor_weak.upgrade() {
                        monitor.cleanup().await;
                        tracing::trace!("Signal monitor cleanup completed");
                    } else {
                        // Monitor was dropped, exit cleanup task
                        break;
                    }
                }
            });
        }

        monitor
    }

    /// Create a monitor that's disabled (no-op).
    pub fn disabled() -> Self {
        Self::new(SignalMonitorConfig {
            enabled: false,
            ..Default::default()
        })
    }

    /// Check if monitoring is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Process an incoming signal and check for anomalies.
    pub async fn process_incoming(&self, signal: &Signal) -> Vec<SignalAnomaly> {
        if !self.config.enabled || signal.network_id.len() > MAX_NETWORK_ID_BYTES {
            return vec![];
        }

        let mut anomalies = vec![];
        let conn_id = signal.connection_id;
        let network_id = &signal.network_id;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.signals_processed = stats.signals_processed.saturating_add(1);
            if stats.unique_network_ids.len() < MAX_TRACKED_ENTRIES {
                stats.unique_network_ids.insert(network_id.clone());
            }
        }

        // Check for signal flooding
        if let Some(anomaly) = self.check_flood(network_id).await {
            anomalies.push(anomaly);
        }

        // Get or create connection state
        let mut connections = self.connections.write().await;
        if connections.len() >= MAX_TRACKED_ENTRIES && !connections.contains_key(&conn_id) {
            return anomalies;
        }
        let state = connections
            .entry(conn_id)
            .or_insert_with(ConnectionState::new);

        // Track network IDs for this connection
        let is_new_network = !state.seen_network_ids.contains(network_id);
        if state.seen_network_ids.len() < MAX_NETWORKS_PER_CONNECTION {
            state.seen_network_ids.insert(network_id.clone());
        }

        match signal.typ {
            SignalType::Offer => {
                // CONNECTREQUEST from remote
                if state.request_received && is_new_network {
                    // Multiple networks sending offers for same connection - suspicious
                    anomalies.push(SignalAnomaly {
                        anomaly_type: AnomalyType::UnknownNetworkId,
                        connection_id: conn_id,
                        network_id: network_id.clone(),
                        description: format!(
                            "CONNECTREQUEST from unexpected network {} (already saw request)",
                            network_id
                        ),
                        timestamp: Instant::now(),
                        severity: 3,
                    });
                }
                state.request_received = true;

                // Check SDP validity
                if let Some(anomaly) = self.check_sdp_validity(conn_id, network_id, &signal.data) {
                    anomalies.push(anomaly);
                }
            }
            SignalType::Answer => {
                // We shouldn't receive CONNECTRESPONSE as a listener - we send them
                // If we receive one, someone is trying to act as a fake server
                anomalies.push(SignalAnomaly {
                    anomaly_type: AnomalyType::DuplicateResponse,
                    connection_id: conn_id,
                    network_id: network_id.clone(),
                    description: format!(
                        "Received CONNECTRESPONSE as listener from {} - possible hijack attempt",
                        network_id
                    ),
                    timestamp: Instant::now(),
                    severity: 5,
                });
            }
            SignalType::Candidate => {
                state.candidate_count = state.candidate_count.saturating_add(1);

                // Check if candidate is from expected network
                if is_new_network && state.request_received {
                    anomalies.push(SignalAnomaly {
                        anomaly_type: AnomalyType::UnexpectedCandidate,
                        connection_id: conn_id,
                        network_id: network_id.clone(),
                        description: format!(
                            "ICE candidate from unexpected network {} (original request from different network)",
                            network_id
                        ),
                        timestamp: Instant::now(),
                        severity: 4,
                    });
                }

                // Check for suspicious TURN servers in candidate
                if let Some(anomaly) = self.check_candidate_turn(conn_id, network_id, &signal.data)
                {
                    anomalies.push(anomaly);
                }
            }
            SignalType::Error => {
                state.error_count = state.error_count.saturating_add(1);

                let error_code =
                    crate::signaling::parse_signal_error_code(&signal.data).unwrap_or(-1);

                // Update stats
                {
                    let mut stats = self.stats.write().await;
                    stats.errors_received = stats.errors_received.saturating_add(1);
                }

                // Log error details
                let error_name = match error_code {
                    0 => "None",
                    1 => "DestinationNotLoggedIn",
                    2 => "NegotiationTimeout",
                    3 => "WrongTransportVersion",
                    4 => "FailedToCreatePeerConnection",
                    5 => "Ice",
                    6 => "ConnectRequest",
                    7 => "ConnectResponse",
                    8 => "CandidateAdd",
                    9 => "InactivityTimeout",
                    10 => "FailedToCreateOffer",
                    11 => "FailedToCreateAnswer",
                    12 => "FailedToSetLocalDescription",
                    13 => "FailedToSetRemoteDescription",
                    14 => "NegotiationTimeoutWaitingForResponse",
                    15 => "NegotiationTimeoutWaitingForAccept",
                    16 => "IncomingConnectionIgnored",
                    17 => "SignalingParsingFailure",
                    18 => "SignalingUnknownError",
                    19 => "SignalingUnicastMessageDeliveryFailed",
                    20 => "SignalingBroadcastDeliveryFailed",
                    21 => "SignalingMessageDeliveryFailed",
                    22 => "SignalingTurnAuthFailed",
                    23 => "SignalingFallbackToBestEffortDelivery",
                    24 => "NoSignalingChannel",
                    25 => "NotLoggedIn",
                    26 => "SignalingFailedToSend",
                    _ => "Unknown",
                };

                let severity = match error_code {
                    13 => 5, // FailedToSetRemoteDescription - likely attack
                    5 => 4,  // ICE failure - could be attack
                    22 => 4, // TURN auth failed - could be credential attack
                    _ => 3,
                };

                anomalies.push(SignalAnomaly {
                    anomaly_type: AnomalyType::ErrorReceived(error_code),
                    connection_id: conn_id,
                    network_id: network_id.clone(),
                    description: format!(
                        "CONNECTERROR {} ({}) from {} - {} errors for this connection",
                        error_code, error_name, network_id, state.error_count
                    ),
                    timestamp: Instant::now(),
                    severity,
                });

                // Special handling for error 13
                if error_code == 13 {
                    warn!(
                        conn_id,
                        network_id = %network_id,
                        "⚠️ CONNECTERROR 13 (FailedToSetRemoteDescription) - possible SDP/signaling attack!"
                    );
                }
            }
        }

        // Store anomalies
        if !anomalies.is_empty() {
            let mut stored = self.anomalies.write().await;
            let remaining = MAX_TRACKED_ENTRIES.saturating_sub(stored.len());
            stored.extend(anomalies.iter().take(remaining).cloned());

            // Update stats
            let mut stats = self.stats.write().await;
            stats.anomalies_detected = stats
                .anomalies_detected
                .saturating_add(anomalies.len() as u64);

            // Log anomalies
            for anomaly in &anomalies {
                match anomaly.severity {
                    5 => error!(
                        "🚨 CRITICAL: {} - {} (conn_id: {}, network: {})",
                        anomaly.anomaly_type,
                        anomaly.description,
                        anomaly.connection_id,
                        anomaly.network_id
                    ),
                    4 => warn!(
                        "⚠️ HIGH: {} - {} (conn_id: {}, network: {})",
                        anomaly.anomaly_type,
                        anomaly.description,
                        anomaly.connection_id,
                        anomaly.network_id
                    ),
                    3 => warn!(
                        "⚠️ MEDIUM: {} - {} (conn_id: {}, network: {})",
                        anomaly.anomaly_type,
                        anomaly.description,
                        anomaly.connection_id,
                        anomaly.network_id
                    ),
                    _ => info!(
                        "ℹ️ LOW: {} - {} (conn_id: {}, network: {})",
                        anomaly.anomaly_type,
                        anomaly.description,
                        anomaly.connection_id,
                        anomaly.network_id
                    ),
                }
            }
        }

        anomalies
    }

    /// Process an outgoing signal (for tracking state).
    pub async fn process_outgoing(&self, signal: &Signal) {
        if !self.config.enabled || signal.network_id.len() > MAX_NETWORK_ID_BYTES {
            return;
        }

        let conn_id = signal.connection_id;

        let mut connections = self.connections.write().await;
        if connections.len() >= MAX_TRACKED_ENTRIES && !connections.contains_key(&conn_id) {
            return;
        }
        let state = connections
            .entry(conn_id)
            .or_insert_with(ConnectionState::new);

        if signal.typ == SignalType::Answer {
            state.response_sent = true;
            if self.config.verbose {
                debug!(conn_id, "Marked response sent for connection");
            }
        }
    }

    /// Check for signal flooding from a network ID.
    async fn check_flood(&self, network_id: &str) -> Option<SignalAnomaly> {
        let mut counts = self.signal_counts.write().await;
        let now = Instant::now();
        if counts.len() >= MAX_TRACKED_ENTRIES && !counts.contains_key(network_id) {
            return None;
        }

        let (count, first_seen) = counts.entry(network_id.to_string()).or_insert((0, now));

        *count = count.saturating_add(1);

        // Check if too many signals in short time
        let elapsed = first_seen.elapsed();
        if elapsed < Duration::from_secs(1) && *count > 50 {
            return Some(SignalAnomaly {
                anomaly_type: AnomalyType::SignalFlood,
                connection_id: 0,
                network_id: network_id.to_string(),
                description: format!(
                    "{} signals from {} in {:?} - possible DoS",
                    count, network_id, elapsed
                ),
                timestamp: now,
                severity: 4,
            });
        }

        // Reset counter periodically
        if elapsed > Duration::from_secs(5) {
            *count = 1;
            *first_seen = now;
        }

        None
    }

    /// Check SDP validity.
    fn check_sdp_validity(
        &self,
        conn_id: u64,
        network_id: &str,
        sdp: &str,
    ) -> Option<SignalAnomaly> {
        // Check for required fields
        if !sdp.contains("a=ice-ufrag:") {
            return Some(SignalAnomaly {
                anomaly_type: AnomalyType::MalformedSdp,
                connection_id: conn_id,
                network_id: network_id.to_string(),
                description: "SDP missing ice-ufrag - truncated or malformed".to_string(),
                timestamp: Instant::now(),
                severity: 4,
            });
        }

        if !sdp.contains("a=ice-pwd:") {
            return Some(SignalAnomaly {
                anomaly_type: AnomalyType::MalformedSdp,
                connection_id: conn_id,
                network_id: network_id.to_string(),
                description: "SDP missing ice-pwd - truncated or malformed".to_string(),
                timestamp: Instant::now(),
                severity: 4,
            });
        }

        // Check for suspiciously short SDP
        if sdp.len() < 100 {
            return Some(SignalAnomaly {
                anomaly_type: AnomalyType::MalformedSdp,
                connection_id: conn_id,
                network_id: network_id.to_string(),
                description: format!("SDP suspiciously short ({} bytes)", sdp.len()),
                timestamp: Instant::now(),
                severity: 3,
            });
        }

        None
    }

    /// Check ICE candidate for suspicious TURN servers.
    fn check_candidate_turn(
        &self,
        conn_id: u64,
        network_id: &str,
        candidate: &str,
    ) -> Option<SignalAnomaly> {
        // Look for relay candidates (TURN servers)
        if !candidate.contains("typ relay") {
            return None;
        }

        // Extract address from candidate
        // Format: candidate:... <address> <port> typ relay raddr ...
        let parts: Vec<&str> = candidate.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }

        let relay_addr = parts[4];

        // Check if this is an expected TURN server
        let is_expected =
            self.config.expected_turn_urls.iter().any(|url| {
                candidate.contains(url) || relay_addr.contains(&url.replace("turn:", ""))
            });

        // Also allow Microsoft Azure TURN servers (20.x.x.x range)
        // Microsoft uses these for their relay infrastructure
        let is_azure_relay = relay_addr
            .split('.')
            .next()
            .map(|first_octet| first_octet == "20")
            .unwrap_or(false);

        if !is_expected && !is_azure_relay && !relay_addr.is_empty() {
            // Update stats
            // Note: Can't await in sync function, so we'll just return the anomaly
            return Some(SignalAnomaly {
                anomaly_type: AnomalyType::SuspiciousTurnServer,
                connection_id: conn_id,
                network_id: network_id.to_string(),
                description: format!(
                    "ICE candidate uses unexpected TURN server: {} - possible MITM",
                    relay_addr
                ),
                timestamp: Instant::now(),
                severity: 5,
            });
        }

        None
    }

    /// Get recent anomalies.
    pub async fn get_anomalies(&self) -> Vec<SignalAnomaly> {
        let anomalies = self.anomalies.read().await;
        let cutoff = Instant::now() - self.config.tracking_window;
        anomalies
            .iter()
            .filter(|a| a.timestamp > cutoff)
            .cloned()
            .collect()
    }

    /// Get current statistics.
    pub async fn get_stats(&self) -> MonitorStats {
        self.stats.read().await.clone()
    }

    /// Clear old tracking data.
    pub async fn cleanup(&self) {
        let cutoff = Instant::now() - self.config.tracking_window;

        // Clean up old connections
        {
            let mut connections = self.connections.write().await;
            connections.retain(|_, state| state.first_seen > cutoff);
        }

        // Clean up old anomalies
        {
            let mut anomalies = self.anomalies.write().await;
            anomalies.retain(|a| a.timestamp > cutoff);
        }

        // Clean up signal counts
        {
            let mut counts = self.signal_counts.write().await;
            counts.retain(|_, (_, first_seen)| *first_seen > cutoff);
        }
    }

    /// Get a summary of monitoring status.
    pub async fn summary(&self) -> String {
        let (signals_processed, anomalies_detected, errors_received, unique_networks) = {
            let stats = self.stats.read().await;
            (
                stats.signals_processed,
                stats.anomalies_detected,
                stats.errors_received,
                stats.unique_network_ids.len(),
            )
        };
        let anomalies = self.anomalies.read().await;

        let recent_critical = anomalies.iter().filter(|a| a.severity >= 4).count();

        format!(
            "Signal Monitor: {} signals, {} anomalies ({} critical), {} errors, {} unique networks",
            signals_processed,
            anomalies_detected,
            recent_critical,
            errors_received,
            unique_networks
        )
    }
}

impl Default for SignalMonitor {
    fn default() -> Self {
        Self::new(SignalMonitorConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn summary_does_not_hold_stats_lock_while_waiting_for_anomalies() {
        use futures::FutureExt;
        let monitor = SignalMonitor::default();
        let anomalies = monitor.anomalies.write().await;
        let mut summary = Box::pin(monitor.summary());
        assert!(summary.as_mut().now_or_never().is_none());
        assert!(monitor.stats.try_write().is_ok());
        drop(anomalies);
        summary.await;
    }

    #[tokio::test]
    async fn peer_controlled_tracking_is_bounded() {
        let monitor = SignalMonitor::new(SignalMonitorConfig {
            enabled: true,
            ..Default::default()
        });
        for id in 0..1100 {
            let signal = Signal {
                typ: SignalType::Offer,
                connection_id: id,
                network_id: id.to_string(),
                data: "bad".into(),
            };
            monitor.process_incoming(&signal).await;
            monitor.process_outgoing(&signal).await;
        }
        assert!(monitor.connections.read().await.len() <= 1024);
        assert!(monitor.signal_counts.read().await.len() <= 1024);
        assert!(monitor.stats.read().await.unique_network_ids.len() <= 1024);
        assert!(monitor.anomalies.read().await.len() <= 1024);
        for id in 0..100 {
            monitor
                .process_incoming(&Signal {
                    typ: SignalType::Candidate,
                    connection_id: 0,
                    network_id: id.to_string(),
                    data: String::new(),
                })
                .await;
        }
        assert!(monitor.connections.read().await[&0].seen_network_ids.len() <= 16);
    }

    #[tokio::test]
    async fn zero_tracking_window_does_not_panic() {
        let monitor = SignalMonitor::new_with_cleanup(SignalMonitorConfig {
            enabled: true,
            tracking_window: Duration::ZERO,
            ..Default::default()
        });
        tokio::task::yield_now().await;
        drop(monitor);
    }

    #[tokio::test]
    async fn test_error_detection() {
        let monitor = SignalMonitor::new(SignalMonitorConfig {
            enabled: true,
            ..Default::default()
        });

        let signal = Signal {
            typ: SignalType::Error,
            connection_id: 123,
            data: "13".to_string(), // FailedToSetRemoteDescription
            network_id: "attacker_xuid".to_string(),
        };

        let anomalies = monitor.process_incoming(&signal).await;
        assert!(!anomalies.is_empty());
        assert!(matches!(
            anomalies[0].anomaly_type,
            AnomalyType::ErrorReceived(13)
        ));
        assert_eq!(anomalies[0].severity, 5);
    }

    #[tokio::test]
    async fn test_malformed_sdp_detection() {
        let monitor = SignalMonitor::new(SignalMonitorConfig {
            enabled: true,
            ..Default::default()
        });

        let signal = Signal {
            typ: SignalType::Offer,
            connection_id: 123,
            data: "v=0\r\n".to_string(), // Missing ice-ufrag
            network_id: "client_xuid".to_string(),
        };

        let anomalies = monitor.process_incoming(&signal).await;
        assert!(!anomalies.is_empty());
        assert!(matches!(
            anomalies[0].anomaly_type,
            AnomalyType::MalformedSdp
        ));
    }

    #[tokio::test]
    async fn test_disabled_monitor() {
        let monitor = SignalMonitor::disabled();

        let signal = Signal {
            typ: SignalType::Error,
            connection_id: 123,
            data: "13".to_string(),
            network_id: "attacker".to_string(),
        };

        let anomalies = monitor.process_incoming(&signal).await;
        assert!(anomalies.is_empty()); // Should not detect when disabled
    }

    #[tokio::test]
    async fn test_duplicate_response_detection() {
        let monitor = SignalMonitor::new(SignalMonitorConfig {
            enabled: true,
            ..Default::default()
        });

        // Receiving CONNECTRESPONSE as a listener is always suspicious
        let signal = Signal {
            typ: SignalType::Answer,
            connection_id: 123,
            data: "fake_sdp".to_string(),
            network_id: "attacker".to_string(),
        };

        let anomalies = monitor.process_incoming(&signal).await;
        assert!(!anomalies.is_empty());
        assert!(matches!(
            anomalies[0].anomaly_type,
            AnomalyType::DuplicateResponse
        ));
        assert_eq!(anomalies[0].severity, 5);
    }
}
