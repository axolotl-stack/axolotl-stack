//! Connection handling and join sequence.
//!
//! Contains the handshake logic for new player connections.

use glam::DVec3;
use jolyne::auth::ValidatedIdentity;
use jolyne::stream::server::ServerHandshakeConfig;
use jolyne::valentine::BlockCoordinates;
use jolyne::valentine::types::Vec3F;
use jolyne::{JolyneError, ServerLogin, ServerPlay, WorldTemplate};
use p384::SecretKey;

use crate::config::{PlayerDataStore, SpawnLocation, UnastarConfig};

/// Performs the complete join sequence for a connecting player.
///
/// This includes:
/// 1. Network settings negotiation
/// 2. Authentication
/// 3. Encryption handshake
/// 4. Resource pack negotiation
/// 5. Spawn location resolution
/// 6. Start game packet
pub async fn accept_join_sequence(
    template: &WorldTemplate,
    server_key: &SecretKey,
    config: &UnastarConfig,
    player_data_store: &PlayerDataStore,
    session_id: u64,
    handshake_stream: ServerLogin,
) -> Result<(ServerPlay, ValidatedIdentity, Vec3F), JolyneError> {
    // 1. Network Settings
    let login = handshake_stream.accept_network_settings().await?;

    // 2. Auth
    let (secure, identity) = login.authenticate().await?;

    // 3. Encryption/handshake
    let packs = secure
        .finish_handshake(
            &ServerHandshakeConfig {
                server_key: server_key.clone(),
            },
            &identity.identity_public_key,
        )
        .await?;

    // 4. Resource packs (none/default)
    let start_game_state = packs.negotiate_packs(false).await?;

    // 5. Resolve spawn before StartGame.
    let spawn = resolve_spawn_location(config, &identity, template, player_data_store).await;
    let initial_position = Vec3F {
        x: spawn.x,
        y: spawn.y,
        z: spawn.z,
    };

    // 6. Build join params (use session_id as entity/runtime ID for now).
    let mut join_params = template.to_join_params(session_id as i64);
    join_params.start_game.player_position = initial_position.clone();
    join_params.start_game.spawn_position = BlockCoordinates {
        x: spawn.x.floor() as i32,
        y: spawn.y.floor() as i32,
        z: spawn.z.floor() as i32,
    };
    join_params.start_game.rotation = jolyne::valentine::Vec2F {
        x: spawn.pitch,
        z: spawn.yaw,
    };

    // 7. Join.
    let play = start_game_state.start_game(join_params).await?;
    Ok((play, identity, initial_position))
}

/// Resolves the spawn location for a player based on config rules.
pub async fn resolve_spawn_location(
    config: &UnastarConfig,
    identity: &ValidatedIdentity,
    template: &WorldTemplate,
    player_data_store: &PlayerDataStore,
) -> SpawnLocation {
    use jolyne::valentine::StartGamePacketDimension;

    let uuid = identity.uuid.as_deref();
    let world_dimension = match template.start_game_template.dimension {
        StartGamePacketDimension::Overworld => 0,
        StartGamePacketDimension::Nether => 1,
        StartGamePacketDimension::End => 2,
    };

    // Check for previous position first
    for rule in &config.spawn_rules {
        if rule.previous_position {
            if let Some(uuid) = uuid {
                if let Ok(Some(last)) = player_data_store.load_last_position(uuid).await {
                    if last.dimension == world_dimension {
                        return last.location;
                    }
                }
            }
        }
        if rule.always_at_location {
            if let Some(location) = rule.location {
                // If using Vanilla generator, verify spawn is safe
                if let crate::world::WorldGenerator::Vanilla { seed } = config.world.generator {
                    let generator = crate::world::generator::VanillaGenerator::new(seed);
                    let height = generator.find_safe_spawn();
                    // Use safe spawn Y if configured Y seems underground
                    if location.y < height.1 as f32 - 10.0 {
                        return SpawnLocation {
                            x: height.0 as f32 + 0.5,
                            y: height.1 as f32,
                            z: height.2 as f32 + 0.5,
                            yaw: location.yaw,
                            pitch: location.pitch,
                        };
                    }
                }
                return location;
            }
        }
    }

    // Fallback: if any rule has a location, use it.
    if let Some(location) = config.spawn_rules.iter().find_map(|r| r.location) {
        // If using Vanilla generator, verify spawn is safe
        if let crate::world::WorldGenerator::Vanilla { seed } = config.world.generator {
            let generator = crate::world::generator::VanillaGenerator::new(seed);
            let height = generator.find_safe_spawn();
            // Use safe spawn if configured Y seems underground
            if location.y < height.1 as f32 - 10.0 {
                return SpawnLocation {
                    x: height.0 as f32 + 0.5,
                    y: height.1 as f32,
                    z: height.2 as f32 + 0.5,
                    yaw: location.yaw,
                    pitch: location.pitch,
                };
            }
        }
        return location;
    }

    // Final fallback: use safe spawn for Vanilla, or template spawn otherwise
    if let crate::world::WorldGenerator::Vanilla { seed } = config.world.generator {
        let generator = crate::world::generator::VanillaGenerator::new(seed);
        let (x, y, z) = generator.find_safe_spawn();
        return SpawnLocation {
            x: x as f32 + 0.5,
            y: y as f32,
            z: z as f32 + 0.5,
            yaw: 0.0,
            pitch: 0.0,
        };
    }

    SpawnLocation {
        x: template.start_game_template.player_position.x,
        y: template.start_game_template.player_position.y,
        z: template.start_game_template.player_position.z,
        yaw: template.start_game_template.rotation.z,
        pitch: template.start_game_template.rotation.x,
    }
}

/// Convert a spawn location to a DVec3 position.
pub fn spawn_to_dvec3(pos: &Vec3F) -> DVec3 {
    DVec3::new(pos.x as f64, pos.y as f64, pos.z as f64)
}
