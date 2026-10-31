//! Validated, serializable player-state capture and authoritative restoration.
//!
//! Snapshots retain raw life state, including zero health and the dead marker.
//! Restoration is a state operation: it does not drop items, award statistics or
//! emit gameplay death/respawn notifications. Use the normal lifecycle APIs for
//! gameplay respawns. Entity, account, connection and runtime identities remain
//! owned by the target; snapshots never replace them. Open windows and input,
//! fall and hurt bookkeeping are cleared while captured velocity is restored.

use std::collections::BTreeSet;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsMobEffectPacketPayloadEvent, PlayerInputTick, Vec3,
};
use jolyne::valentine::{
    ActorEventPacket, ContainerClosePacket, MobEffectPacket, SetActorMotionPacket,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ecs::resources::TickCounter;
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{
    ActiveEffect, AirSupply, ArmourInventory, BreakingState, ContainerWindows, EffectType, Effects,
    EnderChestInventory, Experience, FallDistance, GameMode, Health, HeldSlot, Hunger,
    MainInventory, OffhandSlot, OnFire, PLAYER_UI_SIZE, Player, PlayerSession, PlayerUi,
    PlayerUuid, Position, Rotation, RuntimeEntityId, ScreenKind, Velocity,
};
use crate::entity::mob::combat::HurtTimer;
use crate::entity::mob::state::LastHurt;
use crate::entity::player_life::environment::{Burning, PlayerFall};
use crate::entity::player_life::{Dead, attributes as life_attributes};
use crate::storage::{PlayerData, StoredItemStack};

use super::attributes::{Absorption, MAX_ABSORPTION, SentAttributes};
use super::effects::SentEffects;
use super::types::{ItemRegistryResource, ServerConfigResource};

const SNAPSHOT_VERSION: u32 = 1;

/// Account binding retained after transport ownership is removed.
#[derive(Component)]
pub(super) struct DetachedIdentity(Option<String>);

/// Preserve the verified account binding before detaching a connection.
pub(super) fn remember_identity(world: &mut World, player: Entity) {
    let identity = world
        .get::<PlayerSession>(player)
        .and_then(|session| session.xuid.clone());
    if let Ok(mut actor) = world.get_entity_mut(player) {
        actor.insert(DetachedIdentity(identity));
    }
}

/// Recreate a validated sessionless actor without synthesizing a network join.
/// The actor receives a fresh runtime ID and no connection, input or chunk loader.
pub fn spawn_detached(
    world: &mut World,
    name: &str,
    snapshot: &PlayerSnapshot,
) -> Result<Entity, SnapshotError> {
    use crate::entity::components::{
        ChunkRadius, LastBroadcastPosition, PlayerName, PlayerState, SpatialChunk,
    };
    snapshot.validate()?;
    if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        return Err(invalid("invalid actor display name"));
    }
    if world
        .get_resource::<ServerConfigResource>()
        .ok_or(SnapshotError::MissingResource)?
        .0
        .world
        .dimension
        != snapshot.data.dimension
    {
        return Err(SnapshotError::WrongDimension);
    }
    let _ = prepare(world, snapshot)?;
    let runtime = loop {
        let runtime = {
            let mut counter = world
                .get_resource_mut::<super::ItemEntityIdCounter>()
                .ok_or(SnapshotError::MissingResource)?;
            counter.0 = counter
                .0
                .checked_add(1)
                .ok_or_else(|| invalid("runtime identifier exhausted"))?;
            counter.0
        };
        let mut query = world.query::<&RuntimeEntityId>();
        if query.iter(world).all(|existing| existing.0 != runtime) {
            break runtime;
        }
    };
    let position = Position(DVec3::from_array(snapshot.data.position));
    let rotation = Rotation::new(snapshot.data.rotation[0], snapshot.data.rotation[1]);
    let player = world
        .spawn((
            Player,
            PlayerName(name.into()),
            PlayerUuid(
                uuid::Uuid::parse_str(&snapshot.data.uuid)
                    .map_err(|_| SnapshotError::WrongAccount)?,
            ),
            RuntimeEntityId(runtime),
            position,
            rotation,
            GameMode::Survival,
            PlayerState::default(),
            ChunkRadius(0),
            SpatialChunk::from_position(&position),
            LastBroadcastPosition {
                x: position.0.x,
                y: position.0.y,
                z: position.0.z,
                yaw: rotation.yaw,
                pitch: rotation.pitch,
            },
            super::player_events::DetachedPlayer,
            DetachedIdentity(snapshot.xuid.clone()),
        ))
        .id();
    world.entity_mut(player).insert((
        MainInventory::default(),
        ArmourInventory::default(),
        OffhandSlot::default(),
        PlayerUi::default(),
        EnderChestInventory::default(),
        HeldSlot::default(),
        ContainerWindows::default(),
        super::RequestIdAssignments::default(),
        Health::default(),
        Hunger::default(),
        Experience::default(),
        Effects::default(),
    ));
    world.entity_mut(player).insert((
        PlayerFall::default(),
        super::hunger::FoodTimer::default(),
        SentEffects::default(),
        SentAttributes::sent(&Health::default(), &Hunger::default(), 0.0),
    ));
    if let Err(error) = restore(world, player, snapshot) {
        world.despawn(player);
        return Err(error);
    }
    world.write_message(crate::world::ecs::PlayerSpawnedEvent {
        entity: player,
        position: position.0,
        runtime_id: runtime,
    });
    Ok(player)
}

