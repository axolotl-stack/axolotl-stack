//! Type definitions for the game server.
//!
//! Contains resource types, spawn data, and helper functions.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::McpePacket;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::config::PlayerLastPosition;
use crate::network::SessionId;

use jolyne::valentine::{
    TextPacket, TextPacketCategory, TextPacketContent, TextPacketContentAuthored, TextPacketExtra,
    TextPacketExtraAnnouncement, TextPacketType,
};

/// Wrapper for WorldTemplate to serve as an ECS Resource.
#[derive(Resource)]
pub struct ServerWorldTemplate(pub Arc<jolyne::WorldTemplate>);

/// Wrapper for ItemRegistry to serve as an ECS Resource.
#[derive(Resource)]
pub struct ItemRegistryResource(pub Arc<crate::registry::ItemRegistry>);

/// Wrapper for BlockRegistry to serve as an ECS Resource.
#[derive(Resource)]
pub struct BlockRegistryResource(pub Arc<crate::registry::BlockRegistry>);

/// Mapping from session ID to ECS entity.
#[derive(Resource, Default)]
pub struct SessionEntityMap {
    map: HashMap<SessionId, Entity>,
}

impl SessionEntityMap {
    pub fn insert(&mut self, session_id: SessionId, entity: Entity) {
        self.map.insert(session_id, entity);
    }

    pub fn remove(&mut self, session_id: SessionId) -> Option<Entity> {
        self.map.remove(&session_id)
    }

    pub fn get(&self, session_id: SessionId) -> Option<Entity> {
        self.map.get(&session_id).copied()
    }

    pub fn contains(&self, session_id: SessionId) -> bool {
        self.map.contains_key(&session_id)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (SessionId, Entity)> + '_ {
        self.map.iter().map(|(&k, &v)| (k, v))
    }
}

/// Data needed to spawn a player entity.
pub struct PlayerSpawnData {
    pub session_id: SessionId,
    pub display_name: String,
    pub xuid: Option<String>,
    pub uuid: Option<String>,
    pub runtime_id: i64,
    pub position: DVec3,
    /// Bounded outbound channel to prevent memory explosion on slow connections.
    pub outbound_tx: mpsc::Sender<McpePacket>,
    pub chunk_radius: i32,
}

/// Data for persisting player state (position, etc.)
pub struct PlayerPersistenceData {
    pub uuid: String,
    pub last_position: PlayerLastPosition,
}

/// Create a system text message packet.
pub fn system_text(message: &str) -> TextPacket {
    TextPacket {
        needs_translation: false,
        category: TextPacketCategory::Authored,
        content: Some(TextPacketContent::Authored(TextPacketContentAuthored {
            chat: "chat".to_string(),
            whisper: "whisper".to_string(),
            announcement: "announcement".to_string(),
        })),
        type_: TextPacketType::Chat,
        extra: Some(TextPacketExtra::Chat(TextPacketExtraAnnouncement {
            source_name: "§ePlugin§r".to_string(),
            message: message.to_string(),
        })),
        xuid: "0".to_string(),
        platform_chat_id: String::new(),
        filtered_message: None,
    }
}
