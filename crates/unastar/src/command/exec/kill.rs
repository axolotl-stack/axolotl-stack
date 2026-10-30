//! `/kill [target]`.
//!
//! Measured (BDS 1.26.51): mobs die (death event, drops) and the answer is
//! `commands.kill.successful` with the killed names joined by `, ` and the
//! success count; a creative target is skipped with
//! `commands.kill.attemptKillPlayerCreative` (a successful message) and, when
//! nothing died, `commands.generic.noTargetMatch` follows. Players die
//! through the player life systems; dropped items and orbs are removed.

use crate::command::prelude::*;
use crate::entity::components::{GameMode, Health, Player};
use crate::entity::mob::MobKind;
use crate::entity::mob::combat::{Hit, hurt_mob};
use crate::entity::mob::state::Dead;

overloads! {
    pub enum KillArgs for "kill" {
        0 => Kill { target: Option<Target> },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: KillArgs) -> CommandResult {
    let KillArgs::Kill { target } = args;
    let targets = ctx.entities(target.as_ref())?;
    let mut output = CommandOutput::default();
    let mut killed = Vec::new();
    for target in targets {
        let name = ctx.name(target);
        let died = if ctx.world.get::<Player>(target).is_some() {
            if ctx.world.get::<GameMode>(target) == Some(&GameMode::Creative) {
                output.add_message("commands.kill.attemptKillPlayerCreative", vec![]);
                continue;
            }
            crate::entity::player_life::kill_player(ctx.world, target)
        } else if ctx.world.get::<MobKind>(target).is_some() {
            kill_mob(ctx.world, target)
        } else {
            ctx.world.despawn(target);
            true
        };
        if died {
            killed.push(name);
        }
    }
    if killed.is_empty() {
        output.add_failure("commands.generic.noTargetMatch", vec![]);
        return Err(output);
    }
    output.add_message("commands.kill.successful", vec![killed.join(", ")]);
    Ok(output.with_count(killed.len()))
}

/// Override damage of the mob's whole health, as `/kill` deals it (it ignores
/// the hurt timer); returns whether the mob died.
fn kill_mob(world: &mut World, mob: Entity) -> bool {
    let Some(health) = world.get::<Health>(mob).map(|h| h.current) else {
        return false;
    };
    let hit = Hit {
        damage: health,
        cause: unastar_data::actor_components::body::ActorDamageCause::Override,
        attacker: None,
        by_player: false,
        knockback_from: None,
        knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
    };
    hurt_mob(world, mob, hit) > 0.0 && world.get::<Dead>(mob).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;

    #[test]
    fn mobs_die_and_creative_players_are_spared() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        h.run(op, "/summon cow ~ ~ ~3");
        h.run(op, "/summon pig ~ ~ ~3 0 0 minecraft:entity_spawned Bob");
        assert_eq!(
            h.run(op, "/kill @e[type=cow]"),
            CommandOutput::success("commands.kill.successful", vec!["Cow".into()])
        );
        assert_eq!(
            h.run(op, "/kill @e[type=pig,c=1]"),
            CommandOutput::success("commands.kill.successful", vec!["Bob".into()])
        );
        assert_eq!(
            h.run(op, "/kill @e[type=zombie]"),
            CommandOutput::no_targets()
        );

        h.run(op, "/gamemode 1");
        let output = h.run(op, "/kill @s");
        assert_eq!(output.success_count, 0);
        assert_eq!(output.messages.len(), 2);
        assert_eq!(
            output.messages[0].key,
            "commands.kill.attemptKillPlayerCreative"
        );
        assert!(output.messages[0].success);
        assert_eq!(output.messages[1].key, "commands.generic.noTargetMatch");
        assert!(!output.messages[1].success);
    }
}
