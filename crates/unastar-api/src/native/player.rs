use super::{PluginEntity, RawPluginHost_TO, Vec3};
use abi_stable::{
    std_types::{RBox, ROption, RStr, RString},
    StableAbi,
};

/// Wrapper around a player entity with cached component data.
#[repr(C)]
#[derive(StableAbi)]
pub struct Player<'a> {
    pub entity: PluginEntity,
    pub(crate) host: RawPluginHost_TO<'a, RBox<()>>,

    // Cached component data (extracted server-side)
    pub name: ROption<RString>,
    pub position: ROption<Vec3>,
    pub uuid: ROption<RString>,
}

impl<'a> Player<'a> {
    /// Create a new Player wrapper with component data.
    pub fn new(
        entity: PluginEntity,
        host: RawPluginHost_TO<'a, RBox<()>>,
        name: Option<String>,
        position: Option<Vec3>,
        uuid: Option<String>,
    ) -> Self {
        Self {
            entity,
            host,
            name: name.map(RString::from).into(),
            position: position.into(),
            uuid: uuid.map(RString::from).into(),
        }
    }

    // ===== Accessors =====

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().into_option().map(|s| s.as_str())
    }

    pub fn position(&self) -> Option<Vec3> {
        self.position.into_option()
    }

    pub fn uuid(&self) -> Option<&str> {
        self.uuid.as_ref().into_option().map(|s| s.as_str())
    }

    // ===== Action Methods =====

    /// Send a chat message to this player.
    pub fn message(&mut self, message: impl Into<String>) {
        if let Some(uuid) = self.uuid.as_ref().into_option().map(|s| s.as_str()) {
            let msg = message.into();
            self.host.send_message(uuid.into(), msg.as_str().into());
        }
    }

    /// Teleport this player to a position.
    pub fn teleport(&mut self, position: Vec3) {
        if let Some(uuid) = self.uuid.as_ref().into_option().map(|s| s.as_str()) {
            self.host.teleport(uuid.into(), position);
        }
    }

    /// Kick this player from the server.
    pub fn kick(&mut self, reason: impl Into<String>) {
        if let Some(uuid) = self.uuid.as_ref().into_option().map(|s| s.as_str()) {
            let r = reason.into();
            self.host.kick(uuid.into(), r.as_str().into());
        }
    }

    /// Give an item to this player.
    pub fn give_item(&mut self, item_id: impl Into<String>, count: u8) {
        if let Some(uuid) = self.uuid.as_ref().into_option().map(|s| s.as_str()) {
            let i = item_id.into();
            self.host.give_item(uuid.into(), i.as_str().into(), count);
        }
    }
}
