//! `/gamerule [rule] [value]` over every BDS rule (`world::rules::GameRules`).
//!
//! Measured (BDS 1.26.51): the list and a query answer a literal message
//! (`keepinventory = true`; the list with BDS's spellings joined by `, `);
//! a change broadcasts `GameRulesChanged` with that rule only and answers
//! `commands.gamerule.success [rule as typed, value]`. `playerWaypoints` is an
//! int on the wire, shown and set as `everyone`/`off`.

use crate::command::prelude::*;
use crate::world::rules::GameRules;
use jolyne::valentine::GameRulesChangedPacket;
use jolyne::valentine::types::GameRulesChangedPacketData;
use unastar_data::game_rules::GameRuleValue;

arg_enum! {
    /// Captured `playerwaypointsValues`.
    pub enum Waypoints in "playerwaypointsValues" {
        Everyone = "everyone",
        Off = "off",
    }
}

impl Waypoints {
    /// [INFERENCE] 1 is `everyone` (the BDS default, shown as `everyone`);
    /// 0 is the enum's other value.
    const fn wire(self) -> i32 {
        match self {
            Self::Everyone => 1,
            Self::Off => 0,
        }
    }
}

overloads! {
    pub enum GameRuleArgs for "gamerule" {
        0 => List {},
        1 => Waypoints { rule: EnumValue, value: Waypoints },
        2 => Bool { rule: EnumValue, value: Option<bool> },
        3 => Int { rule: EnumValue, value: Option<i32> },
    }
}

const PLAYER_WAYPOINTS: &str = "playerWaypoints";

/// How BDS prints a rule value.
fn shown(name: &str, value: GameRuleValue) -> String {
    match (name, value) {
        (PLAYER_WAYPOINTS, GameRuleValue::Int(1)) => "everyone".into(),
        (PLAYER_WAYPOINTS, GameRuleValue::Int(0)) => "off".into(),
        _ => value.to_string(),
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: GameRuleArgs) -> CommandResult {
    let (typed, value) = match args {
        GameRuleArgs::List {} => {
            let list = ctx
                .world
                .resource::<GameRules>()
                .iter()
                .map(|(rule, value)| format!("{} = {}", rule.name, shown(&rule.name, value)))
                .collect::<Vec<_>>()
                .join(", ");
            return success!(list);
        }
        GameRuleArgs::Waypoints { rule, value } => (rule, Some(GameRuleValue::Int(value.wire()))),
        GameRuleArgs::Bool { rule, value } => (rule, value.map(GameRuleValue::Bool)),
        GameRuleArgs::Int { rule, value } => (rule, value.map(GameRuleValue::Int)),
    };
    let (rule, current) = ctx
        .world
        .resource::<GameRules>()
        .value(&typed)
        .ok_or_else(|| {
            CommandOutput::failure("commands.gamerule.norule", vec![typed.to_string()])
        })?;
    let Some(value) = value else {
        return success!(format!("{} = {}", &*typed, shown(&rule.name, current)));
    };
    ctx.world
        .resource_mut::<GameRules>()
        .set_value(&rule.name, value)
        .ok_or_else(|| {
            CommandOutput::failure("commands.gamerule.type.invalid", vec![typed.to_string()])
        })?;
    let wire = ctx
        .world
        .resource::<GameRules>()
        .wire_named(&rule.name)
        .expect("a captured rule");
    ctx.broadcast(GameRulesChangedPacket {
        rule_data: GameRulesChangedPacketData {
            rules_list: vec![wire],
        },
    });
    success!(
        "commands.gamerule.success",
        &*typed,
        shown(&rule.name, value)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;

    #[test]
    fn waypoint_values_cover_the_captured_enum() {
        crate::command::args::assert_covers::<Waypoints>();
    }

    #[test]
    fn list_query_and_set_match_the_bds_capture() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let list = h.run(op, "/gamerule");
        assert_eq!(list.success_count, 1);
        let text = &list.messages[0].key;
        assert!(
            text.starts_with(
                "commandBlockOutput = true, doDayLightCycle = true, doEntityDrops = true"
            ),
            "{text}"
        );
        assert!(text.contains("playerWaypoints = everyone, locatorbar = true"));
        assert!(text.contains("randomTickSpeed = 1"));

        assert_eq!(
            h.run(op, "/gamerule keepinventory true"),
            CommandOutput::success(
                "commands.gamerule.success",
                vec!["keepinventory".into(), "true".into()]
            )
        );
        assert!(h.world().resource::<GameRules>().keep_inventory);
        assert_eq!(
            h.run(op, "/gamerule keepinventory"),
            CommandOutput::success("keepinventory = true", vec![])
        );
        assert_eq!(
            h.run(op, "/gamerule randomtickspeed 5"),
            CommandOutput::success(
                "commands.gamerule.success",
                vec!["randomtickspeed".into(), "5".into()]
            )
        );
        assert_eq!(
            h.run(op, "/gamerule randomtickspeed"),
            CommandOutput::success("randomtickspeed = 5", vec![])
        );
        assert_eq!(
            h.run(op, "/gamerule playerwaypoints off"),
            CommandOutput::success(
                "commands.gamerule.success",
                vec!["playerwaypoints".into(), "off".into()]
            )
        );
        assert!(
            h.run(op, "/gamerule").messages[0]
                .key
                .contains("playerWaypoints = off")
        );
        let announced: Vec<_> = h
            .sent(op)
            .iter()
            .filter_map(|p| match p {
                jolyne::valentine::McpePacketData::GameRulesChangedPacket(p) => {
                    Some(p.rule_data.rules_list[0].rule_name.to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            announced,
            ["keepInventory", "randomTickSpeed", "playerWaypoints"]
        );
    }
}
