//! Typed damage proposals and positive-health-loss observations over native rules.

use super::lifecycle::{damage_cause, invoke, invoke_validated};
use super::{PluginManager, make_player_resource, make_world_tx, players, wit_types as wit};
use crate::ecs::events::EventKind;
use crate::entity::components::Player;
use crate::entity::damage::DamageAttribution;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::player_life::hooks::{PlayerDamage, PlayerDamaged};
use bevy_ecs::prelude::*;

/// Copy immutable launch/deflection facts without resolving a missing owner into a fake actor.
pub(super) fn attribution(source: &DamageAttribution) -> wit::DamageAttribution {
    wit::DamageAttribution {
        source_dimension: source.dimension(),
        source_world_lifetime: source.world_lifetime().map(|id| id.to_string()),
        owner: source.owner().map(|owner| wit::DamageOwner {
            is_player: owner.is_player(),
            uuid: owner.player_uuid().map(|id| id.to_string()),
            authenticated: owner
                .authenticated()
                .map(|account| wit::AuthenticatedDamageAccount {
                    uuid: account.uuid().to_string(),
                    xuid: account.xuid(),
                }),
            connection_id: owner.session_id(),
            dimension: owner.dimension(),
            world_lifetime: owner.world_lifetime().map(|id| id.to_string()),
            feet: owner.feet_position().map(|position| wit::Vec3 {
                x: position.x,
                y: position.y,
                z: position.z,
            }),
        }),
    }
}

/// Native knockback remains the source of truth for units and defaults.
fn knockback(power: KnockbackPower) -> wit::Knockback {
    wit::Knockback {
        horizontal: power.horizontal,
        vertical: power.vertical,
        vertical_cap: power.vertical_cap,
        retain: power.retain,
    }
}

/// Validate the entire adjustment before changing any field of the native proposal.
fn valid(tuning: &wit::DamageTuning) -> bool {
    tuning.damage.is_finite()
        && tuning.damage >= 0.0
        && tuning.post_armour_multiplier.is_finite()
        && tuning.post_armour_multiplier >= 0.0
        && [
            tuning.knockback.horizontal,
            tuning.knockback.vertical,
            tuning.knockback.vertical_cap,
            tuning.knockback.retain,
        ]
        .into_iter()
        .all(|value| value.is_finite() && (value as f32).is_finite() && value >= 0.0)
}

impl PluginManager {
    /// Apply ordered component tuning before any native health, timer or inventory change.
    pub(super) fn player_damage(&mut self, world: &mut World, event: &mut PlayerDamage) {
        if !self.begin_tick(world) {
            event.cancel();
            return;
        }
        for owner in self.subscribers(EventKind::PlayerDamage) {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let player = event.player();
            let attacker = event
                .attribution()
                .live_owner(world)
                .filter(|actor| world.get::<Player>(*actor).is_some());
            let proposal = wit::DamageProposal {
                cause: damage_cause(event.hit.cause),
                attribution: attribution(event.attribution()),
                tuning: wit::DamageTuning {
                    damage: event.hit.damage,
                    knockback: knockback(event.hit.knockback_power),
                    hurt_ticks: event.hurt_ticks,
                    bypass_armour: event.bypass_armour,
                    post_armour_multiplier: event.post_armour_multiplier,
                },
            };
            let result = invoke_validated(
                plugin,
                world,
                |store, bindings| {
                    let tx = make_world_tx(store)?;
                    let player = make_player_resource(store, player)?;
                    let attacker = attacker
                        .map(|actor| make_player_resource(store, actor))
                        .transpose()?;
                    bindings.call_on_player_damage(store, tx, player, attacker, &proposal)
                },
                |tuning| tuning.as_ref().is_none_or(valid),
            );
            match result {
                Ok(Some(tuning)) if valid(&tuning) => {
                    event.hit.damage = tuning.damage;
                    event.hit.knockback_power = KnockbackPower {
                        horizontal: tuning.knockback.horizontal,
                        vertical: tuning.knockback.vertical,
                        vertical_cap: tuning.knockback.vertical_cap,
                        retain: tuning.knockback.retain,
                    };
                    event.hurt_ticks = tuning.hurt_ticks;
                    event.bypass_armour = tuning.bypass_armour;
                    event.post_armour_multiplier = tuning.post_armour_multiplier;
                }
                Ok(None) => {
                    event.cancel();
                    break;
                }
                Ok(Some(_)) => {
                    self.callback_failed(world, &owner, "invalid damage tuning");
                    event.cancel();
                    break;
                }
                Err(error) => {
                    self.callback_failed(world, &owner, &error.to_string());
                    event.cancel();
                    break;
                }
            }
        }
    }

    /// Observe only positive loss emitted by the engine after its final damage rules.
    pub(super) fn player_damaged(&mut self, world: &mut World, event: &PlayerDamaged) {
        if !self.begin_tick(world) {
            return;
        }
        let Ok(identity) = players::owned_identity(world, event.player) else {
            return;
        };
        let Some(chunks) = world.get_resource::<crate::world::ChunkManager>() else {
            return;
        };
        let record = wit::DamageCommitted {
            identity,
            dimension: chunks.dimension(),
            cause: damage_cause(event.hit.cause),
            attribution: attribution(event.attribution()),
            incoming_damage: event.hit.damage,
            actual_health_loss: event.damage,
            knockback: knockback(event.hit.knockback_power),
        };
        for owner in self.subscribers(EventKind::PlayerDamaged) {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let result = invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let player = make_player_resource(store, event.player)?;
                bindings.call_on_player_damaged(store, tx, player, &record)
            });
            if let Err(error) = result {
                self.callback_failed(world, &owner, &error.to_string());
                if self.readiness.check().is_err() {
                    break;
                }
            }
        }
    }
}
