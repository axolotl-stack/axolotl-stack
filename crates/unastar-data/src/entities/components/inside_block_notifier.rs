use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct InsideBlockNotifierBlockListBlockStates {
    /// Additional dynamic entries not captured by the upstream schema.
    pub additional: std::collections::HashMap<String, f32>,
}
impl Default for InsideBlockNotifierBlockListBlockStates {
    fn default() -> Self {
        Self {
            additional: std::collections::HashMap::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InsideBlockNotifierBlockListBlock {
    ///The block id, for example: `minecraft:air'.
    pub name: Option<String>,
    ///The block states.
    pub states: Option<InsideBlockNotifierBlockListBlockStates>,
}
impl Default for InsideBlockNotifierBlockListBlock {
    fn default() -> Self {
        Self {
            name: None,
            states: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InsideBlockNotifierBlockListEnteredBlockEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for InsideBlockNotifierBlockListEnteredBlockEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InsideBlockNotifierBlockListExitedBlockEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for InsideBlockNotifierBlockListExitedBlockEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InsideBlockNotifierBlockList {
    ///block
    pub block: Option<InsideBlockNotifierBlockListBlock>,
    ///Event to run when this mob enters a valid block.
    pub entered_block_event: Option<InsideBlockNotifierBlockListEnteredBlockEvent>,
    ///Event to run when this mob leaves a valid block.
    pub exited_block_event: Option<InsideBlockNotifierBlockListExitedBlockEvent>,
}
impl Default for InsideBlockNotifierBlockList {
    fn default() -> Self {
        Self {
            block: None,
            entered_block_event: None,
            exited_block_event: None,
        }
    }
}
/// Bedrock component `minecraft:inside_block_notifier`. Verifies whether the entity is inside any of the listed blocks.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct InsideBlockNotifier {
    ///List of blocks, with certain block states, that we are monitoring to see if the entity is inside.
    pub block_list: Option<Vec<InsideBlockNotifierBlockList>>,
}
impl Default for InsideBlockNotifier {
    fn default() -> Self {
        Self { block_list: None }
    }
}
