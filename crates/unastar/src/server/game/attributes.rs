//! Player attributes on the wire (`UpdateAttributesPacket`).
//!
//! The ranges are the ones BDS 1.26.51 sends at join (capture
//! `20261025T172246Z/member/join/00046-UpdateAttributesPacket`): hunger
//! 0..20 default 20, saturation 0..20 default 5, exhaustion 0..20 default 0,
//! absorption 0..16 default 0, health 0..max default max, movement 0..f32
//! max default 0.1. After join only the attributes whose value changed are
//! sent, one packet per player per tick, from [`sync_player_attributes`], to
//! the player and the players that see it (BDS 1.26.51 sends a player's
//! health, exhaustion and movement to its viewers too; movement capture).
//! A sprinting player's movement is 0.13 with BDS's sprint modifier. Experience
//! is sent by the experience code itself (`entity::xp_orb`).

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorRuntimeId, AttributeData};
use jolyne::valentine::{McpePacket, UpdateAttributesPacket};

use crate::entity::components::{
    ActorViewers, Health, Hunger, Player, PlayerInput, PlayerSession, PlayerState, RuntimeEntityId,
};

/// Maximum food level.
pub const MAX_FOOD: f32 = 20.0;
/// Maximum saturation (and the attribute range BDS sends).
pub const MAX_SATURATION: f32 = 20.0;
/// Default saturation of a new player.
pub const DEFAULT_SATURATION: f32 = 5.0;
/// Upper end of the exhaustion attribute.
pub const MAX_EXHAUSTION: f32 = 20.0;
/// Upper end of the absorption attribute.
pub const MAX_ABSORPTION: f32 = 16.0;
/// `minecraft:movement` of a player that is not sprinting.
pub const WALK_SPEED: f32 = 0.1;
/// The sprint modifier BDS lists on a sprinting player's movement:
/// +30 % of the total (operation and operand 2), not saved.
const SPRINT_MODIFIER_ID: &str = "d208fc00-42aa-4aad-9276-d5446530de43";
const SPRINT_MODIFIER_NAME: &str = "Sprinting speed boost";
const SPRINT_BOOST: f32 = 0.3;

fn attr(name: &str, current: f32, max: f32, default: f32) -> AttributeData {
    AttributeData {
        min_value: 0.0,
        max_value: max,
        current_value: current,
        default_min_value: 0.0,
        default_max_value: max,
        default_value: default,
        name: name.into(),
        modifiers: vec![],
    }
}

pub fn health(health: &Health) -> AttributeData {
    attr("minecraft:health", health.current, health.max, health.max)
}

pub fn absorption(amount: f32) -> AttributeData {
    attr("minecraft:absorption", amount, MAX_ABSORPTION, 0.0)
}

pub fn food(hunger: &Hunger) -> AttributeData {
    attr(
        "minecraft:player.hunger",
        hunger.food_level as f32,
        MAX_FOOD,
        MAX_FOOD,
    )
}

pub fn saturation(hunger: &Hunger) -> AttributeData {
    attr(
        "minecraft:player.saturation",
        hunger.saturation,
        MAX_SATURATION,
        DEFAULT_SATURATION,
    )
}

pub fn exhaustion(hunger: &Hunger) -> AttributeData {
    attr(
        "minecraft:player.exhaustion",
        hunger.exhaustion,
        MAX_EXHAUSTION,
        0.0,
    )
}

/// `minecraft:movement` of a player, sprinting or not (BDS 1.26.51: 0.13
/// with the sprint modifier, 0.1 without).
pub fn movement(sprinting: bool) -> AttributeData {
    let mut data = attr("minecraft:movement", WALK_SPEED, f32::MAX, WALK_SPEED);
    if sprinting {
        data.current_value = WALK_SPEED * (1.0 + SPRINT_BOOST);
        data.modifiers
            .push(jolyne::valentine::types::AttributeModifier {
                id: SPRINT_MODIFIER_ID.into(),
                name: SPRINT_MODIFIER_NAME.into(),
                amount: SPRINT_BOOST,
                operation: 2,
                operand: 2,
                is_serializable: false,
            });
    }
    data
}

/// Extra health from the absorption effect, drained before health.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct Absorption(pub f32);

/// The attribute values a player's client last received.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct SentAttributes {
    health: Option<(f32, f32)>,
    absorption: Option<f32>,
    food: Option<i32>,
    saturation: Option<f32>,
    exhaustion: Option<f32>,
    /// The movement attribute: sprinting or not.
    movement: Option<bool>,
}

