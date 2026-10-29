//! Every packet about the player that carries a `PlayerInputTick` must carry
//! the tick of the player's latest `PlayerAuthInput`: clients treat the field
//! as their own input timeline and disconnect when it goes backwards.

use jolyne::valentine::RespawnPacket;
use jolyne::valentine::types::PlayerInputTick;

use super::*;
use crate::entity::components::{EffectType, Effects, Hunger};
use crate::entity::xp_orb::spawn_orb;

/// The `PlayerInputTick` of each packet about `runtime_id`, in send order.
fn input_ticks(packets: &[McpePacketData], runtime_id: u64) -> Vec<(&'static str, u64)> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::UpdateAttributesPacket(p)
                if p.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                Some(("UpdateAttributes", p.tick.inputtick))
            }
            McpePacketData::SetActorMotionPacket(p)
                if p.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                Some(("SetActorMotion", p.tick.inputtick))
            }
            McpePacketData::MobEffectPacket(p)
                if p.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                Some(("MobEffect", p.tick.inputtick))
            }
            McpePacketData::MovePlayerPacket(p)
                if p.player_runtime_id.actor_runtime_id == runtime_id =>
            {
                Some(("MovePlayer", p.tick.inputtick))
            }
            McpePacketData::SetActorDataPacket(p)
                if p.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                Some(("SetActorData", p.tick.inputtick))
            }
            _ => None,
        })
        .collect()
}

struct Client {
    tick: u64,
    seen: Vec<(&'static str, u64, u64)>,
}

impl Client {
    /// One `PlayerAuthInput` standing at [`HOME`] with the next client tick,
    /// then a server tick; records every tick field received.
    fn input(&mut self, h: &mut Harness) {
        self.tick += 1;
        let position = eyes(h.feet());
        h.route(PlayerAuthInputPacket {
            position: Vec3 {
                x: position.x as f32,
                y: position.y as f32,
                z: position.z as f32,
            },
            input_data: vec![EnumsPlayerAuthInputPacketPayloadInputData::Verticalcollision],
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            ..Default::default()
        });
        h.tick();
        self.record(h);
    }

    fn record(&mut self, h: &mut Harness) {
        let packets = h.drain();
        let tick = self.tick;
        self.seen.extend(
            input_ticks(&packets, SESSION)
                .into_iter()
                .map(|(kind, sent)| (kind, sent, tick)),
        );
    }
}

#[test]
fn every_tick_field_sent_to_a_player_follows_its_input_timeline() {
    let mut h = harness(Difficulty::Normal);
    // The server has run far longer than the client's input timeline, the
    // way a client joining a long-running server sees it.
    h.ticks(300);
    h.drain();
    let mut client = Client {
        tick: 10,
        seen: Vec::new(),
    };
    client.input(&mut h);

    // A mob hit: health, then knockback.
    let cow = h.cow();
    h.hit_from(cow, 4.0);
    client.record(&mut h);
    client.input(&mut h);

    // Hunger changes, synced by the attribute system.
    let player = h.player;
    h.world().get_mut::<Hunger>(player).unwrap().food_level = 12;
    client.input(&mut h);

    // An effect starts.
    h.world()
        .entity_mut(player)
        .insert(Effects::default())
        .get_mut::<Effects>()
        .unwrap()
        .add(EffectType::SPEED, 0, 200);
    client.input(&mut h);

    // An experience orb lands on the player and is collected.
    let at = h.feet();
    spawn_orb(h.world(), at, 3);
    for _ in 0..20 {
        client.input(&mut h);
    }

    // Death, then the client asks to respawn.
    h.hit_from(cow, 100.0);
    client.record(&mut h);
    client.input(&mut h);
    h.route(RespawnPacket {
        position: Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        state: EnumsPlayerRespawnState::Clientreadytospawn,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: SESSION,
        },
    });
    client.input(&mut h);
    client.input(&mut h);

    let kinds: Vec<&str> = client.seen.iter().map(|(kind, ..)| *kind).collect();
    for expected in [
        "UpdateAttributes",
        "SetActorMotion",
        "MobEffect",
        "MovePlayer",
    ] {
        assert!(
            kinds.contains(&expected),
            "no {expected} reached the player: {kinds:?}"
        );
    }
    for (kind, sent, latest) in &client.seen {
        assert_eq!(
            sent, latest,
            "{kind} carried tick {sent}, but the latest input was {latest}: {:?}",
            client.seen
        );
    }
    assert!(
        client.seen.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "tick fields went backwards: {:?}",
        client.seen
    );
}