/// One active effect, independent of ECS component storage and hash iteration order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEffect {
    pub id: u8,
    pub level: u8,
    pub duration_ticks: u32,
    pub ambient: bool,
    pub show_particles: bool,
    pub infinite: bool,
}

/// Complete raw state required to move a player's gameplay state between actors.
/// Positions follow the host's player eye-coordinate convention.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSnapshot {
    pub version: u32,
    pub data: PlayerData,
    pub xuid: Option<String>,
    /// Includes every UI slot, including the cursor and crafting inputs.
    pub ui: Vec<StoredItemStack>,
    pub velocity: [f64; 3],
    pub head_yaw: f32,
    pub absorption: f32,
    pub air_current: u32,
    pub air_max: u32,
    pub on_fire_ticks: u32,
    pub burning_remaining: u64,
    pub burning_next: u64,
    pub effects: Vec<SnapshotEffect>,
    pub dead: bool,
}

/// A snapshot failure leaves the target's gameplay and network state unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SnapshotError {
    #[error("target is not a fully initialized player")]
    MissingPlayer,
    #[error("snapshot account does not match the target player")]
    WrongAccount,
    #[error("snapshot dimension does not match the current world")]
    WrongDimension,
    #[error("snapshot format version is unsupported")]
    UnsupportedVersion,
    #[error("required player-state registry or configuration is absent")]
    MissingResource,
    #[error("invalid player snapshot: {0}")]
    InvalidState(String),
}

impl PlayerSnapshot {
    /// Validate the complete serializable structure before resolving item types.
    pub fn validate(&self) -> Result<(), SnapshotError> {
        if self.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::UnsupportedVersion);
        }
        let uuid =
            uuid::Uuid::parse_str(&self.data.uuid).map_err(|_| SnapshotError::WrongAccount)?;
        self.data.validate(uuid).map_err(invalid)?;
        if self.data.inventory.len() != 36
            || self.data.ender_chest.len() != crate::entity::components::ENDER_CHEST_SIZE
            || self.ui.len() != PLAYER_UI_SIZE
            || self.ui.first() != Some(&self.data.cursor)
            || self.dead != (self.data.health == 0.0)
            || !self.data.position.iter().all(|value| {
                value.is_finite() && *value >= f64::from(i32::MIN) && *value < f64::from(i32::MAX)
            })
            || !DVec3::from_array(self.velocity).is_finite()
            || !DVec3::from_array(self.velocity).as_vec3().is_finite()
            || !self.head_yaw.is_finite()
            || !self.absorption.is_finite()
            || !(0.0..=MAX_ABSORPTION).contains(&self.absorption)
            || !(0.0..=20.0).contains(&self.data.saturation)
            || !(0.0..=20.0).contains(&self.data.exhaustion)
            || self.air_max == 0
            || self.air_max > u32::MAX - 4
            || self.air_current > self.air_max
            || (self.burning_remaining == 0 && self.burning_next != 0)
            || self.effects.len() > 36
            || self
                .effects
                .iter()
                .any(|effect| !(1..=36).contains(&effect.id))
            || self
                .effects
                .iter()
                .map(|effect| effect.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.effects.len()
            || self.xuid.as_ref().is_some_and(|xuid| {
                xuid.is_empty() || xuid.len() > 128 || xuid.chars().any(char::is_control)
            })
        {
            return Err(SnapshotError::InvalidState(
                "inconsistent life, inventory or spatial state".into(),
            ));
        }
        Ok(())
    }
}

