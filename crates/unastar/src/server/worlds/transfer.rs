//! Loaded geometry, durable player-position publication and owned ECS migration.

use bevy_ecs::component::ComponentId;
use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::{
    ActorRuntimeId, BlockPos, ChangeDimensionPacket, DimensionType, EnumsPlayerActionType,
    McpePacket, PlayStatusPacket, PlayStatusPacketStatus, PlayerActionPacket, StopSoundPacket,
    Vec3,
};

use super::components::TransferComponents;
use super::hooks::{
    self, PlayerWorldTransfer, PlayerWorldTransferred, TransferError, TransferQueue,
    TransferRequest,
};
use super::loading::DimensionLoading;
use crate::ecs::resources::TickCounter;
use crate::entity::components::*;
use crate::entity::systems::physics::collect_block_boxes;
use crate::entity::systems::{BlockSource, collides_with_blocks};
use crate::registry::collision::Aabb;
use crate::server::game::{
    self, GameServer, SessionEntityMap, inventory_effects, player_events, player_state, types,
};
use crate::world::ecs::{
    ChunkLoader, ChunkManager, ChunkManagerWorldExt, LastPublisherState, PendingChunkGenerations,
};

/// Exact source gameplay facts across native callbacks. Portable snapshots omit
/// network slot IDs, so these and request aliases are additionally retained.
pub(super) struct SourceBaseline {
    snapshot: player_state::PlayerSnapshot,
    ids: Vec<i32>,
    windows: ContainerWindows,
    aliases: game::RequestIdAssignments,
    input: (f32, f32, bool, bool, bool, bool, i64),
    state: (bool, bool, bool, bool, bool),
    tick: u64,
    runtime: i64,
    components: Vec<ComponentId>,
    hurt: Option<crate::entity::mob::combat::HurtTimer>,
    last_hurt: Option<crate::entity::mob::state::LastHurt>,
}
impl SourceBaseline {
    /// Capture complete initialized inventory/life state before extension calls.
    pub(super) fn capture(world: &World, player: Entity) -> Result<Self, TransferError> {
        let snapshot =
            player_state::capture(world, player).map_err(|_| TransferError::SourceUnavailable)?;
        if snapshot.dead
            || inventory_effects::is_quarantined(world, player)
            || player_events::initial_state_pending(world, player)
            || world
                .get::<PlayerUi>(player)
                .is_none_or(|ui| !ui.0.is_empty())
        {
            return Err(TransferError::SourceUnavailable);
        }
        let ids = slot_ids(world, player).ok_or(TransferError::SourceUnavailable)?;
        let input = world
            .get::<PlayerInput>(player)
            .ok_or(TransferError::SourceUnavailable)?;
        let state = world
            .get::<PlayerState>(player)
            .ok_or(TransferError::SourceUnavailable)?;
        Ok(Self {
            snapshot,
            ids,
            windows: world
                .get::<ContainerWindows>(player)
                .ok_or(TransferError::SourceUnavailable)?
                .clone(),
            aliases: world
                .get::<game::RequestIdAssignments>(player)
                .ok_or(TransferError::SourceUnavailable)?
                .clone(),
            input: (
                input.move_x,
                input.move_z,
                input.jumping,
                input.on_ground,
                input.sneaking,
                input.sprinting,
                input.tick,
            ),
            state: (
                state.sneaking,
                state.sprinting,
                state.swimming,
                state.gliding,
                state.flying,
            ),
            tick: world.resource::<TickCounter>().current,
            runtime: world
                .get::<RuntimeEntityId>(player)
                .ok_or(TransferError::SourceUnavailable)?
                .0,
            components: world.entity(player).archetype().components().to_vec(),
            hurt: world
                .get::<crate::entity::mob::combat::HurtTimer>(player)
                .copied(),
            last_hurt: world
                .get::<crate::entity::mob::state::LastHurt>(player)
                .copied(),
        })
    }
    /// Never overwrite a nested accepted operation or a changed session state.
    pub(super) fn matches(&self, world: &World, player: Entity) -> bool {
        Self::capture(world, player).is_ok_and(|current| {
            current.snapshot == self.snapshot
                && current.ids == self.ids
                && current.windows == self.windows
                && current.aliases == self.aliases
                && current.input == self.input
                && current.state == self.state
                && current.tick == self.tick
                && current.runtime == self.runtime
                && current.components == self.components
                && current.hurt == self.hurt
                && current.last_hurt == self.last_hurt
        })
    }
}

