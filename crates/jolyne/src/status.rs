//! Server status (MOTD) advertised to RakNet pings and NetherNet LAN discovery.
//!
//! Mirrors gophertunnel's `ServerStatusProvider`: the listener asks the
//! provider for a fresh [`ServerStatus`] every `status_refresh` interval and
//! whenever a player joins or leaves.

use crate::valentine::{GAME_VERSION, PROTOCOL_VERSION};

/// Game mode shown in the server list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusGameMode {
    /// Survival mode (game type `0`).
    #[default]
    Survival,
    /// Creative mode (game type `1`).
    Creative,
    /// Adventure mode (game type `2`).
    Adventure,
    /// Spectator mode (game type `6`).
    Spectator,
}

impl StatusGameMode {
    fn name(self) -> &'static str {
        match self {
            Self::Survival => "Survival",
            Self::Creative => "Creative",
            Self::Adventure => "Adventure",
            Self::Spectator => "Spectator",
        }
    }

    #[cfg(feature = "discovery")]
    fn id(self) -> i32 {
        match self {
            Self::Survival => 0,
            Self::Creative => 1,
            Self::Adventure => 2,
            Self::Spectator => 6,
        }
    }
}

/// What the server list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    /// Primary MOTD line; `;` is stripped from the RakNet pong.
    pub server_name: String,
    /// Secondary line (world/level name); `;` is stripped from the RakNet pong.
    pub sub_name: String,
    /// Players currently online.
    pub player_count: usize,
    /// Zero is advertised as `player_count + 1` (gophertunnel behaviour).
    pub max_players: usize,
    /// Game mode shown next to the server entry.
    pub game_mode: StatusGameMode,
}

impl ServerStatus {
    fn advertised_max(&self) -> usize {
        if self.max_players == 0 {
            self.player_count + 1
        } else {
            self.max_players
        }
    }

    /// The RakNet unconnected-pong payload, in gophertunnel's field order.
    /// The field after the game mode must be `1` or clients drop the pong.
    pub fn raknet_pong(&self, server_guid: u64, port: u16) -> String {
        format!(
            "MCPE;{};{};{};{};{};{};{};{};1;{};{};0;0;",
            self.server_name.replace(';', ""),
            PROTOCOL_VERSION,
            GAME_VERSION,
            self.player_count,
            self.advertised_max(),
            server_guid,
            self.sub_name.replace(';', ""),
            self.game_mode.name(),
            port,
            port,
        )
    }

    /// NetherNet LAN discovery data.
    #[cfg(feature = "discovery")]
    pub fn lan_server_data(&self) -> tokio_nethernet::discovery::ServerData {
        let mut data = tokio_nethernet::discovery::ServerData::builder()
            .name(self.server_name.clone())
            .level(self.sub_name.clone())
            .players(
                i32::try_from(self.player_count).unwrap_or(i32::MAX),
                i32::try_from(self.advertised_max()).unwrap_or(i32::MAX),
            )
            .game_type(self.game_mode.id())
            .build();
        data.protocol = PROTOCOL_VERSION;
        data.version = GAME_VERSION.to_string();
        data
    }
}

/// Supplies the advertised status.
pub trait StatusProvider: Send + Sync {
    /// `player_count` and `max_players` are the listener's live numbers.
    fn status(&self, player_count: usize, max_players: usize) -> ServerStatus;
}

/// A fixed name and sub-name with live player counts
/// (gophertunnel `NewStatusProvider`).
#[derive(Debug, Clone)]
pub struct StaticStatus {
    /// Primary MOTD line.
    pub server_name: String,
    /// Secondary line (world/level name).
    pub sub_name: String,
    /// Advertised game mode.
    pub game_mode: StatusGameMode,
}

impl StaticStatus {
    /// A status with the given name lines in survival mode.
    pub fn new(server_name: impl Into<String>, sub_name: impl Into<String>) -> Self {
        Self {
            server_name: server_name.into(),
            sub_name: sub_name.into(),
            game_mode: StatusGameMode::Survival,
        }
    }
}

impl StatusProvider for StaticStatus {
    fn status(&self, player_count: usize, max_players: usize) -> ServerStatus {
        ServerStatus {
            server_name: self.server_name.clone(),
            sub_name: self.sub_name.clone(),
            player_count,
            max_players,
            game_mode: self.game_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pong_matches_gophertunnel_layout_and_unlimited_max() {
        let status = StaticStatus::new("A;B", "Jolyne").status(3, 0);
        assert_eq!(
            status.raknet_pong(42, 19132),
            format!(
                "MCPE;AB;{PROTOCOL_VERSION};{GAME_VERSION};3;4;42;Jolyne;Survival;1;19132;19132;0;0;"
            )
        );
    }

    #[cfg(feature = "discovery")]
    #[test]
    fn lan_server_data_carries_names_counts_and_version() {
        let data = StaticStatus::new("LAN", "World")
            .status(2, 8)
            .lan_server_data();
        assert_eq!(data.server_name, "LAN");
        assert_eq!(data.level_name, "World");
        assert_eq!((data.player_count, data.max_player_count), (2, 8));
        assert_eq!(data.protocol, PROTOCOL_VERSION);
        assert_eq!(data.version, GAME_VERSION);
        let decoded = tokio_nethernet::discovery::ServerData::decode(&data.encode()).unwrap();
        assert_eq!(decoded.server_name, "LAN");
    }
}