/// Capture raw player state without normalizing a dead player to its next respawn.
/// Detached actors are supported; a missing connection does not change ownership.
pub fn capture(world: &World, player: Entity) -> Result<PlayerSnapshot, SnapshotError> {
    if world.get::<Player>(player).is_none()
        || world.get::<Health>(player).is_none()
        || world.get::<Hunger>(player).is_none()
        || world.get::<MainInventory>(player).is_none()
        || world.get::<ArmourInventory>(player).is_none()
        || world.get::<OffhandSlot>(player).is_none()
        || world.get::<EnderChestInventory>(player).is_none()
        || world.get::<HeldSlot>(player).is_none()
    {
        return Err(SnapshotError::MissingPlayer);
    }
    if !world.contains_resource::<ServerConfigResource>() {
        return Err(SnapshotError::MissingResource);
    }
    let (_, data) =
        super::capture_player_record(world, player, false).ok_or(SnapshotError::MissingPlayer)?;
    let ui = world
        .get::<PlayerUi>(player)
        .ok_or(SnapshotError::MissingPlayer)?;
    let air = world.get::<AirSupply>(player).cloned().unwrap_or_default();
    let now = world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current);
    let burning = world
        .get::<Burning>(player)
        .filter(|burning| burning.until_tick > now);
    let mut effects: Vec<_> = world
        .get::<Effects>(player)
        .into_iter()
        .flat_map(|effects| &effects.active)
        .map(|(kind, effect)| SnapshotEffect {
            id: kind.0,
            level: effect.level,
            duration_ticks: effect.duration_ticks,
            ambient: effect.ambient,
            show_particles: effect.show_particles,
            infinite: effect.infinite,
        })
        .collect();
    effects.sort_by_key(|effect| effect.id);
    let snapshot = PlayerSnapshot {
        version: SNAPSHOT_VERSION,
        data,
        xuid: world
            .get::<PlayerSession>(player)
            .and_then(|session| session.xuid.clone())
            .or_else(|| {
                world
                    .get::<DetachedIdentity>(player)
                    .and_then(|identity| identity.0.clone())
            })
            .filter(|id| !id.is_empty()),
        ui: ui.0.slots().iter().map(StoredItemStack::from).collect(),
        velocity: world
            .get::<Velocity>(player)
            .map_or(DVec3::ZERO, |velocity| velocity.0)
            .to_array(),
        head_yaw: world
            .get::<Rotation>(player)
            .ok_or(SnapshotError::MissingPlayer)?
            .head_yaw,
        absorption: world
            .get::<Absorption>(player)
            .map_or(0.0, |amount| amount.0),
        air_current: air.current_ticks,
        air_max: air.max_ticks,
        on_fire_ticks: world
            .get::<OnFire>(player)
            .map_or(0, |fire| fire.ticks_remaining),
        burning_remaining: burning.map_or(0, |burning| burning.until_tick - now),
        burning_next: burning.map_or(0, |burning| burning.next_burn_tick.saturating_sub(now)),
        effects,
        dead: world.get::<Dead>(player).is_some(),
    };
    snapshot.validate()?;
    Ok(snapshot)
}

