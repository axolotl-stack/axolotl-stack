//! Complete raw-state reconstruction from an acknowledged, nonrecursive handoff.

use super::*;
use crate::storage::{RestorationCheckpoint, RestorationEffect, RestorationSupplement};

impl PlayerSnapshot {
    /// Keep extra fields typed and separate from inventory and proof history.
    pub(crate) fn supplement(&self) -> RestorationSupplement {
        RestorationSupplement {
            version: 1,
            xuid: self.xuid.clone(),
            effective_mode: self.data.game_mode,
            base_mode: self.base_game_mode,
            flight_override: self.flight_override,
            velocity: self.velocity,
            head_yaw: self.head_yaw,
            absorption: self.absorption,
            air_current: self.air_current,
            air_max: self.air_max,
            on_fire_ticks: self.on_fire_ticks,
            burning_remaining: self.burning_remaining,
            burning_next: self.burning_next,
            effects: self
                .effects
                .iter()
                .map(|effect| RestorationEffect {
                    id: effect.id,
                    level: effect.level,
                    duration_ticks: effect.duration_ticks,
                    ambient: effect.ambient,
                    show_particles: effect.show_particles,
                    infinite: effect.infinite,
                })
                .collect(),
            dead: self.dead,
            food_policy: self.food_policy,
            food_ticks: self.food_ticks,
            food_regeneration: self.food_regeneration,
        }
    }

    /// Reconstruct gameplay only; canonical account fields, epoch and receipts stay current.
    pub(crate) fn from_checkpoint(
        data: PlayerData,
        checkpoint: &RestorationCheckpoint,
    ) -> Result<Self, SnapshotError> {
        checkpoint.validate(&data).map_err(invalid)?;
        Self::from_supplement(data, &checkpoint.supplement)
    }

