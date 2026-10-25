//! Java-style surface stage: the compiled overworld surface rule
//! (`unastar_noise::overworld_surface`) applied column by column, plus the eroded-badlands
//! pillar and frozen-ocean iceberg extensions.

mod system;

pub use system::SurfaceSystem;
