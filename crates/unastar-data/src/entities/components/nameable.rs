use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct NameableNameActionsOnNamed {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for NameableNameActionsOnNamed {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct NameableNameActions {
    ///List of special names that will cause the events defined in `on_named` to fire.
    pub name_filter: Option<String>,
    ///Event to be called when this entity acquires the name specified in `name_filter'.
    pub on_named: Option<NameableNameActionsOnNamed>,
}
impl Default for NameableNameActions {
    fn default() -> Self {
        Self {
            name_filter: None,
            on_named: None,
        }
    }
}
/// Bedrock component `minecraft:nameable`. Allows this entity to be named (e.g. using a name tag).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Nameable {
    ///If true, this entity can be renamed with name tags.
    pub allow_name_tag_renaming: Option<bool>,
    ///If true, the name will always be shown.
    pub always_show: Option<bool>,
    ///Trigger to run when the entity gets named.
    pub default_trigger: Option<crate::types::BedrockValue>,
    ///Describes the special names for this entity and the events to call when the entity acquires those names.
    pub name_actions: Option<Vec<NameableNameActions>>,
}
impl Default for Nameable {
    fn default() -> Self {
        Self {
            allow_name_tag_renaming: Some(true),
            always_show: Some(false),
            default_trigger: None,
            name_actions: None,
        }
    }
}