/// Restore validated gameplay state while retaining account, entity and connection IDs.
/// All item resolution and structural validation finish before windows, state or
/// packets change. Item stack network IDs are freshly allocated for the target.
pub fn restore(
    world: &mut World,
    player: Entity,
    snapshot: &PlayerSnapshot,
) -> Result<(), SnapshotError> {
    snapshot.validate()?;
    validate_target(world, player, snapshot)?;
    let prepared = prepare(world, snapshot)?;
    let now = world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current);
    let burning = if snapshot.burning_remaining == 0 {
        None
    } else {
        Some(Burning {
            until_tick: now
                .checked_add(snapshot.burning_remaining)
                .ok_or_else(|| invalid("burning deadline overflow"))?,
            next_burn_tick: now
                .checked_add(snapshot.burning_next)
                .ok_or_else(|| invalid("burning pulse overflow"))?,
        })
    };
    let old_effects: BTreeSet<_> = world
        .get::<Effects>(player)
        .into_iter()
        .flat_map(|effects| effects.active.keys().map(|kind| kind.0))
        .chain(
            world
                .get::<SentEffects>(player)
                .into_iter()
                .flat_map(|sent| sent.effect_types().map(|kind| kind.0)),
        )
        .collect();
    close_window(world, player);
    let health = Health::with_current(snapshot.data.health, snapshot.data.max_health);
    let hunger = Hunger {
        food_level: snapshot.data.food,
        saturation: snapshot.data.saturation,
        exhaustion: snapshot.data.exhaustion,
    };
    let experience = Experience {
        level: snapshot.data.experience,
        progress: snapshot.data.experience_progress,
    };
    let effects = Effects {
        active: snapshot
            .effects
            .iter()
            .map(|effect| {
                (
                    EffectType(effect.id),
                    ActiveEffect {
                        level: effect.level,
                        duration_ticks: effect.duration_ticks,
                        ambient: effect.ambient,
                        show_particles: effect.show_particles,
                        infinite: effect.infinite,
                    },
                )
            })
            .collect(),
    };
    let sent_effects = SentEffects::sent(&effects);
    {
        let mut entity = world.entity_mut(player);
        entity.insert((
            prepared.main,
            prepared.armour,
            prepared.offhand,
            prepared.ui,
            prepared.ender,
            HeldSlot(snapshot.data.held_slot),
            health.clone(),
            hunger.clone(),
            experience.clone(),
            effects,
            Absorption(snapshot.absorption),
            AirSupply {
                current_ticks: snapshot.air_current,
                max_ticks: snapshot.air_max,
            },
            OnFire {
                ticks_remaining: snapshot.on_fire_ticks,
            },
            Velocity(DVec3::from_array(snapshot.velocity)),
        ));
        entity.insert((
            PlayerFall::default(),
            FallDistance::default(),
            SentAttributes::sent(&health, &hunger, snapshot.absorption),
            sent_effects,
            super::RequestIdAssignments::default(),
        ));
        entity.remove::<(HurtTimer, LastHurt, super::item_use::ItemUsing, Burning)>();
        if snapshot.dead {
            entity.insert(Dead);
        } else {
            entity.remove::<Dead>();
        }
        if let Some(mut breaking) = entity.get_mut::<BreakingState>() {
            breaking.stop();
        }
        if let Some(burning) = burning {
            entity.insert(burning);
        }
    }
    crate::command::exec::set_game_mode(world, player, prepared.mode);
    let eye = DVec3::from_array(snapshot.data.position);
    crate::command::exec::move_entity(
        world,
        player,
        eye - DVec3::Y * PLAYER_EYE_HEIGHT,
        snapshot.data.rotation[0],
        snapshot.data.rotation[1],
    );
    // Preserve the captured double-precision value across feet/eye conversion.
    if let Some(mut position) = world.get_mut::<Position>(player) {
        position.0 = eye;
    }
    if let Some(mut rotation) = world.get_mut::<Rotation>(player) {
        rotation.head_yaw = snapshot.head_yaw;
    }
    super::send_inventory_contents(world, player);
    life_attributes::send_attributes(
        world,
        player,
        life_attributes::player_attributes(&health, &hunger, snapshot.absorption, &experience),
    );
    send_effects(world, player, &old_effects, snapshot);
    send_life_and_motion(world, player, snapshot);
    Ok(())
}

