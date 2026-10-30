//! The vanilla zombie through a real `GameServer`: native AddActor
//! equality, targeting and melee hits on players, daylight burning, baby
//! size and speed, and loot. Door breaking is in `tests::doors`.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::EnumsActorEvent;
use jolyne::valentine::{AddActorPacket, McpePacket, McpePacketData};
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;
use valentine::bedrock::codec::BedrockCodec;

use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, Experience, GameMode, Health, HeldSlot, ItemEntityStack, MainInventory,
    PhysicsBody, Player, PlayerSession, Position, Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::components::MovementConfig;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::combat::fire::{self, Burning};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::mob::*;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

/// Noon and midnight on the world clock.
const NOON: u64 = 6_000;
const MIDNIGHT: u64 = 18_000;

struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at `time`, viewed by
/// one player in `mode` standing on it at (8.5, 64, 0.5).
fn harness(seed: u64, difficulty: Difficulty, time: u64, mode: GameMode) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = difficulty;
    crate::world::clock::set_time(world, time);
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
            mode,
            MainInventory::default(),
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

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(mob).unwrap().get(index).unwrap()
}

fn native(bytes: &[u8]) -> AddActorPacket {
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let packet = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    packet
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// A zombie at `feet` that is a baby exactly when `baby` (the spawn
    /// event makes babies 5 % of the time).
    fn zombie(&mut self, feet: DVec3, baby: bool) -> Entity {
        let world = self.world();
        for _ in 0..2000 {
            let mob = spawn_mob(world, "minecraft:zombie", feet, 0.0, SpawnCause::Spawned).unwrap();
            apply_pending_definitions(world);
            if flag(world, mob, actor_flags::BABY) == baby {
                return mob;
            }
            world.despawn(mob);
        }
        panic!("no zombie with baby = {baby} in 2000 spawns");
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

    fn ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.server.tick();
        }
    }

    fn health(&mut self) -> f32 {
        let player = self.player;
        self.world().get::<Health>(player).unwrap().current
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

    /// Every dropped item stack, removing them from the world.
    fn take_drops(&mut self) -> Vec<(String, u32)> {
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
        for (entity, _, _) in &drops {
            world.despawn(*entity);
        }
        drops.into_iter().map(|(_, id, n)| (id, n)).collect()
    }
}

#[test]
fn vanilla_zombie_compiles_to_a_spawnable_archetype() {
    let mut h = harness(1, Difficulty::Easy, MIDNIGHT, GameMode::Survival);
    let registry = h.world().resource::<MobRegistry>();
    assert!(
        registry.get("minecraft:zombie").is_some(),
        "zombie unsupported: {:?}",
        registry.unsupported_reason("minecraft:zombie")
    );
}

