//! A real vanilla Enderman and focused authored block/teleport controls through
//! native actor spawning, physics, AI, damage, replication and persistence.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::{McpePacket, McpePacketData};
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::EntityRandom;
use crate::entity::components::{
    ArmourInventory, GameMode, Health, HeldSlot, MainInventory, PhysicsBody, Player, PlayerSession,
    Position, Rotation, RuntimeEntityId,
};
use crate::entity::mob::components::teleport::Teleport;
use crate::entity::mob::{MobRegistry, SpawnCause, spawn_mob};
use crate::entity::systems::BlockSource;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{
    ChunkData, ChunkManager, ChunkPosition, ChunkStateFlags, ChunkViewers, world_to_chunk_coords,
    world_to_local_coords,
};
use crate::world::rules::{Difficulty, GameRules};

const FEET: DVec3 = DVec3::new(4.5, 64.0, 4.5);
struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A wide loaded solid floor supplies real safe destinations; natural spawning
/// is disabled so every actor and inventory in a test has a known owner.
fn harness() -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let world = server.ecs.world_mut();
    world.init_resource::<crate::server::game::ContainerMutationLocks>();
    world.insert_resource(EntityRandom::seeded(33));
    *world.resource_mut::<Difficulty>() = Difficulty::Normal;
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    crate::world::clock::set_time(world, 18_000);
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(FEET + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 6.0)),
            Rotation::default(),
            GameMode::Creative,
            Health::default(),
            ArmourInventory::default(),
            MainInventory::default(),
            crate::entity::components::OffhandSlot::default(),
            crate::entity::components::PlayerUi::default(),
            crate::entity::components::Experience::default(),
            HeldSlot(0),
            RuntimeEntityId(1),
            PlayerSession::new(1, "observer".into(), None, None, tx),
        ))
        .id();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut chunk = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    chunk.set_block(x, 63, z, *blocks::STONE);
                }
            }
            let mut viewers = ChunkViewers::default();
            viewers.insert(player);
            let entity = world
                .spawn((ChunkData::new(chunk), viewers, ChunkStateFlags::default()))
                .id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(cx, cz), entity);
        }
    }
    Harness { server, player, rx }
}

impl Harness {
    /// Spawn only through the public runtime registry, surfacing the real reason
    /// an incomplete authored definition is not spawnable.
    fn spawn(&mut self, name: &str, feet: DVec3) -> Entity {
        assert!(
            self.server
                .ecs
                .world()
                .resource::<MobRegistry>()
                .get(name)
                .is_some(),
            "{:?}",
            self.server
                .ecs
                .world()
                .resource::<MobRegistry>()
                .unsupported_reason(name)
        );
        spawn_mob(
            self.server.ecs.world_mut(),
            name,
            feet,
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap()
    }
    /// Run complete native server frames.
    fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.server.tick();
        }
    }
    /// Set one real loaded block cell for a geometry/environment control.
    fn set(&mut self, position: IVec3, state: u32) {
        let world = self.server.ecs.world_mut();
        let (cx, cz) = world_to_chunk_coords(position.x, position.z);
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(cx, cz)
            .unwrap();
        let (x, y, z) = world_to_local_coords(position.x, position.y, position.z);
        assert!(
            world
                .get_mut::<ChunkData>(chunk)
                .unwrap()
                .inner
                .set_block(x, y, z, state)
                .is_some()
        );
    }
    /// Disable unrelated random/chase hops while testing another component.
    fn quiet_teleport(&mut self, actor: Entity) {
        let mut state = self
            .server
            .ecs
            .world_mut()
            .get_mut::<Teleport>(actor)
            .unwrap();
        let config = std::sync::Arc::make_mut(&mut state.config);
        config.random = false;
        config.target_chance = 0.0;
        self.server
            .ecs
            .world_mut()
            .entity_mut(actor)
            .remove::<crate::entity::mob::components::RandomStrollGoal>();
    }
}

