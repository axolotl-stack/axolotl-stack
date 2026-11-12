//! Shared behaviour, checked mob by mob. Each test runs one behaviour check
//! for every spawnable vanilla mob in its list, so a parity sub-row
//! `<mob>/<component>` names a test that exercises that mob, not a cow or a
//! zombie standing in for it. A mob that becomes spawnable joins the lists
//! of the components it uses.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::EnumsActorEvent;
use jolyne::valentine::{McpePacket, McpePacketData};
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, Experience, GameMode, Health, HeldSlot, ItemEntityStack, MainInventory, OnGround,
    PhysicsBody, Player, PlayerSession, Position, Rotation, RuntimeEntityId, RuntimeId, Velocity,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::families::ActorFamilies;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::components::{RandomLookAroundGoal, RandomStrollGoal};
use crate::entity::mob::gear::{CanPickUpLoot, GearSlot, MobEquipment, equipment_count};
use crate::entity::mob::state::{AttackTarget, Dead, Persistent};
use crate::entity::mob::*;
use crate::entity::systems::physics::collides_with_blocks;
use crate::entity::xp_orb::XpOrb;
use crate::registry::BlockRegistry;
use crate::registry::collision::Aabb;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::Chunk;
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

/// Top of the floor: feet of a mob standing on it.
pub(super) const SURFACE: f64 = 64.0;
const MIDNIGHT: u64 = 18_000;

/// The mobs these tests cover, in the order the lists below use.
const HUSK: &str = "minecraft:husk";
const WITHER_SKELETON: &str = "minecraft:wither_skeleton";
const ENDERMITE: &str = "minecraft:endermite";
const ZOMBIE_PIGMAN: &str = "minecraft:zombie_pigman";
const ZOGLIN: &str = "minecraft:zoglin";
const SKELETON: &str = "minecraft:skeleton";
const STRAY: &str = "minecraft:stray";
const BOGGED: &str = "minecraft:bogged";
const PARCHED: &str = "minecraft:parched";
/// `/summon minecraft:zombie_villager` creates this actor (see
/// `hostile::summoned_zombie_villager_is_a_v2_whose_add_actor_matches_native_summon`).
const ZOMBIE_VILLAGER: &str = "minecraft:zombie_villager_v2";
const SILVERFISH: &str = "minecraft:silverfish";
const SPIDER: &str = "minecraft:spider";
const CAVE_SPIDER: &str = "minecraft:cave_spider";

pub(super) fn block(name: &str) -> u32 {
    BlockRegistry::vanilla()
        .default_state_id_by_name(name)
        .expect(name)
}

pub(super) struct Arena {
    pub(super) server: GameServer,
    pub(super) player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor whose top is y = 64 over chunks -1..=1 × -1..=1, plus the
/// blocks `build` places (y 50..80), at midnight. One player in `mode`
/// stands at (12.5, 64, 12.5) and views every chunk.
pub(super) fn arena(
    seed: u64,
    difficulty: Difficulty,
    mode: GameMode,
    build: impl Fn(IVec3) -> Option<u32>,
) -> Arena {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = difficulty;
    crate::world::clock::set_time(world, MIDNIGHT);
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                12.5,
                SURFACE + crate::entity::PLAYER_EYE_HEIGHT,
                12.5,
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
    let grass = block("minecraft:grass_block");
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for lx in 0u8..16 {
                for lz in 0u8..16 {
                    column.set_block(lx, 63, lz, grass);
                    for y in 50i16..80 {
                        let pos = IVec3::new(
                            cx * 16 + i32::from(lx),
                            i32::from(y),
                            cz * 16 + i32::from(lz),
                        );
                        if let Some(state) = build(pos) {
                            column.set_block(lx, y, lz, state);
                        }
                    }
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
    Arena { server, player, rx }
}

/// [`arena`] on the plain floor.
pub(super) fn floor(seed: u64, difficulty: Difficulty, mode: GameMode) -> Arena {
    arena(seed, difficulty, mode, |_| None)
}

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(mob).unwrap().get(index).unwrap()
}

impl Arena {
    pub(super) fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// A mob of `identifier` with its feet at `feet`, a baby exactly when
    /// `baby`, that drew a spawn branch with component groups (the zombie
    /// pigman can draw an undeclared one and spawn without any).
    pub(super) fn spawn(&mut self, identifier: &str, feet: DVec3, baby: bool) -> Entity {
        let world = self.world();
        for _ in 0..2000 {
            let mob = spawn_mob(world, identifier, feet, 0.0, SpawnCause::Spawned)
                .unwrap_or_else(|error| panic!("{identifier}: {error}"));
            apply_pending_definitions(world);
            if flag(world, mob, actor_flags::BABY) == baby
                && world.get::<ActorFamilies>(mob).is_some()
            {
                return mob;
            }
            world.despawn(mob);
        }
        panic!("no {identifier} with baby = {baby} in 2000 spawns");
    }

    pub(super) fn adult(&mut self, identifier: &str, feet: DVec3) -> Entity {
        self.spawn(identifier, feet, false)
    }

    pub(super) fn feet(&mut self, mob: Entity) -> DVec3 {
        self.world().get::<Position>(mob).unwrap().0
    }

    pub(super) fn ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.server.tick();
        }
    }