/// Prepared item components remain local until every slot resolves successfully.
struct Prepared {
    main: MainInventory,
    armour: ArmourInventory,
    offhand: OffhandSlot,
    ui: PlayerUi,
    ender: EnderChestInventory,
    mode: GameMode,
}

/// Resolve complete snapshots with the ordinary persistence item-validation path.
fn prepare(world: &World, snapshot: &PlayerSnapshot) -> Result<Prepared, SnapshotError> {
    let registry = &world
        .get_resource::<ItemRegistryResource>()
        .ok_or(SnapshotError::MissingResource)?
        .0;
    let mut saved = snapshot.data.clone();
    let (main, armour, offhand, mut ui, ender) =
        super::restore_items(&mut saved, registry).map_err(invalid)?;
    for (index, item) in snapshot.ui.iter().cloned().enumerate().skip(1) {
        ui.0.set_item(index, item.restore(registry).map_err(invalid)?)
            .map_err(invalid)?;
    }
    let block_ids = super::types::block_network_ids(world);
    if main
        .0
        .slots()
        .iter()
        .chain(armour.slots())
        .chain(std::iter::once(&offhand.0))
        .chain(ui.0.slots())
        .chain(ender.0.slots())
        .any(|stack| {
            !stack.is_empty()
                && (super::inventory::wire_item(stack, block_ids).is_none()
                    || !crate::item::enchant::valid_user_data(stack))
        })
    {
        return Err(invalid("invalid stored item wire data"));
    }
    let mode = match snapshot.data.game_mode {
        0 => GameMode::Survival,
        1 => GameMode::Creative,
        2 => GameMode::Adventure,
        6 => GameMode::Spectator,
        _ => return Err(invalid("invalid game mode")),
    };
    Ok(Prepared {
        main,
        armour,
        offhand,
        ui,
        ender,
        mode,
    })
}

/// Reject cross-account or cross-world restoration before even closing a window.
fn validate_target(
    world: &World,
    player: Entity,
    snapshot: &PlayerSnapshot,
) -> Result<(), SnapshotError> {
    if world.get::<Player>(player).is_none()
        || world.get::<RuntimeEntityId>(player).is_none()
        || world.get::<Position>(player).is_none()
        || world.get::<Rotation>(player).is_none()
    {
        return Err(SnapshotError::MissingPlayer);
    }
    if world
        .get::<PlayerUuid>(player)
        .map(|id| id.0.to_string())
        .as_deref()
        != Some(snapshot.data.uuid.as_str())
    {
        return Err(SnapshotError::WrongAccount);
    }
    if let (Some(expected), Some(session)) = (&snapshot.xuid, world.get::<PlayerSession>(player))
        && session.xuid.as_ref() != Some(expected)
    {
        return Err(SnapshotError::WrongAccount);
    }
    let config = world
        .get_resource::<ServerConfigResource>()
        .ok_or(SnapshotError::MissingResource)?;
    if config.0.world.dimension != snapshot.data.dimension {
        return Err(SnapshotError::WrongDimension);
    }
    if snapshot.dead
        && world
            .get::<crate::entity::player_life::respawn::PlayerSpawnPoint>(player)
            .is_none()
        && !world.contains_resource::<crate::entity::player_life::respawn::WorldSpawn>()
    {
        return Err(SnapshotError::MissingResource);
    }
    Ok(())
}

/// Close viewer ownership while preserving the snapshot's cursor and grid for replacement.
fn close_window(world: &mut World, player: Entity) {
    let state = world
        .get::<ContainerWindows>(player)
        .map(|windows| (windows.open_id(), windows.screen()));
    if let Some((Some(window), screen)) = state {
        if let ScreenKind::Block(block) = screen {
            super::containers::stop_viewing(world, player, block);
        }
        if let Some(mut windows) = world.get_mut::<ContainerWindows>(player) {
            windows.close();
        }
        if let Some(session) = world.get::<PlayerSession>(player) {
            session.send(
                ContainerClosePacket {
                    container_id: window,
                    container_type: match screen {
                        ScreenKind::Block(block) => block.kind.wire_type() as u8,
                        _ => 0xff,
                    },
                    server_initiated_close: true,
                }
                .into(),
            );
        }
    }
}

