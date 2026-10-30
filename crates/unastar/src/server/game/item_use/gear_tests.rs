//! Shields, the totem of undying, the spyglass, goat horns, elytra, firework
//! rockets and books, run through the server's systems.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    BookEditActionAddPage, BookEditActionDeletePage, BookEditActionFinalize,
    BookEditActionReplacePage, BookEditActionSwapPages, BookEditPacketOperation as Operation,
    DataItemEntryPayload, EnumsActorEvent,
};
use jolyne::valentine::{AddActorPacket, BookEditPacket, McpePacket, McpePacketData, actor_flags};
use unastar_data::actor_components::body::ActorDamageCause;
use valentine::bedrock::codec::BedrockCodec;
use zuri_nbt::{NBTTag, tag};

use super::books::{self, BookEditQueue, MAX_PAGE_CHARS, MAX_PAGES, Page};
use super::elytra::{self, Gliding};
use super::fireworks::{self, Rocket};
use super::shield::{self, ShieldRaised};
use super::{ItemUseQueue, ItemUsing, UseRequest};
use crate::entity::components::{
    ArmourInventory, EffectType, Effects, GameMode, Health, HeldSlot, Hunger, MainInventory,
    OffhandSlot, Player, PlayerInput, PlayerSession, PlayerState, Position, Rotation,
    RuntimeEntityId,
};
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::combat::{Hit, hurt_mob};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::item::ItemStack;
use crate::item::enchant::{durability_damage, item_tag, with_durability_damage, with_item_tag};
use crate::item::test_support::stack;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};

struct Field {
    server: GameServer,
    player: Entity,
    rx: tokio::sync::mpsc::Receiver<McpePacket>,
}

/// A stone floor at y = 63 over chunk (0, 0) and a survival player
/// standing at (4.5, 64, 4.5) looking south (+z) with `hotbar`.
fn field(hotbar: &[(&str, u8)]) -> Field {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(crate::entity::EntityRandom::seeded(11));
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
    let (tx, rx) = tokio::sync::mpsc::channel(1 << 16);
    let mut main = MainInventory::default();
    for (slot, (name, count)) in hotbar.iter().enumerate() {
        main.0.set_item(slot, stack(name, *count)).unwrap();
    }
    let player = world
        .spawn((
            (
                Player,
                Position(DVec3::new(
                    4.5,
                    64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                    4.5,
                )),
                Rotation::default(),
                GameMode::Survival,
                main,
                OffhandSlot::default(),
                ArmourInventory::default(),
                HeldSlot(0),
                Health::default(),
            ),
            (
                Hunger::default(),
                PlayerInput::default(),
                PlayerState::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "gear".into(), None, None, tx),
            ),
        ))
        .id();
    let mut viewers = ChunkViewers::default();
    viewers.insert(player);
    world.entity_mut(chunk).insert(viewers);
    Field { server, player, rx }
}

impl Field {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn ticks(&mut self, n: usize) {
        for _ in 0..n {
            self.server.tick();
        }
    }

    fn send(&mut self, request: UseRequest) {
        let player = self.player;
        self.world()
            .resource_mut::<ItemUseQueue>()
            .0
            .push((player, request));
        self.server.tick();
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        let mut out = Vec::new();
        while let Ok(packet) = self.rx.try_recv() {
            out.push(packet.data);
        }
        out
    }

    fn sounds(&mut self) -> Vec<String> {
        self.packets()
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::LevelSoundEventPacket(p) => Some(p.sound_event.to_string()),
                _ => None,
            })
            .collect()
    }

    fn slot(&mut self, slot: usize) -> ItemStack {
        let player = self.player;
        self.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(slot)
            .unwrap()
            .clone()
    }

    fn sneak(&mut self, on: bool) {
        let player = self.player;
        self.world()
            .get_mut::<PlayerState>(player)
            .unwrap()
            .sneaking = on;
    }

    fn health(&mut self) -> f32 {
        let player = self.player;
        self.world().get::<Health>(player).unwrap().current
    }

    /// A melee-like hit of `damage` from `from`.
    fn hit_from(&mut self, damage: f32, cause: ActorDamageCause, from: DVec3) -> f32 {
        let player = self.player;
        hurt_mob(
            self.world(),
            player,
            Hit {
                damage,
                cause,
                attacker: None,
                by_player: false,
                knockback_from: Some(from),
                knockback_power: KnockbackPower::DEFAULT,
            },
        )
    }
}