#[test]
fn spawned_baby_zombie_add_actor_matches_native_summon() {
    let mut h = harness(2, Difficulty::Easy, MIDNIGHT, GameMode::Survival);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/zombie.bin"
    ));
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let zombie = h.zombie(feet, true);
    let world = h.world();
    let ours = add_actor_packet(
        "minecraft:zombie",
        world.get::<RuntimeId>(zombie).unwrap().0,
        world.get::<Position>(zombie).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        world.get::<Rotation>(zombie).unwrap(),
        world.get::<EntityMetadata>(zombie).unwrap(),
        world.get::<ActorProperties>(zombie).unwrap(),
        world.get::<ActorAttributes>(zombie).unwrap(),
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
fn baby_zombies_are_faster_and_smaller() {
    let mut h = harness(3, Difficulty::Easy, MIDNIGHT, GameMode::Survival);
    let adult = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
    let baby = h.zombie(DVec3::new(-3.5, 64.0, 0.5), true);
    let world = h.world();
    let speed = |mob| world.get::<MovementConfig>(mob).unwrap().value;
    assert_eq!(speed(adult), 0.23);
    assert_eq!(speed(baby), 0.35);
    let (a, b) = (
        world.get::<PhysicsBody>(adult).unwrap(),
        world.get::<PhysicsBody>(baby).unwrap(),
    );
    // Boxes are authored as f32.
    assert_eq!((a.width as f32, a.height as f32), (0.6, 1.9));
    assert_eq!((b.width as f32, b.height as f32), (0.49, 0.98));
}

/// The ticks on which the player lost health, and how much, over `ticks`.
fn hits_on_player(h: &mut Harness, ticks: u64) -> Vec<(u64, f32)> {
    let mut hits = Vec::new();
    let mut health = h.health();
    for tick in 0..ticks {
        h.server.tick();
        let now = h.health();
        if now < health {
            hits.push((tick, health - now));
        }
        health = now;
        if hits.len() == 3 {
            break;
        }
    }
    hits
}

#[test]
fn zombie_targets_a_survival_player_and_hits_on_the_native_cooldown() {
    for (difficulty, damage) in [(Difficulty::Normal, 3.0), (Difficulty::Hard, 4.5)] {
        let mut h = harness(4, difficulty, MIDNIGHT, GameMode::Survival);
        let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
        let player = h.player;
        let hits = hits_on_player(&mut h, 600);
        assert_eq!(
            h.world().get::<AttackTarget>(zombie),
            Some(&AttackTarget(player))
        );
        assert_eq!(hits.len(), 3, "{difficulty:?}: hits {hits:?}");
        for (_, dealt) in &hits {
            assert_eq!(*dealt, damage, "{difficulty:?}: hits {hits:?}");
        }
        // The player stands still, so the zombie stays in reach and hits
        // every time the 1 s cooldown runs out.
        assert_eq!(hits[1].0 - hits[0].0, 20, "{difficulty:?}: hits {hits:?}");
        assert_eq!(hits[2].0 - hits[1].0, 20, "{difficulty:?}: hits {hits:?}");
        let runtime_id = h.world().get::<RuntimeId>(zombie).unwrap().0 as u64;
        let swings = h
            .packets()
            .into_iter()
            .filter(|packet| {
                matches!(packet, McpePacketData::ActorEventPacket(event)
                    if event.event_id == EnumsActorEvent::StartAttacking
                        && event.target_runtime_id.actor_runtime_id == runtime_id)
            })
            .count();
        assert_eq!(swings, 3, "one swing per hit with an empty hand");
    }
}

#[test]
fn zombie_ignores_creative_players() {
    let mut h = harness(5, Difficulty::Hard, MIDNIGHT, GameMode::Creative);
    let zombie = h.zombie(DVec3::new(4.5, 64.0, 0.5), false);
    h.ticks(300);
    assert!(h.world().get::<AttackTarget>(zombie).is_none());
    assert_eq!(h.health(), 20.0);
}

/// Tick until the daylight check has run a few times and report whether
/// `zombie` caught fire.
fn burns(h: &mut Harness, zombie: Entity) -> bool {
    h.ticks(6);
    let world = h.world();
    let burning = world.get::<Burning>(zombie).is_some();
    assert_eq!(burning, flag(world, zombie, actor_flags::ONFIRE));
    burning
}

#[test]
fn zombie_burns_in_daylight_and_takes_fire_damage() {
    let mut h = harness(6, Difficulty::Easy, NOON, GameMode::Creative);
    let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
    assert!(burns(&mut h, zombie));
    let max = h.world().get::<Health>(zombie).unwrap().max;
    h.ticks(41);
    let health = h.world().get::<Health>(zombie).unwrap().current;
    assert!(health < max, "fire damage through the hurt rules");
}

#[test]
fn zombie_does_not_burn_at_night_in_shade_in_water_or_with_a_helmet() {
    let mut h = harness(7, Difficulty::Easy, MIDNIGHT, GameMode::Creative);
    let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
    assert!(!burns(&mut h, zombie), "night");

    let mut h = harness(7, Difficulty::Easy, NOON, GameMode::Creative);
    for x in -3..=3 {
        for z in -3..=3 {
            h.set_block(IVec3::new(x, 67, z), "minecraft:stone");
        }
    }
    let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
    assert!(!burns(&mut h, zombie), "shade");

    let mut h = harness(7, Difficulty::Easy, NOON, GameMode::Creative);
    for x in -2..=2 {
        for z in -2..=2 {
            for y in 64..=65 {
                h.set_block(IVec3::new(x, y, z), "minecraft:water");
            }
        }
    }
    let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
    assert!(!burns(&mut h, zombie), "water");

    let mut h = harness(7, Difficulty::Easy, NOON, GameMode::Creative);
    let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
    let helmet = world_stack(h.world(), "minecraft:iron_helmet", 1).unwrap();
    h.world()
        .entity_mut(zombie)
        .entry::<MobEquipment>()
        .or_default()
        .get_mut()
        .set(GearSlot::Head, helmet, false);
    assert!(!burns(&mut h, zombie), "helmet");
}

#[test]
fn burning_zombie_spreads_fire_to_the_player_it_hits_on_hard() {
    let mut h = harness(8, Difficulty::Hard, MIDNIGHT, GameMode::Survival);
    let zombie = h.zombie(DVec3::new(6.5, 64.0, 0.5), false);
    fire::ignite(h.world(), zombie, 10_000);
    let player = h.player;
    let mut burned = false;
    for _ in 0..400 {
        h.server.tick();
        if h.world()
            .get::<crate::entity::player_life::environment::Burning>(player)
            .is_some()
        {
            burned = true;
            break;
        }
    }
    assert!(
        burned,
        "1.92 > U(0, 1): every hit on Hard sets the player alight"
    );
}

const RARE_DROPS: [&str; 3] = [
    "minecraft:iron_ingot",
    "minecraft:carrot",
    "minecraft:potato",
];

#[test]
fn zombie_drops_rotten_flesh_and_rare_drops_only_when_killed_by_a_player() {
    let mut h = harness(9, Difficulty::Easy, MIDNIGHT, GameMode::Survival);
    for by_player in [true, false] {
        let (mut flesh, mut rare) = (0, 0);
        for _ in 0..300 {
            let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
            h.kill(zombie, by_player);
            for (item, count) in h.take_drops() {
                if item == "minecraft:rotten_flesh" {
                    flesh += count;
                } else if RARE_DROPS.contains(&item.as_str()) {
                    rare += count;
                }
            }
            h.world().despawn(zombie);
        }
        assert!(flesh > 0, "by_player = {by_player}");
        if by_player {
            assert!(rare > 0, "2.5 % per player kill over 300 kills");
        } else {
            assert_eq!(rare, 0, "rare drops need a player kill");
        }
    }
}

// ----- equipment -----

mod equipment {
    use super::*;
    use crate::entity::mob::gear::{CanPickUpLoot, PendingEquip, equipment_count};
    use crate::entity::mob::state::Persistent;
    use crate::entity::spawn_item_drop;
    use crate::item::enchant::{
        Enchantment, durability_damage, enchantment_level, with_durability_damage,
        with_enchantments,
    };

    impl Harness {
        fn stack(&mut self, item: &str) -> crate::item::ItemStack {
            world_stack(self.world(), item, 1).expect("vanilla item")
        }

        fn equip(&mut self, mob: Entity, slot: GearSlot, item: &str, picked_up: bool) {
            let stack = self.stack(item);
            self.world()
                .entity_mut(mob)
                .entry::<MobEquipment>()
                .or_default()
                .get_mut()
                .set(slot, stack, picked_up);
        }

        fn gear(&mut self, mob: Entity) -> MobEquipment {
            self.world().get::<MobEquipment>(mob).unwrap().clone()
        }
    }

    fn count(drops: &[(String, u32)], id: &str) -> u32 {
        drops
            .iter()
            .filter(|(item, _)| item == id)
            .map(|(_, n)| n)
            .sum()
    }

    /// The weapons the zombie gear table rolls.
    const HAND_GEAR: [&str; 3] = [
        "minecraft:iron_sword",
        "minecraft:iron_shovel",
        "minecraft:iron_spear",
    ];

    #[test]
    fn hard_zombies_spawn_with_gear_their_viewers_see() {
        let mut h = harness(20, Difficulty::Hard, MIDNIGHT, GameMode::Creative);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let mut equipped = None;
        for _ in 0..300 {
            let world = h.world();
            let mob = spawn_mob(world, "minecraft:zombie", feet, 0.0, SpawnCause::Spawned).unwrap();
            apply_pending_definitions(world);
            if equipment_count(world, mob) > 0 {
                equipped = Some(mob);
                break;
            }
            world.despawn(mob);
        }
        let zombie = equipped.expect("a hard zombie rolls gear 5 % + 15 % of the time");
        let gear = h.gear(zombie);
        for slot in GearSlot::ALL {
            let stack = gear.get(slot);
            if !stack.is_empty() {
                let ok = if slot.is_hand() {
                    HAND_GEAR.contains(&stack.item_id())
                } else {
                    stack.item().and_then(crate::entity::mob::gear::worn_slot) == Some(slot)
                };
                assert!(ok, "{slot:?} holds {}", stack.item_id());
            }
        }
        h.packets();
        h.ticks(1);
        let runtime_id = h.world().get::<RuntimeId>(zombie).unwrap().0 as u64;
        let packets = h.packets();
        let added = packets
            .iter()
            .position(|p| match p {
                McpePacketData::AddActorPacket(a) => {
                    a.target_runtime_id.actor_runtime_id == runtime_id
                }
                _ => false,
            })
            .expect("the viewer gets the zombie");
        let hands: Vec<_> = packets
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match p {
                McpePacketData::MobEquipmentPacket(e)
                    if e.target_runtime_id.actor_runtime_id == runtime_id =>
                {
                    Some((i, e.slot, e.container_id, e.item.id))
                }
                _ => None,
            })
            .collect();
        assert_eq!(hands.len(), 2, "{hands:?}");
        assert!(hands.iter().all(|(i, ..)| *i > added));
        assert_eq!((hands[0].1, hands[0].2), (0, 0));
        assert_eq!((hands[1].1, hands[1].2), (1, 119));
        let main = gear.get(GearSlot::Mainhand);
        let main_id = if main.is_empty() {
            0
        } else {
            main.item().unwrap().network_id() as i16
        };
        assert_eq!(hands[0].3, main_id);
        let armour = packets
            .iter()
            .position(|p| match p {
                McpePacketData::MobArmorEquipmentPacket(a) => {
                    a.target_runtime_id.actor_runtime_id == runtime_id
                }
                _ => false,
            })
            .expect("the viewer gets the armour");
        assert!(armour > hands[1].0);
    }

    #[test]
    fn easy_zombies_never_spawn_with_gear_or_pick_items_up() {
        let mut h = harness(21, Difficulty::Easy, MIDNIGHT, GameMode::Creative);
        let world = h.world();
        for _ in 0..300 {
            let mob = spawn_mob(
                world,
                "minecraft:zombie",
                DVec3::new(0.5, 64.0, 0.5),
                0.0,
                SpawnCause::Spawned,
            )
            .unwrap();
            apply_pending_definitions(world);
            assert_eq!(equipment_count(world, mob), 0);
            assert_eq!(world.get::<CanPickUpLoot>(mob), Some(&CanPickUpLoot(false)));
            world.despawn(mob);
        }
    }

    #[test]
    fn zombie_gear_drops_on_a_player_kill_worn_down() {
        let mut h = harness(22, Difficulty::Easy, MIDNIGHT, GameMode::Survival);
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let max = crate::item::test_support::VANILLA
            .get_by_name("minecraft:iron_helmet")
            .unwrap()
            .tool_info()
            .max_damage;
        for by_player in [true, false] {
            let mut helmets = Vec::new();
            let mut swords = 0;
            for _ in 0..200 {
                let zombie = h.zombie(feet, false);
                h.equip(zombie, GearSlot::Head, "minecraft:iron_helmet", false);
                h.equip(zombie, GearSlot::Mainhand, "minecraft:stone_sword", true);
                h.take_drops();
                h.kill(zombie, by_player);
                let world = h.world();
                let drops: Vec<crate::item::ItemStack> = world
                    .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
                    .iter(world)
                    .map(|stack| stack.0.clone())
                    .collect();
                helmets.extend(
                    drops
                        .iter()
                        .filter(|s| s.item_id() == "minecraft:iron_helmet")
                        .map(durability_damage),
                );
                swords += drops
                    .iter()
                    .filter(|s| s.item_id() == "minecraft:stone_sword")
                    .count();
                h.take_drops();
                h.world().despawn(zombie);
            }
            assert_eq!(swords, 200, "picked-up items always drop");
            if by_player {
                // 25 % of 200.
                assert!((25..=80).contains(&helmets.len()), "{}", helmets.len());
                assert!(
                    helmets.iter().all(|wear| (4..=max).contains(wear)),
                    "{helmets:?}"
                );
            } else {
                assert!(helmets.is_empty(), "rolled gear needs a player kill");
            }
        }
        // A guaranteed slot drops as it is, without a player.
        let zombie = h.zombie(feet, false);
        h.equip(zombie, GearSlot::Chest, "minecraft:iron_chestplate", false);
        h.world()
            .get_mut::<MobEquipment>(zombie)
            .unwrap()
            .set_drop_chance(GearSlot::Chest, Some(1.0));
        h.kill(zombie, false);
        let drops = h.take_drops();
        assert_eq!(count(&drops, "minecraft:iron_chestplate"), 1, "{drops:?}");
    }

    #[test]
    fn zombie_picks_up_a_better_sword_and_drops_its_old_one() {
        let mut h = harness(23, Difficulty::Easy, MIDNIGHT, GameMode::Creative);
        let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
        h.world().entity_mut(zombie).insert(CanPickUpLoot(true));
        h.equip(zombie, GearSlot::Mainhand, "minecraft:wooden_sword", false);
        let feet = h.world().get::<Position>(zombie).unwrap().0;
        let sword = h.stack("minecraft:iron_sword");
        let item =
            spawn_item_drop(h.world(), feet + DVec3::new(0.8, 0.0, 0.0), sword, None).unwrap();
        h.ticks(1);
        let item_rid = h.world().get::<RuntimeId>(item).unwrap().0 as u64;
        let zombie_rid = h.world().get::<RuntimeId>(zombie).unwrap().0 as u64;
        h.packets();
        let mut packets = Vec::new();
        for _ in 0..200 {
            h.ticks(1);
            packets.extend(h.packets());
            if h.gear(zombie).get(GearSlot::Mainhand).item_id() == "minecraft:iron_sword" {
                break;
            }
        }
        let gear = h.gear(zombie);
        assert_eq!(
            gear.get(GearSlot::Mainhand).item_id(),
            "minecraft:iron_sword"
        );
        assert!(gear.item(GearSlot::Mainhand).picked_up);
        let world = h.world();
        assert!(world.get_entity(item).is_err(), "the item actor is gone");
        assert!(world.get::<PendingEquip>(zombie).is_none());
        assert!(world.get::<Persistent>(zombie).is_some());
        let took = packets.iter().any(|p| match p {
            McpePacketData::TakeItemActorPacket(t) => {
                t.item_runtime_id.actor_runtime_id == item_rid
                    && t.actor_runtime_id.actor_runtime_id == zombie_rid
            }
            _ => false,
        });
        assert!(took, "TakeItemActor (item, zombie)");
        h.ticks(1);
        packets.extend(h.packets());
        let shown = packets.iter().any(|p| match p {
            McpePacketData::MobEquipmentPacket(e) => {
                e.target_runtime_id.actor_runtime_id == zombie_rid
                    && (e.slot, e.container_id) == (0, 0)
                    && e.item.id != 0
            }
            _ => false,
        });
        assert!(shown, "the main hand change reaches the viewer");
        let drops = h.take_drops();
        assert_eq!(count(&drops, "minecraft:wooden_sword"), 1, "{drops:?}");
    }

    #[test]
    fn zombie_gear_survives_a_save_without_rerolling() {
        let mut h = harness(24, Difficulty::Hard, MIDNIGHT, GameMode::Creative);
        let zombie = h.zombie(DVec3::new(0.5, 64.0, 0.5), false);
        let world = h.world();
        world
            .entity_mut(zombie)
            .insert((MobEquipment::default(), CanPickUpLoot(true)));
        h.equip(zombie, GearSlot::Mainhand, "minecraft:iron_sword", true);
        let helmet = h.stack("minecraft:diamond_helmet");
        let helmet = with_durability_damage(
            &with_enchantments(&helmet, &[(Enchantment::Unbreaking, 2)]),
            37,
        );
        let world = h.world();
        let mut gear = world.get_mut::<MobEquipment>(zombie).unwrap();
        gear.set(GearSlot::Head, helmet, false);
        gear.set_drop_chance(GearSlot::Head, Some(0.5));
        let nbt = persistence::encode_for_tests(world, zombie);
        world.despawn(zombie);
        for _ in 0..30 {
            let world = h.world();
            let restored = persistence::restore_mob(world, &nbt).unwrap();
            assert_eq!(
                world.get::<CanPickUpLoot>(restored),
                Some(&CanPickUpLoot(true))
            );
            let gear = world.get::<MobEquipment>(restored).unwrap();
            assert_eq!(gear.count(), 2, "restoring rolls no new gear");
            let sword = gear.item(GearSlot::Mainhand);
            assert_eq!(sword.stack.item_id(), "minecraft:iron_sword");
            assert!(sword.picked_up);
            let head = gear.item(GearSlot::Head);
            assert_eq!(head.stack.item_id(), "minecraft:diamond_helmet");
            assert!(!head.picked_up);
            assert_eq!(durability_damage(&head.stack), 37);
            assert_eq!(enchantment_level(&head.stack, Enchantment::Unbreaking), 2);
            assert_eq!(gear.drop_chance(GearSlot::Head), Some(0.5));
            assert_eq!(gear.drop_chance(GearSlot::Mainhand), None);
            world.despawn(restored);
        }
    }
}
