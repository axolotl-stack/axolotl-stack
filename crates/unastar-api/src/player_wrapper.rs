use crate::{GameContext, PlayerHandle, PlayerInfo, PluginAction, Vec3};

// ============================================================================
// OOP Wrappers
// ============================================================================

/// A wrapper around a player handle, context, and action buffer.
/// This allows for a more object-oriented API within the `on_tick` loop.
pub struct Player<'a> {
    pub handle: PlayerHandle,
    pub info: PlayerInfo,
    ctx: &'a GameContext,
}

impl<'a> Player<'a> {
    /// Create a new Player wrapper.
    /// Returns None if the player handle is invalid or the player is not found in the context.
    pub fn new(handle: PlayerHandle, ctx: &'a GameContext) -> Option<Self> {
        let info = ctx.player_info(handle)?;
        Some(Self { handle, info, ctx })
    }

    /// Send a chat message to this player.
    pub fn message(&mut self, message: impl Into<String>) {
        self.ctx.push_action(PluginAction::SendMessage {
            player_id: self.info.uuid.clone(),
            message: message.into(),
        });
    }

    /// Teleport this player to a position.
    pub fn teleport(&mut self, position: Vec3) {
        self.ctx.push_action(PluginAction::Teleport {
            player_id: self.info.uuid.clone(),
            position,
        });
    }

    /// Give an item to this player.
    pub fn give_item(&mut self, item_id: impl Into<String>, count: u8) {
        self.ctx.push_action(PluginAction::GiveItem {
            player_id: self.info.uuid.clone(),
            item_id: item_id.into(),
            count,
        });
    }

    /// Kick this player from the server.
    pub fn kick(&mut self, reason: impl Into<String>) {
        self.ctx.push_action(PluginAction::Kick {
            player_id: self.info.uuid.clone(),
            reason: reason.into(),
        });
    }
}

impl<'a> std::ops::Deref for Player<'a> {
    type Target = PlayerInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}
