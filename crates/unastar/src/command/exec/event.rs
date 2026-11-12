//! `/event entity <target> <eventName>`: fires an actor definition event
//! (the same events a definition's own triggers fire).
//!
//! Measured on BDS 1.26.51: `commands.event.success [event, type]` for a
//! mob that has the event and `commands.event.error.failed [event, type]`
//! for one that has not, naming the target by its type identifier.
//!
//! Players have no actor definition in unastar, so an event never runs on a
//! player.

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
        let handle = ctx
            .world
            .get::<MobKind>(target)
            .and_then(|kind| kind.0.events.event(&event_name));
        let ok = handle.is_some_and(|event| fire_event(ctx.world, target, &event, None).is_ok());
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
