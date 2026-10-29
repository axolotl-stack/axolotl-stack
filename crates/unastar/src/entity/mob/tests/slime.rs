//! The slime through a real `GameServer`: native AddActor equality, the
//! hop timing of each size, contact damage, splitting, loot and experience
//! ([`behaviour`]); natural spawning (the slime rule's slime-chunk and swamp
//! placement test, the slime class's superflat native check).

mod behaviour {
    use bevy_ecs::prelude::*;
    use glam::DVec3;
    use jolyne::valentine::{AddActorPacket, McpePacket, actor_data_i_ds};
    use tokio::sync::mpsc;
    use unastar_data::actor_components::body::ActorDamageCause;
    use valentine::bedrock::codec::BedrockCodec;

    use crate::entity::EntityRandom;
    use crate::entity::components::{
        DroppedItem, Experience, GameMode, Health, HeldSlot, ItemEntityStack, MainInventory,
        OnGround, PhysicsBody, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
        RuntimeId,
    };
    use crate::entity::definition::apply_pending_definitions;
    use crate::entity::definition::attributes::ActorAttributes;
    use crate::entity::definition::components::MovementConfig;
    use crate::entity::definition::metadata::EntityMetadata;
    use crate::entity::definition::properties::ActorProperties;
    use crate::entity::mob::components::KnockbackPower;
    use crate::entity::mob::net::network::add_actor_packet;
    use crate::entity::mob::state::{AttackTarget, Dead};
    use crate::entity::mob::*;
    use crate::entity::xp_orb::XpOrb;
    use crate::server::game::{GameServer, ServerConfig};
    use crate::world::chunk::{Chunk, blocks};
    use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
    use crate::world::rules::Difficulty;

    const MIDNIGHT: u64 = 18_000;
    const SLIME: &str = "minecraft:slime";

    /// The three sizes: spawn event, `VARIANT`, box, movement speed, health
    /// and contact damage.
    const SIZES: [(&str, i32, f32, f32, f32, f32); 3] = [
        ("spawn_small", 1, 0.52, 0.3, 1.0, 0.0),
        ("spawn_medium", 2, 1.04, 0.4, 4.0, 2.0),
        ("spawn_large", 4, 2.08, 0.6, 16.0, 4.0),
    ];

    struct Harness {
        server: GameServer,
        player: Entity,
        _rx: mpsc::Receiver<McpePacket>,
    }

    /// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at midnight on
    /// normal, viewed by one player in `mode` standing on it at
    /// (8.5, 64, 0.5).
    fn harness(seed: u64, mode: GameMode) -> Harness {
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
        Harness {
            server,
            player,
            _rx: rx,
        }
    }

    fn native(bytes: &[u8]) -> AddActorPacket {
        assert_eq!(bytes[0], 0x0d);
        let mut input = &bytes[1..];
        let packet = AddActorPacket::decode(&mut input, ()).unwrap();
        assert!(input.is_empty());
        packet
    }

    fn variant(world: &World, mob: Entity) -> i32 {
        world
            .get::<EntityMetadata>(mob)
            .unwrap()
            .get_int(actor_data_i_ds::VARIANT)
            .unwrap()
    }

    impl Harness {
        fn world(&mut self) -> &mut World {
            self.server.ecs.world_mut()
        }

        /// A slime of the size `event` spawns.
        fn slime(&mut self, event: &str, feet: DVec3) -> Entity {
            let world = self.world();
            let mob = spawn_mob_with_event(world, SLIME, feet, 0.0, event).unwrap();
            apply_pending_definitions(world);
            mob
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

        /// Every living slime.
        fn slimes(&mut self) -> Vec<Entity> {
            let world = self.world();
            world
                .query_filtered::<Entity, (With<MobKind>, Without<Dead>)>()
                .iter(world)
                .collect()
        }

        /// Kill `mob`: punched by the player with one health left, or by a
        /// damage without an attacker. Then let the death animation end.
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
            self.ticks(u64::from(constants::DEATH_TICKS) + 2);
            assert!(self.world().get_entity(mob).is_err(), "body removed");
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

        /// The grounded streaks (ticks on the ground before each takeoff)
        /// of `mob` over `ticks`, skipping the landing after its spawn. The
        /// player is healed every tick so contact damage cannot kill it.
        fn ground_waits(&mut self, mob: Entity, ticks: u64) -> Vec<u32> {
            let mut waits = Vec::new();
            let mut streak = 0;
            let mut landed_once = false;
            let player = self.player;
            for _ in 0..ticks {
                self.world().get_mut::<Health>(player).unwrap().current = 20.0;
                self.server.tick();
                let on_ground = self.world().get::<OnGround>(mob).unwrap().0;
                if on_ground {
                    streak += 1;
                } else if streak > 0 {
                    if landed_once {
                        waits.push(streak);
                    }
                    landed_once = true;
                    streak = 0;
                }
            }
            waits
        }
    }