#[test]
fn vanilla_enderman_is_spawnable_with_real_health_body_and_authored_teleport() {
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    let world = h.server.ecs.world();
    let health = world.get::<Health>(actor).unwrap();
    assert_eq!((health.current, health.max), (40.0, 40.0));
    let body = world.get::<PhysicsBody>(actor).unwrap();
    assert!((body.width - 0.6).abs() < 1e-6 && (body.height - 2.9).abs() < 1e-6);
    let air = crate::server::game::types::block_network_ids(world).to_wire(*blocks::AIR) as i32;
    assert!(
        matches!(world.get::<crate::entity::definition::metadata::EntityMetadata>(actor).unwrap()
        .get(jolyne::valentine::actor_data_i_ds::DEPRECATED_CARRY_BLOCK_RUNTIME_ID),
        Some(jolyne::valentine::types::DataItemEntryPayload::DataItemIntPayload(value)) if value.value==air)
    );
    assert!(
        world
            .get::<Teleport>(actor)
            .unwrap()
            .config
            .projectile_reactive
    );
    h.quiet_teleport(actor);
    h.ticks(1);
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<PhysicsBody>(actor)
            .unwrap()
            .step_height,
        1.0625
    );
    assert!(std::iter::from_fn(||h.rx.try_recv().ok()).any(|packet|
        matches!(packet.data,McpePacketData::AddActorPacket(actor) if actor.actor_type=="minecraft:enderman")));
}

#[test]
fn actual_water_contact_hurts_then_escapes_to_loaded_dry_ground_with_effects() {
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.quiet_teleport(actor);
    let water = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:water")
        .unwrap()
        .default_state_id;
    for y in 64..=66 {
        h.set(IVec3::new(4, y, 4), water);
    }
    h.ticks(1);
    let world = h.server.ecs.world();
    let feet = world.get::<Position>(actor).unwrap().0;
    assert!(
        feet.distance_squared(FEET) > 1.0,
        "water escape is a real position change"
    );
    assert_eq!(world.get::<Health>(actor).unwrap().current, 39.0);
    let body = world.get::<PhysicsBody>(actor).unwrap();
    assert!(
        crate::entity::systems::physics::living_liquid(
            world,
            crate::registry::collision::Aabb::from_feet(feet, body.width, body.height)
        )
        .is_none()
    );
    assert!(
        world
            .block_at((feet - DVec3::Y * 0.01).floor().as_ivec3())
            .is_some()
    );
    assert!(std::iter::from_fn(||h.rx.try_recv().ok()).any(|packet|
        matches!(packet.data,McpePacketData::SpawnParticleEffectPacket(effect) if effect.effect_name=="minecraft:mob_portal")));
}

impl Harness {
    /// Aim the player's actual native rotation at the actor's current head.
    fn aim(&mut self, actor: Entity) {
        let world = self.server.ecs.world();
        let eye = world.get::<Position>(self.player).unwrap().0;
        let body = world.get::<PhysicsBody>(actor).unwrap();
        let head = world.get::<Position>(actor).unwrap().0
            + DVec3::Y * body.height * crate::entity::mob::constants::MOB_EYE_HEIGHT_RATIO;
        let delta = (head - eye).normalize();
        let yaw = (delta.z.atan2(delta.x).to_degrees() - 90.0) as f32;
        let pitch = (-delta.y.asin().to_degrees()) as f32;
        *self
            .server
            .ecs
            .world_mut()
            .get_mut::<Rotation>(self.player)
            .unwrap() = Rotation::new(yaw, pitch);
    }
    /// Read actual terrain rather than a client preview.
    fn block(&self, position: IVec3) -> u32 {
        self.server.ecs.world().block_at(position).unwrap()
    }
    /// Deterministic geometry controls retain the real compiled descriptor and
    /// visibility rules, while making each native goal attempt predictable.
    fn carry_control(&mut self, actor: Entity) {
        use crate::entity::mob::components::carry::{PlaceBlockGoal, TakeBlockGoal};
        self.quiet_teleport(actor);
        self.server
            .ecs
            .world_mut()
            .entity_mut(actor)
            .remove::<crate::entity::mob::components::RandomStrollGoal>();
        let world = self.server.ecs.world_mut();
        let mut take = world.get_mut::<TakeBlockGoal>(actor).unwrap();
        take.base.chance = 1.0;
        take.base.xz = (1, 1);
        take.base.y = (0, 0);
        let mut place = world.get_mut::<PlaceBlockGoal>(actor).unwrap();
        place.base.chance = 1.0;
        place.base.xz = (2, 2);
        place.base.y = (0, 0);
    }
    /// Apply a real committed player-attributed terminal hit through the normal
    /// health, death, loot and carried-state ownership boundary.
    fn kill(&mut self, actor: Entity) {
        crate::entity::mob::combat::hurt_mob(
            self.server.ecs.world_mut(),
            actor,
            crate::entity::mob::combat::Hit {
                damage: 1_000.0,
                cause: ActorDamageCause::Override,
                attacker: Some(self.player),
                by_player: true,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
            },
        );
    }
    /// Count native dropped stacks without collecting or editing them.
    fn drops(&mut self, name: &str) -> u32 {
        self.server.ecs.world_mut().query_filtered::<&crate::entity::components::ItemEntityStack,
            With<crate::entity::components::DroppedItem>>().iter(self.server.ecs.world())
            .filter(|item|item.0.item_id()==name).map(|item|u32::from(item.0.count)).sum()
    }
}

