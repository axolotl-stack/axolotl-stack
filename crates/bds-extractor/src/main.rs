//! BDS Data Extractor
//!
//! Connects to a Bedrock Dedicated Server and extracts game data
//! (items, blocks, creative content, etc.) for code generation.

mod output;

use anyhow::{Context, Result};
use clap::Parser;
use jolyne::BedrockStream;
use jolyne::stream::client::ClientHandshakeConfig;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "bds-extractor")]
#[command(about = "Extract game data from Bedrock Dedicated Server")]
struct Args {
    /// BDS server address
    #[arg(short, long, default_value = "127.0.0.1:19132")]
    addr: String,

    /// Output JSON file path
    #[arg(short, long, default_value = "bds-data.json")]
    output: PathBuf,

    /// Connection timeout in seconds
    #[arg(short, long, default_value = "30")]
    timeout: u64,

    /// Player name to use for connection
    #[arg(short, long, default_value = "BDSExtractor")]
    name: String,

    /// Read and validate an existing extractor JSON fixture instead of connecting to BDS
    #[arg(long)]
    fixture: Option<PathBuf>,

    /// Log level filter
    #[arg(long, default_value = "info")]
    log: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log.parse().unwrap_or_else(|_| "info".parse().unwrap())),
        )
        .init();

    if let Some(fixture) = args.fixture.as_ref() {
        info!("Loading extraction fixture from {}...", fixture.display());
        let data = load_fixture(fixture)?;
        write_extracted_data(&args.output, &data)?;
        return Ok(());
    }

    let addr: SocketAddr = args.addr.parse().context("Invalid server address format")?;

    info!("Connecting to BDS at {}...", addr);

    // Connect with timeout
    let connect_result = tokio::time::timeout(
        Duration::from_secs(args.timeout),
        extract_data(addr, &args.name),
    )
    .await;

    match connect_result {
        Ok(Ok(data)) => {
            info!("Extraction complete!");
            info!("  Items: {}", data.items.registry.len());
            info!("  Blocks: {}", data.blocks.properties.len());
            info!(
                "  Creative Groups: {}",
                data.creative.as_ref().map_or(0, |c| c.groups.len())
            );
            info!(
                "  Creative Items: {}",
                data.creative.as_ref().map_or(0, |c| c.items.len())
            );

            write_extracted_data(&args.output, &data)?;
            Ok(())
        }
        Ok(Err(e)) => {
            anyhow::bail!("Extraction failed: {}", e);
        }
        Err(_) => {
            anyhow::bail!("Connection timed out after {} seconds", args.timeout);
        }
    }
}

fn load_fixture(path: &Path) -> Result<output::ExtractedData> {
    let json = std::fs::read_to_string(path).context("Failed to read fixture")?;
    let data: output::ExtractedData =
        serde_json::from_str(&json).context("Failed to parse fixture JSON")?;
    data.validate()
        .map_err(|e| anyhow::anyhow!("Fixture validation failed: {e}"))?;
    Ok(data)
}

fn write_extracted_data(output: &Path, data: &output::ExtractedData) -> Result<()> {
    data.validate()
        .map_err(|e| anyhow::anyhow!("Extracted data validation failed: {e}"))?;
    let json = serde_json::to_string_pretty(data).context("Failed to serialize data")?;
    std::fs::write(output, json).context("Failed to write output file")?;
    info!("Data written to: {}", output.display());
    Ok(())
}

async fn extract_data(addr: SocketAddr, player_name: &str) -> Result<output::ExtractedData> {
    // Connect to BDS
    let handshake_stream = BedrockStream::connect(addr)
        .await
        .context("Failed to connect to BDS")?;

    info!("Connected! Starting handshake...");

    // Configure client with random identity (self-signed auth for offline BDS)
    let config = ClientHandshakeConfig::random(addr, player_name);

    // Join - this handles settings, auth, encryption, resource packs, and captures game data
    let (_play_stream, game_data) = handshake_stream
        .join(config)
        .await
        .context("Failed to complete handshake")?;

    info!("Handshake complete! Extracting data...");

    // Convert to our output format
    let extracted = output::ExtractedData::from_game_data(game_data);

    // Note: We drop the play_stream here which disconnects
    // In the future we could stay connected to receive more packets if needed

    Ok(extracted)
}
