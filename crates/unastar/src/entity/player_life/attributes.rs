//! The player's attributes on its own client: the full set at join and the
//! hunger and experience values after a respawn or a death. Later changes
//! (health, hunger, absorption) are sent by
//! [`crate::server::game::attributes::sync_player_attributes`]; the value
//! ranges come from the same module, which matches the BDS 1.26.51 join
//! capture.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorRuntimeId, AttributeData};
use jolyne::valentine::{McpePacket, UpdateAttributesPacket};

use crate::entity::components::{Experience, Health, Hunger, PlayerSession, RuntimeEntityId};
use crate::server::game::attributes as wire;

/// `minecraft:movement`: 0.1, unbounded above, as BDS 1.26.51 sends it.
fn movement() -> AttributeData {
    AttributeData {
        min_value: 0.0,
        max_value: f32::MAX,
        current_value: 0.1,
        default_min_value: 0.0,
        default_max_value: f32::MAX,
        default_value: 0.1,
        name: "minecraft:movement".into(),
        modifiers: vec![],
    }
}

/// `minecraft:player.hunger`, `player.saturation` and `player.exhaustion`.
pub fn hunger_attributes(hunger: &Hunger) -> [AttributeData; 3] {
    [
        wire::food(hunger),
        wire::saturation(hunger),
        wire::exhaustion(hunger),
    ]
}

/// Every attribute a joining player receives.
pub fn player_attributes(
    health: &Health,
    hunger: &Hunger,
    absorption: f32,
    experience: &Experience,
) -> Vec<AttributeData> {
    let mut attributes = vec![
        wire::health(health),
        wire::absorption(absorption),
        movement(),
    ];
    attributes.extend(hunger_attributes(hunger));
    attributes.extend(crate::entity::xp_orb::experience_attributes(experience));
    attributes
}

/// Send `attributes` of `player` to its own client.
pub fn send_attributes(world: &World, player: Entity, attributes: Vec<AttributeData>) {
    let (Some(session), Some(runtime_id)) = (
        world.get::<PlayerSession>(player),
        world.get::<RuntimeEntityId>(player),
    ) else {
        return;
    };
    let _ = session.send(McpePacket::from(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        attribute_list: attributes,
        tick: crate::entity::components::input_tick_of(world, player),
    }));
}
