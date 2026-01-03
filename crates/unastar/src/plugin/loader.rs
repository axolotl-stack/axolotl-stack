use abi_stable::std_types::RBox;
use libloading::{Library, Symbol};
use std::fs;
use std::path::Path;
use tracing::{error, info, warn};
use unastar_api::native::{RawPlugin, RawPlugin_TO};

pub struct PluginLoader;

impl PluginLoader {
    /// Load all plugins from a directory
    pub fn load_from_dir<P: AsRef<Path>>(dir: P) -> Vec<RawPlugin_TO<RBox<()>>> {
        let mut plugins = Vec::new();
        let dir = dir.as_ref();

        if !dir.exists() {
            info!("Plugins directory does not exist: {:?}", dir);
            if let Err(e) = fs::create_dir_all(dir) {
                error!("Failed to create plugins directory: {}", e);
            }
            return plugins;
        }

        let Ok(entries) = fs::read_dir(dir) else {
            error!("Failed to read plugins directory: {:?}", dir);
            return plugins;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy();
                // Check for DLL extensions
                if ext_str == "dll" || ext_str == "so" || ext_str == "dylib" {
                    if let Some(plugin) = Self::load_dll(&path) {
                        info!("Loaded dynamic plugin from {:?}", path);
                        plugins.push(plugin);
                    }
                }
            }
        }

        plugins
    }

    /// Load a single plugin DLL
    pub fn load_dll(path: &Path) -> Option<RawPlugin_TO<RBox<()>>> {
        info!("Loading DLL: {:?}", path);

        // Load the library
        // SAFETY: Loading arbitrary DLLs is unsafe. User must trust plugins.
        // We leak the library reference to keep it loaded.
        let lib = unsafe {
            match Library::new(path) {
                Ok(l) => l,
                Err(e) => {
                    warn!("Failed to load library {:?}: {}", path, e);
                    return None;
                }
            }
        };

        // Find the creation function
        // Returns RawPlugin_TO by value (it's a struct wrapping a pointer)
        type CreateFn = extern "C" fn() -> RawPlugin_TO<RBox<()>>;

        let create_fn: Symbol<CreateFn> = unsafe {
            match lib.get(b"_create_plugin") {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to find _create_plugin in {:?}: {}", path, e);
                    return None;
                }
            }
        };

        // Call the function to get the plugin instance
        let plugin = create_fn();

        // Leak the library to prevent unloading code while plugin is active
        // In a real system we would track this handle for hot reloading
        std::mem::forget(lib);

        Some(plugin)
    }
}
