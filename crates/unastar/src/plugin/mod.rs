pub mod manager;
pub mod manifest;

pub(crate) mod archive_store;
mod io_budget;
pub(crate) mod storage;

pub use manager::PluginManager;
pub use manifest::{PluginCapability, PluginId, PluginManifest};
