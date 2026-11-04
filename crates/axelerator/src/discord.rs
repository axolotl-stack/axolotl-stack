//! Discord webhook notifications for Axelerator.
//!
//! Sends critical error notifications to a Discord channel via webhook.
//! This is optional and only activates if a webhook URL is configured.

use crate::config::DiscordWebhookConfig;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

/// Notification severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    /// Informational (startup, shutdown)
    Info,
    /// Warning (recoverable issues)
    Warning,
    /// Error (auth failures, crashes)
    Error,
    /// Critical (unrecoverable, needs immediate attention)
    Critical,
}

impl NotificationLevel {
    /// Get Discord embed color for this level.
    fn color(&self) -> u32 {
        match self {
            NotificationLevel::Info => 0x3498db,     // Blue
            NotificationLevel::Warning => 0xf39c12,  // Orange
            NotificationLevel::Error => 0xe74c3c,    // Red
            NotificationLevel::Critical => 0x9b59b6, // Purple
        }
    }

    /// Get emoji prefix for this level.
    fn emoji(&self) -> &'static str {
        match self {
            NotificationLevel::Info => ":information_source:",
            NotificationLevel::Warning => ":warning:",
            NotificationLevel::Error => ":x:",
            NotificationLevel::Critical => ":rotating_light:",
        }
    }
}

/// Discord embed structure.
#[derive(Debug, Serialize)]
struct DiscordEmbed {
    title: String,
    description: String,
    color: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    footer: Option<EmbedFooter>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<EmbedField>,
}

#[derive(Debug, Serialize)]
struct EmbedFooter {
    text: String,
}

#[derive(Debug, Serialize)]
struct EmbedField {
    name: String,
    value: String,
    inline: bool,
}

/// Discord webhook payload.
#[derive(Debug, Serialize)]
struct WebhookPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    embeds: Vec<DiscordEmbed>,
}

/// What a notification reports. Each kind has its own rate limit, so a
/// startup message never suppresses a crash report that follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationKind {
    Startup,
    Shutdown,
    AuthError,
    Crash,
    Tampering,
    /// [`DiscordNotifier::notify`] and [`DiscordNotifier::notify_with_fields`].
    Custom,
}

/// Per-kind rate limit. A slot is taken when a notification is raised,
/// before it is posted, so concurrent notifications of one kind cannot all
/// pass the check, and a failing webhook is not retried more often than the
/// limit allows.
struct RateLimiter {
    interval: Duration,
    last: std::sync::Mutex<HashMap<NotificationKind, Instant>>,
}

impl RateLimiter {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn try_acquire(&self, kind: NotificationKind, now: Instant) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        match last.get(&kind) {
            Some(previous) if now.saturating_duration_since(*previous) < self.interval => false,
            _ => {
                last.insert(kind, now);
                true
            }
        }
    }
}

/// Discord webhook notifier.
///
/// Handles sending notifications to Discord with a per-kind rate limit.
/// Posting is non-blocking; [`DiscordNotifier::flush`] waits for posts still
/// in flight (call it before the process exits).
#[derive(Clone)]
pub struct DiscordNotifier {
    config: DiscordWebhookConfig,
    client: reqwest::Client,
    rate_limit: Arc<RateLimiter>,
    in_flight: Arc<std::sync::Mutex<Vec<JoinHandle<()>>>>,
    gamertag: Arc<RwLock<Option<String>>>,
}

impl DiscordNotifier {
    /// Create a new Discord notifier.
    pub fn new(config: DiscordWebhookConfig) -> Self {
        let rate_limit = Arc::new(RateLimiter::new(Duration::from_secs(
            config.rate_limit_secs,
        )));
        Self {
            config,
            client: reqwest::Client::new(),
            rate_limit,
            in_flight: Arc::default(),
            gamertag: Arc::new(RwLock::new(None)),
        }
    }