#[test]
fn real_pumpkin_gaze_then_anger_melee_and_calm_follow_the_authored_lifecycle() {
    use crate::entity::mob::components::reactive::Angry;
    use crate::entity::mob::state::AttackTarget;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.quiet_teleport(actor);
    h.server
        .ecs
        .world_mut()
        .entity_mut(actor)
        .remove::<crate::entity::mob::components::RandomStrollGoal>();
    *h.server
        .ecs
        .world_mut()
        .get_mut::<GameMode>(h.player)
        .unwrap() = GameMode::Survival;
    h.aim(actor);
    let pumpkin =
        crate::server::game::world_stack(h.server.ecs.world(), "minecraft:carved_pumpkin", 1)
            .unwrap();
    h.server
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(h.player)
        .unwrap()
        .set_helmet(pumpkin)
        .unwrap();
    h.ticks(10);
    assert!(h.server.ecs.world().get::<AttackTarget>(actor).is_none());
    h.server
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(h.player)
        .unwrap()
        .set_helmet(crate::item::ItemStack::empty())
        .unwrap();
    h.aim(actor);
    h.ticks(7);
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<AttackTarget>(actor)
            .map(|target| target.0),
        Some(h.player)
    );
    assert!(h.server.ecs.world().get::<Angry>(actor).is_some());
    for _ in 0..160 {
        h.ticks(1);
        if h.server
            .ecs
            .world()
            .get::<Health>(h.player)
            .unwrap()
            .current
            < 20.0
        {
            break;
        }
    }
    assert!(
        h.server
            .ecs
            .world()
            .get::<Health>(h.player)
            .unwrap()
            .current
            < 20.0,
        "native navigation and melee actually hurt the player"
    );
    *h.server
        .ecs
        .world_mut()
        .get_mut::<GameMode>(h.player)
        .unwrap() = GameMode::Creative;
    let deadline = h.server.ecs.world().get::<Angry>(actor).unwrap().until_tick;
    h.server
        .ecs
        .world_mut()
        .resource_mut::<crate::ecs::TickCounter>()
        .current = deadline;
    h.ticks(1);
    assert!(h.server.ecs.world().get::<Angry>(actor).is_none());
    assert!(h.server.ecs.world().get::<AttackTarget>(actor).is_none());
}

#[test]
fn checked_hops_reject_unloaded_or_wet_geometry_and_publish_forced_native_movement() {
    use crate::entity::components::{FallDistance, Velocity};
    use crate::entity::mob::teleport::teleport_mob;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.quiet_teleport(actor);
    h.ticks(1);
    while h.rx.try_recv().is_ok() {}
    let before = h.server.ecs.world().get::<Position>(actor).unwrap().0;
    assert!(!teleport_mob(
        h.server.ecs.world_mut(),
        actor,
        DVec3::new(10_000.5, 65.0, 10_000.5)
    ));
    assert_eq!(
        h.server.ecs.world().get::<Position>(actor).unwrap().0,
        before
    );
    let water = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:water")
        .unwrap()
        .default_state_id;
    h.set(IVec3::new(12, 64, 4), water);
    assert!(!teleport_mob(
        h.server.ecs.world_mut(),
        actor,
        DVec3::new(12.5, 64.0, 4.5)
    ));
    h.set(IVec3::new(12, 64, 4), *blocks::AIR);
    h.set(IVec3::new(12, 65, 4), *blocks::STONE);
    assert!(!teleport_mob(
        h.server.ecs.world_mut(),
        actor,
        DVec3::new(12.5, 64.0, 4.5)
    ));
    h.set(IVec3::new(12, 65, 4), *blocks::AIR);
    h.server
        .ecs
        .world_mut()
        .get_mut::<FallDistance>(actor)
        .unwrap()
        .distance = 50.0;
    assert!(teleport_mob(
        h.server.ecs.world_mut(),
        actor,
        DVec3::new(12.5, 70.0, 4.5)
    ));
    assert_eq!(
        h.server.ecs.world().get::<Position>(actor).unwrap().0,
        DVec3::new(12.5, 64.0, 4.5)
    );
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<FallDistance>(actor)
            .unwrap()
            .distance,
        0.0
    );
    assert_eq!(
        h.server.ecs.world().get::<Velocity>(actor).unwrap().0,
        DVec3::ZERO
    );
    h.ticks(1);
    assert!(std::iter::from_fn(||h.rx.try_recv().ok()).any(|packet|matches!(packet.data,
        McpePacketData::MoveActorDeltaPacket(update) if update.move_data.force_move && update.move_data.force_completion
            && update.move_data.new_position_x==Some(12.5) && update.move_data.new_position_y==Some(64.0))));
}

