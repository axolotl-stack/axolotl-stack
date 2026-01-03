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
    PlayerMove {
        entity: Entity,
        from: (f64, f64, f64),
        to: (f64, f64, f64),
    },
    PlayerJump {
        entity: Entity,
    },
    PlayerToggleSneak {
        entity: Entity,
        is_sneaking: bool,
    },
    PlayerToggleSprint {
        entity: Entity,
        is_sprinting: bool,
    },
    PlayerQuit {
        entity: Entity,
    },
    PlayerHeldSlotChange {
        entity: Entity,
        old_slot: u8,
        new_slot: u8,
    },
    PlayerStartBreak {
        entity: Entity,
        position: (i32, i32, i32),
        face: u8,
    },
    PlayerInteractBlock {
        entity: Entity,
        position: (i32, i32, i32),
        face: u8,
    },
    PlayerItemUse {
        entity: Entity,
    },
    PlayerSwing {
        entity: Entity,
    },
    TaskComplete {
        task_id: u32,
        result: unastar_api::TaskResult,
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
            ServerEvent::PlayerMove { .. } => EventKind::PlayerMove,
            ServerEvent::PlayerJump { .. } => EventKind::PlayerJump,
            ServerEvent::PlayerToggleSneak { .. } => EventKind::PlayerToggleSneak,
            ServerEvent::PlayerToggleSprint { .. } => EventKind::PlayerToggleSprint,
            ServerEvent::PlayerQuit { .. } => EventKind::PlayerQuit,
            ServerEvent::PlayerHeldSlotChange { .. } => EventKind::PlayerHeldSlotChange,
            ServerEvent::PlayerStartBreak { .. } => EventKind::PlayerStartBreak,
            ServerEvent::PlayerInteractBlock { .. } => EventKind::PlayerInteractBlock,
            ServerEvent::PlayerItemUse { .. } => EventKind::PlayerItemUse,
            ServerEvent::PlayerSwing { .. } => EventKind::PlayerSwing,
            ServerEvent::TaskComplete { .. } => EventKind::TaskComplete,
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