    pub(super) fn set_block(&mut self, pos: IVec3, name: &str) {
        let state = block(name);
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

    pub(super) fn move_player(&mut self, feet: DVec3) {
        let player = self.player;
        self.world().get_mut::<Position>(player).unwrap().0 =
            feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    }

    /// Drop the goals that make an isolated behaviour random.
    fn without<T: Component>(&mut self, mob: Entity) {
        self.world().entity_mut(mob).remove::<T>();
    }

    pub(super) fn packets(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect()
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

    /// Every experience orb's value, removing the orbs from the world.
    fn take_orbs(&mut self) -> Vec<u32> {
        let world = self.world();
        let orbs: Vec<(Entity, u32)> = world
            .query::<(Entity, &XpOrb)>()
            .iter(world)
            .map(|(entity, orb)| (entity, orb.value))
            .collect();
        for (entity, _) in &orbs {
            world.despawn(*entity);
        }
        orbs.into_iter().map(|(_, value)| value).collect()
    }

    /// The player, two blocks west of `mob`, punches it with one health left.
    fn kill_by_player(&mut self, mob: Entity) {
        let player = self.player;
        let world = self.world();
        world.get_mut::<Health>(mob).unwrap().current = 1.0;
        let feet = world.get::<Position>(mob).unwrap().0;
        world.get_mut::<Position>(player).unwrap().0 =
            feet + DVec3::new(-2.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
        combat::player_attack(world, player, mob, &Default::default());
        assert!(world.get::<Dead>(mob).is_some());
    }
}

fn inside_blocks(world: &World, mob: Entity) -> bool {
    let feet = world.get::<Position>(mob).unwrap().0;
    let body = world.get::<PhysicsBody>(mob).unwrap();
    collides_with_blocks(world, Aabb::from_feet(feet, body.width, body.height))
}

fn horizontal(a: DVec3, b: DVec3) -> f64 {
    DVec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

/// `physics`: dropped from six blocks up, each mob lands on the floor and
/// stays on it.
#[test]
fn listed_mobs_fall_and_stay_on_the_ground() {
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        ENDERMITE,
        ZOMBIE_PIGMAN,
        ZOGLIN,
        ZOMBIE_VILLAGER,
        SILVERFISH,
        SPIDER,
        CAVE_SPIDER,
    ] {
        let mut a = floor(1, Difficulty::Normal, GameMode::Creative);
        let mob = a.adult(identifier, DVec3::new(0.5, 70.0, 0.5));
        a.without::<RandomStrollGoal>(mob);
        a.ticks(60);
        let feet = a.feet(mob);
        assert!((feet.y - SURFACE).abs() < 1e-9, "{identifier} at {feet}");
        assert!(a.world().get::<OnGround>(mob).unwrap().0, "{identifier}");
        for _ in 0..60 {
            a.ticks(1);
            let feet = a.feet(mob);
            assert!(
                (feet.y - SURFACE).abs() < 1e-9,
                "{identifier} left the ground: {feet}"
            );
        }
    }
}

/// `movement.basic`, `navigation.walk`, `behavior.random_stroll`: each mob
/// strolls more than once, each stroll ends within the goal's range of its
/// start, and the mob never enters a block of the pillars around it.
#[test]
fn listed_mobs_stroll_within_their_range_around_obstacles() {
    let stone = block("minecraft:stone");
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        ENDERMITE,
        ZOMBIE_PIGMAN,
        ZOGLIN,
        ZOMBIE_VILLAGER,
    ] {
        let mut a = arena(3, Difficulty::Normal, GameMode::Creative, move |pos| {
            let pillar = matches!((pos.x, pos.z), (3, 1) | (-2, 4) | (1, -3));
            (pillar && (64..=66).contains(&pos.y)).then_some(stone)
        });
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.move_player(DVec3::new(-14.5, SURFACE, -14.5));
        let range = f64::from(a.world().get::<RandomStrollGoal>(mob).unwrap().xz_dist);
        let mut start = None;
        let mut strolls = 0;
        for _ in 0..2400 {
            a.ticks(1);
            let world = a.world();
            assert!(!inside_blocks(world, mob), "{identifier} inside a block");
            let feet = world.get::<Position>(mob).unwrap().0;
            let navigating = world.get::<MobBrain>(mob).unwrap().is_navigating();
            match (start, navigating) {
                (None, true) => start = Some(feet),
                (Some(from), false) => {
                    let travelled = (feet - from).abs();
                    assert!(
                        travelled.x <= range + 1.0 && travelled.z <= range + 1.0,
                        "{identifier}: stroll from {from} ended at {feet}"
                    );
                    strolls += 1;
                    start = None;
                }
                _ => {}
            }
            if strolls >= 2 {
                break;
            }
        }
        assert!(strolls >= 2, "{identifier}: only {strolls} strolls");
    }
}

/// `jump.static`: in a one-block-deep pit three blocks wide, each mob
/// jumps out to reach a stroll target on the raised ground around it.
#[test]
fn listed_mobs_jump_a_block_to_reach_a_stroll_target() {
    let grass = block("minecraft:grass_block");
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        ENDERMITE,
        ZOMBIE_PIGMAN,
        ZOMBIE_VILLAGER,
    ] {
        let mut a = arena(5, Difficulty::Normal, GameMode::Creative, move |pos| {
            let pit = (-1..=1).contains(&pos.x) && (-1..=1).contains(&pos.z);
            (pos.y == 64 && !pit).then_some(grass)
        });
        a.move_player(DVec3::new(-14.5, SURFACE + 1.0, -14.5));
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        let mut climbed = false;
        for _ in 0..2400 {
            a.ticks(1);
            assert!(!inside_blocks(a.world(), mob), "{identifier}");
            if a.feet(mob).y >= SURFACE + 1.0 - 1e-9 {
                climbed = true;
                break;
            }
        }
        assert!(climbed, "{identifier} still in the pit at {}", a.feet(mob));
    }
}

/// `pushable_by_entity`: two overlapping mobs of the same kind push each
/// other until their boxes no longer overlap.
#[test]
fn overlapping_listed_mobs_push_each_other_apart() {
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        ENDERMITE,
        ZOMBIE_PIGMAN,
        ZOGLIN,
        ZOMBIE_VILLAGER,
        SILVERFISH,
        SPIDER,
        CAVE_SPIDER,
    ] {
        let mut a = floor(7, Difficulty::Normal, GameMode::Creative);
        let first = a.adult(identifier, DVec3::new(0.4, SURFACE, 0.5));
        let second = a.adult(identifier, DVec3::new(0.6, SURFACE, 0.5));
        for mob in [first, second] {
            a.without::<RandomStrollGoal>(mob);
            a.without::<RandomLookAroundGoal>(mob);
        }
        let width = a.world().get::<PhysicsBody>(first).unwrap().width;
        a.ticks(80);
        let gap = (a.feet(second).x - a.feet(first).x).abs();
        assert!(
            gap >= width - 0.05,
            "{identifier}: still overlapping, {gap} apart with width {width}"
        );
    }
}

