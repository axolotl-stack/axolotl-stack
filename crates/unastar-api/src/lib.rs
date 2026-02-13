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
