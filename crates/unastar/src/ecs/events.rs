use bevy_ecs::prelude::*;
use unastar_api::{PluginAction, PluginEvent};

use bevy_ecs::prelude::Entity;

/// Internal event representation carrying Entity references.
/// These are converted to PluginEvent (with handles) by the PluginManager.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    Tick {
        tick_id: u64,
    },
    PlayerJoin {
        entity: Entity,
        player_id: String,
        username: String,
    },
    PlayerChat {
        entity: Entity,
        player_id: String,
        message: String,
    },
    BlockBreak {
        entity: Entity,
        player_id: String,
        position: (i32, i32, i32),
        block_id: u32,
    },
    BlockPlace {
        entity: Entity,
        player_id: String,
        position: (i32, i32, i32),
        block_id: u32,
    },
    Timer {
        id: u64,
    },
}

impl ServerEvent {
    pub fn kind(&self) -> unastar_api::EventKind {
        use unastar_api::EventKind;
        match self {
            ServerEvent::Tick { .. } => EventKind::Tick,
            ServerEvent::PlayerJoin { .. } => EventKind::PlayerJoin,
            ServerEvent::PlayerChat { .. } => EventKind::PlayerChat,
            ServerEvent::BlockBreak { .. } => EventKind::BlockBreak,
            ServerEvent::BlockPlace { .. } => EventKind::BlockPlace,
            ServerEvent::Timer { .. } => EventKind::Timer,
        }
    }
}

/// A buffer of events that occurred during the current tick.
/// This is drained by the PluginManager and sent to WASM plugins.
#[derive(Resource, Default)]
pub struct EventBuffer {
    events: Vec<ServerEvent>,
}

impl EventBuffer {
    pub fn push(&mut self, event: ServerEvent) {
        self.events.push(event);
    }

    pub fn drain(&mut self) -> Vec<ServerEvent> {
        std::mem::take(&mut self.events)
    }
}

/// A queue of actions requested by plugins or internal logic.
/// These are processed in the post-simulation phase.
#[derive(Resource, Default)]
pub struct ActionQueue {
    actions: Vec<PluginAction>,
}

impl ActionQueue {
    pub fn push(&mut self, action: PluginAction) {
        self.actions.push(action);
    }

    pub fn drain(&mut self) -> Vec<PluginAction> {
        std::mem::take(&mut self.actions)
    }
}
