//! The vanilla enderman through a real `GameServer`: native AddActor
//! equality, anger at a staring player (not one wearing a carved pumpkin),
//! teleport destinations and packets, fleeing water, carrying blocks under
//! `mobGriefing`, the carried block's drop, loot and experience.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use jolyne::valentine::{AddActorPacket, McpePacket, McpePacketData};
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;
use valentine::bedrock::codec::BedrockCodec;

use crate::entity::EntityRandom;
use crate::entity::components::{
    ArmourInventory, DroppedItem, Experience, GameMode, Health, HeldSlot, ItemEntityStack,
    MainInventory, PhysicsBody, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
    RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::components::teleport::{TeleportConfig, TeleportTimer};
use crate::entity::mob::constants::{LEVEL_EVENT_PARTICLES_TELEPORT_TRAIL, MOVE_ABSOLUTE_TELEPORT};
use crate::entity::mob::gear::{GearSlot, MobEquipment, mainhand};
use crate::entity::mob::life::teleport::{random_teleport, teleport_toward};
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::mob::*;
use crate::entity::systems::physics::{collides_with_blocks, living_liquid};
use crate::entity::xp_orb::XpOrb;
use crate::item::enchant::{Enchantment, with_enchantments};
use crate::registry::collision::Aabb;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::{Difficulty, GameRules};

const MIDNIGHT: u64 = 18_000;
const ENDERMAN: &str = "minecraft:enderman";

struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at midnight, viewed
/// by one survival player standing on it at (8.5, 64, 0.5).
fn harness(seed: u64) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = Difficulty::Normal;
    crate::world::clock::set_time(world, MIDNIGHT);
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                8.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                0.5,
            )),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            ArmourInventory::default(),
            HeldSlot(0),
            Health::default(),
            Experience::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "steve".into(), None, None, tx),
        ))
        .id();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
                }
            }
            let mut viewers = ChunkViewers::default();
            viewers.insert(player);
            let chunk = world.spawn((ChunkData::new(column), viewers)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(cx, cz), chunk);
        }
    }
    Harness { server, player, rx }
}

