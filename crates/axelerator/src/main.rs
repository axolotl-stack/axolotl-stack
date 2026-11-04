//! Axelerator CLI - Xbox Live friend broadcast server.
//!
//! Advertises a Minecraft server to Xbox Live friends via WebRTC,
//! then transfers players to the actual server.

use anyhow::Result;
use axelerator::{Axelerator, AxeleratorConfig};
use clap::{Parser, Subcommand};
use std::time::Duration;
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

    // On Ctrl+C/SIGTERM, request shutdown and keep driving `run` so its
    // cleanup (presence inactive, RTA, Discord) completes. A shutdown during
    // device-code login ends `run` promptly too.
    let result = run_until_signal(
        axelerator.run(),
        shutdown_signal(),
        axelerator.shutdown(),
        SHUTDOWN_GRACE,
    )
    .await;

    if let Err(ref e) = result {
        tracing::error!("Axelerator exited with error: {:#}", e);
    } else {
        tracing::info!("Axelerator shutdown complete");
    }

    result
}

/// How long cleanup may take after a shutdown signal; below the systemd
/// unit's `TimeoutStopSec=30`, after which it would be killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(20);

/// Resolves on Ctrl+C, or SIGTERM on Unix.
async fn shutdown_signal() {
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
}

/// Drives `run` to completion. When `signal` fires first, `request_shutdown`
/// runs and `run` gets `grace` to finish its own cleanup; it is never dropped
/// mid-cleanup unless the grace period expires.
async fn run_until_signal(
    run: impl Future<Output = Result<()>>,
    signal: impl Future<Output = ()>,
    request_shutdown: impl Future<Output = ()>,
    grace: Duration,
) -> Result<()> {
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => result,
        () = signal => {
            tracing::info!("Initiating graceful shutdown...");
            request_shutdown.await;
            match tokio::time::timeout(grace, run).await {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!(
                    "graceful shutdown did not finish within {} s",
                    grace.as_secs()
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio_util::sync::CancellationToken;

    /// A stand-in for `Axelerator::run`: waits for shutdown, then cleans up.
    async fn fake_run(shutdown: CancellationToken, cleaned_up: Arc<AtomicBool>) -> Result<()> {
        shutdown.cancelled().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        cleaned_up.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// main used to race `run()` against the signal and drop it, skipping
    /// the presence-inactive, RTA and Discord shutdown steps.
    #[tokio::test(start_paused = true)]
    async fn signal_lets_run_finish_its_cleanup() {
        let shutdown = CancellationToken::new();
        let cleaned_up = Arc::new(AtomicBool::new(false));
        let result = run_until_signal(
            fake_run(shutdown.clone(), cleaned_up.clone()),
            async {},
            async { shutdown.cancel() },
            Duration::from_secs(20),
        )
        .await;
        assert!(result.is_ok());
        assert!(cleaned_up.load(Ordering::SeqCst), "cleanup was skipped");
    }

    #[tokio::test(start_paused = true)]
    async fn stuck_cleanup_is_an_error_after_the_grace_period() {
        let result = run_until_signal(
            std::future::pending::<Result<()>>(),
            async {},
            async {},
            Duration::from_secs(20),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_errors_without_a_signal_are_returned() {
        let result = run_until_signal(
            async { Err(anyhow::anyhow!("fatal")) },
            std::future::pending(),
            async {},
            Duration::from_secs(20),
        )
        .await;
        assert!(result.is_err());
    }
}