/// Mobs with `behavior.float` (the cow's own test is in `ai::tests`).
const FLOATERS: &[&str] = &[ENDERMITE];

/// `behavior.float`: a mob sunk in deep water swims up to the surface.
#[test]
fn listed_mobs_float_up_in_water() {
    let stone = block("minecraft:stone");
    let water = block("minecraft:water");
    for &identifier in FLOATERS {
        let mut a = arena(
            9,
            Difficulty::Normal,
            GameMode::Creative,
            move |pos| match pos.y {
                52 => Some(stone),
                53..=63 => Some(water),
                _ => None,
            },
        );
        let mob = a.adult(identifier, DVec3::new(0.5, 56.0, 0.5));
        a.ticks(80);
        let feet = a.feet(mob);
        assert!(feet.y > 61.5, "{identifier} still sunk at {feet}");
    }
}

/// `behavior.look_at_player`: with its other idle goals removed, each mob
/// turns its head (not its body) toward a creative player four blocks east.
#[test]
fn listed_mobs_turn_their_heads_toward_a_nearby_player() {
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        ZOMBIE_PIGMAN,
        ZOGLIN,
        ZOMBIE_VILLAGER,
    ] {
        let mut a = floor(11, Difficulty::Normal, GameMode::Creative);
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.without::<RandomStrollGoal>(mob);
        a.without::<RandomLookAroundGoal>(mob);
        // East of the mob: yaw -90.
        a.move_player(DVec3::new(4.5, SURFACE, 0.5));
        let mut turned = false;
        for _ in 0..2400 {
            a.ticks(1);
            let rotation = *a.world().get::<Rotation>(mob).unwrap();
            if (rotation.head_yaw + 90.0).abs() < 1.0 {
                assert!(rotation.yaw.abs() < 1e-6, "{identifier}: {rotation:?}");
                turned = true;
                break;
            }
        }
        assert!(turned, "{identifier} never looked at the player");
    }
}

/// `behavior.random_look_around`: an idle mob turns its head within the
/// goal's angle range of its body without moving.
#[test]
fn idle_listed_mobs_look_around_within_the_angle_range() {
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        ZOMBIE_PIGMAN,
        ZOGLIN,
        ZOMBIE_VILLAGER,
    ] {
        let mut a = floor(13, Difficulty::Normal, GameMode::Creative);
        a.move_player(DVec3::new(-14.5, SURFACE, -14.5));
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.without::<RandomStrollGoal>(mob);
        let goal = a
            .world()
            .get::<RandomLookAroundGoal>(mob)
            .cloned()
            .unwrap_or_else(|| panic!("{identifier} looks around"));
        let (min, max) = goal.angle_range;
        a.ticks(5);
        let start = a.feet(mob);
        let mut largest = 0.0f32;
        for _ in 0..1200 {
            a.ticks(1);
            let rotation = *a.world().get::<Rotation>(mob).unwrap();
            let off = (rotation.head_yaw - rotation.yaw + 540.0).rem_euclid(360.0) - 180.0;
            assert!(
                off >= min - 1e-3 && off <= max + 1e-3,
                "{identifier}: head {off}° off the body, range {min}..{max}"
            );
            largest = largest.max(off.abs());
        }
        assert!(largest > 5.0, "{identifier}: the head never turned");
        assert!(horizontal(a.feet(mob), start) < 1e-6, "{identifier} moved");
    }
}

/// `despawn` (`despawn_from_distance` defaults): each mob is removed at once
/// when the only player is 200 blocks away, and kept while it is within
/// 32 blocks.
#[test]
fn listed_mobs_despawn_far_from_players_and_stay_near_them() {
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        ENDERMITE,
        ZOMBIE_PIGMAN,
        ZOGLIN,
        ZOMBIE_VILLAGER,
        SILVERFISH,
        SPIDER,
        CAVE_SPIDER,
    ] {
        let mut a = floor(15, Difficulty::Normal, GameMode::Creative);
        let near = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.without::<RandomStrollGoal>(near);
        a.move_player(DVec3::new(16.5, SURFACE, 16.5));
        a.ticks(100);
        assert!(a.world().get_entity(near).is_ok(), "{identifier} near");
        a.move_player(DVec3::new(200.5, SURFACE, 0.5));
        a.ticks(1);
        assert!(a.world().get_entity(near).is_err(), "{identifier} far");
    }
}