fn native(bytes: &[u8]) -> AddActorPacket {
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let packet = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    packet
}

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(mob).unwrap().get(index).unwrap()
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn enderman(&mut self, feet: DVec3) -> Entity {
        let world = self.world();
        let mob = spawn_mob(world, ENDERMAN, feet, 0.0, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        mob
    }

    /// An enderman at `feet` that never teleports on its own (its teleport
    /// component removed; no group change in these tests adds it back).
    fn pinned_enderman(&mut self, feet: DVec3) -> Entity {
        let mob = self.enderman(feet);
        self.world()
            .entity_mut(mob)
            .remove::<(TeleportConfig, TeleportTimer)>();
        mob
    }

    fn set_block(&mut self, pos: IVec3, name: &str) {
        let state = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap();
        let world = self.world();
        let (cx, cz) = crate::world::ecs::world_to_chunk_coords(pos.x, pos.z);
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(cx, cz)
            .unwrap();
        let (x, y, z) = crate::world::ecs::world_to_local_coords(pos.x, pos.y, pos.z);
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(x, y, z, state);
    }

    fn block_name(&mut self, pos: IVec3) -> String {
        use crate::entity::systems::physics::BlockSource;
        let world = self.world();
        let state = world.block_at(pos).unwrap();
        world
            .registry()
            .get_by_runtime_id(state)
            .unwrap()
            .string_id
            .clone()
    }

    fn feet(&mut self, mob: Entity) -> DVec3 {
        self.world().get::<Position>(mob).unwrap().0
    }

    fn target(&mut self, mob: Entity) -> Option<Entity> {
        self.world().get::<AttackTarget>(mob).map(|t| t.0)
    }

    /// Turn the player's head toward the enderman's head.
    fn stare_at(&mut self, mob: Entity) {
        let player = self.player;
        let world = self.world();
        let feet = world.get::<Position>(mob).unwrap().0;
        let height = world.get::<PhysicsBody>(mob).unwrap().height;
        let head = feet + DVec3::Y * height * constants::MOB_EYE_HEIGHT_RATIO;
        let eye = world.get::<Position>(player).unwrap().0;
        let dir = (head - eye).normalize();
        let yaw = (-dir.x).atan2(dir.z).to_degrees() as f32;
        let pitch = (-dir.y).asin().to_degrees() as f32;
        *world.get_mut::<Rotation>(player).unwrap() = Rotation::with_head_yaw(yaw, pitch, yaw);
    }

    /// Tick `ticks` times with the player staring at `mob`; whether the
    /// enderman made the player its target and turned angry.
    fn stared_at(&mut self, mob: Entity, ticks: u64) -> bool {
        for _ in 0..ticks {
            self.stare_at(mob);
            self.server.tick();
            if self.target(mob) == Some(self.player) && flag(self.world(), mob, actor_flags::ANGRY)
            {
                return true;
            }
        }
        false
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect()
    }

    /// Kill `mob`: punched by the player with one health left, or by a
    /// damage without an attacker.
    fn kill(&mut self, mob: Entity, by_player: bool) {
        let player = self.player;
        let world = self.world();
        world.get_mut::<Health>(mob).unwrap().current = 1.0;
        if by_player {
            let feet = world.get::<Position>(mob).unwrap().0;
            world.get_mut::<Position>(player).unwrap().0 =
                feet + DVec3::new(-2.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
            combat::player_attack(world, player, mob, &Default::default());
        } else {
            combat::hurt_mob(
                world,
                mob,
                combat::Hit {
                    damage: 5.0,
                    cause: ActorDamageCause::Magic,
                    attacker: None,
                    by_player: false,
                    knockback_from: None,
                    knockback_power: KnockbackPower::DEFAULT,
                },
            );
        }
        assert!(world.get::<Dead>(mob).is_some());
    }

    /// Every dropped item stack and the total experience of every orb,
    /// removing both from the world.
    fn take_drops(&mut self) -> (Vec<(String, u32)>, u32) {
        let world = self.world();
        let drops: Vec<(Entity, String, u32)> = world
            .query_filtered::<(Entity, &ItemEntityStack), With<DroppedItem>>()
            .iter(world)
            .map(|(entity, stack)| {
                (
                    entity,
                    stack.0.item_id().to_owned(),
                    u32::from(stack.0.count),
                )
            })
            .collect();
        let orbs: Vec<(Entity, u32)> = world
            .query::<(Entity, &XpOrb)>()
            .iter(world)
            .map(|(entity, orb)| (entity, orb.value))
            .collect();
        for entity in drops.iter().map(|d| d.0).chain(orbs.iter().map(|o| o.0)) {
            world.despawn(entity);
        }
        (
            drops.into_iter().map(|(_, id, n)| (id, n)).collect(),
            orbs.iter().map(|o| o.1).sum(),
        )
    }
}

#[test]
fn vanilla_enderman_compiles_to_a_spawnable_archetype() {
    let mut h = harness(1);
    let registry = h.world().resource::<MobRegistry>();
    assert!(
        registry.get(ENDERMAN).is_some(),
        "enderman unsupported: {:?}",
        registry.unsupported_reason(ENDERMAN)
    );
}

#[test]
fn spawned_enderman_add_actor_matches_native_summon() {
    let mut h = harness(2);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/enderman.bin"
    ));
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let mob = h.enderman(feet);
    let world = h.world();
    let ours = add_actor_packet(
        ENDERMAN,
        world.get::<RuntimeId>(mob).unwrap().0,
        world.get::<Position>(mob).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        world.get::<Rotation>(mob).unwrap(),
        world.get::<EntityMetadata>(mob).unwrap(),
        world.get::<ActorProperties>(mob).unwrap(),
        world.get::<ActorAttributes>(mob).unwrap(),
    );
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.attributes_list, native.attributes_list);
    let missing: Vec<_> = native
        .actor_data
        .data
        .iter()
        .filter(|entry| !ours.actor_data.data.contains(entry))
        .collect();
    let extra: Vec<_> = ours
        .actor_data
        .data
        .iter()
        .filter(|entry| !native.actor_data.data.contains(entry))
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "native only: {missing:?}\nours only: {extra:?}"
    );
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.actor_links, native.actor_links);
    assert_eq!(ours.synched_properties, native.synched_properties);
}