    /// Rebuild raw values beside their current canonical slots and proof history.
    pub(crate) fn from_supplement(
        mut data: PlayerData,
        supplement: &RestorationSupplement,
    ) -> Result<Self, SnapshotError> {
        supplement.validate(&data).map_err(invalid)?;
        data.restoration_checkpoint = None;
        data.source_actor = None;
        data.game_mode = supplement.effective_mode;
        let snapshot = Self {
            version: SNAPSHOT_VERSION,
            ui: data.ui.clone(),
            data,
            base_game_mode: supplement.base_mode,
            flight_override: supplement.flight_override,
            xuid: supplement.xuid.clone(),
            velocity: supplement.velocity,
            head_yaw: supplement.head_yaw,
            absorption: supplement.absorption,
            air_current: supplement.air_current,
            air_max: supplement.air_max,
            on_fire_ticks: supplement.on_fire_ticks,
            burning_remaining: supplement.burning_remaining,
            burning_next: supplement.burning_next,
            effects: supplement
                .effects
                .iter()
                .map(|effect| SnapshotEffect {
                    id: effect.id,
                    level: effect.level,
                    duration_ticks: effect.duration_ticks,
                    ambient: effect.ambient,
                    show_particles: effect.show_particles,
                    infinite: effect.infinite,
                })
                .collect(),
            dead: supplement.dead,
            food_policy: supplement.food_policy,
            food_ticks: supplement.food_ticks,
            food_regeneration: supplement.food_regeneration,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

/// Stock join installs the already-acknowledged raw handoff before any admission callback.
/// This does not replay an effect or overwrite current canonical inventory/history.
pub(crate) fn resume_checkpoint(
    world: &mut World,
    player: Entity,
    data: PlayerData,
) -> Result<(), SnapshotError> {
    let Some(checkpoint) = data.restoration_checkpoint.as_ref() else {
        return Ok(());
    };
    let snapshot = PlayerSnapshot::from_checkpoint(data.clone(), checkpoint)?;
    if !super::super::player_events::defer_initial_state(world, player) {
        return Err(SnapshotError::MissingPlayer);
    }
    restore(world, player, &snapshot)?;
    if !super::super::player_events::release_initial_state(world, player) {
        return Err(SnapshotError::MissingPlayer);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::{Age, EffectType, Effects, PlayerSession};
    use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
    use crate::storage::PlayerProvider;
    use crate::storage::player_writer::SerializedPlayerProvider;
    use std::sync::Arc;

    /// A real native player with an authenticated identity and usable initial packet channel.
    fn spawn(
        server: &mut GameServer,
        data: Option<PlayerData>,
        session: u64,
    ) -> (
        Entity,
        tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    ) {
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(8192);
        let position = data
            .as_ref()
            .map_or([4.5, 65.62, 4.5], |data| data.position);
        let entity = server
            .spawn_player(PlayerSpawnData {
                session_id: session,
                runtime_id: session as i64 + 100,
                display_name: format!("Actor{session}"),
                xuid: Some("73".into()),
                uuid: Some(uuid::Uuid::from_u128(73).to_string()),
                position: DVec3::from_array(position),
                saved_data: data,
                outbound_tx,
                chunk_radius: 0,
            })
            .unwrap();
        (entity, rx)
    }
    /// Distinct ECS lifetimes deliberately start with unrelated native clocks.
    fn server() -> GameServer {
        GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        })
    }
    /// Capture real raw state whose supplement would be lost by inventory-only persistence.
    fn desired() -> PlayerSnapshot {
        let mut server = server();
        let (actor, _packets) = spawn(&mut server, None, 1);
        let world = server.ecs.world_mut();
        world.resource_mut::<TickCounter>().current = 1_000;
        world.entity_mut(actor).insert((
            Health::with_current(7.0, 30.0),
            Hunger {
                food_level: 9,
                saturation: 2.0,
                exhaustion: 1.0,
            },
            Velocity(DVec3::new(0.25, 0.5, -0.25)),
            Absorption(4.0),
            AirSupply {
                current_ticks: 17,
                max_ticks: 300,
            },
            OnFire {
                ticks_remaining: 40,
            },
            Burning {
                until_tick: 1_030,
                next_burn_tick: 1_006,
            },
            Age(77),
        ));
        world.get_mut::<Effects>(actor).unwrap().active.insert(
            EffectType(1),
            ActiveEffect {
                level: 2,
                duration_ticks: 93,
                ambient: true,
                show_particles: false,
                infinite: false,
            },
        );
        crate::command::exec::set_game_mode(world, actor, GameMode::Survival);
        crate::command::exec::set_mode_override(world, actor, Some(GameMode::Creative));
        super::super::super::flight::set_override(world, actor, Some(true));
        capture(world, actor).unwrap()
    }
    /// Store a real complete record and its handoff in one shared-writer commit.
    async fn commit(writer: &SerializedPlayerProvider, snapshot: &PlayerSnapshot) -> PlayerData {
        let mut candidate = snapshot.data.clone();
        candidate.game_mode = snapshot.base_game_mode.unwrap_or(snapshot.data.game_mode);
        let result = writer
            .commit_state_effect(
                uuid::Uuid::from_u128(73),
                "restore:example",
                [19; 32],
                candidate,
                snapshot.supplement(),
                crate::storage::player_writer::StateOrigin::default(),
            )
            .await
            .unwrap();
        assert!(!result.already_committed);
        writer
            .load(uuid::Uuid::from_u128(73))
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn full_checkpoint_reopens_before_live_application_without_changing_identity_or_receipts()
    {
        let directory = tempfile::tempdir().unwrap();
        let provider =
            Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
        let writer = SerializedPlayerProvider::new(provider.clone());
        let snapshot = desired();
        let row = commit(&writer, &snapshot).await;
        let receipt = row.inventory_effects.clone();
        assert_eq!(row.inventory_revision, 1);
        drop(writer);
        drop(provider);
        let reopened = crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap();
        let row = reopened
            .load(uuid::Uuid::from_u128(73))
            .await
            .unwrap()
            .unwrap();
        let mut target = server();
        target.ecs.world_mut().resource_mut::<TickCounter>().current = 40;
        let (actor, _packets) = spawn(&mut target, Some(row), 2);
        let world = target.ecs.world();
        assert_eq!(world.get::<PlayerSession>(actor).unwrap().session_id, 2);
        assert_eq!(
            world.get::<PlayerUuid>(actor).unwrap().0,
            uuid::Uuid::from_u128(73)
        );
        let current = capture(world, actor).unwrap();
        assert_eq!(current.effects, snapshot.effects);
        assert_eq!(current.velocity, snapshot.velocity);
        assert_eq!(current.air_current, 17);
        assert_eq!(current.absorption, 4.0);
        assert_eq!(current.burning_remaining, 30);
        assert_eq!(current.burning_next, 6);
        assert_eq!(current.data.age_ticks, 77);
        assert_eq!(current.base_game_mode, Some(0));
        assert_eq!(current.data.game_mode, 1);
        assert_eq!(current.flight_override, Some(true));
        assert_eq!(current.data.inventory_effects, receipt);
        assert_eq!(current.data.inventory_revision, 1);
    }

    #[tokio::test]
    async fn fresh_ordinary_save_retires_checkpoint_and_never_reopens_old_supplement() {
        let directory = tempfile::tempdir().unwrap();
        let provider =
            Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
        let writer = SerializedPlayerProvider::new(provider);
        let row = commit(&writer, &desired()).await;
        let mut target = server();
        let (actor, _packets) = spawn(&mut target, Some(row), 2);
        let world = target.ecs.world_mut();
        world.get_mut::<Health>(actor).unwrap().current = 3.0;
        world.get_mut::<Position>(actor).unwrap().0 = DVec3::new(9.5, 65.62, 9.5);
        world.get_mut::<Effects>(actor).unwrap().active.clear();
        let (_, fresh) = super::super::super::player_record(world, actor).unwrap();
        assert!(fresh.restoration_checkpoint.is_none());
        assert_eq!(
            fresh.game_mode, 0,
            "ordinary saves retain the durable base mode"
        );
        writer
            .save(uuid::Uuid::from_u128(73), &fresh)
            .await
            .unwrap();
        let fresh = writer
            .load(uuid::Uuid::from_u128(73))
            .await
            .unwrap()
            .unwrap();
        assert!(fresh.restoration_checkpoint.is_none());
        assert_eq!(fresh.inventory_revision, 1);
        let mut next = server();
        let (actor, _packets) = spawn(&mut next, Some(fresh), 3);
        let current = capture(next.ecs.world(), actor).unwrap();
        assert_eq!(current.data.health, 3.0);
        assert_eq!(current.data.position, [9.5, 65.62, 9.5]);
        assert!(current.effects.is_empty());
        assert_eq!(current.burning_remaining, 0);
        assert_eq!(current.base_game_mode, None);
        assert_eq!(current.flight_override, None);
        assert_eq!(current.data.game_mode, 0);
    }

    #[tokio::test]
    async fn old_epoch_cannot_overwrite_a_complete_checkpoint_or_its_canonical_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let writer = SerializedPlayerProvider::new(Arc::new(
            crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap(),
        ));
        let snapshot = desired();
        let row = commit(&writer, &snapshot).await;
        assert!(matches!(
            writer.save(uuid::Uuid::from_u128(73), &snapshot.data).await,
            Err(crate::storage::StorageError::PlayerRevisionConflict)
        ));
        let current = writer
            .load(uuid::Uuid::from_u128(73))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current, row);
        let mut invalid = current;
        invalid
            .restoration_checkpoint
            .as_mut()
            .unwrap()
            .request_hash = [0; 32];
        assert!(invalid.validate(uuid::Uuid::from_u128(73)).is_err());
    }

    #[tokio::test]
    async fn ordinary_save_cannot_keep_an_old_checkpoint_beside_advanced_canonical_state() {
        let directory = tempfile::tempdir().unwrap();
        let writer = SerializedPlayerProvider::new(Arc::new(
            crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap(),
        ));
        let mut row = commit(&writer, &desired()).await;
        row.health = 3.0;
        row.position = [12.5, 65.62, 12.5];
        assert!(row.restoration_checkpoint.is_some());
        writer.save(uuid::Uuid::from_u128(73), &row).await.unwrap();
        let saved = writer
            .load(uuid::Uuid::from_u128(73))
            .await
            .unwrap()
            .unwrap();
        assert!(saved.restoration_checkpoint.is_none());
        assert_eq!(saved.health, 3.0);
        assert_eq!(saved.position, row.position);
        assert_eq!(saved.inventory_effects, row.inventory_effects);
    }
}