/// Read every source stack identity in the same complete group order.
fn slot_ids(world: &World, player: Entity) -> Option<Vec<i32>> {
    let mut ids = world
        .get::<MainInventory>(player)?
        .0
        .slots()
        .iter()
        .map(|stack| stack.net_id)
        .collect::<Vec<_>>();
    ids.extend(
        world
            .get::<ArmourInventory>(player)?
            .slots()
            .iter()
            .map(|stack| stack.net_id),
    );
    ids.push(world.get::<OffhandSlot>(player)?.0.net_id);
    ids.extend(
        world
            .get::<PlayerUi>(player)?
            .0
            .slots()
            .iter()
            .map(|stack| stack.net_id),
    );
    ids.extend(
        world
            .get::<EnderChestInventory>(player)?
            .0
            .slots()
            .iter()
            .map(|stack| stack.net_id),
    );
    Some(ids)
}

/// Whether every chunk touched by a human player's body/support is authoritative.
fn loaded(world: &World, feet: DVec3) -> bool {
    if !feet.is_finite()
        || feet.x.abs() > f64::from(i32::MAX - 32)
        || feet.z.abs() > f64::from(i32::MAX - 32)
    {
        return false;
    }
    let Some(manager) = world.get_resource::<ChunkManager>() else {
        return false;
    };
    let min = (feet - DVec3::new(0.3, 0.0625, 0.3)).floor().as_ivec3();
    let max = (feet + DVec3::new(0.3, 1.8, 0.3)).floor().as_ivec3();
    let bounds = crate::world::dimension_block_bounds(manager.dimension());
    if min.y < bounds.start || max.y >= bounds.end {
        return false;
    }
    (min.x >> 4..=max.x >> 4).all(|x| {
        (min.z >> 4..=max.z >> 4).all(|z| {
            manager
                .get_by_coords(x, z)
                .is_some_and(|chunk| world.get::<crate::world::ecs::ChunkData>(chunk).is_some())
        })
    })
}

/// Validate loaded native body clearance, support and hazardous block occupancy.
/// Missing terrain is never accepted as air.
pub fn is_safe_destination(world: &World, feet: DVec3) -> bool {
    if !loaded(world, feet) {
        return false;
    }
    let body = Aabb::new(
        feet + DVec3::new(-0.3, 0.0, -0.3),
        feet + DVec3::new(0.3, 1.8, 0.3),
    );
    let support = Aabb::new(
        feet + DVec3::new(-0.3, -0.0625, -0.3),
        feet + DVec3::new(0.3, 0.0, 0.3),
    );
    if collides_with_blocks(world, body) || !collides_with_blocks(world, support) {
        return false;
    }
    let min = (body.min - DVec3::Y * 0.0625).floor().as_ivec3();
    let max = body.max.floor().as_ivec3();
    let registry = &world.resource::<types::BlockRegistryResource>().0;
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let Some(state) = world.block_at(IVec3::new(x, y, z)) else {
                    return false;
                };
                let Some(block) = registry.get_by_runtime_id(state) else {
                    return false;
                };
                if registry.liquid_state(state).is_some()
                    || matches!(
                        block.string_id.as_str(),
                        "minecraft:fire"
                            | "minecraft:soul_fire"
                            | "minecraft:cactus"
                            | "minecraft:powder_snow"
                            | "minecraft:magma"
                            | "minecraft:magma_block"
                            | "minecraft:campfire"
                            | "minecraft:soul_campfire"
                    )
                {
                    return false;
                }
            }
        }
    }
    true
}

/// Choose the nearest actual collision-surface height within the approved radius.
/// Native shape tops handle slabs and other fractional support heights.
pub fn find_safe_destination(world: &World, desired: DVec3, radius: u32) -> Option<DVec3> {
    if radius > 256 || !desired.is_finite() {
        return None;
    }
    if is_safe_destination(world, desired) {
        return Some(desired);
    }
    let mut boxes = Vec::new();
    let area = Aabb::new(
        desired + DVec3::new(-0.3, -f64::from(radius) - 1.0, -0.3),
        desired + DVec3::new(0.3, f64::from(radius) + 0.01, 0.3),
    );
    collect_block_boxes(world, area, &mut boxes);
    let mut heights = boxes
        .into_iter()
        .map(|area| area.max.y)
        .filter(|y| (y - desired.y).abs() <= f64::from(radius))
        .collect::<Vec<_>>();
    heights.sort_by(|a, b| {
        (a - desired.y)
            .abs()
            .total_cmp(&(b - desired.y).abs())
            .then_with(|| b.total_cmp(a))
    });
    heights.dedup();
    heights
        .into_iter()
        .map(|y| DVec3::new(desired.x, y, desired.z))
        .find(|&feet| is_safe_destination(world, feet))
}