    #[test]
    fn vanilla_slime_compiles_to_a_spawnable_archetype() {
        let mut h = harness(1, GameMode::Creative);
        let registry = h.world().resource::<MobRegistry>();
        assert!(
            registry.get(SLIME).is_some(),
            "slime unsupported: {:?}",
            registry.unsupported_reason(SLIME)
        );
    }

    #[test]
    fn spawned_small_slime_add_actor_matches_native_summon() {
        let mut h = harness(2, GameMode::Creative);
        let native = native(include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/slime.bin"
        ));
        let feet = DVec3::new(
            f64::from(native.position.x),
            f64::from(native.position.y),
            f64::from(native.position.z),
        );
        let slime = h.slime("spawn_small", feet);
        let world = h.world();
        let ours = add_actor_packet(
            SLIME,
            world.get::<RuntimeId>(slime).unwrap().0,
            world.get::<Position>(slime).unwrap().0,
            DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
            world.get::<Rotation>(slime).unwrap(),
            world.get::<EntityMetadata>(slime).unwrap(),
            world.get::<ActorProperties>(slime).unwrap(),
            world.get::<ActorAttributes>(slime).unwrap(),
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
    fn each_size_has_its_body_and_waits_its_jump_delay_between_hops() {
        for (index, &(event, size, width, speed, health, _)) in SIZES.iter().enumerate() {
            // A creative player is never targeted: the slime stays calm.
            let mut h = harness(10 + index as u64, GameMode::Creative);
            let slime = h.slime(event, DVec3::new(0.5, 64.0, 8.5));
            let world = h.world();
            assert_eq!(variant(world, slime), size, "{event}");
            let body = world.get::<PhysicsBody>(slime).unwrap();
            assert_eq!((body.width as f32, body.height as f32), (width, width));
            assert_eq!(world.get::<MovementConfig>(slime).unwrap().value, speed);
            assert_eq!(world.get::<Health>(slime).unwrap().current, health);
            let start = world.get::<Position>(slime).unwrap().0;
            // Calm: 0.5–1.5 s on the ground before each hop.
            let waits = h.ground_waits(slime, 400);
            assert!(waits.len() >= 5, "{event}: waits {waits:?}");
            assert!(
                waits.iter().all(|w| (10..=30).contains(w)),
                "{event}: waits {waits:?}"
            );
            let moved = h.world().get::<Position>(slime).unwrap().0 - start;
            assert!(
                moved.x.hypot(moved.z) > 1.0,
                "{event}: hops forward, moved {moved}"
            );
        }
    }

    #[test]
    fn a_slime_with_a_target_hops_at_its_aggressive_delay() {
        let mut h = harness(20, GameMode::Survival);
        let slime = h.slime("spawn_large", DVec3::new(0.5, 64.0, 8.5));
        let player = h.player;
        for _ in 0..200 {
            h.server.tick();
            if h.world().get::<AttackTarget>(slime).is_some() {
                break;
            }
        }
        assert_eq!(
            h.world().get::<AttackTarget>(slime),
            Some(&AttackTarget(player))
        );
        // Aggressive: 0.16–0.5 s on the ground before each hop.
        let waits = h.ground_waits(slime, 200);
        assert!(waits.len() >= 5, "waits {waits:?}");
        assert!(
            waits.iter().all(|w| (3..=10).contains(w)),
            "waits {waits:?}"
        );
    }

    #[test]
    fn medium_and_large_slimes_hurt_on_contact_and_small_ones_do_not() {
        for (index, &(event, _, _, _, _, damage)) in SIZES.iter().enumerate() {
            let mut h = harness(30 + index as u64, GameMode::Survival);
            h.slime(event, DVec3::new(8.5, 64.0, 0.5));
            let mut hits = Vec::new();
            let mut health = h.health();
            for tick in 0..60 {
                h.server.tick();
                let now = h.health();
                if now < health {
                    hits.push((tick, health - now));
                }
                health = now;
            }
            if damage == 0.0 {
                assert!(hits.is_empty(), "{event}: hits {hits:?}");
                continue;
            }
            assert!(hits.len() >= 2, "{event}: hits {hits:?}");
            assert!(hits[0].0 < 5, "{event}: touching hurts at once: {hits:?}");
            for pair in hits.windows(2) {
                assert!(pair[1].0 - pair[0].0 >= 10, "{event}: hits {hits:?}");
            }
            for (_, dealt) in &hits {
                assert_eq!(*dealt, damage, "{event}: hits {hits:?}");
            }
        }
    }

    #[test]
    fn dead_slimes_split_into_two_to_four_halves() {
        let mut counts = std::collections::BTreeSet::new();
        for seed in 0..16 {
            for &(event, size, _, _, _, _) in &SIZES {
                let mut h = harness(100 + seed, GameMode::Creative);
                let parent = h.slime(event, DVec3::new(0.3, 64.0, 8.7));
                h.ticks(5);
                let feet = h.world().get::<Position>(parent).unwrap().0;
                h.kill(parent, false);
                let children = h.slimes();
                if size == 1 {
                    assert!(children.is_empty(), "small slimes leave nothing");
                    continue;
                }
                assert!(
                    (2..=4).contains(&children.len()),
                    "{event}: {} children",
                    children.len()
                );
                if size == 4 {
                    counts.insert(children.len());
                }
                let world = h.world();
                for child in children {
                    assert_eq!(variant(world, child), size / 2, "{event}");
                    let health = world.get::<Health>(child).unwrap().current;
                    assert_eq!(health, if size == 4 { 4.0 } else { 1.0 }, "{event}");
                    // Children spawn at the block centre of the body, half a
                    // block up, and may have started falling.
                    let at = world.get::<Position>(child).unwrap().0;
                    let centre = feet.floor() + DVec3::new(0.5, 0.0, 0.5);
                    assert!((at.x - centre.x).abs() < 0.3, "{event}: {at} vs {feet}");
                    assert!((at.z - centre.z).abs() < 0.3, "{event}: {at} vs {feet}");
                }
            }
        }
        assert!(counts.len() >= 2, "child counts vary: {counts:?}");
    }

    #[test]
    fn only_small_slimes_drop_slimeballs() {
        for &(event, size, _, _, _, _) in &SIZES {
            let mut balls = 0;
            for seed in 0..12 {
                let mut h = harness(200 + seed, GameMode::Creative);
                let slime = h.slime(event, DVec3::new(0.5, 64.0, 8.5));
                h.kill(slime, true);
                let (drops, _) = h.take_drops();
                for (id, count) in drops {
                    assert_eq!(id, "minecraft:slime_ball", "{event}");
                    balls += count;
                }
            }
            if size == 1 {
                assert!(balls > 0, "small slimes drop slimeballs");
            } else {
                assert_eq!(balls, 0, "{event} drops nothing");
            }
        }
    }

    #[test]
    fn slimes_give_their_size_in_experience_to_players_only() {
        for &(event, size, _, _, _, _) in &SIZES {
            for by_player in [true, false] {
                let mut h = harness(300, GameMode::Creative);
                let slime = h.slime(event, DVec3::new(0.5, 64.0, 8.5));
                h.kill(slime, by_player);
                let (_, xp) = h.take_drops();
                let expected = if by_player { size as u32 } else { 0 };
                assert_eq!(xp, expected, "{event}, by player {by_player}");
            }
        }
    }
}

// Natural spawning.
mod natural_spawning {
    use bevy_ecs::prelude::*;
    use glam::IVec3;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use unastar_data::actor_identifiers::ActorType;

