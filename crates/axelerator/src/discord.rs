//! Discord webhook notifications for Axelerator.
//!
//! Sends critical error notifications to a Discord channel via webhook.
//! This is optional and only activates if a webhook URL is configured.

use crate::config::DiscordWebhookConfig;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
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

/// Discord webhook notifier.
///
/// Handles sending notifications to Discord with rate limiting.
/// All operations are non-blocking and fire-and-forget.
pub struct DiscordNotifier {
    config: DiscordWebhookConfig,
    client: reqwest::Client,
    last_notification: Arc<RwLock<Option<Instant>>>,
    gamertag: Arc<RwLock<Option<String>>>,
}

impl DiscordNotifier {
    /// Create a new Discord notifier.
    pub fn new(config: DiscordWebhookConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            last_notification: Arc::new(RwLock::new(None)),
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
        if !self.is_enabled() {
            return;
        }

        // Clone what we need for the async task
        let config = self.config.clone();
        let client = self.client.clone();
        let last_notification = self.last_notification.clone();
        let gamertag = self.gamertag.clone();
        let rate_limit_secs = self.config.rate_limit_secs;
        let title = title.to_string();
        let message = message.to_string();

        tokio::spawn(async move {
            // Check rate limit
            {
                let last = last_notification.read().await;
                if let Some(last_time) = *last
                    && last_time.elapsed() < Duration::from_secs(rate_limit_secs)
                {
                    debug!("Discord notification rate limited");
                    return;
                }
            }

            // Build the embed
            let gamertag_str = gamertag.read().await.clone();
            let footer_text = if let Some(gt) = gamertag_str {
                format!("Account: {} | Axelerator", gt)
            } else {
                "Axelerator".to_string()
            };

            let embed = DiscordEmbed {
                title: format!("{} {}", level.emoji(), title),
                description: message,
                color: level.color(),
                footer: Some(EmbedFooter { text: footer_text }),
                fields: Vec::new(),
            };

            let payload = WebhookPayload {
                username: config.username.clone(),
                avatar_url: config.avatar_url.clone(),
                embeds: vec![embed],
            };

            // Send the webhook
            if let Some(url) = &config.webhook_url {
                match client.post(url).json(&payload).send().await {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            debug!("Discord notification sent successfully");
                            // Update rate limit
                            *last_notification.write().await = Some(Instant::now());
                        } else {
                            warn!(
                                status = %resp.status(),
                                "Discord webhook returned non-success status"
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send Discord notification: {}", e);
                    }
                }
            }
        });
    }

    /// Send startup notification.
    pub fn notify_startup(&self, server_ip: &str, server_port: u16) {
        if !self.config.notify_on_startup {
            return;
        }

        self.notify(
            NotificationLevel::Info,
            "Axelerator Started",
            &format!(
                "Server is now online and advertising to Xbox Live friends.\n\n\
                **Target Server:** `{}:{}`",
                server_ip, server_port
            ),
        );
    }

    /// Send shutdown notification.
    pub fn notify_shutdown(&self, reason: &str) {
        if !self.config.notify_on_shutdown {
            return;
        }

        self.notify(
            NotificationLevel::Info,
            "Axelerator Stopped",
            &format!("Server has shut down.\n\n**Reason:** {}", reason),
        );
    }

    /// Send authentication error notification.
    pub fn notify_auth_error(&self, error: &str) {
        if !self.config.notify_on_auth_error {
            return;
        }

        self.notify(
            NotificationLevel::Error,
            "Authentication Failed",
            &format!(
                "Token refresh failed after multiple attempts. Manual intervention may be required.\n\n\
                **Error:** ```{}```",
                error
            ),
        );
    }

    /// Send crash/fatal error notification.
    pub fn notify_crash(&self, error: &str) {
        if !self.config.notify_on_crash {
            return;
        }

        self.notify(
            NotificationLevel::Critical,
            "Fatal Error",
            &format!(
                "Axelerator encountered an unrecoverable error and is shutting down.\n\n\
                **Error:** ```{}```",
                error
            ),
        );
    }

    /// Send tampering detection notification.
    pub fn notify_tampering(&self, details: &str) {
        if !self.config.notify_on_tampering {
            return;
        }

        self.notify(
            NotificationLevel::Warning,
            "Session Tampering Detected",
            &format!(
                "Suspicious activity detected on the Xbox Live session.\n\n\
                **Details:**\n{}",
                details
            ),
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
        if !self.is_enabled() {
            return;
        }

        let config = self.config.clone();
        let client = self.client.clone();
        let last_notification = self.last_notification.clone();
        let gamertag = self.gamertag.clone();
        let rate_limit_secs = self.config.rate_limit_secs;
        let title = title.to_string();
        let message = message.to_string();

        tokio::spawn(async move {
            // Check rate limit
            {
                let last = last_notification.read().await;
                if let Some(last_time) = *last
                    && last_time.elapsed() < Duration::from_secs(rate_limit_secs)
                {
                    debug!("Discord notification rate limited");
                    return;
                }
            }

            let gamertag_str = gamertag.read().await.clone();
            let footer_text = if let Some(gt) = gamertag_str {
                format!("Account: {} | Axelerator", gt)
            } else {
                "Axelerator".to_string()
            };

            let embed_fields: Vec<EmbedField> = fields
                .into_iter()
                .map(|(name, value, inline)| EmbedField {
                    name,
                    value,
                    inline,
                })
                .collect();

            let embed = DiscordEmbed {
                title: format!("{} {}", level.emoji(), title),
                description: message,
                color: level.color(),
                footer: Some(EmbedFooter { text: footer_text }),
                fields: embed_fields,
            };

            let payload = WebhookPayload {
                username: config.username.clone(),
                avatar_url: config.avatar_url.clone(),
                embeds: vec![embed],
            };

            if let Some(url) = &config.webhook_url {
                match client.post(url).json(&payload).send().await {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            debug!("Discord notification sent successfully");
                            *last_notification.write().await = Some(Instant::now());
                        } else {
                            warn!(
                                status = %resp.status(),
                                "Discord webhook returned non-success status"
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send Discord notification: {}", e);
                    }
                }
            }
        });
    }
}

impl Clone for DiscordNotifier {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            client: self.client.clone(),
            last_notification: self.last_notification.clone(),
            gamertag: self.gamertag.clone(),
        }
    }
}
