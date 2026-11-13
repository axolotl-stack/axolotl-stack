//! `/event entity <target> <eventName>`: fires an actor definition event
//! (the same events a definition's own triggers fire).
//!
//! Measured on BDS 1.26.51: `commands.event.success [event, type]` for a
//! mob that has the event and `commands.event.error.failed [event, type]`
//! for one that has not, naming the target by its type identifier.
//!
//! A player answers like BDS for the events of the vanilla `player.json`
//! (`minecraft:gain_raid_omen`, `minecraft:trigger_raid`, …): measured, they
//! succeed and change nothing a client sees (the raid omen spell effect of
//! `gain_raid_omen` is not applied, `/effect @s clear` right after finds no
//! effect), so unastar answers them without running the component groups.

use crate::command::context::actor_type;
use crate::command::prelude::*;
use crate::entity::components::Player;
use crate::entity::mob::MobKind;
use crate::entity::mob::events::fire_event;

overloads! {
    pub enum EventArgs for "event" {
        0 => Fire { entity: EnumValue, target: Target, event_name: SoftEnumValue = "eventName" },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: EventArgs) -> CommandResult {
    let EventArgs::Fire {
        target, event_name, ..
    } = args;
    let targets = ctx.entities(Some(&target))?;
    let mut fired = Vec::new();
    let mut failed = Vec::new();
    for target in targets {
        let ok = if ctx.world.get::<Player>(target).is_some() {
            player_has_event(&event_name)
        } else {
            let handle = ctx
                .world
                .get::<MobKind>(target)
                .and_then(|kind| kind.0.events.event(&event_name));
            handle.is_some_and(|event| fire_event(ctx.world, target, &event, None).is_ok())
        };
        let kind = type_identifier(ctx.world, target);
        if ok {
            fired.push(kind);
        } else {
            failed.push(kind);
        }
    }
    crate::entity::definition::apply_pending_definitions(ctx.world);
    // Measured: two cows answer one message naming `minecraft:cow` once,
    // success count 1. [INFERENCE] Several types are listed once each.
    let distinct = |kinds: Vec<String>| {
        let mut out: Vec<String> = Vec::new();
        for kind in kinds {
            if !out.contains(&kind) {
                out.push(kind);
            }
        }
        out.join(", ")
    };
    let mut output = CommandOutput::default();
    if !fired.is_empty() {
        output.add_success(
            "commands.event.success",
            vec![event_name.to_string(), distinct(fired.clone())],
        );
    }
    if !failed.is_empty() {
        output.add_failure(
            "commands.event.error.failed",
            vec![event_name.to_string(), distinct(failed)],
        );
    }
    if fired.is_empty() {
        return Err(output);
    }
    Ok(output)
}

/// Whether the vanilla player definition has the event.
fn player_has_event(name: &str) -> bool {
    unastar_data::actor_definitions::get("minecraft:player")
        .is_some_and(|definition| definition.events.contains_key(name))
}

fn type_identifier(world: &World, entity: Entity) -> String {
    if world.get::<Player>(entity).is_some() {
        return "minecraft:player".into();
    }
    actor_type(world, entity).unwrap_or_default()
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn event_matches_the_bds_capture() {
        Replay::new().assert_segment("event", &[]);
    }
}