#[test]
fn a_real_arrow_misses_after_escape_and_hits_normally_when_no_safe_hop_exists() {
    use crate::entity::projectile::launch::{LaunchSpec, launch_with};
    use unastar_data::actor_identifiers::ActorType;
    for escapes in [true, false] {
        let mut h = harness();
        let actor = h.spawn("minecraft:enderman", FEET);
        h.quiet_teleport(actor);
        h.server
            .ecs
            .world_mut()
            .entity_mut(actor)
            .remove::<crate::entity::mob::components::RandomStrollGoal>();
        if !escapes {
            let mut state = h.server.ecs.world_mut().get_mut::<Teleport>(actor).unwrap();
            std::sync::Arc::make_mut(&mut state.config).cube = DVec3::ZERO;
        }
        h.aim(actor);
        let projectile = launch_with(
            h.server.ecs.world_mut(),
            h.player,
            ActorType::Arrow,
            None,
            LaunchSpec {
                speed: Some(3.0),
                throw_sound: false,
                ..Default::default()
            },
        )
        .unwrap();
        h.ticks(6);
        let health = h.server.ecs.world().get::<Health>(actor).unwrap().current;
        if escapes {
            assert_eq!(health, 40.0);
            assert!(h.server.ecs.world().get_entity(projectile).is_ok());
            assert!(
                h.server
                    .ecs
                    .world()
                    .get::<Position>(actor)
                    .unwrap()
                    .0
                    .distance_squared(FEET)
                    > 1.0
            );
        } else {
            assert!(
                health < 40.0,
                "failed teleport preserves ordinary projectile damage"
            );
        }
    }
}

#[test]
fn native_take_and_place_move_one_real_block_with_display_and_no_overwrite() {
    use crate::entity::mob::carry::CarriedBlock;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let source = IVec3::new(5, 64, 5);
    let target = IVec3::new(6, 64, 6);
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    h.set(source, dirt);
    h.set(target, *blocks::STONE);
    h.ticks(1);
    assert_eq!(h.block(source), *blocks::AIR);
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(actor),
        Some(&CarriedBlock(dirt))
    );
    h.ticks(1);
    assert_eq!(h.block(target), *blocks::STONE);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_some());
    h.set(target, *blocks::AIR);
    h.server
        .ecs
        .world_mut()
        .get_mut::<Position>(h.player)
        .unwrap()
        .0 = target.as_dvec3() + DVec3::new(0.5, crate::entity::PLAYER_EYE_HEIGHT, 0.5);
    h.ticks(1);
    assert_eq!(h.block(target), *blocks::AIR);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_some());
    h.server
        .ecs
        .world_mut()
        .get_mut::<Position>(h.player)
        .unwrap()
        .0 = FEET + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 6.0);
    assert!(
        h.server
            .ecs
            .world_mut()
            .resource_mut::<crate::server::game::ContainerMutationLocks>()
            .acquire("pending constructor", [target])
    );
    h.ticks(1);
    assert_eq!(h.block(target), *blocks::AIR);
    assert!(
        h.server
            .ecs
            .world_mut()
            .resource_mut::<crate::server::game::ContainerMutationLocks>()
            .release("pending constructor", [target])
    );
    h.ticks(1);
    assert_eq!(h.block(target), dirt);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_none());
    let expected =
        crate::server::game::types::block_network_ids(h.server.ecs.world()).to_wire(dirt);
    let runtime = h
        .server
        .ecs
        .world()
        .get::<crate::entity::components::RuntimeId>(actor)
        .unwrap()
        .0 as u64;
    let mut carried = false;
    let mut cleared = false;
    while let Ok(packet) = h.rx.try_recv() {
        if let McpePacketData::MobEquipmentPacket(packet) = packet.data
            && packet.target_runtime_id.actor_runtime_id == runtime
            && (packet.slot, packet.selected_slot, packet.container_id) == (0, 0, 0)
        {
            if packet.item.stacksize == 1 && packet.item.block_runtime_id == expected {
                carried = true;
                assert!(packet.item.net_id_variant.is_none());
            } else if packet.item.id == 0 && packet.item.stacksize == 0 && carried {
                cleared = true;
            }
        }
    }
    assert!(
        carried && cleared,
        "native carried main-hand display follows ownership without changing physical gear"
    );
}

