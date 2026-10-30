//! `/xp <amount> [player]` and `/xp <amount>L [player]`.
//!
//! Measured on BDS 1.26.51: points answer `commands.xp.success` (amount,
//! name) and levels `commands.xp.success.levels`, or for a negative level
//! count `commands.xp.success.negative.levels` with the absolute amount
//! asked for (`/xp -100L` on a level-6 player says 100). Each target then
//! gets `UpdateAttributes` with its level and progress. Negative points are
//! refused with `commands.xp.failure.widthdrawXp`.

use crate::command::prelude::*;
use crate::entity::components::Experience;
use crate::entity::player_life::attributes::send_attributes;
use crate::entity::xp_orb::{add_experience, experience_attributes};

overloads! {
    pub enum XpArgs for "xp" {
        0 => Points { amount: i32, player: Option<Target> },
        1 => Levels { amount: Postfixed, player: Option<Target> },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: XpArgs) -> CommandResult {
    let (change, player) = match args {
        XpArgs::Points { amount, player } => {
            if amount < 0 {
                return failure!("commands.xp.failure.widthdrawXp");
            }
            (Change::Points(amount.unsigned_abs()), player)
        }
        XpArgs::Levels {
            amount: Postfixed(amount),
            player,
        } => (Change::Levels(amount), player),
    };
    let players = ctx.players(player.as_ref())?;
    let mut output = CommandOutput::default();
    for player in players {
        let mut experience = ctx
            .world
            .get::<Experience>(player)
            .cloned()
            .unwrap_or_default();
        change.apply(&mut experience);
        send_attributes(
            ctx.world,
            player,
            experience_attributes(&experience).to_vec(),
        );
        ctx.world.entity_mut(player).insert(experience);
        let name = ctx.name(player);
        match change {
            Change::Points(points) => {
                output.add_success("commands.xp.success", vec![points.to_string(), name]);
            }
            Change::Levels(levels) if levels < 0 => output.add_success(
                "commands.xp.success.negative.levels",
                vec![levels.unsigned_abs().to_string(), name],
            ),
            Change::Levels(levels) => {
                output.add_success("commands.xp.success.levels", vec![levels.to_string(), name]);
            }
        }
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
enum Change {
    Points(u32),
    Levels(i32),
}

impl Change {
    fn apply(self, experience: &mut Experience) {
        match self {
            Self::Points(points) => add_experience(experience, points),
            Self::Levels(levels) => {
                let level = experience.level.saturating_add(levels);
                // Measured: taking more levels than the player has leaves
                // level 0 with no progress; otherwise progress is kept.
                if level < 0 {
                    *experience = Experience::default();
                } else {
                    experience.level = level;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use jolyne::valentine::McpePacketData;

    fn sent_attributes(h: &mut Harness, player: Entity) -> Vec<(String, f32)> {
        h.sent(player)
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::UpdateAttributesPacket(p) => Some(p.attribute_list),
                _ => None,
            })
            .flatten()
            .map(|a| (a.name.to_string(), a.current_value))
            .collect()
    }

    fn experience(h: &mut Harness, player: Entity) -> (i32, f32) {
        let xp = h.world().get::<Experience>(player).unwrap().clone();
        (xp.level, xp.progress)
    }

    #[test]
    fn points_levels_and_negative_levels_match_the_bds_capture() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);

        let output = h.run(alice, "/xp 10");
        assert_eq!(
            output,
            CommandOutput::success("commands.xp.success", vec!["10".into(), "Alice".into()])
        );
        let (level, progress) = experience(&mut h, alice);
        assert_eq!(level, 1);
        assert!((progress - 1.0 / 3.0).abs() < 1e-5);
        let attributes = sent_attributes(&mut h, alice);
        assert!(attributes.contains(&("minecraft:player.level".into(), 1.0)));

        let output = h.run(alice, "/xp 5L");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.xp.success.levels",
                vec!["5".into(), "Alice".into()]
            )
        );
        let (level, progress) = experience(&mut h, alice);
        assert_eq!(level, 6);
        assert!((progress - 1.0 / 3.0).abs() < 1e-5);

        let output = h.run(alice, "/xp -100L");
        assert_eq!(
            output,
            CommandOutput::success(
                "commands.xp.success.negative.levels",
                vec!["100".into(), "Alice".into()]
            )
        );
        assert_eq!(experience(&mut h, alice), (0, 0.0));
        let attributes = sent_attributes(&mut h, alice);
        assert!(attributes.contains(&("minecraft:player.level".into(), 0.0)));
        assert!(attributes.contains(&("minecraft:player.experience".into(), 0.0)));
    }

    #[test]
    fn negative_points_are_refused_and_other_players_can_be_targeted() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        let bob = h.player("Bob", false);
        assert_eq!(
            h.run(alice, "/xp -5"),
            CommandOutput::failure("commands.xp.failure.widthdrawXp", vec![])
        );
        assert_eq!(experience(&mut h, alice), (0, 0.0));

        let output = h.run(alice, "/xp 2L Bob");
        assert_eq!(output.messages[0].params, ["2", "Bob"]);
        assert_eq!(experience(&mut h, bob).0, 2);
        assert_eq!(experience(&mut h, alice).0, 0);
        assert_eq!(
            h.run(alice, "/xp 10 NoSuchPlayer"),
            CommandOutput::no_targets()
        );
    }
}
