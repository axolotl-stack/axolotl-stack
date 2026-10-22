//! BDS Data Extractor
//!
//! Connects to a Bedrock Dedicated Server and extracts game data
//! (items, blocks, creative content, etc.) for code generation.

mod output;

use anyhow::{Context, Result};
use clap::Parser;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::valentine::EnumsConnectionDisconnectFailReason;
use jolyne::{BedrockStream, JolyneError};
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

    /// Timeout in seconds for joining and for collecting post-spawn definitions
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

    let data = extract_data(addr, &args.name, Duration::from_secs(args.timeout)).await?;
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

fn load_fixture(path: &Path) -> Result<output::ExtractedData> {
    let json = std::fs::read_to_string(path).context("Failed to read fixture")?;
    let data: output::ExtractedData =
        serde_json::from_str(&json).context("Failed to parse fixture JSON")?;
    data.validate().context("Fixture validation failed")?;
    Ok(data)
}

fn write_extracted_data(output: &Path, data: &output::ExtractedData) -> Result<()> {
    data.validate()
        .context("Extracted data validation failed")?;
    let json = serde_json::to_string_pretty(data).context("Failed to serialize data")?;
    std::fs::write(output, json).context("Failed to write output file")?;
    info!("Data written to: {}", output.display());
    Ok(())
}

async fn extract_data(
    addr: SocketAddr,
    player_name: &str,
    collection_timeout: Duration,
) -> Result<output::ExtractedData> {
    // Bound login separately so collection timeouts can report missing sections
    // and still send a disconnect.
    let (mut play_stream, mut game_data) = tokio::time::timeout(collection_timeout, async {
        let handshake_stream = BedrockStream::connect(addr)
            .await
            .context("Failed to connect to BDS")?;
        let config = ClientHandshakeConfig::random(addr, player_name);
        handshake_stream
            .join(config)
            .await
            .context("Failed to complete handshake")
    })
    .await
    .context("Connection/login timed out")??;

    info!("Handshake complete! Collecting game definitions...");
    let deadline = tokio::time::Instant::now() + collection_timeout;
    let collection = async {
        while let Some(section) = missing_section(&game_data) {
            let packet = match tokio::time::timeout_at(deadline, play_stream.recv_packet())
                .await
                .map_err(|_| output::ValidationError::MissingSection(section))?
            {
                Ok(packet) => packet,
                Err(JolyneError::Disconnected(disconnect)) => anyhow::bail!(
                    "Server disconnected during extraction: {:?}",
                    disconnect.reason
                ),
                Err(error) => {
                    return Err(anyhow::Error::new(error)
                        .context(format!("Receiving required section: {section}")));
                }
            };
            capture_definition(&mut game_data, packet.data);
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;

    // Disconnect even when collection fails; never wait indefinitely for a peer
    // that stopped reading. `disconnect` sends `Disconnect`, drains pending
    // writes and closes the transport.
    let disconnect = tokio::time::timeout(
        Duration::from_secs(2),
        play_stream.disconnect(EnumsConnectionDisconnectFailReason::Disconnected, ""),
    )
    .await;
    collection?;
    disconnect
        .context("Disconnect timed out")?
        .context("Failed to disconnect")?;
    let extracted = output::ExtractedData::from_game_data(game_data);
    extracted.validate()?;
    Ok(extracted)
}

fn missing_section(data: &jolyne::GameData) -> Option<&'static str> {
    if data.biome_definitions.is_none() {
        Some("biomes")
    } else if data.entity_identifiers.is_none() {
        Some("entities")
    } else if data.creative_content.is_none() {
        Some("creative")
    } else {
        None
    }
}

fn capture_definition(data: &mut jolyne::GameData, packet: jolyne::valentine::McpePacketData) {
    use jolyne::valentine::McpePacketData;
    match packet {
        McpePacketData::BiomeDefinitionListPacket(packet) => data.biome_definitions = Some(packet),
        McpePacketData::AvailableActorIdentifiersPacket(packet) => {
            data.entity_identifiers = Some(packet)
        }
        McpePacketData::CreativeContentPacket(packet) => data.creative_content = Some(packet),
        McpePacketData::ItemRegistryPacket(packet) => data.item_registry = packet,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::{
        AvailableEntityIdentifiersPacket, BiomeDefinitionListPacket, CreativeContentPacket,
        ItemRegistryPacket, McpePacketData, StartGamePacket,
    };

    #[test]
    fn collection_requires_all_definitions_in_any_order() {
        let mut data = jolyne::GameData {
            start_game: StartGamePacket::default(),
            item_registry: ItemRegistryPacket::default(),
            biome_definitions: None,
            entity_identifiers: None,
            creative_content: None,
        };
        capture_definition(
            &mut data,
            McpePacketData::CreativeContentPacket(CreativeContentPacket::default()),
        );
        assert_eq!(missing_section(&data), Some("biomes"));
        capture_definition(
            &mut data,
            McpePacketData::BiomeDefinitionListPacket(BiomeDefinitionListPacket::default()),
        );
        assert_eq!(missing_section(&data), Some("entities"));
        capture_definition(
            &mut data,
            McpePacketData::AvailableActorIdentifiersPacket(
                AvailableEntityIdentifiersPacket::default(),
            ),
        );
        assert_eq!(missing_section(&data), None);
    }
}