#[test]
fn enderman_steps_up_a_full_block() {
    let mut h = harness(3);
    let mob = h.pinned_enderman(DVec3::new(0.5, 64.0, 0.5));
    let body = h.world().get::<PhysicsBody>(mob).unwrap();
    assert_eq!(body.step_height, 1.0625);
}

#[test]
fn enderman_turns_angry_at_a_player_staring_at_it() {
    let mut h = harness(4);
    let mob = h.enderman(DVec3::new(0.5, 64.0, 8.5));
    assert!(h.stared_at(mob, 100), "the stared-at enderman turns angry");
    // Anger plays the angry ambient sound.
    let world = h.world();
    let metadata = world.get::<EntityMetadata>(mob).unwrap();
    assert!(matches!(
        metadata.get(jolyne::valentine::actor_data_i_ds::AMBIENT_SOUND_EVENT_NAME),
        Some(jolyne::valentine::types::DataItemEntryPayload::DataItemStringPayload(name))
            if &*name.value == constants::MAD_SOUND
    ));
}

#[test]
fn enderman_ignores_a_player_wearing_a_carved_pumpkin() {
    let mut h = harness(4);
    let player = h.player;
    let world = h.world();
    let pumpkin = world_stack(world, "minecraft:carved_pumpkin", 1).unwrap();
    world
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .set_slot(0, pumpkin)
        .unwrap();
    let mob = h.enderman(DVec3::new(0.5, 64.0, 8.5));
    assert!(!h.stared_at(mob, 100));
    assert_eq!(h.target(mob), None);
}

#[test]
fn random_teleports_land_on_ground_inside_the_cube() {
    let mut h = harness(5);
    let mob = h.enderman(DVec3::new(8.5, 64.0, 8.5));
    assert_eq!(
        h.world().get::<TeleportConfig>(mob).unwrap().random_cube,
        DVec3::splat(32.0)
    );
    // Give the viewer the AddActor first.
    h.server.tick();
    h.packets();
    let mut landed = 0;
    for _ in 0..200 {
        let before = h.feet(mob);
        let world = h.world();
        if !random_teleport(world, mob) {
            assert_eq!(world.get::<Position>(mob).unwrap().0, before);
            continue;
        }
        landed += 1;
        let feet = world.get::<Position>(mob).unwrap().0;
        let moved = feet - before;
        assert!(moved.x.abs() <= 16.0 && moved.z.abs() <= 16.0, "{moved:?}");
        // The only ground is the grass floor.
        assert_eq!(feet.y, 64.0);
        let body = *world.get::<PhysicsBody>(mob).unwrap();
        let aabb = Aabb::from_feet(feet, body.width, body.height);
        assert!(!collides_with_blocks(world, aabb));
        assert!(living_liquid(world, aabb).is_none());
    }
    assert!(landed > 20, "{landed} of 200 random teleports landed");

    // The viewer sees the teleport: an absolute move with the teleport bit,
    // the trail particles and the teleport sound.
    h.server.tick();
    let packets = h.packets();
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::MoveActorAbsolutePacket(m)
            if m.move_data.header & MOVE_ABSOLUTE_TELEPORT != 0)));
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::LevelEventGenericPacket(e)
            if e.event_id == LEVEL_EVENT_PARTICLES_TELEPORT_TRAIL)));
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::LevelEventPacket(e)
            if e.event_id == constants::LEVEL_EVENT_SOUND_TELEPORT)));
}