/// `type_family`: each adult answers `is_family` for every family its
/// definition gives it, and not for a family it lacks.
#[test]
fn listed_mobs_belong_to_their_authored_families() {
    let cases: [(&str, &[&str], &str); 13] = [
        (SKELETON, &["skeleton", "undead", "monster", "mob"], "stray"),
        (
            STRAY,
            &["stray", "skeleton", "monster", "mob", "undead"],
            "bogged",
        ),
        (
            BOGGED,
            &["bogged", "skeleton", "monster", "mob", "undead"],
            "parched",
        ),
        (
            PARCHED,
            &["parched", "skeleton", "monster", "mob", "undead"],
            "wither",
        ),
        (
            ZOMBIE_VILLAGER,
            &["zombie", "zombie_villager", "undead", "monster", "mob"],
            "baby_undead",
        ),
        (
            ZOGLIN,
            &["zoglin", "zoglin_adult", "undead", "monster", "mob"],
            "zoglin_baby",
        ),
        (
            HUSK,
            &["husk", "zombie", "undead", "monster", "mob"],
            "baby_undead",
        ),
        (
            WITHER_SKELETON,
            &["wither", "monster", "undead", "skeleton", "mob"],
            "zombie",
        ),
        (
            ENDERMITE,
            &["endermite", "arthropod", "monster", "mob"],
            "undead",
        ),
        (
            ZOMBIE_PIGMAN,
            &["zombie_pigman", "undead", "monster", "mob"],
            "baby_zombie_pigman",
        ),
        (
            SILVERFISH,
            &["silverfish", "monster", "mob", "arthropod"],
            "undead",
        ),
        (SPIDER, &["spider", "monster", "mob", "arthropod"], "undead"),
        (
            CAVE_SPIDER,
            &["cavespider", "monster", "arthropod", "mob"],
            "spider",
        ),
    ];
    let mut a = floor(17, Difficulty::Normal, GameMode::Creative);
    for (identifier, families, absent) in cases {
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        let world = a.world();
        for family in families {
            assert!(
                crate::entity::mob::events::is_family(world, mob, family),
                "{identifier} is not {family}"
            );
        }
        assert!(
            !crate::entity::mob::events::is_family(world, mob, absent),
            "{identifier} is {absent}"
        );
        world.despawn(mob);
    }
}

/// `loot`, `experience_reward`, `health`: killed by the player, each mob
/// drops only items of its loot table (the common one at least once over
/// the kills) and, twenty ticks later, its `experience_reward` for a player
/// kill without equipment.
#[test]
fn listed_mobs_drop_their_loot_and_experience_when_killed_by_a_player() {
    // The stray, bogged and parched also drop a tipped arrow on a player
    // kill (`set_potion`), which loot does not implement.
    let cases: [(&str, &[&str], Option<&str>, u32); 10] = [
        (
            SKELETON,
            &["minecraft:arrow", "minecraft:bone"],
            Some("minecraft:bone"),
            5,
        ),
        (
            ZOMBIE_VILLAGER,
            &[
                "minecraft:rotten_flesh",
                "minecraft:iron_ingot",
                "minecraft:carrot",
                "minecraft:potato",
            ],
            Some("minecraft:rotten_flesh"),
            5,
        ),
        (
            ZOGLIN,
            &["minecraft:rotten_flesh"],
            Some("minecraft:rotten_flesh"),
            5,
        ),
        (
            HUSK,
            &[
                "minecraft:rotten_flesh",
                "minecraft:iron_ingot",
                "minecraft:carrot",
                "minecraft:potato",
            ],
            Some("minecraft:rotten_flesh"),
            5,
        ),
        (
            WITHER_SKELETON,
            &[
                "minecraft:coal",
                "minecraft:bone",
                "minecraft:wither_skeleton_skull",
            ],
            Some("minecraft:bone"),
            5,
        ),
        (ENDERMITE, &[], None, 3),
        (SILVERFISH, &[], None, 5),
        (
            SPIDER,
            &["minecraft:string", "minecraft:spider_eye"],
            Some("minecraft:string"),
            5,
        ),
        (
            CAVE_SPIDER,
            &["minecraft:string", "minecraft:spider_eye"],
            Some("minecraft:string"),
            5,
        ),
        (
            ZOMBIE_PIGMAN,
            &[
                "minecraft:rotten_flesh",
                "minecraft:gold_nugget",
                "minecraft:gold_ingot",
            ],
            Some("minecraft:rotten_flesh"),
            5,
        ),
    ];
    for (identifier, table, common, experience) in cases {
        let mut a = floor(19, Difficulty::Normal, GameMode::Survival);
        let mut seen_common = false;
        for _ in 0..30 {
            let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
            a.world().entity_mut(mob).remove::<MobEquipment>();
            a.take_drops();
            a.kill_by_player(mob);
            for (item, _) in a.take_drops() {
                assert!(
                    table.contains(&item.as_str()),
                    "{identifier} dropped {item}"
                );
                seen_common |= Some(item.as_str()) == common;
            }
            assert!(
                a.take_orbs().is_empty(),
                "{identifier}: experience before removal"
            );
            a.ticks(21);
            assert!(a.world().get_entity(mob).is_err(), "{identifier} removed");
            let orbs: u32 = a.take_orbs().iter().sum();
            assert_eq!(orbs, experience, "{identifier} experience");
        }
        assert_eq!(seen_common, common.is_some(), "{identifier}: {common:?}");
    }
}

/// `hurt_on_condition` (lava, 4 per tick): a mob standing in lava is hurt
/// by the lava cause and burns to death.
#[test]
fn listed_mobs_standing_in_lava_are_hurt_by_it() {
    for identifier in [
        HUSK,
        ENDERMITE,
        ZOMBIE_VILLAGER,
        SKELETON,
        STRAY,
        BOGGED,
        PARCHED,
        SILVERFISH,
        SPIDER,
        CAVE_SPIDER,
    ] {
        let mut a = floor(21, Difficulty::Normal, GameMode::Creative);
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.without::<RandomStrollGoal>(mob);
        a.set_block(IVec3::new(0, 64, 0), "minecraft:lava");
        let max = a.world().get::<Health>(mob).unwrap().max;
        a.ticks(2);
        let world = a.world();
        let health = world.get::<Health>(mob).unwrap().current;
        assert!(health < max, "{identifier}: {health} of {max}");
        let cause = world
            .get::<crate::entity::mob::state::LastHurt>(mob)
            .map(|hurt| hurt.cause);
        assert_eq!(cause, Some(ActorDamageCause::Lava), "{identifier}");
        a.ticks(200);
        let world = a.world();
        assert!(
            world.get_entity(mob).is_err() || world.get::<Dead>(mob).is_some(),
            "{identifier} survived the lava"
        );
    }
}

