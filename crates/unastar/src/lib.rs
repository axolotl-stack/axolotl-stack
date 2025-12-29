//! Unastar - Minecraft Bedrock Server
//!
//! The core server implementation for Minecraft Bedrock Edition.
//! Uses ECS (bevy_ecs) for entity and player state management.

pub mod command;
pub mod config;
pub mod ecs;
pub mod entity;
pub mod item;
pub mod network;
pub mod plugin;
pub mod registry;
pub mod server;
pub mod storage;
pub mod world;

pub use command::{Command, CommandArgs, CommandContext, CommandOutput, CommandRegistry};
pub use config::{ConfigError, UnastarConfig};
pub use ecs::UnastarEcs;
pub use entity::{DamageSource, HealingSource};
pub use network::{NetworkEvent, SessionId};
pub use registry::{
    biome::{BiomeEntry, BiomeRegistry},
    block::{BlockEntry, BlockRegistry},
    entity::{EntityEntry, EntityRegistry},
    item::{ItemEntry, ItemRegistry},
};
pub use server::{GameServer, PlayerSpawnData, ServerConfig, SessionEntityMap, UnastarServer};
