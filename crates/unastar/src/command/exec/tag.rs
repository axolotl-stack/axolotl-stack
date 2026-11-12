//! `/tag <entity> add|remove <name>` and `/tag <entity> list`, on the
//! [`Tags`] component (the selector argument `tag=` reads it).
//!
//! Measured on BDS 1.26.51:
//! - adding answers `commands.tag.add.success.single [tag, name]`, and a tag
//!   no entity had before is added to the `TagValues` soft enum
//!   (`UpdateSoftEnum` add); a target that already has it fails with
//!   `commands.tag.add.failed`;
//! - removing answers `commands.tag.remove.success.single [tag, name]`, and
//!   the last holder's removal takes the tag out of the soft enum; a target
//!   without it fails with `commands.tag.remove.failed`;
//! - listing answers `commands.tag.list.single.success [name, count, tags]`
//!   with each tag in green (`§a…§r`), `…list.single.empty [name]` without
//!   tags, and for several targets `…list.multiple.success [targets, tags,
//!   list]` or `…list.multiple.empty [targets]`;
//! - several targets answer `…add|remove.success.multiple [tag, count]` with
//!   success count 1.
//!
//! Tags are not saved with the entity yet.

use crate::command::prelude::*;
use crate::entity::components::Tags;
use jolyne::valentine::UpdateSoftEnumPacket;
use jolyne::valentine::types::EnumsSoftEnumUpdateType;

arg_enum! {
    /// Captured `TagChangeAction`.
    pub enum TagChange in "TagChangeAction" {
        Add = "add",
        Remove = "remove",
    }
}

overloads! {
    pub enum TagArgs for "tag" {
        0 => Change { entity: WildcardTarget, action: TagChange, name: SoftEnumValue },
        1 => List { entity: WildcardTarget, action: EnumValue },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: TagArgs) -> CommandResult {
    match args {
        TagArgs::Change {
            entity,
            action,
            name,
        } => {
            let targets = targets(ctx, &entity)?;
            change(ctx, &targets, action, &name)
        }
        TagArgs::List { entity, .. } => {
            let targets = targets(ctx, &entity)?;
            list(ctx, &targets)
        }
    }
}

/// `*` is every player ([INFERENCE]: the wildcard is not captured for `/tag`).
fn targets(
    ctx: &mut CommandContext<'_>,
    entity: &WildcardTarget,
) -> Result<Vec<Entity>, CommandOutput> {
    match &entity.0 {
        Some(target) => ctx.entities(Some(target)),
        None => {
            let all = crate::command::selector::Selector {
                kind: Some(crate::command::selector::SelectorKind::AllPlayers),
                ..Default::default()
            };
            ctx.entities(Some(&Target::Selector(Box::new(all))))
        }
    }
}

fn change(
    ctx: &mut CommandContext<'_>,
    targets: &[Entity],
    action: TagChange,
    tag: &str,
) -> CommandResult {
    let held_before = anyone_has(ctx.world, tag);
    let mut changed = Vec::new();
    for &target in targets {
        let mut tags = ctx.world.get::<Tags>(target).cloned().unwrap_or_default();
        let done = match action {
            TagChange::Add if !tags.contains(tag) => {
                tags.0.push(tag.to_owned());
                true
            }
            TagChange::Remove if tags.contains(tag) => {
                tags.0.retain(|t| t != tag);
                true
            }
            _ => false,
        };
        if done {
            ctx.world.entity_mut(target).insert(tags);
            changed.push(target);
        }
    }
    let (single, multiple, failed) = match action {
        TagChange::Add => (
            "commands.tag.add.success.single",
            "commands.tag.add.success.multiple",
            "commands.tag.add.failed",
        ),
        TagChange::Remove => (
            "commands.tag.remove.success.single",
            "commands.tag.remove.success.multiple",
            "commands.tag.remove.failed",
        ),
    };
    if changed.is_empty() {
        return failure!(failed);
    }
    let held_after = anyone_has(ctx.world, tag);
    let update = match (held_before, held_after) {
        (false, true) => Some(EnumsSoftEnumUpdateType::Add),
        (true, false) => Some(EnumsSoftEnumUpdateType::Remove),
        _ => None,
    };
    if let Some(update_type) = update {
        ctx.broadcast(UpdateSoftEnumPacket {
            enum_name: "TagValues".into(),
            values: vec![tag.into()],
            update_type,
        });
    }
    // Measured: several targets answer one message with success count 1.
    let output = match changed.as_slice() {
        [one] => CommandOutput::success(single, vec![tag.to_owned(), ctx.name(*one)]),
        _ => CommandOutput::success(multiple, vec![tag.to_owned(), changed.len().to_string()]),
    };
    Ok(output)
}

fn anyone_has(world: &mut World, tag: &str) -> bool {
    let mut query = world.query::<&Tags>();
    query.iter(world).any(|tags| tags.contains(tag))
}

/// Measured: several targets list each distinct tag once, last target
/// first (`@e[r=16]` matching the player, then two cows tagged `herd`, lists
/// "herd, builder, second"). [INFERENCE] The order beyond that capture.
fn list(ctx: &mut CommandContext<'_>, targets: &[Entity]) -> CommandResult {
    let mut tags: Vec<String> = Vec::new();
    for &target in targets.iter().rev() {
        for tag in ctx
            .world
            .get::<Tags>(target)
            .map(|t| t.0.clone())
            .unwrap_or_default()
        {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    let shown = tags
        .iter()
        .map(|tag| format!("§a{tag}§r"))
        .collect::<Vec<_>>()
        .join(", ");
    let output = match targets {
        [one] if tags.is_empty() => {
            CommandOutput::success("commands.tag.list.single.empty", vec![ctx.name(*one)])
        }
        [one] => CommandOutput::success(
            "commands.tag.list.single.success",
            vec![ctx.name(*one), tags.len().to_string(), shown],
        ),
        _ if tags.is_empty() => CommandOutput::success(
            "commands.tag.list.multiple.empty",
            vec![targets.len().to_string()],
        ),
        _ => CommandOutput::success(
            "commands.tag.list.multiple.success",
            vec![targets.len().to_string(), tags.len().to_string(), shown],
        ),
    };
    Ok(output)
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn tag_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("tag", &[]);
        assert_same_packets(&steps, &["UpdateSoftEnumPacket"], &[]);
    }
}
