//! Experience orbs (`minecraft:xp_orb`) and player experience.
//!
//! [`spawn_experience`] splits an amount into native orb sizes and spawns
//! one orb actor per part. Orbs fall and slide under block physics, fly
//! towards the nearest collecting player within range, are collected when
//! they touch the player (without merging into each other) and expire after
//! [`constants::XP_ORB_LIFETIME_TICKS`]. Collecting adds the orb's value to
//! the player's [`Experience`] and sends the player its new level and
//! progress attributes.
//!
//! Viewers see an orb through `server::broadcast::sync_actor_viewers`
//! (`AddActor` built by [`add_actor_packet`], then absolute moves and
//! motion like other non-living actors).

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, ActorUniqueId, AttributeData, DataItemEntryPayload, DataItemIntPayload,
    DataItemStringPayload, EnumsDataItemType, PlayerInputTick, Vec2, Vec3,
};
use jolyne::valentine::{
    AddActorPacket, McpePacket, UpdateAttributesPacket, actor_data_i_ds as ids, actor_flags,
};
use rand::Rng;

use crate::entity::components::{
    ActorViewers, Age, Experience, GameMode, GravityStage, GroundFriction, Health, OnGround,
    PhysicsBody, PhysicsModel, Player, PlayerInput, PlayerSession, Position, Rotation,
    RuntimeEntityId, RuntimeId, Velocity,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::item_drop::{EntityRandom, PLAYER_EYE_HEIGHT, next_actor_id};
use crate::entity::mob::constants;
use crate::registry::collision::Aabb;
use crate::server::game::{PICKUP_REACH, player_box};

/// An experience orb worth `value` points.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct XpOrb {
    pub value: u32,
}

/// Orb body: the 0.25 collision box of the vanilla `xp_orb` definition,
/// fed to the wire at its feet; gravity before the move and drag per
/// [`constants`].
pub const XP_ORB_BODY: PhysicsBody = PhysicsBody {
    model: PhysicsModel::Generic,
    width: 0.25,
    height: 0.25,
    network_y_offset: 0.0,
    gravity: constants::XP_ORB_GRAVITY,
    gravity_stage: GravityStage::BeforeMove,
    vertical_drag: constants::XP_ORB_DRAG,
    horizontal_drag: constants::XP_ORB_DRAG,
    ground_friction: GroundFriction::Block,
    step_height: 0.0,
    wing_flap_drag: None,
};

/// `minecraft:buoyant` of the vanilla orb as BDS serialises it into the
/// orb's buoyancy actor data.
const XP_ORB_BUOYANCY: &str = "{\"apply_gravity\":false,\"base_buoyancy\":1.0,\"big_wave_probability\":0.02999999932944775,\"big_wave_speed\":10.0,\"can_auto_step_from_liquid\":false,\"drag_down_on_buoyancy_removed\":0.0,\"liquid_blocks\":[\"minecraft:flowing_water\",\"minecraft:water\"],\"movement_type\":\"waves\"}\n";

/// The orb values `amount` splits into: repeatedly the largest native size
/// that fits.
pub fn orb_values(mut amount: u32) -> impl Iterator<Item = u32> {
    std::iter::from_fn(move || {
        let size = constants::XP_ORB_SIZES
            .into_iter()
            .find(|size| *size <= amount)?;
        amount -= size;
        Some(size)
    })
}

/// Spawn `amount` experience as orbs at `position` (feet).
pub fn spawn_experience(world: &mut World, position: DVec3, amount: u32) {
    for value in orb_values(amount) {
        spawn_orb(world, position, value);
    }
}

/// Spawn one orb worth `value` with a random scatter velocity.
pub fn spawn_orb(world: &mut World, position: DVec3, value: u32) -> Entity {
    let velocity = {
        let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
        let horizontal = constants::XP_ORB_SPAWN_HORIZONTAL;
        DVec3::new(
            rng.gen_range(-horizontal..horizontal),
            rng.gen_range(0.0..constants::XP_ORB_SPAWN_VERTICAL),
            rng.gen_range(-horizontal..horizontal),
        )
    };
    let runtime_id = next_actor_id(world);
    world
        .spawn((
            XpOrb { value },
            Position(position),
            Velocity(velocity),
            Rotation::default(),
            OnGround(false),
            XP_ORB_BODY,
            RuntimeId(runtime_id),
            Age(0),
            ActorViewers::default(),
        ))
        .id()
}

