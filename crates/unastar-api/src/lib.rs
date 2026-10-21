//! Typed plugin API generated from the `unastar-plugin` WIT world.
//!
//! Implement [`Plugin`] and register it with [`export_plugin!`], or use
//! `#[unastar_api_macros::plugin]` on the implementation instead. Both routes
//! export the same [`Guest`] interface; callbacks use the WIT types directly.
//! A full [`Guest`] implementation can instead register with [`export!`].
//! Build plugin `cdylib` crates for `wasm32-wasip2`.

pub use wit_bindgen;

#[doc(hidden)]
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
pub use bindings::unastar::plugin::types::*;

// Plugin trait + export_plugin! macro are generated from WIT by build.rs
include!(concat!(env!("OUT_DIR"), "/plugin_api.rs"));
