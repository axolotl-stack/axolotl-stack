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
use crate::storage::{PlayerData, PlayerProvider, StorageError, StorageResult};

/// Why a join sequence stopped. The client has been sent a disconnect.
#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error(transparent)]
    Protocol(#[from] JolyneError),
    /// The player's stored record exists but can't be read or is invalid.
    /// The join is refused so the record is kept for repair instead of being
    /// overwritten by a save of first-join state.
    ///
    /// This includes transient storage I/O errors from the player database,
    /// on purpose: the server can't tell a passing read failure from a lost
    /// record, and letting the player in with first-join state would let that
    /// session's save overwrite the real one. The player can retry the join.
    #[error("saved player data is unreadable: {0}")]
    SavedPlayerData(StorageError),
}

/// Disconnect message for a join refused over its saved record.
pub const UNREADABLE_PLAYER_DATA_MESSAGE: &str =
    "Unable to load saved player data. Contact the server operator.";

/// What StartGame is built from: the startup template and the live clock,
/// difficulty and game rules.
#[derive(Clone, Copy)]
pub struct JoinWorld<'a> {
    pub template: &'a WorldTemplate,
    pub state: &'a crate::world::rules::StartGameWorldState,
}

/// Performs the complete join sequence for a connecting player.
///
/// This includes:
/// 1. Network settings negotiation
/// 2. Authentication
/// 3. Encryption handshake
/// 4. Resource pack negotiation
/// 5. Spawn location resolution
/// 6. Start game packet, returning at the client's chunk radius request
///    (streamed spawn: the client has not initialized yet)
///
/// A failed step sends the client a reasoned disconnect before returning. A
/// stored player record that can't be loaded refuses the join (see
/// [`load_player_data`]).
pub async fn accept_join_sequence(
    world: JoinWorld<'_>,
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
    JoinError,
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
    let mut saved_data = match load_player_data(player_provider, Some(&uuid)).await {
        Ok(saved) => saved,
        Err(error) => {
            tracing::warn!(
                %uuid,
                name = identity.display_name(),
                xuid = identity.xuid().unwrap_or("none"),
                %error,
                "Refusing join for {}: saved player data is unreadable; keeping the record",
                identity.display_name()
            );
            let _ = start_game_state
                .disconnect(
                    jolyne::valentine::EnumsConnectionDisconnectFailReason::Disconnected,
                    UNREADABLE_PLAYER_DATA_MESSAGE,
                )
                .await;
            return Err(JoinError::SavedPlayerData(error));
        }
    };
    let spawn = resolve_spawn_location(
        config,
        Some(&uuid),
        world.template,
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
    // client gets the view distance it requests, up to `max_chunk_radius`
    // (BDS caps requests at `view-distance`); later radius requests are
    // handled by the chunk systems with the same cap.
    let mut join_params = world.template.to_join_params(session_id as i64);
    join_params.max_chunk_radius = Some(config.server.max_chunk_radius);
    join_params.start_game.position = initial_position.clone();
    // The clock, difficulty and game rules as they are now, not as the
    // template was built at startup.
    world.state.apply(&mut join_params.start_game);
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

    // 7. Join. The streamed spawn returns at the client's radius request, before
    // it initializes: the chunk systems send the view, the chunks they have and
    // `PlayStatus(PlayerSpawn)`, and keep streaming while the client loads (BDS
    // order). The play receive path enforces `SetLocalPlayerAsInitialized`.
    let play = reasoned(start_game_state.start_game_streamed(join_params)).await?;
    Ok((play, identity, initial_position, saved_data))
}

/// Load validated player state once, before StartGame and ECS spawning.
///
/// `Ok(None)` means first join (no provider, no UUID, or no record). A record
/// that can't be read, parsed or validated is an error and is left untouched:
/// joining with defaults instead would let that session's save overwrite it.
pub async fn load_player_data(
    provider: Option<&dyn PlayerProvider>,
    uuid: Option<&str>,
) -> StorageResult<Option<PlayerData>> {
    let Some(provider) = provider else {
        return Ok(None);
    };
    let Some(uuid) = uuid.and_then(|uuid| uuid::Uuid::parse_str(uuid).ok()) else {
        return Ok(None);
    };
    match provider.load(uuid).await? {
        Some(data) => {
            data.validate(uuid)?;
            Ok(Some(data))
        }
        None => Ok(None),
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
        let (generator, bounds) = (config.world.generator, config.world.bounds);
        let spawn = tokio::task::spawn_blocking(move || generator.spawn(bounds))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PlayerLastPosition, SpawnRule};

    const UUID: &str = "00000000-0000-0000-0000-00000000002a";

    fn at(x: f32) -> SpawnLocation {
        SpawnLocation {
            x,
            y: 70.0,
            z: 0.5,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    fn previous() -> SpawnRule {
        SpawnRule {
            name: "previous".into(),
            previous_position: true,
            always_at_location: false,
            location: None,
        }
    }

    fn fixed(x: f32) -> SpawnRule {
        SpawnRule {
            name: format!("fixed {x}"),
            previous_position: false,
            always_at_location: true,
            location: Some(at(x)),
        }
    }

    /// A fallback-only rule: it has a location but never answers on its own.
    fn fallback(x: f32) -> SpawnRule {
        SpawnRule {
            always_at_location: false,
            ..fixed(x)
        }
    }

    fn saved(x: f64, dimension: i32) -> PlayerData {
        PlayerData {
            uuid: UUID.into(),
            position: [x, 70.0, 0.5],
            dimension,
            ..PlayerData::default()
        }
    }

    struct Store(PlayerDataStore, std::path::PathBuf);

    impl Store {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("unastar-spawn-rules-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            Self(PlayerDataStore::new(root.clone()), root)
        }
    }

    impl Drop for Store {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }

    async fn resolve(
        rules: Vec<SpawnRule>,
        store: &PlayerDataStore,
        saved_data: Option<&PlayerData>,
    ) -> f32 {
        let config = UnastarConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            spawn_rules: rules,
            ..UnastarConfig::default()
        };
        let template = WorldTemplate::default();
        resolve_spawn_location(&config, Some(UUID), &template, store, saved_data)
            .await
            .x
    }

    #[tokio::test]
    async fn spawn_rules_are_evaluated_top_to_bottom() {
        let store = Store::new("order");
        let in_world = saved(33.25, 0);
        let other_dimension = saved(44.0, 1);

        // The saved record answers the previous-position rule...
        assert_eq!(
            resolve(vec![previous(), fixed(1.0)], &store.0, Some(&in_world)).await,
            33.25
        );
        // ...but only when listed before a fixed rule.
        assert_eq!(
            resolve(vec![fixed(1.0), previous()], &store.0, Some(&in_world)).await,
            1.0
        );
        // A record from another dimension is skipped; the next rule answers.
        assert_eq!(
            resolve(
                vec![previous(), fixed(1.0), fixed(2.0)],
                &store.0,
                Some(&other_dimension)
            )
            .await,
            1.0
        );

        // Without a record the last-position file answers; a record wins over it.
        store
            .0
            .save_last_position(
                UUID,
                &PlayerLastPosition {
                    dimension: 0,
                    location: at(55.0),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            resolve(vec![previous(), fixed(1.0)], &store.0, None).await,
            55.0
        );
        assert_eq!(
            resolve(vec![previous(), fixed(1.0)], &store.0, Some(&in_world)).await,
            33.25
        );
    }

    #[tokio::test]
    async fn without_an_answering_rule_the_first_location_then_the_template_is_used() {
        let store = Store::new("fallback");
        assert_eq!(
            resolve(
                vec![previous(), fallback(3.0), fallback(4.0)],
                &store.0,
                None
            )
            .await,
            3.0
        );
        let template = WorldTemplate::default();
        assert_eq!(
            resolve(vec![previous()], &store.0, None).await,
            template.start_game_template.position.x
        );
    }

    /// The generator's spawn search stays inside `world.bounds`: with a rect far from
    /// where the unbounded search lands, the spawn is still in a bounded chunk. Java-style
    /// generation needs the structure templates of `java-worldgen`.
    #[cfg(feature = "java-worldgen")]
    #[tokio::test]
    async fn the_generated_spawn_is_inside_the_world_bounds() {
        let store = Store::new("bounded-spawn");
        let bounds = crate::world::WorldBounds::Rect {
            min_x: 100,
            max_x: 101,
            min_z: -101,
            max_z: -100,
        };
        let config = UnastarConfig {
            world: crate::world::WorldConfig {
                bounds,
                generator: crate::world::WorldGenerator::Java { seed: 12345 },
                ..crate::world::WorldConfig::default()
            },
            spawn_rules: vec![previous()],
            ..UnastarConfig::default()
        };
        let spawn =
            resolve_spawn_location(&config, None, &WorldTemplate::default(), &store.0, None).await;
        let chunk = crate::world::ChunkPos::new(
            (spawn.x.floor() as i32) >> 4,
            (spawn.z.floor() as i32) >> 4,
        );
        assert!(
            bounds.contains(chunk),
            "spawn {spawn:?} is in chunk {chunk:?}"
        );
    }

    /// The refused player is told the server, not their client, is at fault.
    #[test]
    fn the_unreadable_record_disconnect_points_to_the_operator() {
        assert_eq!(
            UNREADABLE_PLAYER_DATA_MESSAGE,
            "Unable to load saved player data. Contact the server operator."
        );
    }
}
