//! Typed plugin API generated from the `unastar-plugin` WIT world.
//!
//! Implement [`Handler`] and register it with [`export_handler!`], or use
//! `#[unastar_api_macros::plugin]` on its implementation. A callback provides
//! a scoped [`WorldTx`] and [`PlayerContext`]; live players cannot outlive it.
//! Store [`PlayerHandle`] or [`WorldHandle`] to schedule work with a fresh scope.
//! [`Plugin`] and [`export_plugin!`] expose the canonical bindings directly;
//! the host applies the same scope and identity checks to all guests.
//! Build plugin `cdylib` crates for `wasm32-wasip2`.

pub use wit_bindgen;

#[doc(hidden)]
// Canonical ABI lowering expands record fields into scalar function arguments.
#[allow(clippy::too_many_arguments)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "unastar-plugin",
        pub_export_macro: true,
        default_bindings_module: "unastar_api::bindings",
    });
}

pub use bindings::Guest;
#[doc(hidden)]
pub use bindings::export;
/// Immutable native state archives owned by this module's manifest identity.
pub use bindings::unastar::plugin::archives;
/// Advertise owned command roots without replacing native builtin permissions.
pub use bindings::unastar::plugin::commands;
pub use bindings::unastar::plugin::host;
/// Typed player widgets with exact request and connection ownership.
pub use bindings::unastar::plugin::presentation;
pub use bindings::unastar::plugin::storage;
/// Viewer-specific representation never changes the subject's canonical identity.
pub use bindings::unastar::plugin::views;
pub use presentation::{
    FormCloseReason, FormResponse, PresentationError, Sidebar, SidebarLine, SimpleForm,
};
pub use views::{NameTagVisibility, PlayerProjection, ProjectionError};
/// Canonical WIT bindings for advanced component integrations.
/// Live raw resources still expire at their originating callback boundary.
pub mod raw {
    pub use crate::bindings::unastar::plugin::types::*;
}
pub use raw::{
    AccessError, ActiveStatusEffect, AdmissionAcknowledgement, AdmissionDecision,
    AdmissionResolution, AdmittedEvent, ArchiveError, ArchiveFacts, ArchiveLimits,
    ArchiveReference, ArchiveWorld, ArchivedPlayerState, AuthenticatedDamageAccount, BlockPos,
    DamageAttribution, DamageCause, DamageCommitted, DamageOwner, DamageProposal, DamageTuning,
    DeathCapturePhase, DeathCommitted, DeathFacts, DeathInventory, DeathState, DisconnectDetail,
    DisconnectReason, EffectApplication, EffectKind, Enchantment, GameMode,
    InfiniteEffectApplication, InventoryArea, InventoryCommit, InventoryError, InventoryImage,
    InventoryOperation, InventoryReceipt, InventoryStatus, ItemValue, Knockback, LogLevel,
    MechanicsAcknowledgement, MovementCommitted, PlayerAvailability, PlayerFlags, PlayerHandle,
    PlayerIdentity, PlayerMechanicsState, PlayerSnapshot, PlayerStateImage, QuitDisposition,
    QuitEvent, SourceFence, SourceFenceStatus, StateCommit, StateError, StateOperation,
    StateStatus, StateValues, Task, TaskStatus, TravelAcknowledgement, TravelError, TravelLocation,
    TravelMoved, TravelOutcome, TravelResult, Vec3, WorldHandle,
};
pub use raw::{
    BlockClick, BlockCommitted, BlockFace, BlockHeldItems, BlockOperationKind, BlockProperty,
    BlockPropertyValue, BlockProposal, BlockValue, CommittedBlockCell, PlannedBlockCell,
};
mod scoped;
pub use scoped::{AdmissionHold, Player, PlayerContext, WorldTx, server};

// Plugin trait + export_plugin! macro are generated from WIT by build.rs
include!(concat!(env!("OUT_DIR"), "/plugin_api.rs"));

/// An owned detached item stack, independent of any live inventory.
pub use raw::Stack as ItemStack;

pub use raw::{
    CustomItemOutcome, HandledItemAction, ItemActionCommitted, ItemActionDecision, ItemActionItems,
    ItemActionOutcome, ItemActionProposal, ItemActionStage, ItemActionTarget, ItemActorTarget,
    ItemAmmunitionChange, ItemBlockTarget, LaunchedItemActor,
};

pub use raw::{
    BlockCell, BlockEdit, BlockEditApplied, BlockEditRequest, BlockEditStatus, BlockEditSupport,
    BlockFacts, BlockLayers, DurableBlockError, DurableBlockReceipt, DurableBlockRequest,
    DurableBlockStatus, WorldBlockError,
};
pub use raw::{
    ConnectionCloseKind, ConnectionCloseReason, ConnectionCloseRequest, ConnectionCloseStatus,
    ConnectionControlError,
};
pub use raw::{FoodPolicy, PlayerVitals};
pub use raw::{InventoryMechanicsOperation, InventoryMechanicsStatus};
pub use raw::{TravelCause, TravelProposal};
