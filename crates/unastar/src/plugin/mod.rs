pub mod loader;
pub mod manager;
pub mod manifest;
pub mod registry;

pub use manager::PluginManager;
pub use manifest::{PluginCapability, PluginId, PluginManifest};
pub use registry::PluginRegistry;