    use crate::entity::EntityRandom;
    use crate::entity::mob::native::{NativeTraits, SpawnClass};
    use crate::entity::spawning::NaturalSpawnRules;
    use crate::entity::spawning::rules::SpawnRuleCheck;
    use crate::entity::spawning::slime_chunk::is_slime_chunk;
    use crate::server::game::{GameServer, ServerConfig};
    use crate::storage::ChunkColumn;
    use crate::world::chunk::{Chunk, blocks};
    use crate::world::ecs::manager::chunk_entity_components;
    use crate::world::ecs::{ChunkManager, ChunkPosition};
    use crate::world::rules::WorldTime;
    use crate::world::{WorldConfig, WorldGenerator};

    const DAY_TICKS: u64 = 24_000;
    const MIDNIGHT: u64 = 18_000;
    /// Draws per sampled placement test.
    const DRAWS: usize = 400;

    /// A server with no player whose world uses `generator`.
    fn server(generator: WorldGenerator) -> GameServer {
        let mut server = GameServer::with_config(ServerConfig {
            world: WorldConfig {
                generator,
                ..WorldConfig::void_spawn_platform(0)
            },
            ..ServerConfig::default()
        });
        server
            .ecs
            .world_mut()
            .insert_resource(EntityRandom::seeded(7));
        server
    }

