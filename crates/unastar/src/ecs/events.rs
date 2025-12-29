use bevy_ecs::prelude::*;
use unastar_api::{PluginEvent, PluginAction};

/// A buffer of events that occurred during the current tick.
/// This is drained by the PluginManager and sent to WASM plugins.
#[derive(Resource, Default)]
pub struct EventBuffer {
    events: Vec<PluginEvent>,
}

impl EventBuffer {
    pub fn push(&mut self, event: PluginEvent) {
        self.events.push(event);
    }

    pub fn drain(&mut self) -> Vec<PluginEvent> {
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