#[test]
fn teleports_toward_a_distant_target_close_in() {
    let mut h = harness(6);
    let player = h.player;
    h.world().get_mut::<Position>(player).unwrap().0 =
        DVec3::new(40.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 0.5);
    let mob = h.enderman(DVec3::new(0.5, 64.0, 0.5));
    let mut landed = 0;
    for _ in 0..50 {
        h.world().get_mut::<Position>(mob).unwrap().0 = DVec3::new(0.5, 64.0, 0.5);
        let world = h.world();
        if teleport_toward(world, mob, player) {
            landed += 1;
            let feet = world.get::<Position>(mob).unwrap().0;
            // 16 blocks toward the target, give or take 4 sideways.
            assert!((feet.x - 16.5).abs() <= 4.5, "{feet:?}");
            assert!(feet.z.abs() <= 4.5, "{feet:?}");
        }
    }
    assert!(landed > 10, "{landed} of 50 teleports landed");
}

#[test]
fn enderman_teleports_out_of_water() {
    let mut h = harness(7);
    for x in 6..=10 {
        for z in 6..=10 {
            for y in 64..=67 {
                h.set_block(IVec3::new(x, y, z), "minecraft:water");
            }
        }
    }
    let mob = h.enderman(DVec3::new(8.5, 64.0, 8.5));
    let start = h.feet(mob);
    let mut escaped = false;
    for _ in 0..40 {
        h.server.tick();
        let world = h.world();
        let feet = world.get::<Position>(mob).unwrap().0;
        let body = *world.get::<PhysicsBody>(mob).unwrap();
        if feet != start
            && living_liquid(world, Aabb::from_feet(feet, body.width, body.height)).is_none()
        {
            escaped = true;
            break;
        }
    }
    assert!(escaped, "the enderman teleported out of the water");
}

/// A two-high ring of dirt around (8, 64, 8), two blocks out.
fn dirt_ring(h: &mut Harness) -> Vec<IVec3> {
    let mut ring = Vec::new();
    for x in 6..=10 {
        for z in 6..=10 {
            if x == 6 || x == 10 || z == 6 || z == 10 {
                for y in 64..=65 {
                    let pos = IVec3::new(x, y, z);
                    h.set_block(pos, "minecraft:dirt");
                    ring.push(pos);
                }
            }
        }
    }
    ring
}

#[test]
fn enderman_takes_and_places_blocks_with_mob_griefing() {
    let mut h = harness(8);
    let ring = dirt_ring(&mut h);
    let mob = h.pinned_enderman(DVec3::new(8.5, 64.0, 8.5));
    let took = (0..4000).any(|_| {
        h.server.tick();
        mainhand(h.world(), mob).is_some()
    });
    assert!(took, "the enderman took a block");
    assert_eq!(
        mainhand(h.world(), mob).unwrap().item_id(),
        "minecraft:dirt"
    );
    let removed = ring
        .iter()
        .filter(|&&pos| h.block_name(pos) == "minecraft:air")
        .count();
    assert_eq!(removed, 1, "one ring block was taken");
    let placed = (0..60_000).any(|_| {
        h.server.tick();
        mainhand(h.world(), mob).is_none()
    });
    assert!(placed, "the enderman placed its block");
}

