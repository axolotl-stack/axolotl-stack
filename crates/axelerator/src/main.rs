//! Axelerator CLI - Xbox Live friend broadcast server.
//!
//! Advertises a Minecraft server to Xbox Live friends via WebRTC,
//! then transfers players to the actual server.

use anyhow::Result;
use axelerator::{Axelerator, AxeleratorConfig};
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser, Debug)]
#[command(name = "axelerator")]
#[command(about = "Make Minecraft servers joinable via Xbox Live friends list")]
#[command(version)]
struct Args {
    /// Target server IP address (where players will be transferred)
    #[arg(long, default_value = "127.0.0.1")]
    server_ip: String,

    /// Target server port
    #[arg(long, default_value = "19132")]
    server_port: u16,

    /// Name shown in friends list
    #[arg(long, default_value = "Axelerator Server")]
    host_name: String,

    /// World name displayed
    #[arg(long, default_value = "Minecraft World")]
    world_name: String,

    /// Path to cached OAuth token
    #[arg(long, default_value = "token.json")]
    token_path: String,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging - respect RUST_LOG env var, fall back to debug/info based on --debug flag
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if args.debug {
            EnvFilter::new("debug")
        } else {
            EnvFilter::new("info")
        }
    });

    fmt().with_env_filter(filter).with_target(false).init();

    let config = AxeleratorConfig {
        host_name: args.host_name,
        world_name: args.world_name,
        server_ip: args.server_ip,
        server_port: args.server_port,
        token_cache_path: args.token_path,
        ..Default::default()
    };

    println!(
        r#"
    ___   _  __ ______ __    ______ ____   ___  ______ ____   ____
   /   | | |/ // ____// /   / ____// __ \ /   |/_  __// __ \ / __ \
  / /| | |   // __/  / /   / __/  / /_/ // /| | / /  / / / // /_/ /
 / ___ |/   |/ /___ / /___/ /___ / _, _// ___ |/ /  / /_/ // _, _/
/_/  |_/_/|_/_____//_____/_____//_/ |_|/_/  |_/_/   \____//_/ |_|
                                                                   
    Xbox Live Friend Broadcast Server
    "#
    );

    // Run server
    let axelerator = Axelerator::new(config);

    // Handle Ctrl+C
    let shutdown = tokio::spawn({
        let axelerator = axelerator.clone();
        async move {
            tokio::signal::ctrl_c().await.ok();
            println!("\nShutting down...");
            axelerator.shutdown().await;
        }
    });

    axelerator.run().await?;
    shutdown.abort();

    Ok(())
}