#[test]
fn grief_rule_and_source_locks_leave_both_terrain_and_carried_state_unchanged() {
    use crate::entity::mob::carry::CarriedBlock;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let source = IVec3::new(5, 64, 5);
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    h.set(source, dirt);
    h.server
        .ecs
        .world_mut()
        .resource_mut::<GameRules>()
        .mob_griefing = false;
    h.ticks(2);
    assert_eq!(h.block(source), dirt);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_none());
    h.server
        .ecs
        .world_mut()
        .resource_mut::<GameRules>()
        .mob_griefing = true;
    assert!(
        h.server
            .ecs
            .world_mut()
            .resource_mut::<crate::server::game::ContainerMutationLocks>()
            .acquire("pending constructor", [source])
    );
    h.ticks(2);
    assert_eq!(h.block(source), dirt);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_none());
    assert!(
        h.server
            .ecs
            .world_mut()
            .resource_mut::<crate::server::game::ContainerMutationLocks>()
            .release("pending constructor", [source])
    );
    h.ticks(1);
    assert_eq!(h.block(source), *blocks::AIR);
}

#[test]
fn a_late_world_policy_change_rejects_other_already_staged_takes() {
    use crate::entity::mob::carry::CarriedBlock;
    use crate::world::ecs::BlockChanged;
    let mut h = harness();
    let first = h.spawn("minecraft:enderman", FEET);
    h.carry_control(first);
    let second = h.spawn("minecraft:enderman", FEET + DVec3::new(4.0, 0.0, 4.0));
    h.carry_control(second);
    let sources = [IVec3::new(5, 64, 5), IVec3::new(9, 64, 9)];
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    for source in sources {
        h.set(source, dirt);
    }
    h.server
        .ecs
        .world_mut()
        .add_observer(|_: On<BlockChanged>, mut rules: ResMut<GameRules>| {
            rules.mob_griefing = false;
        });
    h.ticks(1);
    assert_eq!(
        [first, second]
            .iter()
            .filter(|actor| h.server.ecs.world().get::<CarriedBlock>(**actor).is_some())
            .count(),
        1
    );
    assert_eq!(
        sources
            .iter()
            .filter(|source| h.block(**source) == dirt)
            .count(),
        1
    );
}

#[test]
fn actual_death_drops_carried_ownership_once_and_vanilla_pearl_loot_is_available() {
    use crate::entity::mob::carry::CarriedBlock;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let source = IVec3::new(5, 64, 5);
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    h.set(source, dirt);
    h.ticks(1);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_some());
    h.kill(actor);
    assert_eq!(h.drops("minecraft:dirt"), 1);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_none());
    crate::entity::mob::combat::death::begin_death(h.server.ecs.world_mut(), actor);
    assert_eq!(h.drops("minecraft:dirt"), 1);
    for _ in 0..20 {
        let actor = h.spawn("minecraft:enderman", FEET);
        h.kill(actor);
    }
    assert!(
        h.drops("minecraft:ender_pearl") > 0,
        "real default loot tables produce pearls for the gameplay loop"
    );
}

