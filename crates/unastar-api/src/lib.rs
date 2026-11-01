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
pub use bindings::unastar::plugin::host;
/// Canonical WIT bindings for advanced component integrations.
/// Live raw resources still expire at their originating callback boundary.
pub mod raw {
    pub use crate::bindings::unastar::plugin::types::*;
}
pub use raw::{
    AccessError, BlockPos, DamageCause, GameMode, LogLevel, PlayerAvailability, PlayerFlags,
    PlayerHandle, PlayerIdentity, PlayerSnapshot, Task, TaskStatus, Vec3, WorldHandle,
};
mod scoped;
pub use scoped::{Player, PlayerContext, WorldTx, server};

// Plugin trait + export_plugin! macro are generated from WIT by build.rs
include!(concat!(env!("OUT_DIR"), "/plugin_api.rs"));
