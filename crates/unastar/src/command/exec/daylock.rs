//! `/daylock [lock]` (alias `/alwaysday`).
//!
//! Measured on BDS 1.26.51: locking (the default) turns the
//! `doDayLightCycle` rule off and moves the clock to noon (time of day 6000),
//! unlocking turns the rule on; a rule that changes is broadcast as
//! `GameRulesChanged` with that rule only, and the source gets a
//! `Textobject` `commands.always.day.locked` / `…unlocked` instead of any
//! command output. [INFERENCE] Noon of the current day.

use crate::command::prelude::*;
use crate::world::clock;
use crate::world::rules::{GameRule, GameRules, WorldTime};
use jolyne::valentine::GameRulesChangedPacket;
use jolyne::valentine::types::GameRulesChangedPacketData;

overloads! {
    pub enum DayLockArgs for "daylock" {
        0 => Lock { lock: Option<bool> },
    }
}

/// The time of day a lock holds.
const NOON: i64 = 6_000;

pub fn run(ctx: &mut CommandContext<'_>, args: DayLockArgs) -> CommandResult {
    let DayLockArgs::Lock { lock } = args;
    let lock = lock.unwrap_or(true);
    let mut rules = ctx.world.resource_mut::<GameRules>();
    if rules.get(GameRule::DoDaylightCycle) == lock {
        rules.set(GameRule::DoDaylightCycle, !lock);
        let wire = rules.wire(GameRule::DoDaylightCycle);
        ctx.broadcast(GameRulesChangedPacket {
            rule_data: GameRulesChangedPacketData {
                rules_list: vec![wire],
            },
        });
    }
    if lock {
        let now = *ctx.world.resource::<WorldTime>();
        clock::set_time(ctx.world, now.ticks - now.time_of_day() + NOON);
    }
    let key = if lock {
        "commands.always.day.locked"
    } else {
        "commands.always.day.unlocked"
    };
    if let Some(source) = ctx.source.entity() {
        ctx.notify(source, key, vec![]);
    }
    Ok(CommandOutput::default())
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn daylock_and_toggledownfall_match_the_bds_capture() {
        let steps = Replay::new().assert_segment("daylock", &[]);
        assert_same_packets(
            &steps,
            &["GameRulesChangedPacket", "TextPacket", "LevelEventPacket"],
            &[],
        );
    }
}
