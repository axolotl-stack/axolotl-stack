//! Unastar Server Main Entry Point
//!
//! Runs a 20 TPS tick loop with async network handling.
//! Uses ECS for entity/player state management.

use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use unastar::config::UnastarConfig;
use unastar::server::UnastarServer;

/*
#[global_allocator]
static GLOBAL: tracy_client::ProfiledAllocator<std::alloc::System> =
    tracy_client::ProfiledAllocator::new(std::alloc::System, 100);
*/

/*
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc; 
*/

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let _profiler = dhat::Profiler::new_heap();

    // Initialize tracing
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    info!("Unastar Server starting...");

    // Load config (creates `unastar.toml` on first run).
    let config_path = std::env::var("UNASTAR_CONFIG").unwrap_or_else(|_| "unastar.toml".into());
    let app_config = Arc::new(UnastarConfig::load_or_create(&config_path)?);

    // Create and run server
    let mut server = UnastarServer::new(app_config).await?;
    server.run().await
}
