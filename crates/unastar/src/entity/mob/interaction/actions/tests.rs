use super::*;
use glam::DVec3;
use jolyne::valentine::types::{ActorRuntimeId, EnumsActorEvent, Vec3};
use jolyne::valentine::{McpePacket, McpePacketData};
use tokio::sync::mpsc::{self, Receiver};

use crate::entity::components::{
    DroppedItem, GameMode, Health, Hunger, ItemEntityStack, MainInventory, Position,
};
use crate::entity::mob::state::{Dead, LastHurt};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::item::enchant::durability_damage;
use crate::item::test_support::stack;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::world::rules::Difficulty;

/// A joined survival player and its outbound packet channel.
fn join(server: &mut GameServer, id: u64, feet: DVec3) -> (Entity, Receiver<McpePacket>) {
    let (outbound_tx, rx) = mpsc::channel(4096);
    let entity = server
        .spawn_player(PlayerSpawnData {
            session_id: id,
            display_name: format!("Player{id}"),
            xuid: None,
            uuid: None,
            runtime_id: id as i64,
            position: feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
            outbound_tx,
            saved_data: None,
            chunk_radius: 1,
        })
        .unwrap();
    (entity, rx)
}

struct Harness {
    server: GameServer,
    attacker: Entity,
    target: Entity,
    _attacker_rx: Receiver<McpePacket>,
    target_rx: Receiver<McpePacket>,
}

impl Harness {
    /// Two players two blocks apart, the attacker holding a diamond sword.
    fn new(difficulty: Difficulty) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            difficulty,
            ..ServerConfig::default()
        });
        let (attacker, attacker_rx) = join(&mut server, 7, DVec3::new(4.5, 16.0, 4.5));
        let (target, target_rx) = join(&mut server, 8, DVec3::new(6.5, 16.0, 4.5));
        server
            .ecs
            .world_mut()
            .get_mut::<MainInventory>(attacker)
            .unwrap()
            .0
            .set_item(0, stack("minecraft:diamond_sword", 1))
            .unwrap();
        let mut h = Self {
            server,
            attacker,
            target,
            _attacker_rx: attacker_rx,
            target_rx,
        };
        h.packets();
        h
    }

    /// The shared world containing both joined players.
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// Queue an attack carrying forged client positions and an empty item;
    /// only the runtime ID and selected slot may identify the action.
    fn queue(&mut self, runtime_id: u64, slot: i32) {
        let attacker = self.attacker;
        self.world().resource_mut::<MobActionQueue>().0.push((
            attacker,
            ItemUseOnActorInventoryTransaction {
                runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime_id,
                },
                action_type: ActionType::Attack,
                slot,
                from_position: Vec3 {
                    x: 1000.0,
                    y: 1000.0,
                    z: 1000.0,
                },
                ..Default::default()
            },
        ));
    }

    /// Apply queued actor transactions through the production dispatcher.
    fn apply(&mut self) {
        apply_mob_actions(self.world());
    }

    /// All packets addressed to the target since the previous drain.
    fn packets(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.target_rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    /// The attacker's exhaustion and held-sword wear.
    fn attack_cost(&mut self) -> (f32, i32) {
        let attacker = self.attacker;
        let world = self.world();
        (
            world.get::<Hunger>(attacker).unwrap().exhaustion,
            durability_damage(
                world
                    .get::<MainInventory>(attacker)
                    .unwrap()
                    .hotbar(0)
                    .unwrap(),
            ),
        )
    }
}