#[derive(Resource)]
pub(super) struct PreloadAnchor(pub Entity);
#[derive(Resource)]
pub(super) struct PreloadUntil(pub u64);
/// Queue native background generation without treating a source-world Entity as
/// a viewer in another ECS world. Synchronous providers use the same chunk path.
pub(super) fn prepare_chunks(world: &mut World, feet: DVec3) -> Result<bool, TransferError> {
    if !feet.is_finite()
        || feet.x.abs() > f64::from(i32::MAX - 32)
        || feet.z.abs() > f64::from(i32::MAX - 32)
    {
        return Err(TransferError::UnsafeDestination);
    }
    let min = (feet - DVec3::new(0.3, 0.0, 0.3)).floor().as_ivec3();
    let max = (feet + DVec3::new(0.3, 0.0, 0.3)).floor().as_ivec3();
    let anchor = if let Some(anchor) = world.get_resource::<PreloadAnchor>() {
        anchor.0
    } else {
        let entity = world.spawn_empty().id();
        world.insert_resource(PreloadAnchor(entity));
        entity
    };
    let mut complete = true;
    for x in min.x >> 4..=max.x >> 4 {
        for z in min.z >> 4..=max.z >> 4 {
            let manager = world.resource::<ChunkManager>();
            if !manager
                .world_config()
                .bounds
                .contains(crate::world::ChunkPos::new(x, z))
            {
                return Err(TransferError::UnsafeDestination);
            }
            if let Some(entity) = manager.get_by_coords(x, z) {
                if let Some(mut viewers) = world.get_mut::<crate::world::ecs::ChunkViewers>(entity)
                {
                    viewers.insert(anchor);
                }
                continue;
            }
            if manager.has_async_generation() {
                let receiver = world
                    .resource_mut::<ChunkManager>()
                    .request_generation(x, z, anchor);
                if let Some(receiver) = receiver {
                    world
                        .resource_mut::<PendingChunkGenerations>()
                        .add(x, z, receiver);
                }
                complete = false;
            } else {
                let entity = world
                    .try_get_or_create_chunk(x, z)
                    .map_err(|_| TransferError::Persistence)?;
                if let Some(mut viewers) = world.get_mut::<crate::world::ecs::ChunkViewers>(entity)
                {
                    viewers.insert(anchor);
                }
            }
        }
    }
    Ok(complete)
}

/// Retry only loading/backpressure; no accepted state or packets have run yet.
fn retry(source: &mut World, mut request: TransferRequest) {
    if request.remaining_ticks <= 1 {
        hooks::failed(source, request, TransferError::Timeout);
        return;
    }
    request.remaining_ticks -= 1;
    source
        .get_resource_or_init::<TransferQueue>()
        .0
        .push(request);
}

