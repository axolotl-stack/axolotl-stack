//! What a viewer is sent while a piston moves, compared with the BDS 1.26.51
//! watcher capture (`testdata/bds-placements-pistons.json`): the block actors
//! (the piston's `PistonArm` and each `MovingBlock`), the block changes and
//! the sounds, each in BDS's order. A lever beside the piston is switched on
//! and off again as the sweep's player did.

use bevy_ecs::message::Messages;
use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::McpePacketData;
use serde_json::Value;

use super::tick_redstone;
use crate::entity::components::PlayerSession;
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;
use crate::server::game::blocks::set_block;
use crate::server::game::inventory::capture::decode;
use crate::world::block_entity::BlockEntity;
use crate::world::ecs::{
    BlockBroadcastEvent, ChunkData, ChunkManager, ChunkPosition, ChunkViewers,
};

/// One thing a viewer saw: a block actor, a block change or a sound.
#[derive(Debug, PartialEq)]
enum Seen {
    Actor(IVec3, zuri_nbt::tag::Compound),
    Block(IVec3, u32),
    Sound(String, [i32; 3]),
}

fn actor(pos: IVec3, nbt: jolyne::valentine::bedrock::codec::Nbt) -> Seen {
    let compound = BlockEntity::from_network_nbt(nbt)
        .ok()
        .and_then(|actor| actor.bedrock_compound())
        .and_then(Result::ok)
        .expect("a readable block actor");
    Seen::Actor(pos, compound)
}

/// The piston's part of a packet (the lever and everything else left out).
fn seen(packet: &McpePacketData, lever: IVec3) -> Vec<Seen> {
    let at = |p: &jolyne::valentine::BlockPos| IVec3::new(p.x, p.y, p.z);
    match packet {
        McpePacketData::BlockActorDataPacket(data) => {
            vec![actor(
                at(&data.block_position),
                data.actor_data_tags.clone(),
            )]
        }
        McpePacketData::UpdateBlockPacket(update)
            if update.layer == 0 && at(&update.block_position) != lever =>
        {
            vec![Seen::Block(
                at(&update.block_position),
                update.block_runtime_id,
            )]
        }
        McpePacketData::UpdateSubChunkBlocksPacket(update) => update
            .blocks_changed
            .blocks_changed_standards
            .iter()
            .map(|block| Seen::Block(at(&block.pos), block.runtime_id))
            .collect(),
        McpePacketData::LevelSoundEventPacket(sound) if sound.sound_event.starts_with("piston") => {
            vec![Seen::Sound(
                sound.sound_event.to_string(),
                [
                    sound.position.x as i32,
                    sound.position.y as i32,
                    sound.position.z as i32,
                ],
            )]
        }
        _ => Vec::new(),
    }
}

/// Consecutive repeats left out (BDS sends some moving block actors twice).
fn dedup(mut list: Vec<Seen>) -> Vec<Seen> {
    list.dedup();
    list
}

fn case(key: &str) -> Value {
    let path = format!(
        "{}/src/server/game/testdata/bds-placements-pistons.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["key"] == key)
        .unwrap()
        .clone()
}

fn bds(case: &Value, lever: IVec3) -> Vec<Seen> {
    dedup(
        case["watcher_packets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["dir"] == "s2c")
            .filter_map(|p| p["hex"].as_str())
            .flat_map(|hex| seen(&decode(hex), lever))
            .collect(),
    )
}

fn state(name: &str, states: &Value) -> u32 {
    let entry = BlockRegistry::vanilla().get_by_name(name).unwrap();
    let mut id = entry.default_state_id;
    for (property, value) in states.as_object().unwrap() {
        let value = match value {
            Value::Bool(b) => StateValue::Byte(u8::from(*b)),
            Value::Number(n) => StateValue::Int(n.as_i64().unwrap() as i32),
            Value::String(s) => StateValue::Str(Box::leak(s.clone().into_boxed_str())),
            _ => unreachable!(),
        };
        id = entry.with_state_value(id, property, value).unwrap();
    }
    id
}

/// Run the case's setup at BDS's arena origin, switch its lever on and off,
/// and return what the viewer saw and where the lever is.
fn unastar(case: &Value) -> (Vec<Seen>, IVec3) {
    let origin = IVec3::new(4, -60, 4);
    let mut world = World::new();
    world.insert_resource(ChunkManager::new(
        crate::world::WorldConfig::void_spawn_platform(1),
    ));
    world.insert_resource(crate::server::game::types::BlockRegistryResource(
        std::sync::Arc::new(BlockRegistry::vanilla().clone()),
    ));
    world.insert_resource(crate::server::game::types::ServerConfigResource(
        Default::default(),
    ));
    world.insert_resource(Messages::<BlockBroadcastEvent>::default());
    // The sweep world's superflat floor: grass at y = -61.
    let mut floor = crate::world::chunk::Chunk::new(0, 0);
    let grass = state("minecraft:grass_block", &Value::Object(Default::default()));
    for x in 0..16 {
        for z in 0..16 {
            floor.set_block(x, -61, z, grass);
        }
    }
    let chunk = world.spawn(ChunkData::new(floor)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let mut lever = None;
    for block in case["setup"].as_array().unwrap() {
        let offset = |i: usize| block[i].as_i64().unwrap() as i32;
        let pos = origin + IVec3::new(offset(0), offset(1), offset(2));
        let name = block[3].as_str().unwrap();
        if name == "minecraft:lever" {
            lever = Some((pos, block[4].clone()));
        }
        set_block(&mut world, pos, state(name, &block[4]), false);
    }
    let (lever, lever_states) = lever.expect("a lever");
    for _ in 0..10 {
        tick_redstone(&mut world);
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let viewer = world
        .spawn(PlayerSession::new(1, "watcher".into(), None, None, tx))
        .id();
    let mut viewers = ChunkViewers::default();
    viewers.insert(viewer);
    world.entity_mut(chunk).insert(viewers);
    for on in [true, false] {
        let mut states = lever_states.clone();
        states["open_bit"] = Value::Bool(on);
        set_block(&mut world, lever, state("minecraft:lever", &states), false);
        for _ in 0..30 {
            tick_redstone(&mut world);
        }
    }
    let mut out = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        out.extend(seen(&packet.data, lever));
    }
    (dedup(out), lever)
}

/// The things of one kind (`actor`, `block` or `sound`) in `list`.
fn pick<'a>(list: &'a [Seen], kind: &str) -> Vec<&'a Seen> {
    list.iter()
        .filter(|seen| {
            matches!(
                (kind, seen),
                ("actor", Seen::Actor(..))
                    | ("block", Seen::Block(..))
                    | ("sound", Seen::Sound(..))
            )
        })
        .collect()
}

fn compare(key: &str) {
    let case = case(key);
    let (ours, lever) = unastar(&case);
    let theirs = bds(&case, lever);
    for kind in ["actor", "block", "sound"] {
        assert_eq!(
            pick(&ours, kind),
            pick(&theirs, kind),
            "{key}: {kind}s differ from BDS"
        );
    }
}

#[test]
fn a_piston_pushing_stone_sends_what_bds_sends() {
    compare("piston_stone");
}

#[test]
fn a_sticky_piston_moving_slime_sends_what_bds_sends() {
    compare("sticky_piston_slime");
}

#[test]
fn a_blocked_piston_sends_nothing_like_bds() {
    compare("piston_blocked");
}