#[test]
fn queued_melee_hits_players_with_server_owned_damage_and_knockback() {
    for difficulty in [Difficulty::Peaceful, Difficulty::Hard] {
        let mut h = Harness::new(difficulty);
        h.queue(8, 0);
        h.apply();
        let (target, attacker) = (h.target, h.attacker);
        assert_eq!(h.world().get::<Health>(target).unwrap().current, 12.0);
        assert_eq!(
            h.world().get::<LastHurt>(target).unwrap().attacker,
            Some(attacker)
        );
        let cost = h.attack_cost();
        assert!(cost.0 > 0.0);
        assert_eq!(cost.1, 1);
        let packets = h.packets();
        assert!(packets.iter().any(|packet| matches!(packet,
            McpePacketData::ActorEventPacket(event)
                if event.event_id == EnumsActorEvent::Hurt)));
        assert!(packets.iter().any(|packet| matches!(packet,
            McpePacketData::SetActorMotionPacket(motion)
                if motion.motion.x > 0.0 && motion.motion.y > 0.0)));
    }
}

#[test]
fn lethal_queued_melee_drops_items_and_starts_death_only_once() {
    let mut h = Harness::new(Difficulty::Normal);
    let target = h.target;
    h.world().get_mut::<Health>(target).unwrap().current = 1.0;
    h.world()
        .get_mut::<MainInventory>(target)
        .unwrap()
        .0
        .set_item(0, stack("minecraft:diamond", 4))
        .unwrap();
    h.queue(8, 0);
    h.queue(8, 0);
    h.apply();
    assert!(h.world().get::<Dead>(target).is_some());
    assert_eq!(h.world().get::<Health>(target).unwrap().current, 0.0);
    assert_eq!(
        h.attack_cost().1,
        1,
        "the repeated attack costs no second wear"
    );
    let world = h.world();
    let diamonds: u32 = world
        .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
        .iter(world)
        .filter(|item| item.0.item_id() == "minecraft:diamond")
        .map(|item| u32::from(item.0.count))
        .sum();
    assert_eq!(diamonds, 4);
    let packets = h.packets();
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet,
        McpePacketData::ActorEventPacket(event)
            if event.event_id == EnumsActorEvent::Death))
            .count(),
        1
    );
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet, McpePacketData::RespawnPacket(_)))
            .count(),
        2
    );
}

#[test]
fn rejected_targets_and_mismatched_slots_cost_no_food_or_wear() {
    let mut h = Harness::new(Difficulty::Normal);
    let before = h.attack_cost();
    for (runtime_id, slot) in [(7, 0), (u64::MAX, 0), (8, -1), (8, 1), (8, 9)] {
        h.queue(runtime_id, slot);
    }
    h.apply();
    let target = h.target;
    assert_eq!(h.world().get::<Health>(target).unwrap().current, 20.0);
    assert_eq!(h.attack_cost(), before);
    h.server.despawn_player(8);
    h.queue(8, 0);
    h.apply();
    assert_eq!(
        h.attack_cost(),
        before,
        "a disconnected runtime ID is stale"
    );
}

#[test]
fn dead_and_spectator_attackers_and_immune_targets_cannot_be_hit() {
    for case in 0..7 {
        let mut h = Harness::new(Difficulty::Normal);
        let (attacker, target) = (h.attacker, h.target);
        match case {
            0 => *h.world().get_mut::<GameMode>(attacker).unwrap() = GameMode::Spectator,
            1 => {
                h.world().entity_mut(attacker).insert(Dead);
            }
            2 => h.world().get_mut::<Health>(attacker).unwrap().current = 0.0,
            3 => *h.world().get_mut::<GameMode>(target).unwrap() = GameMode::Creative,
            4 => *h.world().get_mut::<GameMode>(target).unwrap() = GameMode::Spectator,
            5 => {
                h.world().entity_mut(target).insert(Dead);
            }
            6 => h.world().get_mut::<Health>(target).unwrap().current = 0.0,
            _ => unreachable!(),
        }
        let before = h.attack_cost();
        let health = h.world().get::<Health>(target).unwrap().current;
        h.queue(8, 0);
        h.apply();
        assert_eq!(
            h.world().get::<Health>(target).unwrap().current,
            health,
            "case {case}"
        );
        assert_eq!(h.attack_cost(), before, "case {case}");
    }
}

