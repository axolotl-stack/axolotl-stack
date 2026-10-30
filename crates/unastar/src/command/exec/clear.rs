//! `/clear [player] [itemName] [data] [maxCount]`.
//!
//! Measured (BDS 1.26.51): matching stacks leave the main inventory, armour,
//! offhand and the UI cursor; the answer is `commands.clear.success [player,
//! removed count]` (stacks with item components count like any other) and
//! the player gets its whole inventory resent. Nothing to remove is
//! `commands.clear.failure.no.items [player]`. `maxCount` caps the removal;
//! `0` only counts (`commands.clear.testing [player, count]`) and, finding
//! nothing, sends no command output at all (`/clear @s wool 0 0`). `data`
//! `-1` or omitted matches every data value.

use crate::command::prelude::*;
use crate::entity::components::{ArmourInventory, MainInventory, OffhandSlot, PlayerUi};
use crate::item::ItemStack;
use crate::item::enchant::durability_damage;
use crate::server::game::types::ItemRegistryResource;

overloads! {
    pub enum ClearArgs for "clear" {
        0 => Clear {
            player: Option<Target>,
            item_name: Option<EnumValue> = "itemName",
            data: Option<i32>,
            max_count: Option<i32> = "maxCount",
        },
    }
}

/// Which stacks `/clear` takes.
#[derive(Debug, Clone, Copy)]
enum Filter {
    Everything,
    /// No registered item answers the name and data: nothing matches.
    Nothing,
    Item {
        id: u32,
        data: Option<i16>,
    },
}

