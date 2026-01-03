//! Re-exported types from unastar for plugin use.
//!
//! This avoids circular dependencies by having unastar-api not depend on unastar,
//! and instead the plugin accesses components via the World directly.

// No component re-exports - plugins access via World.get::<T>() where T comes from unastar
