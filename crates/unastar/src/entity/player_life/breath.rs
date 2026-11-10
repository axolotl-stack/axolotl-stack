//! The player's air supply and its `BREATHING` actor flag, kept and sent as
//! BDS 1.26.51 does (`testdata/bds-1.26.51-breath.json`). The client shows
//! the air bubbles while the flag is off or the air is short of full.
//!
//! Every tick, each live player whose join has handed it over
//! (`PlayStatus(PlayerSpawn)` sent) either holds its breath or breathes:
//! - In survival or adventure, without water breathing or conduit power,
//!   with its eyes under water ([`under_water_at`] its position), it holds
//!   its breath: the flag is off and the air drops by 1, below zero once
//!   it runs out. When it would reach [`BreathRules::drown_at`] it is 0
//!   again and the player takes [`DROWNING_DAMAGE`] as drowning, so a hit
//!   every 20 ticks.
//! - Otherwise it breathes, under water too in creative or spectator or
//!   with either effect: the flag is on and the air refills by
//!   [`BreathRules::inhale_per_tick`] up to the full supply. Air below
//!   zero is set to 0 first, without refilling that tick.
//!
//! A change goes to the player and its viewers as one `SetActorData` with
//! the flag word (key 0, [`player_flags`]) and the air (key 7), whichever
//! changed, carrying the player's input tick. Join sends full air without
//! the flag, so the first tick after the hand-over turns the flag on. A
//! respawn refills the air ([`refill`]); the next tick sends what changed.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    ActorRuntimeId, DataItemEntry, DataItemEntryPayload, DataItemInt64Payload,
    DataItemShortPayload, EnumsDataItemType, PropertySyncData, SynchedActorDataCopyableDataList,
};
use jolyne::valentine::{McpePacket, SetActorDataPacket, actor_data_i_ds as ids};
use unastar_data::actor_components::body::ActorDamageCause;

use super::constants::DROWNING_DAMAGE;
use super::rules::{BreathRules, PlayerRules};
use super::{Dead, hurt_player, send_to_player_and_viewers};
use crate::entity::components::{
    EffectType, Effects, GameMode, Player, Position, RuntimeEntityId, input_tick_of,
};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::events::under_water_at;
use crate::server::game::item_use::{ItemUsing, player_flags};
use crate::world::ecs::AwaitingSpawnStatus;

/// A player's air supply and `BREATHING` flag.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerBreath {
    /// Air left in ticks; below zero while the player drowns.
    pub air: i16,
    /// The `BREATHING` actor flag.
    pub breathing: bool,
    /// The air and flag the clients were last sent.
    sent: (i16, bool),
}

impl PlayerBreath {
    /// A joining player: full air, flag off, as its join `SetActorData`
    /// says.
    pub fn joined(rules: &BreathRules) -> Self {
        Self {
            air: rules.max_air,
            breathing: false,
            sent: (rules.max_air, false),
        }
    }

    /// One tick of holding the breath (`holds`) or breathing; returns
    /// whether a drowning hit lands.
    pub fn tick(&mut self, rules: &BreathRules, holds: bool) -> bool {
        self.breathing = !holds;
        if holds {
            self.air -= 1;
            if self.air <= rules.drown_at {
                self.air = 0;
                return true;
            }
        } else if self.air < 0 {
            self.air = 0;
        } else {
            self.air = (self.air + rules.inhale_per_tick).min(rules.max_air);
        }
        false
    }
}

/// Whether `player` holds its breath: its eyes are under water and neither
/// its game mode nor an effect lets it breathe there. `None` while the block
/// at its eyes is not loaded.
fn holds_breath(world: &World, player: Entity) -> Option<bool> {
    let eyes = world.get::<Position>(player)?.0;
    if !under_water_at(world, eyes)? {
        return Some(false);
    }
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    let effect = world.get::<Effects>(player).is_some_and(|effects| {
        effects.has(EffectType::WATER_BREATHING) || effects.has(EffectType::CONDUIT_POWER)
    });
    Some(matches!(mode, GameMode::Survival | GameMode::Adventure) && !effect)
}

