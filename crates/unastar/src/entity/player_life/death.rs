//! Player death.
//!
//! When a player's health reaches zero ([`die`]) it is marked [`Dead`] and,
//! in order:
//!
//! 1. the player and its viewers get `ActorEvent` death;
//! 2. the player gets `DeathInfo` with the death message for the death
//!    screen, and every player gets it in chat as a translated `Text` when
//!    the `show_death_messages` game rule is on;
//! 3. without `keep_inventory`, the inventory, armour, offhand and cursor
//!    items scatter as item entities at the feet (stacks carrying the
//!    `minecraft:keep_on_death` item component stay in their slots), the
//!    player's `minecraft:experience_reward` (`min(level × 7, 100)` for
//!    vanilla) drops as experience orbs and the player's experience is
//!    cleared;
//! 4. the respawn position goes to the client (`respawn::open_respawn_screen`)
//!    and, with `do_immediate_respawn`, the player respawns at once (the
//!    client skips its death screen by the same game rule).
//!
//! Death messages use the native translation keys: the cause's own key
//! (`death.attack.lava`, `death.attack.outOfWorld`, …; falls of more than
//! [`FALL_ACCIDENT_DISTANCE`] blocks are `death.fell.accident.generic`)
//! with the victim's name, and for melee from an actor
//! `death.attack.mob` / `death.attack.player` with the victim's and the
//! killer's name (a mob's name tag, else its `%entity.<name>.name` key).

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    DataItemEntryPayload, EnumsActorEvent, TextPacketBody, TextPacketPayloadMessageAndParams,
    TextPacketPayloadMessageAndParamsMessageType,
};
use jolyne::valentine::{DeathInfoPacket, McpePacket, TextPacket, actor_data_i_ds as ids};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tracing::warn;
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_rules::molang::eval::{MolangContext, Value as MolangValue, evaluate_num};
use unastar_rules::molang::{BuiltinQuery, QueryId};

use super::constants::FALL_ACCIDENT_DISTANCE;
use super::environment::{Burning, PlayerFall};
use super::rules::PlayerRules;
use super::{Dead, attributes, hooks, respawn};
use crate::entity::components::{
    ArmourInventory, Experience, Health, MainInventory, OffhandSlot, Player, PlayerName,
    PlayerSession, PlayerUi, Position,
};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::combat::{Hit, HurtTimer};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::{EntityRandom, PLAYER_EYE_HEIGHT, spawn_item_drop};
use crate::world::rules::GameRules;

/// What killed a player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeathCause {
    pub cause: ActorDamageCause,
    pub attacker: Option<Entity>,
    /// Blocks fallen, for a landing.
    pub fall_distance: Option<f64>,
}

/// A death message: its translation key and parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathMessage {
    pub key: &'static str,
    pub parameters: Vec<String>,
}

/// Kill `player` as `/kill` does: override damage of its whole health that
/// ignores the hurt timer, armour and game-mode immunity, then the normal
/// death. Returns whether this hit committed a death, including immediate respawn.
pub fn kill_player(world: &mut World, player: Entity) -> bool {
    let Some(health) = world.get::<Health>(player).map(|health| health.current) else {
        return false;
    };
    if health <= 0.0 || world.get::<Dead>(player).is_some() {
        return false;
    }
    super::hurt::apply_hit_result(
        world,
        player,
        &Hit {
            damage: health,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
        None,
    )
    .1
}

/// The death of `player`, whose health just reached zero.
pub(super) fn die(world: &mut World, player: Entity, cause: DeathCause) {
    if world.get::<Dead>(player).is_some() || world.get::<Player>(player).is_none() {
        return;
    }
    world
        .entity_mut(player)
        .insert(Dead)
        .remove::<(Burning, HurtTimer)>();
    if let Some(mut fall) = world.get_mut::<PlayerFall>(player) {
        *fall = PlayerFall::default();
    }
    super::hurt::send_player_event(world, player, EnumsActorEvent::Death);

    let message = death_message(world, player, cause);
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(McpePacket::from(DeathInfoPacket {
            death_cause_attack_name: message.key.into(),
            death_cause_message_list: message
                .parameters
                .iter()
                .map(|parameter| parameter.as_str().into())
                .collect(),
        }));
    }
    let rules = world.resource::<GameRules>().clone();
    if rules.show_death_messages {
        broadcast_death_message(world, &message, player, cause.attacker);
    }
    if !rules.keep_inventory {
        drop_everything(world, player);
    }
    hooks::death(world, hooks::PlayerDeath { player, cause });
    if world.get::<Dead>(player).is_none() || world.get::<Player>(player).is_none() {
        return;
    }
    respawn::open_respawn_screen(world, player);
    if rules.do_immediate_respawn {
        respawn::respawn(world, player);
    }
}

