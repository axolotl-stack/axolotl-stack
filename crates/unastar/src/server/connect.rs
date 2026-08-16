//! Connection handling and join sequence.
//!
//! Contains the handshake logic for new player connections.

use glam::DVec3;
use jolyne::auth::ValidatedIdentity;
use jolyne::stream::raknet_types::{ServerLogin, ServerPlay};
use jolyne::stream::server::ServerHandshakeConfig;
use jolyne::valentine::types::{BlockPos, Vec2, Vec3};
use jolyne::{JolyneError, WorldTemplate};
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
) -> Result<(ServerPlay, ValidatedIdentity, Vec3), JolyneError> {
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
    let start_game_state = packs
        .negotiate_packs(config.server.require_resource_packs)
        .await?;

    // 5. Resolve spawn before StartGame.
    let spawn = resolve_spawn_location(config, &identity, template, player_data_store).await;
    let initial_position = Vec3 {
        x: spawn.x,
        y: spawn.y,
        z: spawn.z,
    };

    // 6. Build join params (use session_id as entity/runtime ID for now).
    let mut join_params = template.to_join_params(session_id as i64);
    join_params.start_game.position = initial_position.clone();
    join_params.start_game.settings.default_spawn_block_position = BlockPos {
        x: spawn.x.floor() as i32,
        y: spawn.y.floor() as i32,
        z: spawn.z.floor() as i32,
    };
    join_params.start_game.rotation = Vec2 {
        x: spawn.pitch,
        y: spawn.yaw,
    };

    // 7. Join.
    let play = start_game_state.start_game(join_params).await?;
    Ok((play, identity, initial_position))
}

/// Resolves the spawn location for a player based on config rules.
///
/// IMPORTANT: This function trusts user-configured spawn locations directly.
/// For vanilla worlds, users should configure an appropriate spawn point in the config.
/// The expensive `find_safe_spawn()` is only called as a last resort when no location
/// is configured at all.
pub async fn resolve_spawn_location(
    config: &UnastarConfig,
    identity: &ValidatedIdentity,
    template: &WorldTemplate,
    player_data_store: &PlayerDataStore,
) -> SpawnLocation {
    let uuid = identity.uuid.as_deref();
    let world_dimension = template
        .start_game_template
        .settings
        .spawn_settings
        .dimension;

    // Check spawn rules in order
    for rule in &config.spawn_rules {
        // Check for previous position first if enabled
        if rule.previous_position
            && let Some(uuid) = uuid
            && let Ok(Some(last)) = player_data_store.load_last_position(uuid).await
            && last.dimension == world_dimension
        {
            return last.location;
        }
        // Use configured location directly (trust the user's config)
        if rule.always_at_location
            && let Some(location) = rule.location
        {
            return location;
        }
    }

    // Fallback: if any rule has a location, use it directly
    if let Some(location) = config.spawn_rules.iter().find_map(|r| r.location) {
        return location;
    }

    // Final fallback: use template spawn (for non-vanilla) or search for safe spawn (vanilla)
    // NOTE: find_safe_spawn() is expensive and should be avoided by configuring spawn in config.
    // This is only called when no spawn location is configured at all.
    if let crate::world::WorldGenerator::Vanilla { seed } = config.world.generator {
        tracing::warn!(
            "No spawn location configured for vanilla world - searching for safe spawn. \
             This is slow! Configure [[spawn_rules]] with a location in your config."
        );
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
        x: template.start_game_template.position.x,
        y: template.start_game_template.position.y,
        z: template.start_game_template.position.z,
        yaw: template.start_game_template.rotation.y,
        pitch: template.start_game_template.rotation.x,
    }
}

/// Convert a spawn location to a DVec3 position.
pub fn spawn_to_dvec3(pos: &Vec3) -> DVec3 {
    DVec3::new(pos.x as f64, pos.y as f64, pos.z as f64)
}
