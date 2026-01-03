use crate::Vec3;
use abi_stable::{
    sabi_trait,
    std_types::{ROption, RStr, RString},
    StableAbi,
};

/// Opaque stable handle for an Entity.
/// Corresponds to bevy_ecs::entity::Entity (u64 bits).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, StableAbi)]
pub struct PluginEntity {
    pub id: u64,
}

impl From<bevy_ecs::entity::Entity> for PluginEntity {
    fn from(e: bevy_ecs::entity::Entity) -> Self {
        Self { id: e.to_bits() }
    }
}

impl PluginEntity {
    pub fn to_bits(&self) -> u64 {
        self.id
    }
}

/// Snapshot of player data returned by the host.
#[repr(C)]
#[derive(StableAbi)]
pub struct PlayerInfo {
    pub uuid: RString,
    pub name: RString,
    pub position: ROption<Vec3>,
}

/// Interface for the host server functionality.
/// This trait serves as the VTable for FFI.
#[sabi_trait]
pub trait RawPluginHost: Send + Sync {
    /// Send a chat message to a player.
    fn send_message(&mut self, player_uuid: RStr<'_>, message: RStr<'_>);

    /// Teleport a player.
    fn teleport(&mut self, player_uuid: RStr<'_>, position: Vec3);

    /// Get the number of entities in the world.
    fn entity_count(&self) -> u32;

    /// Get player info (name, pos, etc.) by entity ID.
    fn get_player_info(&self, entity: PluginEntity) -> ROption<PlayerInfo>;

    /// Kick a player from the server.
    fn kick(&mut self, player_uuid: RStr<'_>, reason: RStr<'_>);

    /// Give an item to a player.
    fn give_item(&mut self, player_uuid: RStr<'_>, item_id: RStr<'_>, count: u8);
}

use abi_stable::std_types::RBox;

#[repr(C)]
#[derive(StableAbi)]
pub struct NativeGameContext<'a> {
    // Hold the host trait object as an RBox (erased type)
    pub(crate) host: RawPluginHost_TO<'a, RBox<()>>,
}

impl<'a> NativeGameContext<'a> {
    pub fn new(host: RawPluginHost_TO<'a, RBox<()>>) -> Self {
        Self { host }
    }

    /// Get the number of entities in the world.
    pub fn entity_count(&self) -> u32 {
        self.host.entity_count()
    }

    /// Get player info (name, pos, etc.) by entity ID.
    pub fn get_player_info(&self, entity: PluginEntity) -> Option<PlayerInfo> {
        self.host.get_player_info(entity).into_option()
    }
}

// Add user-friendly helpers for PlayerInfo since fields are RString
impl PlayerInfo {
    pub fn uuid(&self) -> &str {
        self.uuid.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn position(&self) -> Option<Vec3> {
        self.position.into_option()
    }
}