/// Actor data of an orb: the rows BDS 1.26.51 sends for a summoned orb
/// (retained capture `operator/summons/xp_orb.bin`), with the orb value in
/// `VALUE`. It is the non-mob actor base plus the orb's box, flags and
/// buoyancy.
pub fn xp_orb_metadata(value: u32) -> EntityMetadata {
    let mut flags = ActorFlags::default();
    for flag in [
        actor_flags::HAS_COLLISION,
        actor_flags::HAS_GRAVITY,
        actor_flags::USES_LEGACY_FRICTION,
    ] {
        flags.update(flag, true).expect("catalogue flag index");
    }
    let words = flags.snapshot();
    let mut metadata = EntityMetadata::actor_base();
    metadata.set_long(ids::RESERVED_0, words.words()[0] as i64);
    metadata.set_long(ids::RESERVED_092, words.words()[1] as i64);
    metadata.set_payload(
        ids::VALUE,
        DataItemEntryPayload::DataItemIntPayload(DataItemIntPayload {
            type_: EnumsDataItemType::Int,
            value: i32::try_from(value).unwrap_or(i32::MAX),
        }),
    );
    metadata.set_float(ids::RESERVED_053, XP_ORB_BODY.width as f32);
    metadata.set_float(ids::RESERVED_054, XP_ORB_BODY.height as f32);
    metadata.set_byte(ids::IS_BUOYANT, 1);
    metadata.set_payload(
        ids::BUOYANCY_DATA,
        DataItemEntryPayload::DataItemStringPayload(DataItemStringPayload {
            type_: EnumsDataItemType::String,
            value: XP_ORB_BUOYANCY.into(),
        }),
    );
    metadata.mark_clean();
    metadata
}

/// The `AddActor` a new viewer of an orb receives.
pub fn add_actor_packet(
    runtime_id: i64,
    position: DVec3,
    velocity: DVec3,
    orb: &XpOrb,
) -> AddActorPacket {
    let wire = |v: DVec3| Vec3 {
        x: v.x as f32,
        y: v.y as f32,
        z: v.z as f32,
    };
    AddActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: runtime_id,
        },
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        actor_type: "minecraft:xp_orb".into(),
        position: wire(position),
        velocity: wire(velocity),
        rotation: Vec2 { x: 0.0, y: 0.0 },
        y_head_rotation: 0.0,
        y_body_rotation: 0.0,
        attributes_list: Vec::new(),
        actor_data: xp_orb_metadata(orb.value).snapshot(),
        synched_properties: Default::default(),
        actor_links: Vec::new(),
    }
}

/// Points needed to go from `level` to the next level.
pub fn points_to_next_level(level: i32) -> u32 {
    let level = level.max(0) as u32;
    match level {
        0..=15 => 2 * level + 7,
        16..=30 => 5 * level - 38,
        _ => 9 * level - 158,
    }
}

/// Add `points` to `experience`, levelling up as often as they allow.
pub fn add_experience(experience: &mut Experience, points: u32) {
    let mut needed = points_to_next_level(experience.level);
    let mut total =
        f64::from(experience.progress.clamp(0.0, 1.0)) * f64::from(needed) + f64::from(points);
    while total >= f64::from(needed) {
        total -= f64::from(needed);
        experience.level = experience.level.saturating_add(1);
        needed = points_to_next_level(experience.level);
    }
    experience.progress = (total / f64::from(needed)) as f32;
}

/// The player's level and progress attributes, as the join sequence and
/// experience changes send them.
pub fn experience_attributes(experience: &Experience) -> [AttributeData; 2] {
    let attribute = |name: &str, current: f32, max: f32| AttributeData {
        min_value: 0.0,
        max_value: max,
        current_value: current,
        default_min_value: 0.0,
        default_max_value: max,
        default_value: 0.0,
        name: name.into(),
        modifiers: Vec::new(),
    };
    [
        attribute(
            "minecraft:player.level",
            experience.level as f32,
            i32::MAX as f32,
        ),
        attribute("minecraft:player.experience", experience.progress, 1.0),
    ]
}

fn collects(mode: &GameMode, health: Option<&Health>) -> bool {
    *mode != GameMode::Spectator && health.is_none_or(|h| !h.is_dead())
}