impl Filter {
    fn matches(self, stack: &ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        match self {
            Self::Everything => true,
            Self::Nothing => false,
            Self::Item { id, data } => {
                let Some(item) = stack.item() else {
                    return false;
                };
                item.id == id
                    && data.is_none_or(|data| {
                        let aux = if item.tool_info().max_damage > 0 {
                            durability_damage(stack)
                        } else {
                            i32::from(stack.damage)
                        };
                        aux == i32::from(data)
                    })
            }
        }
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: ClearArgs) -> CommandResult {
    let ClearArgs::Clear {
        player,
        item_name,
        data,
        max_count,
    } = args;
    let filter = match item_name {
        None => Filter::Everything,
        Some(name) => {
            let spelled = match data {
                Some(data) if data != -1 => format!("{}:{data}", &*name),
                _ => name.to_string(),
            };
            match ctx
                .world
                .resource::<ItemRegistryResource>()
                .0
                .resolve_identity(&spelled)
            {
                Some(identity) => Filter::Item {
                    id: identity.id,
                    data: identity.data,
                },
                None => Filter::Nothing,
            }
        }
    };
    // `None`: no limit.
    let limit = max_count
        .filter(|count| *count >= 0)
        .map(|count| count as u32);
    let testing = limit == Some(0);

    let targets = ctx.players(player.as_ref())?;
    let mut output = CommandOutput::default();
    for target in targets {
        let name = ctx.name(target);
        let count = if testing {
            count_matching(ctx.world, target, filter)
        } else {
            clear(ctx.world, target, filter, limit)
        };
        if count == 0 {
            if !testing {
                output.add_failure("commands.clear.failure.no.items", vec![name]);
            }
            continue;
        }
        if testing {
            output.add_success("commands.clear.testing", vec![name, count.to_string()]);
        } else {
            crate::server::game::send_inventory_contents(ctx.world, target);
            output.add_success("commands.clear.success", vec![name, count.to_string()]);
        }
    }
    if output.is_success() {
        Ok(output)
    } else {
        Err(output)
    }
}

/// Items of `player` matching `filter`, over every container `/clear` reads.
fn count_matching(world: &World, player: Entity, filter: Filter) -> u32 {
    let count = |slots: &[ItemStack]| -> u32 {
        slots
            .iter()
            .filter(|stack| filter.matches(stack))
            .map(|stack| u32::from(stack.count))
            .sum()
    };
    world
        .get::<MainInventory>(player)
        .map_or(0, |main| count(main.0.slots()))
        + world
            .get::<ArmourInventory>(player)
            .map_or(0, |armour| count(armour.slots()))
        + world
            .get::<OffhandSlot>(player)
            .map_or(0, |offhand| count(std::slice::from_ref(&offhand.0)))
        + world
            .get::<PlayerUi>(player)
            .map_or(0, |ui| count(std::slice::from_ref(ui.cursor())))
}

/// Remove up to `limit` (`None`: all) matching items from `player`'s main
/// inventory, armour, offhand and cursor, in that order; returns how many.
fn clear(world: &mut World, player: Entity, filter: Filter, mut limit: Option<u32>) -> u32 {
    let mut removed = 0;
    let mut take = |slots: &mut [ItemStack]| {
        for slot in slots {
            if limit == Some(0) {
                return;
            }
            if !filter.matches(slot) {
                continue;
            }
            let count = limit.map_or(u32::from(slot.count), |left| {
                left.min(u32::from(slot.count))
            });
            *slot = slot.grow(-(count as i16)).into_slot();
            removed += count;
            if let Some(left) = &mut limit {
                *left -= count;
            }
        }
    };
    let mut entity = world.entity_mut(player);
    if let Some(mut main) = entity.get_mut::<MainInventory>() {
        take(main.0.slots_mut());
    }
    if let Some(mut armour) = entity.get_mut::<ArmourInventory>() {
        take(armour.inventory_mut().slots_mut());
    }
    if let Some(mut offhand) = entity.get_mut::<OffhandSlot>() {
        take(std::slice::from_mut(&mut offhand.0));
    }
    if let Some(mut ui) = entity.get_mut::<PlayerUi>() {
        let cursor = crate::entity::components::CURSOR_SLOT;
        take(&mut ui.0.slots_mut()[cursor..=cursor]);
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use jolyne::valentine::McpePacketData;

    fn carried(h: &mut Harness, player: Entity) -> u32 {
        count_matching(h.world(), player, Filter::Everything)
    }

    #[test]
    fn clearing_matches_bds() {
        let mut h = Harness::new();
        let op = h.player("BDSExtractor", true);
        h.run(op, "/give @s wheat 64");
        h.run(op, "/give @s minecraft:diamond_sword");
        h.run(op, "/give @s stone 600");
        h.run(op, r#"/give @s wheat 64 0 {"minecraft:keep_on_death":{}}"#);
        h.sent(op);

        assert_eq!(
            h.run(op, "/clear @s wheat"),
            CommandOutput::success(
                "commands.clear.success",
                vec!["BDSExtractor".into(), "128".into()]
            )
        );
        assert!(
            h.sent(op)
                .iter()
                .any(|p| matches!(p, McpePacketData::InventoryContentPacket(_)))
        );
        assert_eq!(
            h.run(op, "/clear @s wheat 0 10"),
            CommandOutput::failure(
                "commands.clear.failure.no.items",
                vec!["BDSExtractor".into()]
            )
        );

        // The cursor and offhand are cleared too.
        let world = h.world();
        world.get_mut::<OffhandSlot>(op).unwrap().0 =
            crate::item::test_support::stack("minecraft:arrow", 5);
        world
            .get_mut::<PlayerUi>(op)
            .unwrap()
            .set_cursor(crate::item::test_support::stack("minecraft:stick", 1));
        assert_eq!(carried(&mut h, op), 607);
        assert_eq!(
            h.run(op, "/clear"),
            CommandOutput::success(
                "commands.clear.success",
                vec!["BDSExtractor".into(), "607".into()]
            )
        );
        assert_eq!(carried(&mut h, op), 0);
        assert_eq!(
            h.run(op, "/clear @s diamond"),
            CommandOutput::failure(
                "commands.clear.failure.no.items",
                vec!["BDSExtractor".into()]
            )
        );
    }

    #[test]
    fn max_count_caps_and_zero_only_counts() {
        let mut h = Harness::new();
        let op = h.player("BDSExtractor", true);
        h.run(op, "/give @s stone 100");
        assert_eq!(
            h.run(op, "/clear @s stone -1 0"),
            CommandOutput::success(
                "commands.clear.testing",
                vec!["BDSExtractor".into(), "100".into()]
            )
        );
        assert_eq!(carried(&mut h, op), 100);
        assert_eq!(
            h.run(op, "/clear @s stone 0 70"),
            CommandOutput::success(
                "commands.clear.success",
                vec!["BDSExtractor".into(), "70".into()]
            )
        );
        assert_eq!(carried(&mut h, op), 30);
        // Counting nothing sends no output.
        let none = h.run(op, "/clear @s wool 0 0");
        assert!(none.messages.is_empty() && !none.is_success());
        // Data values select items.
        assert_eq!(
            h.run(op, "/clear @s stone 3"),
            CommandOutput::failure(
                "commands.clear.failure.no.items",
                vec!["BDSExtractor".into()]
            )
        );
    }
}