impl SentAttributes {
    /// The state after join sent `health`, `hunger`, `absorption` and the
    /// walking movement speed.
    pub fn sent(health: &Health, hunger: &Hunger, absorption: f32) -> Self {
        Self {
            health: Some((health.current, health.max)),
            absorption: Some(absorption),
            food: Some(hunger.food_level),
            saturation: Some(hunger.saturation),
            exhaustion: Some(hunger.exhaustion),
            movement: Some(false),
        }
    }

    /// After a respawn: the respawn sequence already sent `hunger`, and the
    /// restored health is always sent again (even when it equals the last
    /// value sent, as it does when death and respawn fall in one tick).
    pub fn respawned(&mut self, hunger: &Hunger) {
        self.food = Some(hunger.food_level);
        self.saturation = Some(hunger.saturation);
        self.exhaustion = Some(hunger.exhaustion);
        self.health = None;
    }

    /// The attributes that differ from what was sent, in BDS's order
    /// (movement first); records them as sent.
    pub fn diff(
        &mut self,
        health: &Health,
        hunger: &Hunger,
        absorb: f32,
        sprinting: bool,
    ) -> Vec<AttributeData> {
        let mut out = Vec::new();
        if self.movement != Some(sprinting) {
            self.movement = Some(sprinting);
            out.push(movement(sprinting));
        }
        if self.health != Some((health.current, health.max)) {
            self.health = Some((health.current, health.max));
            out.push(self::health(health));
        }
        if self.absorption != Some(absorb) {
            self.absorption = Some(absorb);
            out.push(absorption(absorb));
        }
        if self.food != Some(hunger.food_level) {
            self.food = Some(hunger.food_level);
            out.push(food(hunger));
        }
        if self.saturation != Some(hunger.saturation) {
            self.saturation = Some(hunger.saturation);
            out.push(saturation(hunger));
        }
        if self.exhaustion != Some(hunger.exhaustion) {
            self.exhaustion = Some(hunger.exhaustion);
            out.push(exhaustion(hunger));
        }
        out
    }
}

/// System: send each player and the players that see it (its
/// `ActorViewers`, as `player_life::player_viewers`) the attributes that
/// changed since the last send. The packet carries the player's last input
/// tick, so the client applies it in order with its own prediction.
#[allow(clippy::type_complexity)]
pub fn sync_player_attributes(
    mut players: Query<(
        &RuntimeEntityId,
        &PlayerSession,
        &PlayerInput,
        &Health,
        &Hunger,
        Option<&Absorption>,
        Option<&PlayerState>,
        Option<&ActorViewers>,
        &mut SentAttributes,
    )>,
    others: Query<&PlayerSession, With<Player>>,
) {
    for (runtime_id, session, input, health, hunger, absorb, state, shown_to, mut sent) in
        &mut players
    {
        let sprinting = state.is_some_and(|state| state.sprinting);
        let changed = sent.diff(health, hunger, absorb.map_or(0.0, |a| a.0), sprinting);
        if changed.is_empty() {
            continue;
        }
        let packet = McpePacket::from(UpdateAttributesPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id.0 as u64,
            },
            attribute_list: changed,
            tick: input.wire_tick(),
        });
        let _ = session.send(packet.clone());
        for viewer in shown_to.into_iter().flat_map(|viewers| &viewers.viewers) {
            if let Ok(session) = others.get(*viewer) {
                let _ = session.send(packet.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_match_the_bds_join_capture() {
        let hunger = Hunger::default();
        let s = saturation(&hunger);
        assert_eq!((s.max_value, s.default_value), (20.0, 5.0));
        let e = exhaustion(&hunger);
        assert_eq!(
            (e.max_value, e.default_max_value, e.default_value),
            (20.0, 20.0, 0.0)
        );
        let a = absorption(0.0);
        assert_eq!((a.max_value, a.default_value), (16.0, 0.0));
        let f = food(&hunger);
        assert_eq!(
            (f.current_value, f.max_value, f.default_value),
            (20.0, 20.0, 20.0)
        );
    }

    #[test]
    fn diff_sends_only_changed_attributes_once() {
        let health = Health::new(20.0);
        let mut hunger = Hunger::default();
        let mut sent = SentAttributes::sent(&health, &hunger, 0.0);
        assert!(sent.diff(&health, &hunger, 0.0, false).is_empty());
        hunger.food_level = 17;
        hunger.exhaustion = 0.5;
        let names: Vec<_> = sent
            .diff(&health, &hunger, 0.0, false)
            .into_iter()
            .map(|a| a.name.to_string())
            .collect();
        assert_eq!(
            names,
            ["minecraft:player.hunger", "minecraft:player.exhaustion"]
        );
        assert!(sent.diff(&health, &hunger, 0.0, false).is_empty());
    }
}