#[test]
fn melee_reach_uses_player_feet_and_rejects_non_finite_positions() {
    for case in 0..4 {
        let mut h = Harness::new(Difficulty::Normal);
        let (attacker, target) = (h.attacker, h.target);
        match case {
            // The eye is in reach of a box incorrectly placed at the
            // target's eye position, but out of reach of the real box.
            0 => {
                let target_eyes = h.world().get::<Position>(target).unwrap().0;
                let height = h
                    .world()
                    .resource::<crate::entity::player_life::PlayerRules>()
                    .height;
                h.world().get_mut::<Position>(attacker).unwrap().0 = target_eyes
                    + DVec3::Y
                        * (height + crate::entity::mob::constants::MELEE_REACH - 0.5);
            }
            1 => h.world().get_mut::<Position>(target).unwrap().0.x += 20.0,
            2 => h.world().get_mut::<Position>(target).unwrap().0.x = f64::NAN,
            3 => h.world().get_mut::<Position>(attacker).unwrap().0.x = f64::INFINITY,
            _ => unreachable!(),
        }
        let before = h.attack_cost();
        h.queue(8, 0);
        h.apply();
        assert_eq!(
            h.world().get::<Health>(target).unwrap().current,
            20.0,
            "case {case}"
        );
        assert_eq!(h.attack_cost(), before, "case {case}");
    }
}

#[test]
fn player_runtime_ids_do_not_redirect_mob_interactions() {
    let mut h = Harness::new(Difficulty::Normal);
    let attacker = h.attacker;
    h.world().resource_mut::<MobActionQueue>().0.push((
        attacker,
        ItemUseOnActorInventoryTransaction {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: 8,
            },
            action_type: ActionType::Interact,
            ..Default::default()
        },
    ));
    h.apply();
    let target = h.target;
    assert_eq!(h.world().get::<Health>(target).unwrap().current, 20.0);

    let mob = spawn_mob(
        h.world(),
        "minecraft:cow",
        DVec3::new(6.5, 16.0, 4.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    let runtime_id = h.world().get::<RuntimeId>(mob).unwrap().0 as u64;
    h.queue(runtime_id, 0);
    h.apply();
    assert!(
        h.world().get::<Health>(mob).unwrap().current < 10.0,
        "mob attacks still use the mob's runtime ID"
    );
}

#[test]
fn colliding_player_and_mob_runtime_ids_reject_the_ambiguous_attack() {
    let mut h = Harness::new(Difficulty::Normal);
    let mob = spawn_mob(
        h.world(),
        "minecraft:cow",
        DVec3::new(6.5, 16.0, 4.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    h.world().get_mut::<RuntimeId>(mob).unwrap().0 = 8;
    let mob_health = h.world().get::<Health>(mob).unwrap().current;
    let before = h.attack_cost();
    h.queue(8, 0);
    h.apply();
    let target = h.target;
    assert_eq!(h.world().get::<Health>(target).unwrap().current, 20.0);
    assert_eq!(h.world().get::<Health>(mob).unwrap().current, mob_health);
    assert_eq!(h.attack_cost(), before);
}

#[test]
fn missing_or_non_finite_health_rejects_melee_without_cost() {
    for case in 0..6 {
        let mut h = Harness::new(Difficulty::Normal);
        let subject = if case < 3 { h.attacker } else { h.target };
        match case % 3 {
            0 => {
                h.world().entity_mut(subject).remove::<Health>();
            }
            1 => h.world().get_mut::<Health>(subject).unwrap().current = f32::NAN,
            _ => h.world().get_mut::<Health>(subject).unwrap().max = f32::INFINITY,
        }
        let before = h.attack_cost();
        h.queue(8, 0);
        h.apply();
        assert_eq!(h.attack_cost(), before, "case {case}");
        assert!(h.packets().is_empty(), "case {case}");
    }
}