/// Replace the client's known effect set and preserve each snapshot's particle flags.
fn send_effects(world: &World, player: Entity, old: &BTreeSet<u8>, snapshot: &PlayerSnapshot) {
    let (Some(session), Some(runtime)) = (
        world.get::<PlayerSession>(player),
        world.get::<RuntimeEntityId>(player),
    ) else {
        return;
    };
    for id in old {
        session.send(
            MobEffectPacket {
                target_runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime.0 as u64,
                },
                event_id: EnumsMobEffectPacketPayloadEvent::Remove,
                effect_id: i32::from(*id),
                effect_amplifier: 0,
                show_particles: false,
                effect_duration_ticks: 0,
                tick: effect_tick(world, player, *id),
                ambient: false,
            }
            .into(),
        );
    }
    for effect in &snapshot.effects {
        session.send(
            MobEffectPacket {
                target_runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime.0 as u64,
                },
                event_id: EnumsMobEffectPacketPayloadEvent::Add,
                effect_id: i32::from(effect.id),
                effect_amplifier: i32::from(effect.level),
                show_particles: effect.show_particles,
                effect_duration_ticks: effect.duration_ticks as i32,
                tick: effect_tick(world, player, effect.id),
                ambient: effect.ambient,
            }
            .into(),
        );
    }
}

/// Use the connection's own input timeline only for effects that change movement.
fn effect_tick(world: &World, player: Entity, effect: u8) -> PlayerInputTick {
    if EffectType(effect).affects_movement() {
        crate::entity::components::input_tick_of(world, player)
    } else {
        PlayerInputTick { inputtick: 0 }
    }
}

/// Synchronize life-state presentation and captured velocity without gameplay callbacks.
fn send_life_and_motion(world: &World, player: Entity, snapshot: &PlayerSnapshot) {
    let Some(runtime) = world.get::<RuntimeEntityId>(player) else {
        return;
    };
    let packet = ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime.0 as u64,
        },
        event_id: if snapshot.dead {
            jolyne::valentine::EnumsActorEvent::Death
        } else {
            jolyne::valentine::EnumsActorEvent::SpawnAlive
        },
        data: 0,
        fire_at_position: None,
    };
    crate::entity::player_life::send_to_player_and_viewers(world, player, &packet.into());
    if let Some(session) = world.get::<PlayerSession>(player) {
        session.send(
            SetActorMotionPacket {
                target_runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime.0 as u64,
                },
                motion: Vec3 {
                    x: snapshot.velocity[0] as f32,
                    y: snapshot.velocity[1] as f32,
                    z: snapshot.velocity[2] as f32,
                },
                tick: crate::entity::components::input_tick_of(world, player),
            }
            .into(),
        );
    }
    if snapshot.dead {
        crate::entity::player_life::respawn::open_respawn_screen(world, player);
    }
}