    /// Check if notifications are enabled and configured.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.config.webhook_url.is_some()
    }

    /// Set the gamertag for context in notifications.
    pub async fn set_gamertag(&self, gamertag: String) {
        *self.gamertag.write().await = Some(gamertag);
    }

    /// Send a notification (non-blocking, fire-and-forget).
    pub fn notify(&self, level: NotificationLevel, title: &str, message: &str) {
        self.send(NotificationKind::Custom, level, title, message, Vec::new());
    }

    /// Waits up to `timeout` for notifications that are still being posted.
    pub async fn flush(&self, timeout: Duration) {
        let pending: Vec<_> =
            std::mem::take(&mut *self.in_flight.lock().unwrap_or_else(|e| e.into_inner()));
        if pending.is_empty() {
            return;
        }
        if tokio::time::timeout(timeout, futures::future::join_all(pending))
            .await
            .is_err()
        {
            warn!("Timed out waiting for Discord notifications to be sent");
        }
    }

    fn send(
        &self,
        kind: NotificationKind,
        level: NotificationLevel,
        title: &str,
        message: &str,
        fields: Vec<(String, String, bool)>,
    ) {
        if !self.is_enabled() {
            return;
        }
        let Some(url) = self.config.webhook_url.clone() else {
            return;
        };
        if !self.rate_limit.try_acquire(kind, Instant::now()) {
            debug!(?kind, "Discord notification rate limited");
            return;
        }

        let username = self.config.username.clone();
        let avatar_url = self.config.avatar_url.clone();
        let client = self.client.clone();
        let gamertag = self.gamertag.clone();
        let title = title.to_string();
        let message = message.to_string();

        let handle = tokio::spawn(async move {
            let footer_text = match gamertag.read().await.as_deref() {
                Some(gt) => format!("Account: {} | Axelerator", gt),
                None => "Axelerator".to_string(),
            };

            let embed = DiscordEmbed {
                title: format!("{} {}", level.emoji(), title),
                description: message,
                color: level.color(),
                footer: Some(EmbedFooter { text: footer_text }),
                fields: fields
                    .into_iter()
                    .map(|(name, value, inline)| EmbedField {
                        name,
                        value,
                        inline,
                    })
                    .collect(),
            };

            let payload = WebhookPayload {
                username,
                avatar_url,
                embeds: vec![embed],
            };

            match client.post(&url).json(&payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    debug!("Discord notification sent successfully");
                }
                Ok(resp) => {
                    warn!(
                        status = %resp.status(),
                        "Discord webhook returned non-success status"
                    );
                }
                // The error text can contain the webhook URL (a credential).
                Err(_) => warn!("Failed to send Discord notification"),
            }
        });
        let mut in_flight = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        in_flight.retain(|handle| !handle.is_finished());
        in_flight.push(handle);
    }

    /// Send startup notification.
    pub fn notify_startup(&self, server_ip: &str, server_port: u16) {
        if !self.config.notify_on_startup {
            return;
        }

        self.send(
            NotificationKind::Startup,
            NotificationLevel::Info,
            "Axelerator Started",
            &format!(
                "Server is now online and advertising to Xbox Live friends.\n\n\
                **Target Server:** `{}:{}`",
                server_ip, server_port
            ),
            Vec::new(),
        );
    }

    /// Send shutdown notification.
    pub fn notify_shutdown(&self, reason: &str) {
        if !self.config.notify_on_shutdown {
            return;
        }

        self.send(
            NotificationKind::Shutdown,
            NotificationLevel::Info,
            "Axelerator Stopped",
            &format!("Server has shut down.\n\n**Reason:** {}", reason),
            Vec::new(),
        );
    }

    /// Send authentication error notification.
    pub fn notify_auth_error(&self, error: &str) {
        if !self.config.notify_on_auth_error {
            return;
        }

        self.send(
            NotificationKind::AuthError,
            NotificationLevel::Error,
            "Authentication Failed",
            &format!(
                "Token refresh failed after multiple attempts. Manual intervention may be required.\n\n\
                **Error:** ```{}```",
                error
            ),
            Vec::new(),
        );
    }

    /// Send crash/fatal error notification.
    pub fn notify_crash(&self, error: &str) {
        if !self.config.notify_on_crash {
            return;
        }

        self.send(
            NotificationKind::Crash,
            NotificationLevel::Critical,
            "Fatal Error",
            &format!(
                "Axelerator encountered an unrecoverable error and is shutting down.\n\n\
                **Error:** ```{}```",
                error
            ),
            Vec::new(),
        );
    }

    /// Send tampering detection notification.
    pub fn notify_tampering(&self, details: &str) {
        if !self.config.notify_on_tampering {
            return;
        }

        self.send(
            NotificationKind::Tampering,
            NotificationLevel::Warning,
            "Session Tampering Detected",
            &format!(
                "Suspicious activity detected on the Xbox Live session.\n\n\
                **Details:**\n{}",
                details
            ),
            Vec::new(),
        );
    }

    /// Send a custom notification with fields.
    pub fn notify_with_fields(
        &self,
        level: NotificationLevel,
        title: &str,
        message: &str,
        fields: Vec<(String, String, bool)>,
    ) {
        self.send(NotificationKind::Custom, level, title, message, fields);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The limit used to be one timestamp shared by every kind, so a startup
    /// message suppressed a crash report raised within the window.
    #[test]
    fn rate_limit_is_per_kind() {
        let limiter = RateLimiter::new(Duration::from_secs(60));
        let now = Instant::now();
        assert!(limiter.try_acquire(NotificationKind::Startup, now));
        assert!(limiter.try_acquire(NotificationKind::Crash, now));
        assert!(!limiter.try_acquire(NotificationKind::Startup, now + Duration::from_secs(1)));
        assert!(limiter.try_acquire(NotificationKind::Startup, now + Duration::from_secs(60)));
    }

    /// The timestamp used to be written only after a successful post, so
    /// notifications raised while one was in flight (or while the webhook
    /// kept failing) all passed the check.
    #[test]
    fn slot_is_taken_when_raised_not_after_posting() {
        let limiter = RateLimiter::new(Duration::from_secs(60));
        let now = Instant::now();
        assert!(limiter.try_acquire(NotificationKind::Crash, now));
        assert!(!limiter.try_acquire(NotificationKind::Crash, now));
    }

    #[tokio::test]
    async fn disabled_notifier_posts_nothing() {
        let notifier = DiscordNotifier::new(DiscordWebhookConfig::default());
        notifier.notify_crash("boom");
        assert!(notifier.in_flight.lock().unwrap().is_empty());
        notifier.flush(Duration::from_millis(10)).await;
    }
}
