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

    match (config.logging.json_format, config.logging.show_timestamp) {
        (true, true) => subscriber.json().init(),
        (true, false) => subscriber.json().without_time().init(),
        (false, true) => subscriber.init(),
        (false, false) => subscriber.without_time().init(),
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

    // Load config from file. A missing file means defaults; a malformed one
    // (bad TOML, wrong types, unknown keys) stops startup.
    let mut config = AxeleratorConfig::load(&args.config)?;

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
    // device-code login ends `run` promptly too. A second signal exits at once.
    let result = run_until_signal(
        axelerator.run(),
        shutdown_signal,
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

/// Drives `run` to completion. When `signal()` fires first,
/// `request_shutdown` runs and `run` gets `grace` to finish its own cleanup;
/// it is dropped mid-cleanup only when the grace period expires or a second
/// signal arrives (the operator asking to exit at once).
async fn run_until_signal<F: Future<Output = ()>>(
    run: impl Future<Output = Result<()>>,
    mut signal: impl FnMut() -> F,
    request_shutdown: impl Future<Output = ()>,
    grace: Duration,
) -> Result<()> {
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => result,
        () = signal() => {
            tracing::info!("Initiating graceful shutdown (signal again to exit at once)...");
            request_shutdown.await;
            tokio::select! {
                result = tokio::time::timeout(grace, &mut run) => match result {
                    Ok(result) => result,
                    Err(_) => Err(anyhow::anyhow!(
                        "graceful shutdown did not finish within {} s",
                        grace.as_secs()
                    )),
                },
                () = signal() => {
                    tracing::warn!(
                        "Second shutdown signal: exiting now, skipping the remaining cleanup \
                         (presence inactive, RTA shutdown, history save, Discord notice)"
                    );
                    Err(anyhow::anyhow!("shutdown forced by a second signal; cleanup skipped"))
                }
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
            once(),
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
            once(),
            async {},
            Duration::from_secs(20),
        )
        .await;
        assert!(result.is_err());
    }

    /// A signal source that fires `presses` times, `after` apart (the first
    /// at once), then never again.
    fn presses(
        presses: u32,
        after: Duration,
    ) -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = ()>>> {
        let mut count = 0;
        move || {
            count += 1;
            let fires = count <= presses;
            let first = count == 1;
            Box::pin(async move {
                if !fires {
                    std::future::pending::<()>().await;
                }
                if !first {
                    tokio::time::sleep(after).await;
                }
            })
        }
    }

    fn once() -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = ()>>> {
        presses(1, Duration::ZERO)
    }

    /// A second Ctrl+C used to do nothing: the grace period always ran out.
    #[tokio::test(start_paused = true)]
    async fn second_signal_skips_cleanup() {
        let started = tokio::time::Instant::now();
        let result = run_until_signal(
            std::future::pending::<Result<()>>(),
            presses(2, Duration::from_secs(1)),
            async {},
            Duration::from_secs(20),
        )
        .await;
        let error = result.unwrap_err().to_string();
        assert!(error.contains("cleanup skipped"), "{error}");
        assert_eq!(started.elapsed(), Duration::from_secs(1));
    }

    #[tokio::test]
    async fn run_errors_without_a_signal_are_returned() {
        let result = run_until_signal(
            async { Err(anyhow::anyhow!("fatal")) },
            std::future::pending,
            async {},
            Duration::from_secs(20),
        )
        .await;
        assert!(result.is_err());
    }
}
