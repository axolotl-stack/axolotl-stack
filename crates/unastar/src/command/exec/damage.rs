//! `/damage <target> <amount> [cause]` and
//! `/damage <target> <amount> <cause> entity <damager>`.
//!
//! The damage goes through the same hurt rules as any other hit (the hurt
//! timer, armour, difficulty scaling of a mob attacker; with a damager the
//! target is knocked back from it).
//!
//! Measured on BDS 1.26.51: the answer is `commands.damage.success` with the
//! names of the entities hit (success count: how many), zero damage
//! included; a creative or spectator player is not hit, whatever the cause
//! (`void` too), and answers `commands.damage.failed`; a negative amount is
//! `commands.generic.num.tooSmall [amount, 0]`. The default cause is
//! `none`.

use crate::command::prelude::*;
use crate::entity::components::{GameMode, Health, Player};
use crate::entity::mob::MobKind;
use crate::entity::mob::combat::{Hit, hurt_mob};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::state::Dead;
use crate::entity::player_life::hurt::hurt_player;
use unastar_data::actor_components::body::ActorDamageCause;

overloads! {
    pub enum DamageArgs for "damage" {
        0 => Damage { target: Target, amount: i32, cause: Option<EnumValue> },
        1 => ByActor {
            target: Target,
            amount: i32,
            cause: EnumValue,
            origin: EnumValue,
            damager: Target,
        },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: DamageArgs) -> CommandResult {
    let (target, amount, cause, damager) = match args {
        DamageArgs::Damage {
            target,
            amount,
            cause,
        } => (target, amount, cause, None),
        DamageArgs::ByActor {
            target,
            amount,
            cause,
            damager,
            ..
        } => (target, amount, Some(cause), Some(damager)),
    };
    if amount < 0 {
        return failure!("commands.generic.num.tooSmall", amount, 0);
    }
    let targets = ctx.entities(Some(&target))?;
    let attacker = match damager {
        Some(damager) => match ctx.entities(Some(&damager))?.as_slice() {
            [one] => Some(*one),
            _ => return failure!("commands.damage.tooManySources"),
        },
        None => None,
    };
    let cause = cause.map_or(ActorDamageCause::None, |cause| damage_cause(&cause));
    let by_player = attacker.is_some_and(|a| ctx.world.get::<Player>(a).is_some());
    let knockback_from = attacker.and_then(|a| ctx.feet(a));
    let mut hurt = Vec::new();
    let mut spared = Vec::new();
    for target in targets {
        if !can_be_hit(ctx.world, target) {
            spared.push(target);
            continue;
        }
        let hit = Hit {
            damage: amount as f32,
            cause,
            attacker,
            by_player,
            knockback_from,
            knockback_power: KnockbackPower::DEFAULT,
        };
        if ctx.world.get::<Player>(target).is_some() {
            hurt_player(ctx.world, target, &hit);
        } else {
            hurt_mob(ctx.world, target, hit);
        }
        hurt.push(target);
    }
    if hurt.is_empty() {
        return failure!("commands.damage.failed", ctx.names(&spared));
    }
    Ok(
        CommandOutput::success("commands.damage.success", vec![ctx.names(&hurt)])
            .with_count(hurt.len()),
    )
}

/// A living player that is not in creative or spectator, or a living mob.
fn can_be_hit(world: &World, target: Entity) -> bool {
    let alive = world.get::<Health>(target).is_some_and(|h| !h.is_dead())
        && world.get::<Dead>(target).is_none();
    if !alive {
        return false;
    }
    if world.get::<Player>(target).is_some() {
        return world
            .get::<GameMode>(target)
            .copied()
            .unwrap_or_default()
            .allows_damage();
    }
    world.get::<MobKind>(target).is_some()
}

/// A captured `DamageCause` value as the actor damage cause it names.
fn damage_cause(name: &str) -> ActorDamageCause {
    serde_json::from_value(serde_json::Value::String(name.to_owned()))
        .expect("every captured DamageCause value is an actor damage cause")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_captured_cause_is_a_damage_cause() {
        let causes = unastar_data::commands::vanilla()
            .enum_named("DamageCause")
            .expect("captured DamageCause");
        for cause in &causes.values {
            damage_cause(cause);
        }
        assert_eq!(
            damage_cause("entity_attack"),
            ActorDamageCause::EntityAttack
        );
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, segment};
    use crate::entity::components::Health;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn damage_matches_the_bds_capture() {
        Replay::new().assert_segment("damage", &[]);
    }

    /// After every command, the player's health is what BDS's
    /// `UpdateAttributes` for it said.
    #[test]
    fn damage_leaves_the_health_bds_reports() {
        let mut replay = Replay::new();
        let mut checked = 0;
        for captured in segment("damage") {
            replay.run(&captured.command);
            let reported = captured.packets.iter().find_map(|packet| match packet {
                McpePacketData::UpdateAttributesPacket(update)
                    if update.target_runtime_id.actor_runtime_id == 1 =>
                {
                    update
                        .attribute_list
                        .iter()
                        .find(|a| &*a.name == "minecraft:health")
                        .map(|a| a.current_value)
                }
                _ => None,
            });
            if let Some(health) = reported {
                let ours = replay
                    .h
                    .world()
                    .get::<Health>(replay.player)
                    .unwrap()
                    .current;
                assert_eq!(ours, health, "{}", captured.command);
                checked += 1;
            }
        }
        // `/damage @s 2`, `… 3 fall` and the cow's hit.
        assert_eq!(checked, 3);
    }
}