    /// Load chunk (`cx`, `cz`) of `biome` with a stone floor at `floor`.
    fn add_chunk(world: &mut World, (cx, cz): (i32, i32), biome: &str, floor: i16) {
        let mut chunk = Chunk::new(cx, cz);
        chunk.set_biome(
            unastar_data::biomes::get(biome)
                .unwrap_or_else(|| panic!("{biome} is a vanilla biome"))
                .id,
        );
        for lx in 0..16 {
            for lz in 0..16 {
                chunk.set_block(lx, floor, lz, *blocks::STONE);
            }
        }
        let column = ChunkColumn {
            chunk,
            actors: Vec::new(),
            removed_actors: Default::default(),
        };
        let pos = ChunkPosition::new(cx, cz);
        let (components, _) = chunk_entity_components(pos, column, true);
        let entity = world.spawn(components).id();
        world.resource_mut::<ChunkManager>().insert(pos, entity);
    }

    /// The compiled `minecraft:slime` rule's placement test; no other rule
    /// has one.
    fn slime_check(world: &World) -> SpawnRuleCheck {
        let rules = world.resource::<NaturalSpawnRules>();
        let mut slime = rules
            .entries
            .iter()
            .filter(|entry| entry.actor == Some(ActorType::Slime));
        let check = slime
            .next()
            .and_then(|entry| entry.check.clone())
            .expect("the slime rule has a placement test");
        assert!(slime.all(|entry| entry.check.as_ref() == Some(&check)));
        assert!(
            rules
                .entries
                .iter()
                .filter(|entry| entry.actor != Some(ActorType::Slime))
                .all(|entry| entry.check.is_none())
        );
        check
    }

    /// How many of [`DRAWS`] placement tests pass at `block` with raw
    /// brightness `brightness` at midnight of `day`.
    fn passes(
        world: &mut World,
        check: &SpawnRuleCheck,
        day: u64,
        block: IVec3,
        brightness: i32,
    ) -> usize {
        world.resource_mut::<WorldTime>().ticks = day * DAY_TICKS + MIDNIGHT;
        (0..DRAWS)
            .filter(|_| check.allows(world, block, brightness))
            .count()
    }