#[test]
fn carried_state_survives_actor_restore_and_malformed_named_states_fail() {
    use crate::entity::mob::carry::CarriedBlock;
    use crate::entity::mob::persistence::{encode_for_tests, restore_mob};
    use crate::registry::block::StateValue;
    use zuri_nbt::{NBTTag, encoding::LittleEndian};
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let cactus = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:cactus")
        .unwrap();
    let state = cactus
        .with_state_value(cactus.default_state_id, "age", StateValue::Int(3))
        .unwrap();
    h.set(IVec3::new(5, 64, 5), state);
    h.ticks(1);
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(actor),
        Some(&CarriedBlock(state))
    );
    let saved = encode_for_tests(h.server.ecs.world_mut(), actor);
    h.server.ecs.world_mut().despawn(actor);
    let restored = restore_mob(h.server.ecs.world_mut(), &saved).unwrap();
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(restored),
        Some(&CarriedBlock(state))
    );
    let mut input = saved.as_slice();
    let NBTTag::Compound(mut root) = NBTTag::read(&mut input, LittleEndian).unwrap() else {
        panic!("actor root")
    };
    root.0
        .insert("UniqueID".into(), NBTTag::Long(995_111i64.into()));
    let Some(NBTTag::Compound(carried)) = root.0.get_mut("UnastarCarriedBlock") else {
        panic!("carry")
    };
    let Some(NBTTag::Compound(properties)) = carried.0.get_mut("States") else {
        panic!("states")
    };
    properties
        .0
        .insert("age".into(), NBTTag::Int((-1i32).into()));
    let mut corrupt = Vec::new();
    NBTTag::Compound(root)
        .write(&mut corrupt, LittleEndian)
        .unwrap();
    assert!(restore_mob(h.server.ecs.world_mut(), &corrupt).is_err());
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(restored),
        Some(&CarriedBlock(state))
    );
}

#[test]
fn independent_nonexclusive_goals_keep_generated_blocks_separate_from_carried_ownership() {
    use crate::entity::mob::carry::CarriedBlock;
    use crate::entity::mob::components::carry::{BlockMatcher, PlaceBlockGoal, WeightedBlock};
    use std::sync::Arc;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    let source = IVec3::new(5, 64, 5);
    let target = IVec3::new(6, 64, 6);
    h.set(source, dirt);
    h.server
        .ecs
        .world_mut()
        .get_mut::<PlaceBlockGoal>(actor)
        .unwrap()
        .random = Arc::from([WeightedBlock {
        block: BlockMatcher {
            states: Arc::from([dirt]),
            placement: dirt,
        },
        weight: 1,
        filter: None,
    }]);
    h.ticks(1);
    assert_eq!(h.block(source), *blocks::AIR);
    assert_eq!(h.block(target), dirt);
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(actor),
        Some(&CarriedBlock(dirt))
    );
    h.set(target, *blocks::AIR);
    h.ticks(1);
    assert_eq!(h.block(target), dirt);
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(actor),
        Some(&CarriedBlock(dirt)),
        "generated placement cannot consume an equal carried state"
    );
}

#[test]
fn blocked_sight_defers_taking_until_real_geometry_is_clear() {
    use crate::entity::mob::carry::CarriedBlock;
    use crate::entity::mob::components::carry::{PlaceBlockGoal, TakeBlockGoal};
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    h.server
        .ecs
        .world_mut()
        .get_mut::<TakeBlockGoal>(actor)
        .unwrap()
        .base
        .xz = (2, 2);
    h.server
        .ecs
        .world_mut()
        .get_mut::<PlaceBlockGoal>(actor)
        .unwrap()
        .base
        .chance = 0.0;
    let source = IVec3::new(6, 64, 6);
    h.set(source, dirt);
    h.set(IVec3::new(5, 65, 5), *blocks::STONE);
    h.ticks(1);
    assert_eq!(h.block(source), dirt);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_none());
    h.set(IVec3::new(5, 65, 5), *blocks::AIR);
    h.ticks(1);
    assert_eq!(h.block(source), *blocks::AIR);
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(actor),
        Some(&CarriedBlock(dirt))
    );
}