/// The ticks on which the player lost health over at most `ticks`, until it
/// was hit three times.
fn hits_on_player(a: &mut Arena, ticks: u64) -> Vec<(u64, f32)> {
    let player = a.player;
    let mut hits = Vec::new();
    let mut health = a.world().get::<Health>(player).unwrap().current;
    for tick in 0..ticks {
        a.ticks(1);
        let now = a.world().get::<Health>(player).unwrap().current;
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

fn swings(packets: &[McpePacketData], runtime_id: i64) -> usize {
    packets
        .iter()
        .filter(|packet| {
            matches!(packet, McpePacketData::ActorEventPacket(event)
                if event.event_id == EnumsActorEvent::StartAttacking
                    && event.target_runtime_id.actor_runtime_id == runtime_id as u64)
        })
        .count()
}

/// Keep the player alive through many hits.
fn tough_player(a: &mut Arena) {
    let player = a.player;
    *a.world().get_mut::<Health>(player).unwrap() = Health {
        current: 10_000.0,
        max: 10_000.0,
    };
}

/// `behavior.nearest_attackable_target`, `behavior.melee_box_attack`,
/// `attack`: each hostile mob targets the survival player standing six
/// blocks away and hits it three times, one swing per hit, on the attack
/// cooldown.
#[test]
fn listed_hostile_mobs_target_and_hit_a_survival_player() {
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        ENDERMITE,
        ZOGLIN,
        ZOMBIE_VILLAGER,
        SILVERFISH,
        SPIDER,
    ] {
        let mut a = floor(23, Difficulty::Normal, GameMode::Survival);
        tough_player(&mut a);
        a.move_player(DVec3::new(6.5, SURFACE, 0.5));
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        let player = a.player;
        a.packets();
        let hits = hits_on_player(&mut a, 600);
        assert_eq!(
            a.world().get::<AttackTarget>(mob),
            Some(&AttackTarget(player)),
            "{identifier}"
        );
        assert_eq!(hits.len(), 3, "{identifier}: {hits:?}");
        assert!(hits[1].0 - hits[0].0 >= 20, "{identifier}: {hits:?}");
        let runtime_id = a.world().get::<RuntimeId>(mob).unwrap().0;
        let packets = a.packets();
        assert!(swings(&packets, runtime_id) >= 3, "{identifier} swings");
    }
}

/// `behavior.nearest_attackable_target`: creative players are never
/// targeted.
#[test]
fn listed_hostile_mobs_ignore_creative_players() {
    for identifier in [
        HUSK,
        WITHER_SKELETON,
        ENDERMITE,
        ZOGLIN,
        ZOMBIE_VILLAGER,
        SILVERFISH,
        SPIDER,
        CAVE_SPIDER,
    ] {
        let mut a = floor(25, Difficulty::Hard, GameMode::Creative);
        a.move_player(DVec3::new(4.5, SURFACE, 0.5));
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.ticks(300);
        assert!(a.world().get::<AttackTarget>(mob).is_none(), "{identifier}");
        let player = a.player;
        assert_eq!(a.world().get::<Health>(player).unwrap().current, 20.0);
    }
}

/// `behavior.hurt_by_target`, `behavior.melee_box_attack`, `attack` on the
/// neutral zombie pigman: once the player hurts it, it turns on the player
/// and hits it on the attack cooldown.
#[test]
fn a_provoked_zombie_pigman_hits_the_player_that_hurt_it() {
    let mut a = floor(27, Difficulty::Normal, GameMode::Survival);
    tough_player(&mut a);
    a.move_player(DVec3::new(4.5, SURFACE, 0.5));
    let mob = a.adult(ZOMBIE_PIGMAN, DVec3::new(0.5, SURFACE, 0.5));
    a.ticks(40);
    assert!(a.world().get::<AttackTarget>(mob).is_none(), "neutral");
    let player = a.player;
    let world = a.world();
    combat::player_attack(world, player, mob, &Default::default());
    a.packets();
    let hits = hits_on_player(&mut a, 600);
    assert_eq!(
        a.world().get::<AttackTarget>(mob),
        Some(&AttackTarget(player))
    );
    assert_eq!(hits.len(), 3, "{hits:?}");
    assert!(hits[1].0 - hits[0].0 >= 20, "{hits:?}");
    let runtime_id = a.world().get::<RuntimeId>(mob).unwrap().0;
    let packets = a.packets();
    assert!(swings(&packets, runtime_id) >= 3);
}

/// `behavior.pickup_items`, `behavior.equip_item`, `equip_item`,
/// `shareables`: a mob allowed to pick items up walks to an iron sword
/// beside it, takes it into its main hand and drops the wooden sword it
/// held.
#[test]
fn listed_mobs_pick_up_a_better_sword_and_drop_their_old_one() {
    for identifier in [HUSK, WITHER_SKELETON, ZOMBIE_PIGMAN, ZOMBIE_VILLAGER] {
        let mut a = floor(29, Difficulty::Easy, GameMode::Creative);
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.without::<RandomStrollGoal>(mob);
        let wooden = world_stack(a.world(), "minecraft:wooden_sword", 1).unwrap();
        a.world()
            .entity_mut(mob)
            .insert((MobEquipment::default(), CanPickUpLoot(true)))
            .get_mut::<MobEquipment>()
            .unwrap()
            .set(GearSlot::Mainhand, wooden, false);
        let feet = a.feet(mob);
        let sword = world_stack(a.world(), "minecraft:iron_sword", 1).unwrap();
        let item = crate::entity::spawn_item_drop(
            a.world(),
            feet + DVec3::new(0.8, 0.0, 0.0),
            sword,
            None,
        )
        .unwrap();
        for _ in 0..300 {
            a.ticks(1);
            let held = a.world().get::<MobEquipment>(mob).unwrap();
            if held.get(GearSlot::Mainhand).item_id() == "minecraft:iron_sword" {
                break;
            }
        }
        let world = a.world();
        let gear = world.get::<MobEquipment>(mob).unwrap();
        assert_eq!(
            gear.get(GearSlot::Mainhand).item_id(),
            "minecraft:iron_sword",
            "{identifier}"
        );
        assert!(gear.item(GearSlot::Mainhand).picked_up, "{identifier}");
        assert!(world.get_entity(item).is_err(), "{identifier}: item left");
        assert!(world.get::<Persistent>(mob).is_some(), "{identifier}");
        let drops = a.take_drops();
        assert!(
            drops
                .iter()
                .any(|(item, _)| item == "minecraft:wooden_sword"),
            "{identifier}: {drops:?}"
        );
    }
}

/// `equipment`: each mob spawns with gear from its equipment table (the
/// wither skeleton always holds a stone sword, the zombie pigman a golden
/// sword or spear, a hard husk sometimes rolls the zombie gear), and its
/// viewer gets the main hand after the AddActor.
#[test]
fn listed_mobs_spawn_with_their_equipment_table_gear() {
    let cases: [(&str, &[&str]); 3] = [
        (WITHER_SKELETON, &["minecraft:stone_sword"]),
        (
            ZOMBIE_PIGMAN,
            &["minecraft:golden_sword", "minecraft:golden_spear"],
        ),
        (
            HUSK,
            &[
                "minecraft:iron_sword",
                "minecraft:iron_shovel",
                "minecraft:iron_spear",
            ],
        ),
    ];
    for (identifier, hand) in cases {
        let mut a = floor(31, Difficulty::Hard, GameMode::Creative);
        let feet = DVec3::new(0.5, SURFACE, 0.5);
        let mut equipped = None;
        for _ in 0..600 {
            let mob = a.adult(identifier, feet);
            if equipment_count(a.world(), mob) > 0
                && !a
                    .world()
                    .get::<MobEquipment>(mob)
                    .unwrap()
                    .get(GearSlot::Mainhand)
                    .is_empty()
            {
                equipped = Some(mob);
                break;
            }
            a.world().despawn(mob);
        }
        let mob = equipped.unwrap_or_else(|| panic!("{identifier} never held gear"));
        let main = a
            .world()
            .get::<MobEquipment>(mob)
            .unwrap()
            .get(GearSlot::Mainhand)
            .clone();
        assert!(
            hand.contains(&main.item_id()),
            "{identifier}: {}",
            main.item_id()
        );
        a.packets();
        a.ticks(1);
        let runtime_id = a.world().get::<RuntimeId>(mob).unwrap().0 as u64;
        let packets = a.packets();
        let added = packets
            .iter()
            .position(|p| {
                matches!(p, McpePacketData::AddActorPacket(add)
                if add.target_runtime_id.actor_runtime_id == runtime_id)
            })
            .unwrap_or_else(|| panic!("{identifier} AddActor"));
        let shown = packets.iter().enumerate().any(|(i, p)| {
            matches!(p, McpePacketData::MobEquipmentPacket(e)
                if i > added
                    && e.target_runtime_id.actor_runtime_id == runtime_id
                    && (e.slot, e.container_id) == (0, 0)
                    && e.item.id == main.item().unwrap().network_id() as i16)
        });
        assert!(shown, "{identifier}: the main hand reaches the viewer");
    }
}

/// `is_baby`, `scale`: a baby is flagged, half scale (the scale actor data)
/// and gets the smaller box of its baby group.
#[test]
fn listed_babies_are_flagged_half_scale_and_small() {
    use crate::entity::definition::metadata::EntityMetadata;
    use jolyne::valentine::actor_data_i_ds as ids;
    use jolyne::valentine::types::DataItemEntryPayload;
    for identifier in [HUSK, ZOMBIE_PIGMAN, ZOGLIN, ZOMBIE_VILLAGER] {
        let mut a = floor(33, Difficulty::Normal, GameMode::Creative);
        let adult = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        let baby = a.spawn(identifier, DVec3::new(-3.5, SURFACE, 0.5), true);
        let world = a.world();
        assert!(flag(world, baby, actor_flags::BABY), "{identifier}");
        assert!(!flag(world, adult, actor_flags::BABY), "{identifier}");
        let scale = |mob| match world
            .get::<EntityMetadata>(mob)
            .unwrap()
            .get(ids::RESERVED_038)
        {
            Some(DataItemEntryPayload::DataItemFloatPayload(f)) => f.value,
            other => panic!("{identifier} SCALE {other:?}"),
        };
        assert_eq!(scale(adult), 1.0, "{identifier}");
        assert_eq!(scale(baby), 0.5, "{identifier}");
        let (big, small) = (
            world.get::<PhysicsBody>(adult).unwrap(),
            world.get::<PhysicsBody>(baby).unwrap(),
        );
        assert!(
            small.width < big.width && small.height < big.height,
            "{identifier}: {small:?} vs {big:?}"
        );
    }
}

/// `spawn_egg_interaction`, `offspring`: using the mob's own spawn egg on
/// it spawns a baby of its kind at its feet (the offspring pair of these
/// mobs is the mob itself) and uses one egg.
#[test]
fn listed_mobs_spawn_a_baby_from_their_own_spawn_egg() {
    use jolyne::valentine::types::{
        ActorRuntimeId, EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
        ItemUseOnActorInventoryTransaction,
    };
    for identifier in [HUSK, ZOMBIE_PIGMAN, ZOGLIN, ZOMBIE_VILLAGER] {
        let mut a = floor(35, Difficulty::Normal, GameMode::Survival);
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.without::<RandomStrollGoal>(mob);
        // The zombie villager's egg is named for the summon identifier.
        let egg = format!("{}_spawn_egg", identifier.trim_end_matches("_v2"));
        let stack = world_stack(a.world(), &egg, 2).unwrap();
        let player = a.player;
        a.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
        a.move_player(DVec3::new(2.5, SURFACE, 0.5));
        let runtime_id = a.world().get::<RuntimeId>(mob).unwrap().0;
        let transaction = ItemUseOnActorInventoryTransaction {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            action_type: ActionType::Interact,
            slot: 0,
            ..Default::default()
        };
        crate::entity::mob::interaction::interact::player_interact(
            a.world(),
            player,
            mob,
            &transaction,
        );
        let world = a.world();
        let held = world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count;
        assert_eq!(held, 1, "{identifier}: one egg used");
        let babies: Vec<Entity> = world
            .query_filtered::<(Entity, &MobKind), Without<Dead>>()
            .iter(world)
            .filter(|(entity, kind)| *entity != mob && &*kind.0.identifier == identifier)
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(babies.len(), 1, "{identifier}");
        apply_pending_definitions(world);
        assert!(flag(world, babies[0], actor_flags::BABY), "{identifier}");
        assert_eq!(
            world.get::<Position>(babies[0]).unwrap().0,
            world.get::<Position>(mob).unwrap().0,
            "{identifier}"
        );
    }
}

/// `environment_sensor`, `timer`, `is_shaking` on the husk: under water it
/// starts converting (its 30 s timer starts), shakes once the timer fires,
/// and a husk that leaves the water first stops converting.
#[test]
fn an_underwater_husk_starts_converting_and_shakes_when_its_timer_fires() {
    use crate::entity::mob::components::Timer;
    let feet = DVec3::new(0.5, SURFACE, 0.5);
    let hold = |a: &mut Arena, mob: Entity, n: u64| {
        for _ in 0..n {
            let world = a.world();
            world.get_mut::<Position>(mob).unwrap().0 = feet;
            if let Some(mut velocity) = world.get_mut::<Velocity>(mob) {
                velocity.0 = DVec3::ZERO;
            }
            a.ticks(1);
        }
    };
    let mut a = floor(37, Difficulty::Normal, GameMode::Creative);
    let husk = a.adult(HUSK, feet);
    for y in 64..67 {
        a.set_block(IVec3::new(0, y, 0), "minecraft:water");
    }
    hold(&mut a, husk, 2);
    let timer = a
        .world()
        .get::<Timer>(husk)
        .cloned()
        .expect("the timer runs");
    assert_eq!(timer.span_ticks, 600, "30 s");
    assert!(!flag(a.world(), husk, actor_flags::SHAKING));
    let now = a.world().resource::<crate::ecs::TickCounter>().current;
    hold(&mut a, husk, timer.deadline - now + 2);
    assert!(a.world().get::<Timer>(husk).is_none_or(|t| t.fired));
    assert!(flag(a.world(), husk, actor_flags::SHAKING), "shaking");

    let mut a = floor(38, Difficulty::Normal, GameMode::Creative);
    let husk = a.adult(HUSK, feet);
    for y in 64..67 {
        a.set_block(IVec3::new(0, y, 0), "minecraft:water");
    }
    hold(&mut a, husk, 2);
    assert!(a.world().get::<Timer>(husk).is_some());
    for y in 64..67 {
        a.set_block(IVec3::new(0, y, 0), "minecraft:air");
    }
    hold(&mut a, husk, 2);
    assert!(a.world().get::<Timer>(husk).is_none(), "stopped converting");
    hold(&mut a, husk, 700);
    assert!(!flag(a.world(), husk, actor_flags::SHAKING));
}

/// `despawn` with `filters` (the zoglin's): beyond 54 blocks of every player
/// a mob that is not persistent despawns at once; a persistent one stays;
/// 40 blocks away it is kept for its first 30 inactive seconds and then
/// goes at random (1 in 800 per tick); within 32 blocks it stays.
#[test]
fn a_zoglin_despawns_by_its_filters() {
    let feet = DVec3::new(0.5, SURFACE, 0.5);
    let mut a = floor(41, Difficulty::Normal, GameMode::Creative);
    let far = a.adult(ZOGLIN, feet);
    a.without::<RandomStrollGoal>(far);
    a.move_player(DVec3::new(60.5, SURFACE, 0.5));
    a.ticks(1);
    assert!(a.world().get_entity(far).is_err(), "beyond 54 blocks");

    let mut a = floor(42, Difficulty::Normal, GameMode::Creative);
    let kept = a.adult(ZOGLIN, feet);
    a.without::<RandomStrollGoal>(kept);
    a.world().entity_mut(kept).insert(Persistent);
    a.move_player(DVec3::new(60.5, SURFACE, 0.5));
    a.ticks(50);
    assert!(a.world().get_entity(kept).is_ok(), "persistent");

    let mut a = floor(43, Difficulty::Normal, GameMode::Creative);
    let near = a.adult(ZOGLIN, feet);
    let inactive = a.adult(ZOGLIN, DVec3::new(-10.5, SURFACE, 0.5));
    for mob in [near, inactive] {
        a.without::<RandomStrollGoal>(mob);
    }
    // The player's eye is 26.4 blocks from `near`'s block and 35.6 from
    // `inactive`'s.
    a.move_player(DVec3::new(20.5, SURFACE + 15.0, 0.5));
    a.ticks(590);
    assert!(a.world().get_entity(inactive).is_ok(), "within 30 s");
    let mut gone = None;
    for tick in 0..20_000 {
        a.ticks(1);
        if a.world().get_entity(inactive).is_err() {
            gone = Some(tick);
            break;
        }
    }
    assert!(gone.is_some(), "1 in 800 per tick after 30 s");
    a.ticks(4000);
    assert!(a.world().get_entity(near).is_ok(), "within 32 blocks");
}

/// `custom_hit_test`: a player whose eye is out of reach of the zoglin's
/// collision box but within reach of its wider hit box still hits it; the
/// same distance from a zombie pigman's box misses.
#[test]
fn a_zoglin_is_hit_on_its_hit_box_beyond_its_collision_box() {
    for (identifier, half_width, lands) in [(ZOGLIN, 0.7, true), (ZOMBIE_PIGMAN, 0.3, false)] {
        let mut a = floor(44, Difficulty::Normal, GameMode::Creative);
        let mob = a.adult(identifier, DVec3::new(0.5, SURFACE, 0.5));
        a.without::<RandomStrollGoal>(mob);
        let player = a.player;
        let world = a.world();
        let max = world.get::<Health>(mob).unwrap().current;
        // 6.15 blocks from the collision box's side, level with it.
        world.get_mut::<Position>(player).unwrap().0 =
            DVec3::new(0.5 + half_width + 6.15, SURFACE + 1.0, 0.5);
        combat::player_attack(world, player, mob, &Default::default());
        let hit = world.get::<Health>(mob).unwrap().current < max;
        assert_eq!(hit, lands, "{identifier}");
    }
}

/// A zombie villager in the open at noon, a one-layer oak-leaf canopy over
/// x 4..=8, z -2..=2 (sky light 14 under it), with its idle goals removed;
/// `from_village` gives it `behavior.flee_sun` when `flee`.
fn sunlit_zombie_villager(seed: u64, flee: bool, time: u64) -> (Arena, Entity) {
    let leaves = block("minecraft:oak_leaves");
    let mut a = arena(seed, Difficulty::Normal, GameMode::Creative, move |pos| {
        ((4..=8).contains(&pos.x) && (-2..=2).contains(&pos.z) && pos.y == 66).then_some(leaves)
    });
    crate::world::clock::set_time(a.world(), time);
    let mob = a.adult(ZOMBIE_VILLAGER, DVec3::new(0.5, SURFACE, 0.5));
    if flee {
        let event = a
            .world()
            .get::<MobKind>(mob)
            .unwrap()
            .0
            .events
            .event("from_village")
            .unwrap();
        crate::entity::mob::events::fire_event(a.world(), mob, &event, None).unwrap();
        apply_pending_definitions(a.world());
    }
    a.without::<RandomStrollGoal>(mob);
    a.without::<RandomLookAroundGoal>(mob);
    a.move_player(DVec3::new(-14.5, SURFACE, -14.5));
    (a, mob)
}

fn under_canopy(feet: DVec3) -> bool {
    (4.0..9.0).contains(&feet.x) && (-2.0..3.0).contains(&feet.z)
}

/// `behavior.flee_sun` (the abandoned-village group): a zombie villager
/// set alight by the sun walks under the leaf canopy; without the goal, or
/// at night when it never burns, it stays in the open.
#[test]
fn a_burning_zombie_villager_flees_the_sun_under_cover() {
    use crate::entity::mob::combat::fire::is_burning;
    const NOON: u64 = 6_000;
    for (flee, time, takes_cover) in [
        (true, NOON, true),
        (false, NOON, false),
        (true, MIDNIGHT, false),
    ] {
        let (mut a, mob) = sunlit_zombie_villager(45, flee, time);
        let mut burned = false;
        let mut covered = false;
        for _ in 0..400 {
            a.ticks(1);
            let world = a.world();
            if world.get_entity(mob).is_err() {
                break;
            }
            burned |= is_burning(world, mob);
            if under_canopy(world.get::<Position>(mob).unwrap().0) {
                covered = true;
                break;
            }
        }
        assert_eq!(burned, time == NOON, "flee {flee} at {time}");
        assert_eq!(covered, takes_cover, "flee {flee} at {time}");
    }
}

/// `spell_effects` (the cure's `to_villager` group): weakness is removed and
/// strength is given for 300 s; the group's `heal` is no effect name, so
/// the hurt zombie villager is not healed.
#[test]
fn a_cured_zombie_villager_loses_weakness_and_gains_strength_without_healing() {
    use crate::entity::components::{EffectType, Effects};
    let mut a = floor(46, Difficulty::Normal, GameMode::Creative);
    let mob = a.adult(ZOMBIE_VILLAGER, DVec3::new(0.5, SURFACE, 0.5));
    let world = a.world();
    world.get_mut::<Health>(mob).unwrap().current = 7.0;
    world
        .entity_mut(mob)
        .insert(Effects::default())
        .get_mut::<Effects>()
        .unwrap()
        .add(EffectType::WEAKNESS, 0, 1200);
    let event = world
        .get::<MobKind>(mob)
        .unwrap()
        .0
        .events
        .event("villager_converted")
        .unwrap();
    crate::entity::mob::events::fire_event(world, mob, &event, None).unwrap();
    apply_pending_definitions(world);
    let effects = world.get::<Effects>(mob).unwrap();
    assert!(!effects.has(EffectType::WEAKNESS));
    let strength = effects.get(EffectType::STRENGTH).expect("strength");
    assert_eq!((strength.level, strength.duration_ticks), (0, 6000));
    assert_eq!(effects.active.len(), 1);
    assert_eq!(world.get::<Health>(mob).unwrap().current, 7.0);
    assert!(flag(world, mob, actor_flags::SHAKING), "is_shaking");
}