/// The death message for `player` killed by `cause`.
pub fn death_message(world: &World, player: Entity, cause: DeathCause) -> DeathMessage {
    let victim = world
        .get::<PlayerName>(player)
        .map(|name| name.0.clone())
        .unwrap_or_default();
    let killer = cause
        .attacker
        .filter(|attacker| world.get_entity(*attacker).is_ok());
    if let Some(attacker) = killer {
        let by_player = world.get::<Player>(attacker).is_some();
        let key = match cause.cause {
            ActorDamageCause::Contact | ActorDamageCause::EntityAttack if by_player => {
                Some("death.attack.player")
            }
            ActorDamageCause::Contact | ActorDamageCause::EntityAttack => Some("death.attack.mob"),
            ActorDamageCause::Projectile => Some("death.attack.arrow"),
            ActorDamageCause::EntityExplosion => Some("death.attack.explosion.player"),
            ActorDamageCause::Magic => Some("death.attack.indirectMagic"),
            ActorDamageCause::Thorns => Some("death.attack.thorns"),
            _ => None,
        };
        if let Some(key) = key {
            return DeathMessage {
                key,
                parameters: vec![victim, actor_name(world, attacker)],
            };
        }
    }
    DeathMessage {
        key: cause_key(cause),
        parameters: vec![victim],
    }
}

/// The translation key of a death without a credited killer.
fn cause_key(cause: DeathCause) -> &'static str {
    match cause.cause {
        ActorDamageCause::Suffocation => "death.attack.inWall",
        ActorDamageCause::Fall => match cause.fall_distance {
            Some(distance) if distance > FALL_ACCIDENT_DISTANCE => "death.fell.accident.generic",
            _ => "death.attack.fall",
        },
        ActorDamageCause::Fire => "death.attack.inFire",
        ActorDamageCause::FireTick => "death.attack.onFire",
        ActorDamageCause::Lava => "death.attack.lava",
        ActorDamageCause::Drowning => "death.attack.drown",
        ActorDamageCause::BlockExplosion | ActorDamageCause::EntityExplosion => {
            "death.attack.explosion"
        }
        ActorDamageCause::Void => "death.attack.outOfWorld",
        ActorDamageCause::Magic => "death.attack.magic",
        ActorDamageCause::Wither => "death.attack.wither",
        ActorDamageCause::Starve => "death.attack.starve",
        ActorDamageCause::Anvil => "death.attack.anvil",
        ActorDamageCause::FlyIntoWall => "death.attack.flyIntoWall",
        ActorDamageCause::Fireworks => "death.attack.fireworks",
        ActorDamageCause::Lightning => "death.attack.lightningBolt",
        ActorDamageCause::Freezing => "death.attack.freeze",
        ActorDamageCause::Stalactite => "death.attack.stalactite",
        ActorDamageCause::Stalagmite => "death.attack.stalagmite",
        ActorDamageCause::SonicBoom => "death.attack.sonicBoom",
        _ => "death.attack.generic",
    }
}

/// How a death message names an actor: a player's name, a mob's name tag,
/// else the translation key of its type.
fn actor_name(world: &World, actor: Entity) -> String {
    if let Some(name) = world.get::<PlayerName>(actor) {
        return name.0.clone();
    }
    let tag =
        world
            .get::<EntityMetadata>(actor)
            .and_then(|metadata| match metadata.get(ids::NAME) {
                Some(DataItemEntryPayload::DataItemStringPayload(name))
                    if !name.value.is_empty() =>
                {
                    Some(name.value.to_string())
                }
                _ => None,
            });
    if let Some(tag) = tag {
        return tag;
    }
    world
        .get::<MobKind>(actor)
        .map_or_else(String::new, |kind| {
            let identifier = &kind.0.identifier;
            let name = identifier
                .split_once(':')
                .map_or(identifier.as_ref(), |(_, name)| name);
            format!("%entity.{name}.name")
        })
}

/// Send the death message to every player's chat.
fn broadcast_death_message(
    world: &mut World,
    message: &DeathMessage,
    player: Entity,
    attacker: Option<Entity>,
) {
    let packet = McpePacket::from(TextPacket {
        localize: true,
        body: TextPacketBody::MessageAndParams(TextPacketPayloadMessageAndParams {
            message_type: TextPacketPayloadMessageAndParamsMessageType::Translate,
            message: message.key.into(),
            parameter_list: message
                .parameters
                .iter()
                .map(|parameter| parameter.as_str().into())
                .collect(),
        }),
        senders_xuid: "".into(),
        platform_id: "".into(),
        filtered_message: None,
    });
    let mut sessions = world.query_filtered::<(Entity, &PlayerSession), With<Player>>();
    for (viewer, session) in sessions.iter(world) {
        if crate::server::broadcast::player_views::audience_allows(world, player, viewer)
            && attacker.is_none_or(|attacker| {
                crate::server::broadcast::player_views::audience_allows(world, attacker, viewer)
            })
        {
            let _ = session.send(packet.clone());
        }
    }
}