    #[test]
    fn slimes_spawn_deep_in_slime_chunks_only() {
        let mut server = server(WorldGenerator::VoidSpawnPlatform {
            platform_radius_chunks: 0,
        });
        let world = server.ecs.world_mut();
        // Chunk (-1, 0) is a slime chunk, (0, 0) is not; neither is a swamp.
        assert!(is_slime_chunk(-1, 0) && !is_slime_chunk(0, 0));
        add_chunk(world, (-1, 0), "minecraft:plains", 59);
        add_chunk(world, (0, 0), "minecraft:plains", 59);
        let check = slime_check(world);
        let slime_chunk = |y| IVec3::new(-8, y, 8);
        let plain_chunk = |y| IVec3::new(8, y, 8);
        for y in [-60, 0, 38] {
            // No light test in a slime chunk.
            for brightness in [0, 15] {
                assert_eq!(
                    passes(world, &check, 4, slime_chunk(y), brightness),
                    DRAWS,
                    "slime chunk, y {y}, brightness {brightness}"
                );
            }
            assert_eq!(passes(world, &check, 0, plain_chunk(y), 0), 0, "y {y}");
        }
        for y in [39, 45, 59] {
            assert_eq!(passes(world, &check, 0, slime_chunk(y), 0), 0, "y {y}");
        }
    }

    #[test]
    fn swamp_surface_slimes_follow_the_moon() {
        let mut server = server(WorldGenerator::VoidSpawnPlatform {
            platform_radius_chunks: 0,
        });
        let world = server.ecs.world_mut();
        add_chunk(world, (0, 0), "minecraft:swampland", 62);
        add_chunk(world, (1, 0), "minecraft:mangrove_swamp", 62);
        add_chunk(world, (0, 1), "minecraft:plains", 62);
        let check = slime_check(world);
        let swamp = IVec3::new(8, 62, 8);
        let mangrove = IVec3::new(24, 62, 8);
        // Full moon (brightness 1): every dark draw passes.
        assert_eq!(passes(world, &check, 0, swamp, 0), DRAWS);
        assert_eq!(passes(world, &check, 8, mangrove, 0), DRAWS);
        // New moon (brightness 0): none does.
        assert_eq!(passes(world, &check, 4, swamp, 0), 0);
        assert_eq!(passes(world, &check, 12, mangrove, 0), 0);
        // Half moon: about half.
        let half = passes(world, &check, 2, swamp, 0);
        assert!((140..260).contains(&half), "{half} of {DRAWS} at half moon");
        // Brightness 7 passes only when the draw in 0..8 is 7.
        let dim = passes(world, &check, 0, swamp, 7);
        assert!((20..90).contains(&dim), "{dim} of {DRAWS} at brightness 7");
        assert_eq!(passes(world, &check, 0, swamp, 8), 0, "too bright");
        // Outside the 50..=68 band, or in a biome without the slime tag.
        for y in [49, 69] {
            assert_eq!(passes(world, &check, 0, IVec3::new(8, y, 8), 0), 0, "y {y}");
        }
        for y in [50, 68] {
            assert_eq!(passes(world, &check, 0, IVec3::new(8, y, 8), 0), DRAWS);
        }
        assert_eq!(passes(world, &check, 0, IVec3::new(8, 62, 24), 0), 0);
    }

    #[test]
    fn superflat_slimes_pass_the_native_check_one_time_in_four() {
        assert_eq!(
            NativeTraits::of(ActorType::Slime).spawn_class,
            Some(SpawnClass::Slime)
        );
        let feet = IVec3::new(8, 4, 8);
        let count = |server: &GameServer, class: SpawnClass| {
            let world = server.ecs.world();
            let rules = world.resource::<NaturalSpawnRules>();
            let mut random = StdRng::seed_from_u64(3);
            (0..4_000)
                .filter(|_| {
                    rules
                        .native_blocks
                        .allows(world, class, false, feet, &mut random)
                })
                .count()
        };
        let mut flat = server(WorldGenerator::SuperFlat);
        add_chunk(flat.ecs.world_mut(), (0, 0), "minecraft:plains", 3);
        let slimes = count(&flat, SpawnClass::Slime);
        assert!((850..1_150).contains(&slimes), "{slimes} of 4000");
        assert_eq!(count(&flat, SpawnClass::Mob), 4_000);
        let mut void = server(WorldGenerator::VoidSpawnPlatform {
            platform_radius_chunks: 0,
        });
        add_chunk(void.ecs.world_mut(), (0, 0), "minecraft:plains", 3);
        assert_eq!(count(&void, SpawnClass::Slime), 4_000);
    }
}
