//! Axelerator CLI - Xbox Live friend broadcast server.
//!
//! Advertises a Minecraft server to Xbox Live friends via WebRTC,
//! then transfers players to the actual server.

use anyhow::Result;
use axelerator::{Axelerator, AxeleratorConfig};
use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser, Debug)]
#[command(name = "axelerator")]
#[command(about = "Make Minecraft servers joinable via Xbox Live friends list")]
#[command(version)]
struct Args {
    /// Path to configuration file (TOML format)
    #[arg(short, long, default_value = "axelerator.toml")]
    config: String,

    /// Override: Target server IP address
    #[arg(long)]
    server_ip: Option<String>,

    /// Override: Target server port
    #[arg(long)]
    server_port: Option<u16>,

    /// Override: Name shown in friends list
    #[arg(long)]
    host_name: Option<String>,

    /// Override: Path to cached OAuth token
    #[arg(long)]
    token_path: Option<String>,

    /// Override: Enable session monitoring
    #[arg(long)]
    monitor: bool,

    /// Override: Auto-block attackers (requires --monitor)
    #[arg(long)]
    auto_block: bool,

    /// Override log level (error, warn, info, debug, trace)
    #[arg(long)]
    log_level: Option<String>,

    /// Enable screenshot/showcase image feature
    #[arg(long)]
    screenshots: bool,

    /// Path to a single screenshot image file
    #[arg(long)]
    screenshot_path: Option<String>,

    /// Directory containing screenshot images for cycling
    #[arg(long)]
    screenshot_dir: Option<String>,

    /// Screenshot cycle interval in seconds (default: 300)
    #[arg(long)]
    screenshot_interval: Option<u64>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate an example configuration file
    Init {
        /// Output path for config file
        #[arg(short, long, default_value = "axelerator.toml")]
        output: String,
    },
    /// Run the server (default if no subcommand)
    Run,
}

fn init_logging(config: &AxeleratorConfig, log_level_override: Option<&str>) {
    // Priority: CLI override > RUST_LOG env var > config file
    let level = log_level_override
        .map(String::from)
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| config.logging.level.clone());

    let filter = EnvFilter::try_new(&level).unwrap_or_else(|_| {
        eprintln!("Warning: Invalid log level '{}', using 'info'", level);
        EnvFilter::new("info")
    });

    let subscriber = fmt()
        .with_env_filter(filter)
        .with_target(config.logging.show_target);

    if config.logging.show_timestamp {
        subscriber.init();
    } else {
        subscriber.without_time().init();
    }
}

fn print_banner() {
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands
    match &args.command {
        Some(Commands::Init { output }) => {
            let example = AxeleratorConfig::example_toml();
            std::fs::write(output, &example)?;
            println!("Created example config at: {}", output);
            println!("\nEdit this file and run: axelerator -c {}", output);
            return Ok(());
        }
        Some(Commands::Run) | None => {
            // Continue to run server
        }
    }

    // Load config from file
    let mut config = AxeleratorConfig::load_or_default(&args.config);

    // Apply CLI overrides
    if let Some(ip) = &args.server_ip {
        config.server_ip = ip.clone();
    }
    if let Some(port) = args.server_port {
        config.server_port = port;
    }
    if let Some(name) = &args.host_name {
        config.host_name = name.clone();
    }
    if let Some(path) = &args.token_path {
        config.token_cache_path = path.clone();
    }
    if args.monitor {
        config.monitor_tampering = true;
    }
    if args.auto_block {
        config.auto_block_attackers = true;
    }

    // Screenshot overrides
    if args.screenshots {
        config.screenshots.enabled = true;
    }
    if let Some(path) = &args.screenshot_path {
        config.screenshots.enabled = true;
        config.screenshots.path = Some(path.clone());
    }
    if let Some(dir) = &args.screenshot_dir {
        config.screenshots.enabled = true;
        config.screenshots.directory = Some(dir.clone());
    }
    if let Some(interval) = args.screenshot_interval {
        config.screenshots.cycle_interval = interval;
    }

    // Initialize logging (must be before any tracing calls)
    init_logging(&config, args.log_level.as_deref());

    print_banner();

    // Log effective configuration
    tracing::info!(
        server = format!("{}:{}", config.server_ip, config.server_port),
        host_name = %config.host_name,
        monitor = config.monitor_tampering,
        "Starting Axelerator"
    );

    if config.monitor_tampering {
        tracing::info!(
            interval = config.monitor_interval,
            auto_block = config.auto_block_attackers,
            "Session monitoring enabled"
        );
    }

    if config.screenshots.enabled {
        let source = if config.screenshots.directory.is_some() {
            config.screenshots.directory.as_deref().unwrap_or("(dir)")
        } else if config.screenshots.path.is_some() {
            config.screenshots.path.as_deref().unwrap_or("(path)")
        } else {
            "(none)"
        };
        tracing::info!(
            source = source,
            cycle_interval = config.screenshots.cycle_interval,
            "Screenshots enabled"
        );
    }

    // Run server
    let axelerator = Axelerator::new(config);

    // Use select! to race the server against shutdown signals
    // This ensures we can exit even if stuck in device code auth
    let result = tokio::select! {
        biased;

        // Shutdown signals (prioritized)
        _ = async {
            let ctrl_c = tokio::signal::ctrl_c();

            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigterm = signal(SignalKind::terminate()).ok();

                tokio::select! {
                    _ = ctrl_c => {
                        tracing::info!("Received SIGINT (Ctrl+C)");
                    }
                    _ = async {
                        if let Some(ref mut s) = sigterm {
                            s.recv().await
                        } else {
                            std::future::pending().await
                        }
                    } => {
                        tracing::info!("Received SIGTERM");
                    }
                }
            }

            #[cfg(not(unix))]
            {
                ctrl_c.await.ok();
                tracing::info!("Received shutdown signal");
            }
        } => {
            tracing::info!("Initiating graceful shutdown...");
            axelerator.shutdown().await;
            // Give a brief moment for graceful shutdown to propagate
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Ok(())
        }

        // Main server run
        result = axelerator.run() => {
            result
        }
    };

    if let Err(ref e) = result {
        tracing::error!("Axelerator exited with error: {}", e);
    } else {
        tracing::info!("Axelerator shutdown complete");
    }

    result
}