fn front() -> DVec3 {
    DVec3::new(4.5, 64.0, 7.5)
}

fn behind() -> DVec3 {
    DVec3::new(4.5, 64.0, 1.5)
}

/// The flag word `key` (0, or 92 for flags 64 and up) of every
/// `SetActorData` carrying it.
fn flag_words_at(packets: &[McpePacketData], key: u32) -> Vec<u64> {
    packets
        .iter()
        .filter_map(|p| match p {
            McpePacketData::SetActorDataPacket(p) => {
                p.actor_data.data.iter().find_map(|e| match &e.payload {
                    DataItemEntryPayload::DataItemInt64Payload(v) if e.id == key => {
                        Some(v.value as u64)
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .collect()
}

fn flag_words(packets: &[McpePacketData]) -> Vec<u64> {
    flag_words_at(packets, 0)
}

const BLOCKING_BIT: u64 = 1 << (actor_flags::BLOCKING - 64);

fn cooldowns(packets: &[McpePacketData]) -> Vec<(String, i32)> {
    packets
        .iter()
        .filter_map(|p| match p {
            McpePacketData::PlayerStartItemCooldownPacket(p) => {
                Some((p.item_category.to_string(), p.duration_ticks))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn sneaking_with_a_shield_raises_it_and_sets_the_blocking_flag() {
    let mut f = field(&[("minecraft:shield", 1)]);
    f.sneak(true);
    f.ticks(1);
    let player = f.player;
    assert!(f.world().get::<ShieldRaised>(player).is_some());
    let packets = f.packets();
    assert!(
        flag_words(&packets)
            .iter()
            .any(|w| w & (1 << actor_flags::SNEAKING) != 0)
    );
    let high = flag_words_at(&packets, 92);
    assert_eq!(high, vec![BLOCKING_BIT], "{high:x?}");
    f.sneak(false);
    f.ticks(1);
    assert!(f.world().get::<ShieldRaised>(player).is_none());
    assert_eq!(flag_words_at(&f.packets(), 92), vec![0]);
}

#[test]
fn a_raised_shield_blocks_hits_from_the_front_after_its_raise_delay() {
    let mut f = field(&[("minecraft:shield", 1)]);
    f.sneak(true);
    f.ticks(1);
    // Still raising: the hit lands.
    assert_eq!(
        f.hit_from(2.0, ActorDamageCause::EntityAttack, front()),
        2.0
    );
    f.ticks(shield::RAISE_TICKS as usize + 10);
    f.packets();
    // Blocked: no damage, the block sound, 5 points of wear.
    let before = f.health();
    assert_eq!(
        f.hit_from(5.0, ActorDamageCause::EntityAttack, front()),
        0.0
    );
    assert_eq!(f.health(), before);
    assert!(f.sounds().contains(&"item.shield.block".to_owned()));
    assert_eq!(durability_damage(&f.slot(0)), 6);
    // Small hits do not wear the shield; projectiles are blocked too.
    assert_eq!(f.hit_from(2.0, ActorDamageCause::Projectile, front()), 0.0);
    assert_eq!(durability_damage(&f.slot(0)), 6);
    // From behind, or damage a shield cannot stop, hurts.
    f.ticks(20);
    assert_eq!(
        f.hit_from(3.0, ActorDamageCause::EntityAttack, behind()),
        3.0
    );
    f.ticks(20);
    assert_eq!(f.hit_from(1.0, ActorDamageCause::Fall, front()), 1.0);
}

#[test]
fn an_offhand_shield_blocks_and_wears() {
    let mut f = field(&[("minecraft:stick", 1)]);
    let player = f.player;
    f.world().get_mut::<OffhandSlot>(player).unwrap().0 = stack("minecraft:shield", 1);
    f.sneak(true);
    f.ticks(shield::RAISE_TICKS as usize + 1);
    f.packets();
    assert_eq!(
        f.hit_from(3.5, ActorDamageCause::EntityAttack, front()),
        0.0
    );
    let offhand = f.world().get::<OffhandSlot>(player).unwrap().0.clone();
    assert_eq!(durability_damage(&offhand), 4);
    assert!(
        f.packets().iter().any(
            |p| matches!(p, McpePacketData::InventoryContentPacket(c) if c.container_id == 119)
        ),
        "offhand resent"
    );
}

#[test]
fn an_axe_hit_on_a_raised_shield_disables_shields_for_100_ticks() {
    let mut f = field(&[("minecraft:shield", 1)]);
    let world = f.world();
    let mut axeman = MainInventory::default();
    axeman
        .0
        .set_item(0, stack("minecraft:iron_axe", 1))
        .unwrap();
    let attacker = world
        .spawn((
            Player,
            Position(front() + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
            axeman,
            HeldSlot(0),
        ))
        .id();
    f.sneak(true);
    f.ticks(shield::RAISE_TICKS as usize + 1);
    f.packets();
    let player = f.player;
    let hit = Hit {
        damage: 6.0,
        cause: ActorDamageCause::EntityAttack,
        attacker: Some(attacker),
        by_player: true,
        knockback_from: Some(front()),
        knockback_power: KnockbackPower::DEFAULT,
    };
    assert_eq!(hurt_mob(f.world(), player, hit), 0.0);
    let packets = f.packets();
    assert_eq!(
        cooldowns(&packets),
        vec![(shield::COOLDOWN.to_owned(), shield::DISABLE_TICKS as i32)]
    );
    assert!(f.world().get::<ShieldRaised>(player).is_none());
    // Still sneaking, the shield stays down while disabled.
    f.ticks(50);
    assert!(f.world().get::<ShieldRaised>(player).is_none());
    assert_eq!(
        f.hit_from(2.0, ActorDamageCause::EntityAttack, front()),
        2.0
    );
    f.ticks(60);
    assert!(f.world().get::<ShieldRaised>(player).is_some());
}

#[test]
fn a_totem_cancels_a_lethal_hit() {
    let mut f = field(&[("minecraft:stick", 1)]);
    let player = f.player;
    f.world().get_mut::<OffhandSlot>(player).unwrap().0 = stack(super::totem::TOTEM, 1);
    super::super::effects::add_effect(f.world(), player, EffectType::POISON, 0, 200);
    f.world().get_mut::<Health>(player).unwrap().current = 2.0;
    f.packets();
    assert!(f.hit_from(10.0, ActorDamageCause::Magic, front()) > 0.0);
    assert_eq!(f.health(), 1.0);
    let world = f.world();
    assert!(world.get::<OffhandSlot>(player).unwrap().0.is_empty());
    let effects = world.get::<Effects>(player).unwrap();
    let effect = |e| effects.get(e).map(|e| (e.level, e.duration_ticks));
    assert_eq!(effect(EffectType::POISON), None);
    assert_eq!(effect(EffectType::REGENERATION), Some((1, 900)));
    assert_eq!(effect(EffectType::ABSORPTION), Some((1, 100)));
    assert_eq!(effect(EffectType::FIRE_RESISTANCE), Some((0, 800)));
    assert!(f.packets().iter().any(|p| matches!(
        p,
        McpePacketData::ActorEventPacket(e) if e.event_id == EnumsActorEvent::TalismanActivate
    )));
    // A hotbar totem works too and is used before the offhand.
    f.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack(super::totem::TOTEM, 1))
        .unwrap();
    f.ticks(20);
    f.hit_from(30.0, ActorDamageCause::EntityAttack, front());
    assert_eq!(f.health(), 1.0);
    assert!(f.slot(0).is_empty());
}

#[test]
fn the_void_ignores_the_totem() {
    let mut f = field(&[(super::totem::TOTEM, 1)]);
    f.hit_from(50.0, ActorDamageCause::Void, front());
    assert_eq!(f.health(), 0.0);
    assert_eq!(f.slot(0).count, 1);
}

#[test]
fn the_spyglass_is_raised_and_lowered_with_its_sounds() {
    let mut f = field(&[("minecraft:spyglass", 1)]);
    f.send(UseRequest::UseAir);
    let player = f.player;
    assert!(f.world().get::<ItemUsing>(player).is_some());
    let packets = f.packets();
    assert!(
        flag_words(&packets)
            .iter()
            .any(|w| w & (1 << actor_flags::USINGITEM) != 0)
    );
    f.ticks(40);
    assert!(
        f.world().get::<ItemUsing>(player).is_some(),
        "no time limit"
    );
    f.packets();
    f.send(UseRequest::Release);
    assert!(f.world().get::<ItemUsing>(player).is_none());
    assert!(f.sounds().is_empty(), "the client plays spyglass sounds");
    assert_eq!(f.slot(0).count, 1);
}

#[test]
fn a_goat_horn_plays_its_call_and_cools_down_every_horn() {
    let mut f = field(&[("minecraft:goat_horn", 1), ("minecraft:goat_horn", 1)]);
    let player = f.player;
    f.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .item_mut(0)
        .unwrap()
        .damage = 3;
    f.send(UseRequest::UseAir);
    let packets = f.packets();
    assert_eq!(
        cooldowns(&packets),
        vec![("minecraft:goat_horn".to_owned(), 140)]
    );
    assert!(packets.iter().any(|p| matches!(
        p,
        McpePacketData::LevelSoundEventPacket(s) if &*s.sound_event == "horn_call3"
    )));
    assert!(f.world().get::<ItemUsing>(player).is_some());
    // After the horn's 20-tick use duration: the cooldown again, completed
    // (-1) and the use ends.
    f.ticks(21);
    assert!(f.world().get::<ItemUsing>(player).is_none());
    let packets = f.packets();
    assert_eq!(
        cooldowns(&packets),
        vec![("minecraft:goat_horn".to_owned(), 140)]
    );
    assert!(packets.iter().any(|p| matches!(
        p,
        McpePacketData::CompletedUsingItemPacket(c) if c.item_use_method == -1
    )));
    // Another horn is cooling down too.
    f.world().get_mut::<HeldSlot>(player).unwrap().0 = 1;
    f.send(UseRequest::UseAir);
    assert!(f.sounds().iter().all(|s| !s.starts_with("horn_call")));
    f.ticks(150);
    f.packets();
    f.send(UseRequest::UseAir);
    assert!(f.sounds().contains(&"horn_call0".to_owned()));
    assert_eq!(f.slot(1).count, 1, "nothing is consumed");
}

fn wear_elytra(f: &mut Field, damage: i32) {
    let player = f.player;
    let elytra = with_durability_damage(&stack(elytra::ELYTRA, 1), damage);
    f.world()
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .set_chestplate(elytra)
        .unwrap();
}

fn chest(f: &mut Field) -> ItemStack {
    let player = f.player;
    f.world()
        .get::<ArmourInventory>(player)
        .unwrap()
        .chestplate()
        .clone()
}

#[test]
fn gliding_wears_the_elytra_once_a_second_and_never_breaks_it() {
    let mut f = field(&[]);
    wear_elytra(&mut f, 0);
    let player = f.player;
    f.world().get_mut::<PlayerState>(player).unwrap().gliding = true;
    f.ticks(1);
    assert!(f.world().get::<Gliding>(player).is_some());
    assert!(
        flag_words(&f.packets())
            .iter()
            .any(|w| w & (1 << actor_flags::GLIDING) != 0)
    );
    f.ticks(elytra::WEAR_INTERVAL as usize * 3);
    assert_eq!(durability_damage(&chest(&mut f)), 3);
    // One point from the end it stops gliding instead of breaking.
    wear_elytra(&mut f, 430);
    f.ticks(elytra::WEAR_INTERVAL as usize);
    assert_eq!(durability_damage(&chest(&mut f)), 431);
    f.ticks(1);
    assert!(f.world().get::<Gliding>(player).is_none());
    assert!(!f.world().get::<PlayerState>(player).unwrap().gliding);
    assert_eq!(chest(&mut f).count, 1);
}

#[test]
fn gliding_without_an_elytra_is_dropped() {
    let mut f = field(&[]);
    let player = f.player;
    f.world().get_mut::<PlayerState>(player).unwrap().gliding = true;
    f.ticks(1);
    assert!(f.world().get::<Gliding>(player).is_none());
    assert!(!f.world().get::<PlayerState>(player).unwrap().gliding);
}

fn rocket_stack(flight: i8) -> ItemStack {
    let mut fireworks = std::collections::HashMap::new();
    fireworks.insert("Flight".to_owned(), NBTTag::Byte(tag::Byte(flight as u8)));
    let mut explosion = std::collections::HashMap::new();
    explosion.insert("FireworkType".to_owned(), NBTTag::Byte(tag::Byte(1)));
    explosion.insert("FireworkFlicker".to_owned(), NBTTag::Byte(tag::Byte(1)));
    fireworks.insert(
        "Explosions".to_owned(),
        NBTTag::List(tag::List(vec![NBTTag::Compound(tag::Compound(explosion))])),
    );
    let mut compound = std::collections::HashMap::new();
    compound.insert(
        "Fireworks".to_owned(),
        NBTTag::Compound(tag::Compound(fireworks)),
    );
    let mut rocket = with_item_tag(&stack(fireworks::FIREWORK, 3), tag::Compound(compound));
    rocket.count = 3;
    rocket
}

fn rockets(world: &mut World) -> Vec<(Entity, Rocket)> {
    world
        .query::<(Entity, &Rocket)>()
        .iter(world)
        .map(|(e, r)| (e, r.clone()))
        .collect()
}

#[test]
fn a_rocket_used_while_gliding_boosts_the_player_until_it_explodes() {
    let mut f = field(&[]);
    wear_elytra(&mut f, 0);
    let player = f.player;
    f.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, rocket_stack(0))
        .unwrap();
    // Not gliding: nothing happens.
    f.send(UseRequest::UseAir);
    assert!(rockets(f.world()).is_empty());
    f.world().get_mut::<PlayerState>(player).unwrap().gliding = true;
    f.ticks(1);
    f.packets();
    f.send(UseRequest::UseAir);
    let launched = rockets(f.world());
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0].1.attached, Some(player));
    assert_eq!(f.slot(0).count, 2);
    let metadata = f.world().get::<EntityMetadata>(launched[0].0).unwrap();
    assert!(matches!(
        metadata.get(jolyne::valentine::actor_data_i_ds::CUSTOM_DISPLAY),
        Some(DataItemEntryPayload::DataItemInt64Payload(v)) if v.value == 1
    ));
    let rocket = launched[0].0;
    assert!(matches!(
        metadata.get(jolyne::valentine::actor_data_i_ds::DISPLAY_OFFSET),
        Some(DataItemEntryPayload::DataItemVec3Payload(v)) if v.value.y == 0.0
    ));
    // The player's client boosts itself: the rocket only follows.
    f.world().get_mut::<Position>(player).unwrap().0 += DVec3::new(0.0, 0.0, 2.0);
    f.ticks(3);
    assert!(
        !f.packets()
            .iter()
            .any(|p| matches!(p, McpePacketData::SetActorMotionPacket(m) if m.target_runtime_id.actor_runtime_id == 1))
    );
    assert_eq!(f.world().get::<Position>(rocket).unwrap().0.z, 6.5);
    f.ticks(22);
    assert!(rockets(f.world()).is_empty(), "lives at most 21 ticks");
}

#[test]
fn a_rocket_used_on_a_block_rises_and_explodes_with_its_sounds() {
    let mut f = field(&[]);
    let player = f.player;
    f.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, rocket_stack(1))
        .unwrap();
    assert!(super::on_block::use_on_block(
        f.world(),
        player,
        IVec3::new(4, 63, 6),
        1
    ));
    assert_eq!(f.slot(0).count, 2);
    let launched = rockets(f.world());
    assert_eq!(launched.len(), 1);
    let rocket = launched[0].0;
    assert_eq!(
        f.world().get::<Position>(rocket).unwrap().0,
        DVec3::new(4.5, 64.0, 6.5)
    );
    assert!(f.sounds().contains(&"launch".to_owned()));
    f.ticks(10);
    let y = f.world().get::<Position>(rocket).unwrap().0.y;
    // Measured climb: about 2 blocks in the first 10 ticks.
    assert!((65.9..66.2).contains(&y), "{y}");
    f.ticks(22);
    assert!(
        rockets(f.world()).is_empty(),
        "flight 1 lives at most 31 ticks"
    );
    let packets = f.packets();
    assert!(packets.iter().any(|p| matches!(
        p,
        McpePacketData::ActorEventPacket(e) if e.event_id == EnumsActorEvent::FireworksExplode
    )));
    let sounds: Vec<_> = packets
        .iter()
        .filter_map(|p| match p {
            McpePacketData::LevelSoundEventPacket(s) => Some(s.sound_event.to_string()),
            _ => None,
        })
        .collect();
    assert!(sounds.contains(&"large.blast".to_owned()), "{sounds:?}");
    assert!(sounds.contains(&"twinkle".to_owned()), "{sounds:?}");
}

#[test]
fn rocket_lifetime_follows_the_flight_byte() {
    let mut rng = rand::rngs::mock::StepRng::new(0, 0);
    assert_eq!(fireworks::lifetime(0, &mut rng), 10);
    assert_eq!(fireworks::lifetime(2, &mut rng), 30);
    assert_eq!(fireworks::flight(&rocket_stack(3)), 3);
    assert_eq!(fireworks::flight(&stack(fireworks::FIREWORK, 1)), 0);
}

/// Our rocket matches the AddActor BDS 1.26.51 sends for a summoned rocket.
#[test]
fn summoned_rocket_add_actor_matches_native_summon() {
    let mut f = field(&[]);
    let world = f.world();
    let native = {
        let bytes = include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/fireworks_rocket.bin"
        );
        assert_eq!(bytes[0], 0x0d);
        let mut input = &bytes[1..];
        let packet = AddActorPacket::decode(&mut input, ()).unwrap();
        assert!(input.is_empty());
        packet
    };
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let rocket = spawn_mob(world, fireworks::ROCKET, feet, 0.0, SpawnCause::Spawned).unwrap();
    fireworks::set_rocket_data(
        &mut world.get_mut::<EntityMetadata>(rocket).unwrap(),
        None,
        -1,
    );
    let ours = add_actor_packet(
        fireworks::ROCKET,
        world
            .get::<crate::entity::components::RuntimeId>(rocket)
            .unwrap()
            .0,
        world.get::<Position>(rocket).unwrap().0,
        DVec3::new(
            f64::from(native.velocity.x),
            f64::from(native.velocity.y),
            f64::from(native.velocity.z),
        ),
        world.get::<Rotation>(rocket).unwrap(),
        world.get::<EntityMetadata>(rocket).unwrap(),
        world.get::<ActorProperties>(rocket).unwrap(),
        world.get::<ActorAttributes>(rocket).unwrap(),
    );
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.position, native.position);
    assert_eq!(ours.rotation, native.rotation);
    assert_eq!(ours.attributes_list, native.attributes_list);
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.synched_properties, native.synched_properties);
}

fn op_replace(index: i32, text: &str) -> Operation {
    Operation::ReplacePage(BookEditActionReplacePage {
        page_index: index,
        page_text: text.into(),
        photo_name: "".into(),
    })
}

fn texts(book: &ItemStack) -> Vec<String> {
    books::pages(book).into_iter().map(|p| p.text).collect()
}

#[test]
fn book_pages_are_replaced_added_deleted_and_swapped() {
    let f = field(&[]);
    let world = f.server.ecs.world();
    let book = stack(books::WRITABLE_BOOK, 1);
    let book = books::edit(&book, &op_replace(2, "c"), world).unwrap();
    assert_eq!(texts(&book), ["", "", "c"]);
    let book = books::edit(
        &book,
        &Operation::AddPage(BookEditActionAddPage {
            page_index: 0,
            page_text: "a".into(),
            photo_name: "".into(),
        }),
        world,
    )
    .unwrap();
    assert_eq!(texts(&book), ["a", "", "", "c"]);
    let book = books::edit(
        &book,
        &Operation::SwapPages(BookEditActionSwapPages {
            page_index: 0,
            swap_with_index: 3,
        }),
        world,
    )
    .unwrap();
    assert_eq!(texts(&book), ["c", "", "", "a"]);
    let book = books::edit(
        &book,
        &Operation::DeletePage(BookEditActionDeletePage { page_index: 1 }),
        world,
    )
    .unwrap();
    assert_eq!(texts(&book), ["c", "", "a"]);
    assert_eq!(
        books::pages(&book)[0],
        Page {
            text: "c".into(),
            photo: String::new()
        }
    );
}

#[test]
fn book_edits_beyond_the_limits_are_rejected() {
    let f = field(&[]);
    let world = f.server.ecs.world();
    let book = stack(books::WRITABLE_BOOK, 1);
    assert!(books::edit(&book, &op_replace(MAX_PAGES as i32, "x"), world).is_none());
    assert!(books::edit(&book, &op_replace(-1, "x"), world).is_none());
    let long = "x".repeat(MAX_PAGE_CHARS + 1);
    assert!(books::edit(&book, &op_replace(0, &long), world).is_none());
    let full = "é".repeat(MAX_PAGE_CHARS);
    assert!(books::edit(&book, &op_replace(0, &full), world).is_some());
    let swap = Operation::SwapPages(BookEditActionSwapPages {
        page_index: 0,
        swap_with_index: 1,
    });
    assert!(books::edit(&book, &swap, world).is_none(), "missing pages");
    let delete = Operation::DeletePage(BookEditActionDeletePage { page_index: 0 });
    assert!(books::edit(&book, &delete, world).is_none());
    let sign = |title: &str| {
        Operation::Finalize(BookEditActionFinalize {
            title: title.into(),
            author: "me".into(),
            xuid: "".into(),
        })
    };
    assert!(books::edit(&book, &sign(&"t".repeat(17)), world).is_none());
    assert!(books::edit(&stack("minecraft:book", 1), &op_replace(0, "x"), world).is_none());
}

#[test]
fn signing_turns_the_book_into_a_written_book() {
    let mut f = field(&[("minecraft:stick", 1), (books::WRITABLE_BOOK, 1)]);
    let player = f.player;
    let edits = [
        op_replace(0, "hello"),
        Operation::Finalize(BookEditActionFinalize {
            title: "Diary".into(),
            author: "Someone".into(),
            xuid: "2535400000000000".into(),
        }),
    ];
    for operation in edits {
        f.world().get_resource_or_init::<BookEditQueue>().0.push((
            player,
            BookEditPacket {
                book_slot: 1,
                operation,
            },
        ));
    }
    f.ticks(1);
    let written = f.slot(1);
    assert_eq!(written.item_id(), books::WRITTEN_BOOK);
    assert_eq!(texts(&written), ["hello"]);
    let compound = item_tag(&written).unwrap();
    let string = |key: &str| match compound.0.get(key) {
        Some(NBTTag::String(s)) => s.0.clone(),
        other => panic!("{key}: {other:?}"),
    };
    assert_eq!(string("title"), "Diary");
    assert_eq!(string("author"), "Someone");
    assert_eq!(string("xuid"), "2535400000000000");
    assert!(matches!(compound.0.get("generation"), Some(NBTTag::Int(g)) if g.0 == 0));
    assert!(f.packets().iter().any(|p| matches!(
        p,
        McpePacketData::InventorySlotPacket(s) if s.slot == 1
    )));
    // A signed book cannot be edited.
    f.world().get_resource_or_init::<BookEditQueue>().0.push((
        player,
        BookEditPacket {
            book_slot: 1,
            operation: op_replace(0, "changed"),
        },
    ));
    f.ticks(1);
    assert_eq!(texts(&f.slot(1)), ["hello"]);
}
