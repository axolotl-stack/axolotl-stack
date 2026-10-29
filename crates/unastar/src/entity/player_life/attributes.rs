//! The player's attributes on its own client: the full set at join, the
//! hunger and experience values after a respawn or a death, and the health
//! attribute every time health changes.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorRuntimeId, AttributeData, PlayerInputTick};
use jolyne::valentine::{McpePacket, UpdateAttributesPacket};

use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    Experience, Health, Hunger, Player, PlayerSession, RuntimeEntityId,
};

fn attribute(name: &str, current: f32, max: f32, default: f32, default_max: f32) -> AttributeData {
    AttributeData {
        min_value: 0.0,
        max_value: max,
        current_value: current,
        default_min_value: 0.0,
        default_max_value: default_max,
        default_value: default,
        name: name.into(),
        modifiers: vec![],
    }
}

/// `minecraft:health`.
pub fn health_attribute(health: &Health) -> AttributeData {
    attribute(
        "minecraft:health",
        health.current,
        health.max,
        health.max,
        health.max,
    )
}

/// `minecraft:player.hunger`, `player.saturation` and `player.exhaustion`.
pub fn hunger_attributes(hunger: &Hunger) -> [AttributeData; 3] {
    [
        attribute(
            "minecraft:player.hunger",
            hunger.food_level as f32,
            20.0,
            20.0,
            20.0,
        ),
        attribute(
            "minecraft:player.saturation",
            hunger.saturation,
            20.0,
            20.0,
            20.0,
        ),
        attribute(
            "minecraft:player.exhaustion",
            hunger.exhaustion,
            5.0,
            0.0,
            5.0,
        ),
    ]
}

/// Every attribute a joining player receives.
pub fn player_attributes(
    health: &Health,
    hunger: &Hunger,
    experience: &Experience,
) -> Vec<AttributeData> {
    let mut attributes = vec![
        health_attribute(health),
        attribute("minecraft:absorption", 0.0, f32::MAX, 0.0, f32::MAX),
        attribute("minecraft:movement", 0.1, f32::MAX, 0.1, f32::MAX),
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
    let _ = session.send(update_attributes(
        runtime_id.0,
        attributes,
        world.resource::<TickCounter>().current,
    ));
}

fn update_attributes(runtime_id: i64, attributes: Vec<AttributeData>, tick: u64) -> McpePacket {
    McpePacket::from(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        attribute_list: attributes,
        tick: PlayerInputTick { inputtick: tick },
    })
}

/// System: send the health attribute to each player whose health changed
/// since the last run (the join packets carry the initial value).
pub fn replicate_player_health(
    tick: Res<TickCounter>,
    players: Query<(Ref<Health>, &RuntimeEntityId, &PlayerSession), With<Player>>,
) {
    for (health, runtime_id, session) in &players {
        if health.is_changed() && !health.is_added() {
            let _ = session.send(update_attributes(
                runtime_id.0,
                vec![health_attribute(&health)],
                tick.current,
            ));
        }
    }
}