/// System: expire orbs, pull each towards the nearest collecting player in
/// range and let that player collect it once it touches them.
///
/// The pull aims at the middle of the player's eye height: with `d` the
/// offset divided by the range, the orb gains
/// `unit(d) × (1 − |d|)² × XP_ORB_FOLLOW_ACCELERATION` per tick.
#[allow(clippy::type_complexity)]
pub fn tick_xp_orbs(
    mut commands: Commands,
    mut orbs: Query<(Entity, &XpOrb, &Position, &mut Velocity, &Age)>,
    mut players: Query<
        (
            Entity,
            &Position,
            &GameMode,
            Option<&Health>,
            &mut Experience,
            &RuntimeEntityId,
            Option<&PlayerSession>,
            Option<&PlayerInput>,
        ),
        (
            With<Player>,
            Without<crate::server::game::player_events::PendingInitialState>,
        ),
    >,
) {
    let range_sq = constants::XP_ORB_FOLLOW_RANGE * constants::XP_ORB_FOLLOW_RANGE;
    for (orb, xp, position, mut velocity, age) in &mut orbs {
        if age.0 >= constants::XP_ORB_LIFETIME_TICKS {
            commands.entity(orb).despawn();
            continue;
        }
        let nearest = players
            .iter()
            .filter(|(_, _, mode, health, ..)| collects(mode, *health))
            .map(|(player, eye, ..)| {
                let aim = eye.0 - DVec3::new(0.0, PLAYER_EYE_HEIGHT / 2.0, 0.0);
                (player, eye.0, aim, aim.distance_squared(position.0))
            })
            .filter(|(.., distance_sq)| *distance_sq <= range_sq)
            .min_by(|a, b| a.3.total_cmp(&b.3));
        let Some((player, eye, aim, _)) = nearest else {
            continue;
        };
        let orb_box = Aabb::from_feet(position.0, XP_ORB_BODY.width, XP_ORB_BODY.height);
        if player_box(eye).inflate(PICKUP_REACH).intersects(&orb_box) {
            let Ok((_, _, _, _, mut experience, runtime_id, session, input)) =
                players.get_mut(player)
            else {
                continue;
            };
            add_experience(&mut experience, xp.value);
            if let Some(session) = session {
                let _ = session.send(McpePacket::from(UpdateAttributesPacket {
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: runtime_id.0 as u64,
                    },
                    attribute_list: experience_attributes(&experience).into(),
                    tick: input.map_or(PlayerInputTick { inputtick: 0 }, PlayerInput::wire_tick),
                }));
            }
            commands.entity(orb).despawn();
            continue;
        }
        let pull = (aim - position.0) / constants::XP_ORB_FOLLOW_RANGE;
        let strength = 1.0 - pull.length();
        if strength > 0.0 {
            velocity.0 += pull.normalize_or_zero()
                * strength
                * strength
                * constants::XP_ORB_FOLLOW_ACCELERATION;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::game::{GameServer, ServerConfig};
    use crate::world::ChunkManager;
    use crate::world::chunk::{Chunk, blocks};
    use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};
    use jolyne::valentine::McpePacketData;
    use valentine::bedrock::codec::BedrockCodec;

    #[test]
    fn amounts_split_into_the_largest_native_orb_sizes() {
        assert_eq!(orb_values(0).collect::<Vec<_>>(), Vec::<u32>::new());
        assert_eq!(orb_values(3).collect::<Vec<_>>(), [3]);
        assert_eq!(orb_values(10).collect::<Vec<_>>(), [7, 3]);
        assert_eq!(orb_values(2478).collect::<Vec<_>>(), [2477, 1]);
        assert_eq!(orb_values(50).collect::<Vec<_>>(), [37, 7, 3, 3]);
    }

    #[test]
    fn experience_levels_up_on_the_vanilla_curve() {
        let mut xp = Experience::default();
        add_experience(&mut xp, 7);
        assert_eq!((xp.level, xp.progress), (1, 0.0));
        add_experience(&mut xp, 3);
        assert_eq!(xp.level, 1);
        assert!((xp.progress - 3.0 / 9.0).abs() < 1e-6);
        // Level 0 → 16 takes 352 points; level 16 → 17 another 42.
        let mut xp = Experience::default();
        add_experience(&mut xp, 352 + 21);
        assert_eq!(xp.level, 16);
        assert!((xp.progress - 0.5).abs() < 1e-6);
        assert_eq!(points_to_next_level(30), 112);
    }

    #[test]
    fn orb_add_actor_matches_the_native_summon() {
        let bytes =
            include_bytes!("../../../bds-extractor/captures/1.26.51/operator/summons/xp_orb.bin");
        assert_eq!(bytes[0], 0x0d);
        let mut input = &bytes[1..];
        let native = AddActorPacket::decode(&mut input, ()).unwrap();
        let ours = add_actor_packet(
            7,
            DVec3::new(105.5, 136.0, -25.5),
            DVec3::ZERO,
            &XpOrb { value: 1 },
        );
        assert_eq!(ours.actor_type, native.actor_type);
        assert_eq!(ours.position, native.position);
        assert_eq!(ours.attributes_list, native.attributes_list);
        assert_eq!(ours.actor_data, native.actor_data);
        assert_eq!(ours.synched_properties, native.synched_properties);
        // The native scatter velocity is within the spawn ranges.
        let v = native.velocity;
        assert!(f64::from(v.x).abs() < constants::XP_ORB_SPAWN_HORIZONTAL);
        assert!(f64::from(v.z).abs() < constants::XP_ORB_SPAWN_HORIZONTAL);
        assert!((0.0..constants::XP_ORB_SPAWN_VERTICAL).contains(&f64::from(v.y)));
    }

    fn server_with_player(
        eye: DVec3,
    ) -> (GameServer, Entity, tokio::sync::mpsc::Receiver<McpePacket>) {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(EntityRandom::seeded(3));
        let mut column = Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                column.set_block(x, 63, z, *blocks::STONE);
            }
        }
        let chunk = world.spawn(ChunkData::new(column)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        let player = world
            .spawn((
                Player,
                Position(eye),
                Rotation::default(),
                GameMode::Survival,
                Health::default(),
                Experience::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "collector".into(), None, None, tx),
            ))
            .id();
        let mut viewers = ChunkViewers::default();
        viewers.insert(player);
        world.entity_mut(chunk).insert(viewers);
        (server, player, rx)
    }

    #[test]
    fn orbs_fly_to_a_nearby_player_who_collects_their_experience() {
        let (mut server, player, mut rx) =
            server_with_player(DVec3::new(8.5, 64.0 + PLAYER_EYE_HEIGHT, 8.5));
        let world = server.ecs.world_mut();
        spawn_experience(world, DVec3::new(12.5, 64.0, 8.5), 10);
        let mut packets = Vec::new();
        for _ in 0..200 {
            server.tick();
            packets.extend(std::iter::from_fn(|| rx.try_recv().ok().map(|p| p.data)));
        }
        let world = server.ecs.world_mut();
        assert_eq!(
            world.query::<&XpOrb>().iter(world).count(),
            0,
            "both orbs collected"
        );
        let xp = world.get::<Experience>(player).unwrap();
        assert_eq!(xp.level, 1);
        assert!((xp.progress - 3.0 / 9.0).abs() < 1e-6);
        let adds = packets
            .iter()
            .filter(|p| {
                matches!(p, McpePacketData::AddActorPacket(add)
                if &*add.actor_type == "minecraft:xp_orb")
            })
            .count();
        assert_eq!(adds, 2);
        let level = packets.iter().rev().find_map(|p| match p {
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == 1 =>
            {
                update
                    .attribute_list
                    .iter()
                    .find(|a| &*a.name == "minecraft:player.level")
                    .map(|a| a.current_value)
            }
            _ => None,
        });
        assert_eq!(level, Some(1.0));
        assert_eq!(
            packets
                .iter()
                .filter(|p| matches!(p, McpePacketData::RemoveActorPacket(_)))
                .count(),
            2
        );
    }

    #[test]
    fn orbs_without_a_player_in_range_stay_and_expire() {
        let eye = DVec3::new(0.5, 64.0 + PLAYER_EYE_HEIGHT, 0.5);
        let (mut server, player, _rx) = server_with_player(eye);
        let world = server.ecs.world_mut();
        spawn_experience(world, DVec3::new(12.5, 64.0, 12.5), 1);
        for _ in 0..40 {
            server.tick();
        }
        let world = server.ecs.world_mut();
        let (orb, position) = world
            .query::<(Entity, &Position, &XpOrb)>()
            .iter(world)
            .map(|(orb, position, _)| (orb, position.0))
            .next()
            .expect("the orb is still in the world");
        // The scatter carried it a few blocks at most: still out of range.
        assert!(
            position.distance(eye) > constants::XP_ORB_FOLLOW_RANGE,
            "{position}"
        );
        assert_eq!(world.get::<Experience>(player).unwrap().level, 0);
        world.get_mut::<Age>(orb).unwrap().0 = constants::XP_ORB_LIFETIME_TICKS - 1;
        server.tick();
        let world = server.ecs.world_mut();
        assert!(world.get_entity(orb).is_err(), "expired");
    }
}
