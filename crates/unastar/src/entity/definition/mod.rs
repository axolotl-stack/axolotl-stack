//! The generic behaviour-pack definition layer every pack-driven actor uses.
//!
//! - [`compiled`]: frozen component kinds, compiled definitions and the
//!   per-actor [`DefinitionState`] with its transactional apply barrier
//!   (re-exported here).
//! - [`stack`]: the numeric definition stack behind component groups.
//! - [`components`]: the core literal components (health, movement,
//!   collision box, scale, physics).
//! - [`events`]: actor events bound at startup and evaluated without lookups.
//! - [`families`], [`flags`], [`properties`], [`attributes`], [`metadata`]:
//!   the actor state the client sees, addressed by startup-resolved handles.
//! - [`error`]: the typed errors of compiling and composing definitions.
//!
//! Mob-specific component families live in `entity::mob::components`.

pub mod attributes;
pub mod compiled;
pub mod components;
pub mod error;
pub mod events;
pub mod families;
pub mod flags;
pub mod metadata;
pub mod properties;
mod stack;

pub use compiled::{
    CompiledDefinition, ComponentGroupId, ComponentKindId, ComponentKinds, ComponentKindsBuilder,
    DefinitionState, GroupChange, apply_pending_definitions, attach_definition, queue_group_change,
};
pub use error::{ComponentCompileError, ComponentError, CompositionError, DefinitionError};