#[test]
fn random_minimum_and_maximum_waits_and_target_displacement_are_enforced_by_real_ticks() {
    use crate::entity::mob::state::AttackTarget;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.quiet_teleport(actor);
    {
        let mut state = h.server.ecs.world_mut().get_mut::<Teleport>(actor).unwrap();
        let config = std::sync::Arc::make_mut(&mut state.config);
        config.random = true;
        config.wait = (4, 4);
        config.cube = DVec3::new(8.0, 0.0, 8.0);
        config.dark_chance = 0.0;
        config.light_chance = 0.0;
        state.earliest_random_tick = 4;
        state.next_random_tick = 4;
    }
    h.ticks(4);
    assert_eq!(h.server.ecs.world().get::<Position>(actor).unwrap().0, FEET);
    h.ticks(1);
    assert!(
        h.server
            .ecs
            .world()
            .get::<Position>(actor)
            .unwrap()
            .0
            .distance_squared(FEET)
            > 0.01
    );
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<Teleport>(actor)
            .unwrap()
            .next_random_tick,
        8
    );
    {
        h.server
            .ecs
            .world_mut()
            .get_mut::<Position>(actor)
            .unwrap()
            .0 = FEET;
        *h.server
            .ecs
            .world_mut()
            .get_mut::<GameMode>(h.player)
            .unwrap() = GameMode::Survival;
        h.server
            .ecs
            .world_mut()
            .entity_mut(actor)
            .insert(AttackTarget(h.player));
        let mut state = h.server.ecs.world_mut().get_mut::<Teleport>(actor).unwrap();
        let config = std::sync::Arc::make_mut(&mut state.config);
        config.random = false;
        config.target_chance = 1.0;
        config.target_distance = 4.0;
        config.cube = DVec3::ZERO;
    }
    h.ticks(1);
    let after = h.server.ecs.world().get::<Position>(actor).unwrap().0;
    assert!(after.z > FEET.z && after.distance(FEET) <= 4.0 + 1e-8);
}

#[test]
fn foreign_block_actor_payloads_are_never_removed_by_a_take_goal() {
    use crate::entity::mob::carry::CarriedBlock;
    use crate::world::block_entity::BlockEntity;
    use valentine::bedrock::codec::Nbt;
    use zuri_nbt::{NBTTag, encoding::NetworkLittleEndian, tag};
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let source = IVec3::new(5, 64, 5);
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    h.set(source, dirt);
    let payload = tag::Compound::builder()
        .with_string("id", "Sign")
        .with_int("x", 5)
        .with_int("y", 64)
        .with_int("z", 5)
        .with_string("Text", "preserve foreign actor")
        .build();
    let mut bytes = Vec::new();
    NBTTag::Compound(payload)
        .write(&mut bytes, NetworkLittleEndian)
        .unwrap();
    let foreign = BlockEntity::from_network_nbt(Nbt(bytes.into())).unwrap();
    let world = h.server.ecs.world_mut();
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    assert!(
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block_entity(foreign.clone())
    );
    h.ticks(2);
    assert_eq!(h.block(source), dirt);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_none());
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .block_entities(),
        &[foreign]
    );
}

#[test]
fn saved_teleport_waits_preserve_remaining_deadlines_on_a_new_server_clock() {
    use crate::entity::mob::persistence::{encode_for_tests, restore_mob};
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.quiet_teleport(actor);
    {
        let mut state = h.server.ecs.world_mut().get_mut::<Teleport>(actor).unwrap();
        state.earliest_random_tick = 10;
        state.next_random_tick = 20;
    }
    h.ticks(3);
    let saved = encode_for_tests(h.server.ecs.world_mut(), actor);
    h.server.ecs.world_mut().despawn(actor);
    h.server
        .ecs
        .world_mut()
        .resource_mut::<crate::ecs::TickCounter>()
        .current = 100;
    let actor = restore_mob(h.server.ecs.world_mut(), &saved).unwrap();
    let state = h.server.ecs.world().get::<Teleport>(actor).unwrap();
    assert_eq!(state.earliest_random_tick, 107);
    assert_eq!(state.next_random_tick, 117);
}