/// Convert structural and item-resolution failures to a stable public error.
fn invalid(error: impl std::fmt::Display) -> SnapshotError {
    SnapshotError::InvalidState(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::stack;
    use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
    use crate::world::WorldConfig;
    use jolyne::valentine::{McpePacket, McpePacketData};
    use tokio::sync::mpsc::{self, Receiver};

    /// Spawn an initialized connected actor without any network sockets or external data.
    fn player() -> (GameServer, Entity, Receiver<McpePacket>) {
        let mut server = GameServer::with_config(ServerConfig {
            world: WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let (outbound_tx, mut rx) = mpsc::channel(1 << 16);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: 7,
                display_name: "Alice".into(),
                xuid: Some("123".into()),
                uuid: Some("00000000-0000-0000-0000-000000000007".into()),
                runtime_id: 7,
                position: DVec3::new(1.5, 17.62, 1.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .unwrap();
        while rx.try_recv().is_ok() {}
        (server, player, rx)
    }

    /// Drain synchronous restoration packets without running unrelated game ticks.
    fn packets(rx: &mut Receiver<McpePacket>) -> Vec<McpePacketData> {
        std::iter::from_fn(|| rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    #[test]
    fn raw_round_trip_preserves_life_effects_every_inventory_and_connection_identity() {
        let (mut server, player, mut rx) = player();
        {
            let world = server.ecs.world_mut();
            world.get_mut::<Health>(player).unwrap().current = 7.0;
            world.get_mut::<Hunger>(player).unwrap().food_level = 14;
            world
                .get_mut::<MainInventory>(player)
                .unwrap()
                .0
                .set_item(2, stack("minecraft:stone", 17))
                .unwrap();
            world
                .get_mut::<PlayerUi>(player)
                .unwrap()
                .set_cursor(stack("minecraft:stick", 2));
            world
                .get_mut::<PlayerUi>(player)
                .unwrap()
                .0
                .set_item(28, stack("minecraft:oak_log", 4))
                .unwrap();
            world
                .get_mut::<EnderChestInventory>(player)
                .unwrap()
                .0
                .set_item(1, stack("minecraft:diamond", 3))
                .unwrap();
            world
                .entity_mut(player)
                .insert((Absorption(4.0), Velocity(DVec3::new(0.1, 0.2, 0.3))));
            super::super::effects::add_effect(world, player, EffectType::SPEED, 1, 200);
        }
        let snapshot = capture(server.ecs.world(), player).unwrap();
        let snapshot: PlayerSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let old_net = server
            .ecs
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(2)
            .unwrap()
            .net_id;
        let identity = server.ecs.world().get::<PlayerUuid>(player).unwrap().0;
        let session = server
            .ecs
            .world()
            .get::<PlayerSession>(player)
            .unwrap()
            .session_id;
        let runtime = server.ecs.world().get::<RuntimeEntityId>(player).unwrap().0;
        server
            .ecs
            .world_mut()
            .get_mut::<Health>(player)
            .unwrap()
            .current = 20.0;
        restore(server.ecs.world_mut(), player, &snapshot).unwrap();
        assert_eq!(capture(server.ecs.world(), player).unwrap(), snapshot);
        assert_ne!(
            server
                .ecs
                .world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(2)
                .unwrap()
                .net_id,
            old_net
        );
        assert_eq!(
            server.ecs.world().get::<PlayerUuid>(player).unwrap().0,
            identity
        );
        assert_eq!(
            server
                .ecs
                .world()
                .get::<PlayerSession>(player)
                .unwrap()
                .session_id,
            session
        );
        assert_eq!(
            server.ecs.world().get::<RuntimeEntityId>(player).unwrap().0,
            runtime
        );
        let sent = packets(&mut rx);
        assert!(
            sent.iter()
                .any(|packet| matches!(packet, McpePacketData::MovePlayerPacket(_)))
        );
        assert!(
            sent.iter()
                .any(|packet| matches!(packet, McpePacketData::InventoryContentPacket(_)))
        );
        assert!(
            sent.iter()
                .any(|packet| matches!(packet, McpePacketData::UpdateAttributesPacket(_)))
        );
        assert!(
            sent.iter()
                .any(|packet| matches!(packet, McpePacketData::MobEffectPacket(_)))
        );
    }

    #[test]
    fn malformed_snapshots_change_no_slots_life_windows_or_packets() {
        let (mut server, player, mut rx) = player();
        let initial = capture(server.ecs.world(), player).unwrap();
        for kind in 0..8 {
            let mut bad = initial.clone();
            match kind {
                0 => bad.data.health = f32::NAN,
                1 => bad.data.inventory[35].item_id = "minecraft:not_a_real_item".into(),
                2 => bad.velocity[0] = f64::INFINITY,
                3 => {
                    bad.effects = vec![SnapshotEffect {
                        id: 0,
                        level: 0,
                        duration_ticks: 20,
                        ambient: false,
                        show_particles: true,
                        infinite: false,
                    }]
                }
                4 => bad.data.dimension = 1,
                5 => bad.xuid = Some("456".into()),
                6 => bad.ui.pop().map(|_| ()).unwrap(),
                _ => {
                    bad.data.inventory[35] = StoredItemStack {
                        item_id: "minecraft:stone".into(),
                        count: 1,
                        nbt: Some(vec![255]),
                        ..Default::default()
                    };
                }
            }
            // Unknown non-empty identifiers must be resolved before applying any prior slot.
            if kind == 1 {
                bad.data.inventory[35].count = 1;
            }
            assert!(
                restore(server.ecs.world_mut(), player, &bad).is_err(),
                "case {kind}"
            );
            assert_eq!(capture(server.ecs.world(), player).unwrap(), initial);
            assert!(packets(&mut rx).is_empty(), "case {kind}");
        }
    }

    #[test]
    fn raw_dead_state_is_not_normalized_or_redropped_and_detached_actors_are_supported() {
        let (mut server, player, mut rx) = player();
        server
            .ecs
            .world_mut()
            .get_mut::<Health>(player)
            .unwrap()
            .current = 0.0;
        server.ecs.world_mut().entity_mut(player).insert(Dead);
        let dead = capture(server.ecs.world(), player).unwrap();
        assert_eq!(dead.data.health, 0.0);
        server
            .ecs
            .world_mut()
            .entity_mut(player)
            .remove::<Dead>()
            .remove::<PlayerSession>()
            .insert(super::super::player_events::DetachedPlayer);
        server
            .ecs
            .world_mut()
            .get_mut::<Health>(player)
            .unwrap()
            .current = 20.0;
        restore(server.ecs.world_mut(), player, &dead).unwrap();
        assert!(server.ecs.world().get::<Dead>(player).is_some());
        assert_eq!(
            server.ecs.world().get::<Health>(player).unwrap().current,
            0.0
        );
        assert!(server.ecs.world().get::<PlayerSession>(player).is_none());
        assert!(packets(&mut rx).is_empty());
    }

    #[test]
    fn uuid_mismatch_is_rejected_even_when_names_or_transport_ids_match() {
        let (mut server, player, mut rx) = player();
        let mut snapshot = capture(server.ecs.world(), player).unwrap();
        snapshot.data.uuid = "00000000-0000-0000-0000-000000000099".into();
        assert_eq!(
            restore(server.ecs.world_mut(), player, &snapshot),
            Err(SnapshotError::WrongAccount)
        );
        assert!(packets(&mut rx).is_empty());
    }
}

#[cfg(test)]
mod detached_tests {
    use super::*;
    use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig, SessionEntityMap};
    use crate::world::{WorldConfig, ecs::ChunkLoader};

    #[test]
    fn detached_snapshot_spawn_has_fresh_runtime_identity_and_no_transport_or_join() {
        let mut server = GameServer::with_config(ServerConfig {
            world: WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let (outbound_tx, _rx) = tokio::sync::mpsc::channel(1024);
        let connected = server
            .spawn_player(PlayerSpawnData {
                session_id: 7,
                display_name: "Alice".into(),
                xuid: Some("123".into()),
                uuid: Some("00000000-0000-0000-0000-000000000007".into()),
                runtime_id: 7,
                position: DVec3::new(1.5, 17.62, 1.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .unwrap();
        server
            .ecs
            .world_mut()
            .get_mut::<Health>(connected)
            .unwrap()
            .current = 7.0;
        let snapshot = capture(server.ecs.world(), connected).unwrap();
        let detached = spawn_detached(server.ecs.world_mut(), "Alice", &snapshot).unwrap();
        let world = server.ecs.world();
        assert!(world.get::<PlayerSession>(detached).is_none());
        assert!(
            world
                .get::<crate::entity::components::PlayerInput>(detached)
                .is_none()
        );
        assert!(world.get::<ChunkLoader>(detached).is_none());
        assert!(
            world
                .get::<super::super::player_events::DetachedPlayer>(detached)
                .is_some()
        );
        assert_ne!(world.get::<RuntimeEntityId>(detached).unwrap().0, 7);
        assert_eq!(world.resource::<SessionEntityMap>().get(7), Some(connected));
        let restored = capture(world, detached).unwrap();
        assert_eq!(restored, snapshot);
    }
}