/// Scatter every item `player` carries at its feet, drop its experience and
/// clear both; the client gets the emptied containers and experience.
/// Stacks with `minecraft:keep_on_death` stay where they are.
fn drop_everything(world: &mut World, player: Entity) {
    let Some(feet) = world
        .get::<Position>(player)
        .map(|position| position.0 - glam::DVec3::Y * PLAYER_EYE_HEIGHT)
    else {
        return;
    };
    let mut stacks = Vec::new();
    {
        let mut entity = world.entity_mut(player);
        if let Some(mut inventory) = entity.get_mut::<MainInventory>() {
            take_dropped(inventory.0.slots_mut(), &mut stacks);
        }
        if let Some(mut armour) = entity.get_mut::<ArmourInventory>() {
            take_dropped(armour.inventory_mut().slots_mut(), &mut stacks);
        }
        if let Some(mut offhand) = entity.get_mut::<OffhandSlot>() {
            take_dropped(std::slice::from_mut(&mut offhand.0), &mut stacks);
        }
        if let Some(mut ui) = entity.get_mut::<PlayerUi>() {
            take_dropped(ui.0.slots_mut(), &mut stacks);
        }
    }
    for stack in stacks {
        spawn_item_drop(world, feet, stack, None);
    }
    crate::server::game::send_inventory_contents(world, player);

    let level = world
        .get::<Experience>(player)
        .map_or(0, |experience| experience.level);
    let reward = world.resource::<PlayerRules>().experience_on_death.clone();
    if let Some(expr) = reward {
        let seed = world
            .get_resource_or_init::<EntityRandom>()
            .0
            .r#gen::<u64>();
        let mut ctx = PlayerMolang {
            level,
            rng: StdRng::seed_from_u64(seed),
        };
        match evaluate_num(&expr, &mut ctx) {
            Ok(amount) => {
                crate::entity::xp_orb::spawn_experience(
                    world,
                    feet,
                    amount.round().max(0.0) as u32,
                );
            }
            Err(error) => warn!(%error, "Player experience reward cannot be evaluated"),
        }
    }
    if let Some(mut experience) = world.get_mut::<Experience>(player) {
        *experience = Experience::default();
    }
    attributes::send_attributes(
        world,
        player,
        crate::entity::xp_orb::experience_attributes(&Experience::default()).to_vec(),
    );
}

/// Move every non-empty stack of `slots` without `minecraft:keep_on_death`
/// into `dropped`, leaving the slot empty.
fn take_dropped(slots: &mut [crate::item::ItemStack], dropped: &mut Vec<crate::item::ItemStack>) {
    for slot in slots {
        if !slot.is_empty() && !keeps_on_death(slot) {
            dropped.push(std::mem::take(slot));
        }
    }
}

/// Whether `stack` carries the `minecraft:keep_on_death` item component
/// (a byte `1` under that key in its NBT, as `/give` components write it).
pub(crate) fn keeps_on_death(stack: &crate::item::ItemStack) -> bool {
    crate::item::enchant::item_tag(stack).is_some_and(|tag| {
        matches!(
            tag.0.get("minecraft:keep_on_death"),
            Some(zuri_nbt::NBTTag::Byte(value)) if value.0 == 1
        )
    })
}

/// Molang facts about a dying player (its `experience_reward`); the
/// queries it answers are checked when [`PlayerRules`] compiles.
struct PlayerMolang {
    level: i32,
    rng: StdRng,
}

impl MolangContext for PlayerMolang {
    fn query(&mut self, id: QueryId, _: &[MolangValue]) -> Option<MolangValue> {
        match BuiltinQuery::from_id(id)? {
            BuiltinQuery::PlayerLevel => Some(MolangValue::Num(self.level as f32)),
            _ => None,
        }
    }
    fn variable(&self, _: &str) -> Option<MolangValue> {
        None
    }
    fn set_variable(&mut self, _: &str, _: MolangValue) {}
    fn context(&self, _: &str) -> Option<MolangValue> {
        None
    }
    fn this(&self) -> MolangValue {
        MolangValue::Num(0.0)
    }
    fn enter(&mut self, _: u64) -> bool {
        false
    }
    fn leave(&mut self) {}
    fn random(&mut self) -> f32 {
        self.rng.r#gen()
    }
}
