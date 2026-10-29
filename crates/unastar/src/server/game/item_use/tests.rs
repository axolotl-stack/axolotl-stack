use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::{McpePacket, McpePacketData};

use super::*;
use crate::entity::components::{EffectType, Effects, Hunger, Rotation};
use crate::item::test_support::stack;
use crate::registry::ItemRegistry;
use crate::server::game::attributes::Absorption;
use crate::server::game::types::ItemRegistryResource;

struct Harness {
    world: World,
    player: Entity,
    rx: tokio::sync::mpsc::Receiver<McpePacket>,
}

impl Harness {
    fn new(mode: GameMode, hotbar: &[(&str, u8)]) -> Self {
        let mut world = World::new();
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(ItemRegistryResource(Arc::new(items)));
        world.init_resource::<ItemUseQueue>();
        world.insert_resource(crate::server::game::types::ServerConfigResource(
            Default::default(),
        ));
        world.init_resource::<crate::ecs::resources::TickCounter>();
        world.insert_resource(crate::entity::item_drop::EntityRandom::seeded(7));
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let mut main = MainInventory::default();
        for (slot, (name, count)) in hotbar.iter().enumerate() {
            main.0.set_item(slot, stack(name, *count)).unwrap();
        }
        let player = world
            .spawn((
                mode,
                main,
                HeldSlot(0),
                Health::new(20.0),
                Hunger {
                    food_level: 10,
                    saturation: 0.0,
                    exhaustion: 0.0,
                },
                PlayerInput::default(),
                PlayerState::default(),
                Position(glam::DVec3::new(0.5, 65.62, 0.5)),
                Rotation::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "test".into(), None, None, tx),
            ))
            .id();
        Self { world, player, rx }
    }

    fn at_input_tick(&mut self, tick: i64) -> &mut Self {
        self.world.get_mut::<PlayerInput>(self.player).unwrap().tick = tick;
        self
    }

    fn send(&mut self, request: UseRequest) -> &mut Self {
        self.world
            .resource_mut::<ItemUseQueue>()
            .0
            .push((self.player, request));
        apply_item_use(&mut self.world);
        self
    }

    fn hunger(&self) -> (i32, f32) {
        let h = self.world.get::<Hunger>(self.player).unwrap();
        (h.food_level, h.saturation)
    }

    fn slot(&self, slot: usize) -> (String, u8) {
        let s = self
            .world
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .item(slot)
            .unwrap()
            .clone();
        (s.item_id().to_owned(), s.count)
    }

    fn using(&self) -> Option<ItemUsing> {
        self.world.get::<ItemUsing>(self.player).copied()
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        let mut out = Vec::new();
        while let Ok(packet) = self.rx.try_recv() {
            out.push(packet.data);
        }
        out
    }

    fn completed(&mut self) -> Vec<(i16, i32)> {
        self.packets()
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::CompletedUsingItemPacket(p) => Some((p.item_id, p.item_use_method)),
                _ => None,
            })
            .collect()
    }

    fn effect(&self, effect: EffectType) -> Option<(u8, u32)> {
        self.world
            .get::<Effects>(self.player)?
            .get(effect)
            .map(|e| (e.level, e.duration_ticks))
    }
}

fn network_id(name: &str) -> i16 {
    stack(name, 1).item().unwrap().network_id() as i16
}

#[test]
fn eating_completes_only_after_the_use_duration_in_input_ticks() {
    let mut h = Harness::new(GameMode::Survival, &[("minecraft:apple", 3)]);
    h.at_input_tick(100).send(UseRequest::UseAir);
    assert_eq!(h.using().map(|u| u.since), Some(100));
    // One tick short of the apple's 32 ticks (1.6 s use_modifiers).
    h.at_input_tick(131).send(UseRequest::UseAir);
    assert_eq!(h.hunger(), (10, 0.0));
    assert_eq!(h.slot(0), ("minecraft:apple".into(), 3));
    h.packets();
    h.at_input_tick(132).send(UseRequest::UseAir);
    // Apple: nutrition 4, saturation modifier 0.3 -> +2.4 saturation.
    let (food, saturation) = h.hunger();
    assert_eq!(food, 14);
    assert!((saturation - 2.4).abs() < 1e-5);
    assert_eq!(h.slot(0), ("minecraft:apple".into(), 2));
    assert_eq!(h.completed(), [(network_id("minecraft:apple"), 1)]);
    // Still holding the button: the next apple counts from now.
    assert_eq!(h.using().map(|u| u.since), Some(132));
}

#[test]
fn a_full_player_starts_eating_only_food_that_can_always_be_eaten() {
    let mut h = Harness::new(
        GameMode::Survival,
        &[("minecraft:apple", 1), ("minecraft:golden_apple", 1)],
    );
    h.world.get_mut::<Hunger>(h.player).unwrap().food_level = 20;
    h.send(UseRequest::UseAir);
    assert_eq!(h.using(), None);
    h.world.get_mut::<HeldSlot>(h.player).unwrap().0 = 1;
    h.send(UseRequest::UseAir);
    assert_eq!(h.using().map(|u| u.slot), Some(1));
}

#[test]
fn creative_players_eat_when_full_without_using_up_the_item() {
    let mut h = Harness::new(GameMode::Creative, &[("minecraft:bread", 1)]);
    h.world.get_mut::<Hunger>(h.player).unwrap().food_level = 20;
    h.at_input_tick(0).send(UseRequest::UseAir);
    h.at_input_tick(32).send(UseRequest::UseAir);
    assert_eq!(h.slot(0), ("minecraft:bread".into(), 1));
    assert_eq!(h.completed(), [(network_id("minecraft:bread"), 1)]);
}