#[test]
fn enderman_leaves_blocks_alone_without_mob_griefing() {
    let mut h = harness(8);
    h.world().resource_mut::<GameRules>().mob_griefing = false;
    let ring = dirt_ring(&mut h);
    let mob = h.pinned_enderman(DVec3::new(8.5, 64.0, 8.5));
    for _ in 0..4000 {
        h.server.tick();
    }
    assert!(
        ring.iter()
            .all(|&pos| h.block_name(pos) == "minecraft:dirt")
    );
    assert!(mainhand(h.world(), mob).is_none(), "nothing taken");
    let world = h.world();
    let stack = dirt(world);
    world
        .entity_mut(mob)
        .entry::<MobEquipment>()
        .or_default()
        .get_mut()
        .set(GearSlot::Mainhand, stack, true);
    for _ in 0..4000 {
        h.server.tick();
    }
    assert_eq!(
        mainhand(h.world(), mob).map(|stack| stack.item_id().to_owned()),
        Some("minecraft:dirt".to_owned()),
        "the carried block stays in hand"
    );
}

fn dirt(world: &World) -> crate::item::ItemStack {
    world_stack(world, "minecraft:dirt", 1).unwrap()
}

#[test]
fn enderman_drops_its_carried_block_on_death() {
    let mut h = harness(9);
    let mob = h.pinned_enderman(DVec3::new(8.5, 64.0, 8.5));
    let world = h.world();
    let stack = dirt(world);
    world
        .entity_mut(mob)
        .entry::<MobEquipment>()
        .or_default()
        .get_mut()
        .set(GearSlot::Mainhand, stack, true);
    h.kill(mob, false);
    let drops = h.take_drops().0;
    assert!(
        drops
            .iter()
            .any(|(item, count)| item == "minecraft:dirt" && *count == 1),
        "{drops:?}"
    );
}

#[test]
fn enderman_drops_up_to_one_ender_pearl_more_with_looting_and_experience() {
    let mut h = harness(10);
    let (mut pearls, mut most) = (0, 0);
    for by_player in [true, false] {
        for _ in 0..100 {
            let mob = h.pinned_enderman(DVec3::new(0.5, 64.0, 0.5));
            h.kill(mob, by_player);
            let mut kill = 0;
            for (item, count) in h.take_drops().0 {
                assert_eq!(item, "minecraft:ender_pearl");
                kill += count;
            }
            pearls += kill;
            most = most.max(kill);
            h.world().despawn(mob);
        }
    }
    assert!(pearls > 0);
    assert_eq!(most, 1, "without looting a kill drops at most one pearl");

    // Looting III adds up to three.
    let player = h.player;
    let world = h.world();
    let sword = world_stack(world, "minecraft:diamond_sword", 1).unwrap();
    let sword = with_enchantments(&sword, &[(Enchantment::Looting, 3)]);
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, sword)
        .unwrap();
    let mut most = 0;
    for _ in 0..100 {
        let mob = h.pinned_enderman(DVec3::new(0.5, 64.0, 0.5));
        h.kill(mob, true);
        let kill: u32 = h.take_drops().0.iter().map(|(_, count)| count).sum();
        most = most.max(kill);
        h.world().despawn(mob);
    }
    assert!(
        (2..=4).contains(&most),
        "looting III: at most {most} pearls"
    );

    // Experience: 5 for a player kill, none otherwise.
    for by_player in [true, false] {
        let mob = h.pinned_enderman(DVec3::new(0.5, 64.0, 0.5));
        h.kill(mob, by_player);
        for _ in 0..40 {
            if h.world().get_entity(mob).is_err() {
                break;
            }
            h.server.tick();
        }
        assert_eq!(
            h.take_drops().1,
            if by_player { 5 } else { 0 },
            "by_player = {by_player}"
        );
    }
}

#[test]
fn projectiles_make_the_enderman_teleport_without_hurting_it() {
    let mut h = harness(11);
    let mob = h.enderman(DVec3::new(8.5, 64.0, 8.5));
    let before = h.feet(mob);
    let world = h.world();
    let dealt = combat::hurt_mob(
        world,
        mob,
        combat::Hit {
            damage: 3.0,
            cause: ActorDamageCause::Projectile,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
    assert_eq!(dealt, 0.0);
    assert_eq!(world.get::<Health>(mob).unwrap().current, 40.0);
    assert_ne!(world.get::<Position>(mob).unwrap().0, before);
}