/// The players that breathe: alive, their join handed over.
type Breathers = QueryState<
    Entity,
    (
        With<Player>,
        With<PlayerBreath>,
        Without<Dead>,
        Without<AwaitingSpawnStatus>,
    ),
>;

/// System (exclusive, after the environment damage): one breath tick of
/// every player, its drowning hit and the update to its clients.
pub fn breathe(world: &mut World, players: &mut Breathers, mut due: Local<Vec<Entity>>) {
    due.extend(players.iter(world));
    if due.is_empty() {
        return;
    }
    let rules = world.resource::<PlayerRules>().breath;
    for player in due.drain(..) {
        let Some(holds) = holds_breath(world, player) else {
            continue;
        };
        let drowned = world
            .get_mut::<PlayerBreath>(player)
            .expect("queried breather")
            .tick(&rules, holds);
        if drowned {
            hurt_player(
                world,
                player,
                &Hit {
                    damage: DROWNING_DAMAGE,
                    cause: ActorDamageCause::Drowning,
                    attacker: None,
                    by_player: false,
                    knockback_from: None,
                    knockback_power: KnockbackPower::DEFAULT,
                },
            );
        }
        send_changes(world, player);
    }
}

/// Refill a respawned player's air.
pub fn refill(world: &mut World, player: Entity) {
    let max_air = world.resource::<PlayerRules>().breath.max_air;
    if let Some(mut breath) = world.get_mut::<PlayerBreath>(player) {
        breath.air = max_air;
    }
}

/// Send `player` and its viewers what changed of its breath since the
/// last update.
fn send_changes(world: &mut World, player: Entity) {
    let Some(breath) = world.get::<PlayerBreath>(player).copied() else {
        return;
    };
    let (air, breathing) = breath.sent;
    let mut rows = Vec::with_capacity(2);
    if breathing != breath.breathing {
        let using = world.get::<ItemUsing>(player).is_some();
        rows.push(DataItemEntry {
            id: ids::RESERVED_0,
            payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                type_: EnumsDataItemType::Int64,
                value: player_flags(world, player, using),
            }),
        });
    }
    if air != breath.air {
        rows.push(DataItemEntry {
            id: ids::AIR_SUPPLY,
            payload: DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                type_: EnumsDataItemType::Short,
                value: breath.air,
            }),
        });
    }
    if rows.is_empty() {
        return;
    }
    world
        .get_mut::<PlayerBreath>(player)
        .expect("read above")
        .sent = (breath.air, breath.breathing);
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|id| id.0) else {
        return;
    };
    let packet = McpePacket::from(SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        actor_data: SynchedActorDataCopyableDataList { data: rows },
        synched_properties: PropertySyncData::default(),
        tick: input_tick_of(world, player),
    });
    send_to_player_and_viewers(world, player, &packet);
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: BreathRules = BreathRules {
        max_air: 300,
        inhale_per_tick: 4,
        drown_at: -20,
    };

    fn state(air: i16, breathing: bool) -> PlayerBreath {
        PlayerBreath {
            air,
            breathing,
            sent: (air, breathing),
        }
    }

    #[test]
    fn holding_the_breath_drowns_every_twenty_ticks_after_the_air_runs_out() {
        let mut breath = state(1, true);
        assert!(!breath.tick(&RULES, true));
        assert_eq!((breath.air, breath.breathing), (0, false));
        for air in (-19..=-1).rev() {
            assert!(!breath.tick(&RULES, true));
            assert_eq!(breath.air, air);
        }
        assert!(breath.tick(&RULES, true));
        assert_eq!(breath.air, 0);
    }

    #[test]
    fn breathing_refills_by_four_and_resets_negative_air_first() {
        let mut breath = state(-9, false);
        assert!(!breath.tick(&RULES, false));
        assert_eq!((breath.air, breath.breathing), (0, true));
        breath.tick(&RULES, false);
        assert_eq!(breath.air, 4);
        let mut breath = state(297, true);
        breath.tick(&RULES, false);
        assert_eq!(breath.air, 300);
        breath.tick(&RULES, false);
        assert_eq!(breath.air, 300);
    }
}