#[test]
fn golden_apple_effects_come_from_its_food_component() {
    let mut h = Harness::new(GameMode::Survival, &[("minecraft:golden_apple", 1)]);
    h.at_input_tick(0).send(UseRequest::UseAir);
    h.at_input_tick(32).send(UseRequest::UseAir);
    // regeneration amplifier 1 for 5 s, absorption amplifier 0 for 120 s.
    assert_eq!(h.effect(EffectType::REGENERATION), Some((1, 100)));
    assert_eq!(h.effect(EffectType::ABSORPTION), Some((0, 2400)));
    assert_eq!(h.world.get::<Absorption>(h.player), Some(&Absorption(4.0)));
    assert_eq!(h.slot(0), ("".into(), 0));
}

#[test]
fn honey_removes_poison_and_the_last_bottle_becomes_a_glass_bottle_in_place() {
    let mut h = Harness::new(GameMode::Survival, &[("minecraft:honey_bottle", 1)]);
    crate::server::game::effects::add_effect(&mut h.world, h.player, EffectType::POISON, 0, 600);
    h.at_input_tick(0).send(UseRequest::UseAir);
    h.at_input_tick(40).send(UseRequest::UseAir);
    assert_eq!(h.effect(EffectType::POISON), None);
    assert_eq!(h.slot(0), ("minecraft:glass_bottle".into(), 1));
}

#[test]
fn bottles_from_a_partial_stack_go_to_another_slot() {
    let mut h = Harness::new(GameMode::Survival, &[("minecraft:honey_bottle", 2)]);
    h.at_input_tick(0).send(UseRequest::UseAir);
    h.at_input_tick(40).send(UseRequest::UseAir);
    assert_eq!(h.slot(0), ("minecraft:honey_bottle".into(), 1));
    assert_eq!(h.slot(1), ("minecraft:glass_bottle".into(), 1));
}

#[test]
fn milk_clears_every_effect_and_leaves_a_bucket() {
    let mut h = Harness::new(GameMode::Survival, &[("minecraft:milk_bucket", 1)]);
    h.world.get_mut::<Hunger>(h.player).unwrap().food_level = 20;
    for effect in [EffectType::POISON, EffectType::SPEED] {
        crate::server::game::effects::add_effect(&mut h.world, h.player, effect, 0, 600);
    }
    h.at_input_tick(0).send(UseRequest::UseAir);
    h.at_input_tick(32).send(UseRequest::UseAir);
    assert!(h.world.get::<Effects>(h.player).unwrap().active.is_empty());
    assert_eq!(h.slot(0), ("minecraft:bucket".into(), 1));
    assert_eq!(h.hunger().0, 20);
    assert_eq!(h.completed(), [(network_id("minecraft:milk_bucket"), 3)]);
}

#[test]
fn switching_slots_or_releasing_abandons_eating() {
    let mut h = Harness::new(
        GameMode::Survival,
        &[("minecraft:apple", 1), ("minecraft:apple", 1)],
    );
    h.at_input_tick(0).send(UseRequest::UseAir);
    h.world.get_mut::<HeldSlot>(h.player).unwrap().0 = 1;
    // The finishing packet arrives for a different slot: it starts over.
    h.at_input_tick(40).send(UseRequest::UseAir);
    assert_eq!(h.using().map(|u| (u.slot, u.since)), Some((1, 40)));
    assert_eq!(h.hunger().0, 10);
    h.send(UseRequest::Release);
    assert_eq!(h.using(), None);
    h.at_input_tick(80).send(UseRequest::UseAir);
    assert_eq!(h.hunger().0, 10, "a released meal does not finish");
}

#[test]
fn eaters_show_feed_events_every_four_ticks() {
    let mut h = Harness::new(GameMode::Survival, &[("minecraft:apple", 1)]);
    h.send(UseRequest::UseAir);
    h.packets();
    let mut schedule = Schedule::default();
    schedule.add_systems(tick_using);
    let mut feeds = 0;
    for tick in 1..=8u64 {
        h.world
            .resource_mut::<crate::ecs::resources::TickCounter>()
            .current = tick;
        schedule.run(&mut h.world);
        for packet in h.packets() {
            if let McpePacketData::ActorEventPacket(event) = packet {
                assert_eq!(
                    event.event_id,
                    jolyne::valentine::types::EnumsActorEvent::Feed
                );
                assert_eq!(event.data, i32::from(network_id("minecraft:apple")) << 16);
                feeds += 1;
            }
        }
    }
    assert_eq!(feeds, 2);
}

#[test]
fn every_vanilla_food_is_consumable_with_a_positive_duration() {
    let mut items = ItemRegistry::new();
    items.load_vanilla();
    let foods: Vec<_> = items
        .iter()
        .filter(|entry| entry.components().food.is_some())
        .collect();
    assert_eq!(foods.len(), 40);
    for entry in foods {
        let consumable = food::consumable(entry)
            .unwrap_or_else(|| panic!("{} is not consumable", entry.identifier()));
        assert!(consumable.duration_ticks > 0, "{}", entry.identifier());
        assert_eq!(
            i64::from(consumable.duration_ticks),
            i64::from(entry.data().max_use_duration),
            "{}: pack use duration differs from the native maxUseDuration",
            entry.identifier()
        );
    }
}