#[test]
fn carried_display_uses_the_negotiated_state_numbering_and_empty_hand_clear() {
    use crate::entity::mob::carry;
    use crate::entity::mob::gear::replicate::mainhand_packet;
    use crate::registry::block::BlockNetworkIds;
    use crate::server::game::types::ItemRegistryResource;
    for mode in [BlockNetworkIds::Hashed, BlockNetworkIds::Sequential] {
        let h = harness();
        let world = h.server.ecs.world();
        let registry = crate::registry::BlockRegistry::vanilla();
        let dirt = registry
            .get_by_name("minecraft:dirt")
            .unwrap()
            .default_state_id;
        let items = &world.resource::<ItemRegistryResource>().0;
        let stack = carry::display_stack(dirt, items).unwrap();
        let McpePacketData::MobEquipmentPacket(packet) =
            mainhand_packet(10, &stack, items, mode).data
        else {
            panic!("equipment")
        };
        let expected = match mode {
            BlockNetworkIds::Sequential => dirt,
            BlockNetworkIds::Hashed => registry.network_hash(dirt).unwrap(),
        };
        assert_eq!(packet.item.block_runtime_id, expected);
        assert_eq!(packet.item.stacksize, 1);
        assert!(packet.item.net_id_variant.is_none());
        assert_eq!(
            (packet.slot, packet.selected_slot, packet.container_id),
            (0, 0, 0)
        );
        let McpePacketData::MobEquipmentPacket(packet) =
            mainhand_packet(10, &crate::item::ItemStack::empty(), items, mode).data
        else {
            panic!("equipment")
        };
        assert_eq!(packet.item.id, 0);
        assert_eq!(packet.item.stacksize, 0);
    }
}

#[test]
fn carried_equipment_projection_preserves_physical_gear_and_reaches_late_viewers() {
    use crate::entity::mob::carry::CarriedBlock;
    use crate::entity::mob::gear::{GearSlot, MobEquipment};
    use crate::server::game::types::ItemRegistryResource;
    let mut h = harness();
    let actor = h.spawn("minecraft:enderman", FEET);
    h.carry_control(actor);
    let sword =
        crate::server::game::world_stack(h.server.ecs.world(), "minecraft:iron_sword", 1).unwrap();
    let sword_id = sword.item().unwrap().network_id();
    let mut physical = MobEquipment::default();
    physical.set(GearSlot::Mainhand, sword, true);
    h.server.ecs.world_mut().entity_mut(actor).insert(physical);
    let dirt = crate::registry::BlockRegistry::vanilla()
        .get_by_name("minecraft:dirt")
        .unwrap()
        .default_state_id;
    let dirt_id = h
        .server
        .ecs
        .world()
        .resource::<ItemRegistryResource>()
        .0
        .get_by_name("minecraft:dirt")
        .unwrap()
        .network_id();
    h.set(IVec3::new(5, 64, 5), dirt);
    h.set(IVec3::new(6, 64, 6), *blocks::STONE);
    h.ticks(1);
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<MobEquipment>(actor)
            .unwrap()
            .get(GearSlot::Mainhand)
            .item_id(),
        "minecraft:iron_sword"
    );
    assert_eq!(
        h.server.ecs.world().get::<CarriedBlock>(actor),
        Some(&CarriedBlock(dirt))
    );
    let (tx, mut late_rx) = mpsc::channel(1 << 12);
    let world = h.server.ecs.world_mut();
    let late = world
        .spawn((
            Player,
            Position(FEET + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 8.0)),
            Rotation::default(),
            GameMode::Creative,
            Health::default(),
            MainInventory::default(),
            ArmourInventory::default(),
            crate::entity::components::OffhandSlot::default(),
            crate::entity::components::PlayerUi::default(),
            HeldSlot(0),
            RuntimeEntityId(2),
            PlayerSession::new(2, "late observer".into(), None, None, tx),
        ))
        .id();
    for mut viewers in world.query::<&mut ChunkViewers>().iter_mut(world) {
        viewers.insert(late);
    }
    h.ticks(1);
    assert!(std::iter::from_fn(||late_rx.try_recv().ok()).any(|packet|matches!(packet.data,
        McpePacketData::MobEquipmentPacket(packet) if i32::from(packet.item.id)==dirt_id && packet.slot==0)));
    h.set(IVec3::new(6, 64, 6), *blocks::AIR);
    h.ticks(1);
    assert!(h.server.ecs.world().get::<CarriedBlock>(actor).is_none());
    assert!(std::iter::from_fn(||late_rx.try_recv().ok()).any(|packet|matches!(packet.data,
        McpePacketData::MobEquipmentPacket(packet) if i32::from(packet.item.id)==sword_id && packet.slot==0)));
    h.kill(actor);
    assert_eq!(h.drops("minecraft:iron_sword"), 1);
}