/// Apply one real transfer after target loading. Both owned ECS instances stay
/// exclusive while storage acknowledges the destination record.
pub(super) async fn attempt(
    source: &mut GameServer,
    target: &mut GameServer,
    request: TransferRequest,
    sequence: &mut u32,
) {
    if !hooks::owns_connection(source.ecs.world(), request.player, &request.identity) {
        hooks::failed(source.ecs.world_mut(), request, TransferError::StaleSource);
        return;
    }
    let ready = match prepare_chunks(target.ecs.world_mut(), request.feet) {
        Ok(ready) => ready,
        Err(error) => {
            hooks::failed(source.ecs.world_mut(), request, error);
            return;
        }
    };
    if !ready {
        retry(source.ecs.world_mut(), request);
        return;
    }
    let Some(feet) = find_safe_destination(target.ecs.world(), request.feet, request.safe_y_radius)
    else {
        hooks::failed(
            source.ecs.world_mut(),
            request,
            TransferError::UnsafeDestination,
        );
        return;
    };
    let baseline = match SourceBaseline::capture(source.ecs.world(), request.player) {
        Ok(baseline) => baseline,
        Err(error) => {
            hooks::failed(source.ecs.world_mut(), request, error);
            return;
        }
    };
    let rules = source
        .ecs
        .world()
        .get_resource::<TransferComponents>()
        .cloned()
        .unwrap_or_default();
    let unsupported = rules.unsupported(source.ecs.world(), request.player);
    if !unsupported.is_empty() {
        hooks::failed(
            source.ecs.world_mut(),
            request,
            TransferError::UnsupportedComponents(unsupported),
        );
        return;
    }
    let from_dimension = source.ecs.world().resource::<ChunkManager>().dimension();
    let to_dimension = target.ecs.world().resource::<ChunkManager>().dimension();
    let source_tick = source.ecs.world().resource::<TickCounter>().current;
    let from = source
        .ecs
        .world()
        .get::<Position>(request.player)
        .unwrap()
        .0
        - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    let mut event = PlayerWorldTransfer {
        player: request.player,
        identity: request.identity.clone(),
        request_id: request.request_id.clone(),
        from_dimension,
        to_dimension,
        from,
        feet,
        cancelled: false,
    };
    if !hooks::proposed(source.ecs.world_mut(), target.ecs.world_mut(), &mut event) {
        hooks::failed(source.ecs.world_mut(), request, TransferError::Rejected);
        return;
    }
    if !hooks::owns_connection(source.ecs.world(), request.player, &request.identity)
        || !baseline.matches(source.ecs.world(), request.player)
        || !rules
            .unsupported(source.ecs.world(), request.player)
            .is_empty()
    {
        hooks::failed(source.ecs.world_mut(), request, TransferError::StaleSource);
        return;
    }
    let destination_busy = target
        .ecs
        .world()
        .resource::<SessionEntityMap>()
        .get(request.identity.session_id())
        .is_some()
        || {
            let world = target.ecs.world_mut();
            let mut actors = world.query::<&RuntimeEntityId>();
            actors.iter(world).any(|id| id.0 == baseline.runtime)
        };
    if destination_busy
        || source.ecs.world().resource::<ChunkManager>().dimension() != from_dimension
        || target.ecs.world().resource::<ChunkManager>().dimension() != to_dimension
    {
        hooks::failed(source.ecs.world_mut(), request, TransferError::StaleSource);
        return;
    }
    if !is_safe_destination(target.ecs.world(), event.feet) {
        hooks::failed(
            source.ecs.world_mut(),
            request,
            TransferError::UnsafeDestination,
        );
        return;
    }
    let outbound = source
        .ecs
        .world()
        .get::<PlayerSession>(request.player)
        .unwrap()
        .outbound_tx
        .clone();
    let permits = match outbound.try_reserve_many(4) {
        Ok(permits) => permits,
        Err(_) => {
            retry(source.ecs.world_mut(), request);
            return;
        }
    };
    if !(request.validator)(source.ecs.world(), target.ecs.world(), &event) {
        hooks::failed(source.ecs.world_mut(), request, TransferError::Rejected);
        return;
    }
    let target_tick = target.ecs.world().resource::<TickCounter>().current;
    let recovery =
        match inventory_effects::RecoverySource::capture(source.ecs.world(), request.player) {
            Ok(recovery) => recovery,
            Err(_) => {
                hooks::failed(
                    source.ecs.world_mut(),
                    request,
                    TransferError::SourceUnavailable,
                );
                return;
            }
        };
    let Some((uuid, mut data)) = game::player_record(source.ecs.world(), request.player) else {
        hooks::failed(
            source.ecs.world_mut(),
            request,
            TransferError::SourceUnavailable,
        );
        return;
    };
    let eye = event.feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    data.dimension = to_dimension;
    data.position = eye.to_array();
    let persistent = source
        .ecs
        .world()
        .contains_resource::<types::PlayerProviderResource>()
        || source
            .ecs
            .world()
            .contains_resource::<types::PlayerDataStoreResource>();
    if persistent && !source.save_player_record(uuid, &data).await {
        let _ = inventory_effects::fence_for_recovery(source.ecs.world_mut(), &recovery);
        hooks::failed(
            source.ecs.world_mut(),
            request,
            TransferError::RecoveryRequired,
        );
        return;
    }
    let runtime = source
        .ecs
        .world()
        .get::<RuntimeEntityId>(request.player)
        .unwrap()
        .0;
    let radius = source
        .ecs
        .world()
        .get::<ChunkRadius>(request.player)
        .unwrap()
        .0;
    let tick = source
        .ecs
        .world()
        .get::<PlayerInput>(request.player)
        .unwrap()
        .tick;
    let restricted = source
        .ecs
        .world()
        .get::<ActorAudience>(request.player)
        .is_some_and(|audience| !audience.is_unrestricted());
    game::prepare_world_departure(source.ecs.world_mut(), request.player);
    source
        .ecs
        .world_mut()
        .resource_mut::<SessionEntityMap>()
        .remove(request.identity.session_id());
    let moved = rules.extract(source.ecs.world_mut(), request.player);
    source.ecs.world_mut().despawn(request.player);
    let player = target.ecs.world_mut().spawn_empty().id();
    moved.install(target.ecs.world_mut(), player);
    let position = Position(eye);
    let rotation = *target.ecs.world().get::<Rotation>(player).unwrap();
    let mut loader = ChunkLoader::new(radius);
    loader.move_to((eye.x.floor() as i32) >> 4, (eye.z.floor() as i32) >> 4);
    loader.force_reload();
    // Replace the temporary load pin with a real local viewer before it is
    // released. The destination loader expands this view on its next tick.
    let chunk = target
        .ecs
        .world()
        .resource::<ChunkManager>()
        .get_by_coords((eye.x.floor() as i32) >> 4, (eye.z.floor() as i32) >> 4);
    if let Some(chunk) = chunk
        && let Some(mut viewers) = target
            .ecs
            .world_mut()
            .get_mut::<crate::world::ecs::ChunkViewers>(chunk)
    {
        viewers.insert(player);
    }
    target.ecs.world_mut().entity_mut(player).insert((
        position,
        PlayerState::default(),
        PlayerInput {
            tick,
            ..Default::default()
        },
        BreakingState::default(),
        SpatialChunk::from_position(&position),
        LastBroadcastPosition {
            x: eye.x,
            y: eye.y,
            z: eye.z,
            yaw: rotation.yaw,
            pitch: rotation.pitch,
        },
        loader,
        LastPublisherState::default(),
        ContainerWindows::default(),
        game::RequestIdAssignments::default(),
        game::attributes::SentAttributes::default(),
        game::effects::SentEffects::default(),
        crate::entity::player_life::environment::PlayerFall::default(),
    ));
    if let Some(mut velocity) = target.ecs.world_mut().get_mut::<Velocity>(player) {
        velocity.0 = DVec3::ZERO;
    }
    remap_timers(target.ecs.world_mut(), player, source_tick, target_tick);
    target
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(if restricted {
            ActorAudience::only([])
        } else {
            ActorAudience::all()
        });
    target
        .ecs
        .world_mut()
        .resource_mut::<SessionEntityMap>()
        .insert(request.identity.session_id(), player);
    assert!(
        player_events::defer_initial_state(target.ecs.world_mut(), player),
        "complete migrated player supports publication barrier"
    );
    *sequence = sequence.wrapping_add(1).max(1);
    let loading_id = *sequence;
    target
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(DimensionLoading {
            identity: request.identity.clone(),
            id: loading_id,
            expires: target_tick.saturating_add(200),
        });
    let packets = [
        McpePacket::from(ChangeDimensionPacket {
            dimension_id: DimensionType {
                value: to_dimension,
            },
            position: Vec3 {
                x: eye.x as f32,
                y: eye.y as f32,
                z: eye.z as f32,
            },
            respawn: false,
            loading_screen_id: Some(loading_id),
        }),
        McpePacket::from(StopSoundPacket {
            sound_name: "".into(),
            stop_all_sounds: true,
            stop_music_legacy: true,
        }),
        McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Playerspawn,
        }),
        McpePacket::from(PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime as u64,
            },
            action: EnumsPlayerActionType::Changedimensionack,
            block_position: BlockPos::default(),
            result_pos: BlockPos::default(),
            face: 0,
        }),
    ];
    for (permit, packet) in permits.zip(packets) {
        permit.send(packet);
    }
    hooks::committed(
        source.ecs.world_mut(),
        target.ecs.world_mut(),
        PlayerWorldTransferred {
            identity: request.identity,
            request_id: request.request_id,
            previous_player: request.player,
            player,
            from_dimension,
            to_dimension,
            from,
            feet: event.feet,
        },
    );
    if player_events::release_initial_state(target.ecs.world_mut(), player) {
        target.publish_initial_states();
    }
}

/// Translate native absolute-tick timers and discard actor references belonging
/// to another ECS world; relative effects and stable application IDs are intact.
fn remap_timers(world: &mut World, player: Entity, source: u64, target: u64) {
    if let Some(mut burning) =
        world.get_mut::<crate::entity::player_life::environment::Burning>(player)
    {
        burning.until_tick = target.saturating_add(burning.until_tick.saturating_sub(source));
        burning.next_burn_tick =
            target.saturating_add(burning.next_burn_tick.saturating_sub(source));
    }
    if let Some(mut hurt) = world.get_mut::<crate::entity::mob::combat::HurtTimer>(player) {
        hurt.until_tick = target.saturating_add(hurt.until_tick.saturating_sub(source));
    }
    if let Some(mut hurt) = world.get_mut::<crate::entity::mob::state::LastHurt>(player) {
        hurt.tick = target.saturating_sub(source.saturating_sub(hurt.tick));
        hurt.attacker = None;
    }
}
