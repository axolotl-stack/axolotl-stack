//! Connection handling and join sequence.
//!
//! Contains the handshake logic for new player connections.

use glam::DVec3;
use jolyne::auth::ValidatedIdentity;
use jolyne::stream::raknet_types::{ServerHandshakeStream, ServerPlayStream};
use jolyne::stream::{Failed, RakNetTransport};
use jolyne::valentine::types::{BlockPos, Vec2, Vec3};
use jolyne::{JolyneError, WorldTemplate};
use p384::SecretKey;

use crate::config::{PlayerDataStore, SpawnLocation, UnastarConfig};
use crate::storage::{PlayerData, PlayerProvider};

/// Performs the complete join sequence for a connecting player.
///
/// This includes:
/// 1. Network settings negotiation
/// 2. Authentication
/// 3. Encryption handshake
/// 4. Resource pack negotiation
/// 5. Spawn location resolution
/// 6. Start game packet
///
/// A failed step sends the client a reasoned disconnect before returning.
pub async fn accept_join_sequence(
    template: &WorldTemplate,
    server_key: &SecretKey,
    config: &UnastarConfig,
    player_data_store: &PlayerDataStore,
    player_provider: Option<&dyn PlayerProvider>,
    session_id: u64,
    handshake_stream: ServerHandshakeStream,
) -> Result<
    (
        ServerPlayStream,
        ValidatedIdentity,
        Vec3,
        Option<PlayerData>,
    ),
    JolyneError,
> {
    /// Tells the client why a join step failed, then surfaces the error.
    async fn reasoned<T>(
        step: impl Future<Output = Result<T, Failed<RakNetTransport>>>,
    ) -> Result<T, JolyneError> {
        match step.await {
            Ok(next) => Ok(next),
            Err(failed) => Err(failed.disconnect().await),
        }
    }

    // 1. Network Settings
    let login = reasoned(handshake_stream.accept_network_settings()).await?;

    // 2. Auth
    let secure = reasoned(login.authenticate()).await?;

    // 3. Encryption/handshake: the identity is authenticated once the client
    // proved it holds the login's key.
    let packs = reasoned(secure.finish_handshake(server_key)).await?;
    let identity = packs.login().identity.clone();

    // 4. Resource packs (from the listener config)
    let start_game_state = reasoned(packs.negotiate_packs()).await?;

    // 5. Resolve spawn before StartGame.
    let uuid = identity.uuid().to_string();
    let mut saved_data = load_player_data(player_provider, Some(&uuid)).await;
    let spawn = resolve_spawn_location(
        config,
        Some(&uuid),
        template,
        player_data_store,
        saved_data.as_ref(),
    )
    .await;
    if let Some(saved) = &mut saved_data {
        saved.rotation = [spawn.yaw, spawn.pitch];
    }
    let initial_position = Vec3 {
        x: spawn.x,
        y: spawn.y,
        z: spawn.z,
    };

    // 6. Build join params (use session_id as entity/runtime ID for now). The
    // server grants its default view distance; later radius requests are
    // handled by the chunk systems.
    let mut join_params = template.to_join_params(session_id as i64);
    join_params.chunk_radius = Some(config.server.default_chunk_radius);
    join_params.start_game.position = initial_position.clone();
    if let Some(saved) = &saved_data {
        join_params.start_game.game_type = match saved.game_mode {
            0 => jolyne::valentine::EnumsGameType::Survival,
            1 => jolyne::valentine::EnumsGameType::Creative,
            2 => jolyne::valentine::EnumsGameType::Adventure,
            6 => jolyne::valentine::EnumsGameType::Spectator,
            _ => unreachable!("player state validated on load"),
        };
    }
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
    let play = reasoned(start_game_state.start_game(join_params)).await?;
    Ok((play, identity, initial_position, saved_data))
}

/// Load validated player state once, before StartGame and ECS spawning.
/// Read/parse errors preserve the record and fall back to first-join defaults.
pub async fn load_player_data(
    provider: Option<&dyn PlayerProvider>,
    uuid: Option<&str>,
) -> Option<PlayerData> {
    let provider = provider?;
    let uuid = uuid.and_then(|uuid| uuid::Uuid::parse_str(uuid).ok())?;
    let result = match provider.load(uuid).await {
        Ok(Some(data)) => data.validate(uuid).map(|()| Some(data)),
        other => other,
    };
    match result {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!(%uuid, %error, "Failed to load player data; using defaults without deleting stored data");
            None
        }
    }
}

/// Resolves the spawn location for a player based on config rules.
///
/// IMPORTANT: This function trusts user-configured spawn locations directly. Without a
/// configured location, generated worlds use the generator's spawn search, which runs once
/// per process (see [`crate::world::WorldGenerator::spawn`]).
pub async fn resolve_spawn_location(
    config: &UnastarConfig,
    uuid: Option<&str>,
    template: &WorldTemplate,
    player_data_store: &PlayerDataStore,
    saved_data: Option<&PlayerData>,
) -> SpawnLocation {
    let world_dimension = template
        .start_game_template
        .settings
        .spawn_settings
        .dimension;

    let mut last_position = None;
    if config.spawn_rules.iter().any(|rule| rule.previous_position)
        && let Some(uuid) = uuid
    {
        match player_data_store.load_last_position(uuid).await {
            Ok(last) => last_position = last,
            Err(error) => {
                tracing::warn!(%uuid, %error, "Failed to load last player position; using spawn defaults")
            }
        }
    }
    // Check spawn rules in order
    for rule in &config.spawn_rules {
        // Check for previous position first if enabled
        if rule.previous_position {
            if let Some(saved) = saved_data
                && saved.dimension == world_dimension
            {
                return SpawnLocation {
                    x: saved.position[0] as f32,
                    y: saved.position[1] as f32,
                    z: saved.position[2] as f32,
                    yaw: saved.rotation[0],
                    pitch: saved.rotation[1],
                };
            }
            if let Some(last) = &last_position
                && last.dimension == world_dimension
            {
                return last.location;
            }
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

    // Final fallback: template spawn (flat/void) or the generator's spawn search
    // (terrain), computed once on a blocking thread.
    if config.world.generator.uses_generation_pool() {
        let generator = config.world.generator;
        let spawn = tokio::task::spawn_blocking(move || generator.spawn())
            .await
            .expect("spawn search panicked");
        return SpawnLocation {
            x: spawn.x as f32 + 0.5,
            y: spawn.y as f32,
            z: spawn.z as f32 + 0.5,
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
