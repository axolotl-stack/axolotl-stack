//! Raw JSON text (`/tellraw`, `/titleraw`): checked like BDS, then resolved
//! for each viewer into the `rawtext` JSON the client renders.
//!
//! Measured on BDS 1.26.51 (scripted command captures):
//! - the message must be an object with a `rawtext` array of objects; the
//!   failures are `commands.tellraw.error.noData` (no `rawtext`),
//!   `…notArray`, `…itemIsNotObject`, `…textNotString` and
//!   `…withNotArrayOrRawText` (both commands use the `tellraw` keys); the
//!   lang's other type errors (`translateNotString`, `selectorNotString`,
//!   `scoreNotObject`, `scoreNameNotString`, `scoreObjectiveNotString`) are
//!   used the same way;
//! - `selector` becomes a `text` with the names it matches, `score` a `text`
//!   with the holder's score (`*` is the viewer), and a plain `with` list of
//!   strings becomes a `with.rawtext` list of `text` components (items that
//!   are not strings are left out); a nested `rawtext` stays nested and
//!   unknown keys and keys besides `rawtext` are ignored;
//! - a component that resolves to nothing (no match, no score) is left out,
//!   and a message left with no components is sent as `{"rawtext":null}`.
//!
//! The JSON is written compactly with a trailing newline, as BDS sends it.

use super::context::CommandContext;
use super::output::CommandOutput;
use super::parser::{Cursor, Target};
use super::selector::parse_selector;
use crate::world::scoreboard::{ScoreHolder, Scoreboard};
use bevy_ecs::prelude::Entity;
use serde_json::{Value, json};

/// One element of a `rawtext` list.
#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    Text(String),
    Translate {
        key: String,
        with: With,
    },
    Selector(String),
    Score {
        name: String,
        objective: String,
    },
    RawText(Vec<Component>),
    /// An object with none of the keys above.
    Empty,
}

/// The parameters of a `translate`.
#[derive(Debug, Clone, PartialEq)]
pub enum With {
    None,
    Strings(Vec<String>),
    RawText(Vec<Component>),
}

/// A checked raw text message.
#[derive(Debug, Clone, PartialEq)]
pub struct RawText(pub Vec<Component>);

fn error(key: &str) -> CommandOutput {
    CommandOutput::failure(format!("commands.tellraw.error.{key}"), vec![])
}

impl RawText {
    /// Checks a `{"rawtext": [...]}` message.
    pub fn parse(value: &Value) -> Result<Self, CommandOutput> {
        let rawtext = value
            .as_object()
            .and_then(|object| object.get("rawtext"))
            .ok_or_else(|| error("noData"))?;
        parse_list(rawtext).map(Self)
    }

    /// The message as `viewer` sees it, in the form BDS sends: compact JSON
    /// and a newline.
    pub fn render(&self, ctx: &mut CommandContext<'_>, viewer: Entity) -> String {
        let components = resolve_list(ctx, viewer, &self.0);
        let value = if components.is_empty() {
            json!({ "rawtext": null })
        } else {
            json!({ "rawtext": components })
        };
        format!("{value}\n")
    }
}

fn parse_list(value: &Value) -> Result<Vec<Component>, CommandOutput> {
    value
        .as_array()
        .ok_or_else(|| error("notArray"))?
        .iter()
        .map(parse_component)
        .collect()
}

fn parse_component(value: &Value) -> Result<Component, CommandOutput> {
    let object = value.as_object().ok_or_else(|| error("itemIsNotObject"))?;
    if let Some(text) = object.get("text") {
        let text = text.as_str().ok_or_else(|| error("textNotString"))?;
        return Ok(Component::Text(text.to_owned()));
    }
    if let Some(selector) = object.get("selector") {
        let selector = selector
            .as_str()
            .ok_or_else(|| error("selectorNotString"))?;
        return Ok(Component::Selector(selector.to_owned()));
    }
    if let Some(score) = object.get("score") {
        let score = score.as_object().ok_or_else(|| error("scoreNotObject"))?;
        let name = match score.get("name") {
            Some(name) => name.as_str().ok_or_else(|| error("scoreNameNotString"))?,
            None => return Ok(Component::Empty),
        };
        let objective = match score.get("objective") {
            Some(objective) => objective
                .as_str()
                .ok_or_else(|| error("scoreObjectiveNotString"))?,
            None => return Ok(Component::Empty),
        };
        return Ok(Component::Score {
            name: name.to_owned(),
            objective: objective.to_owned(),
        });
    }
    if let Some(key) = object.get("translate") {
        let key = key.as_str().ok_or_else(|| error("translateNotString"))?;
        let with = match object.get("with") {
            None => With::None,
            // Measured: items that are not strings are left out, and a list
            // left empty drops the `with`.
            Some(Value::Array(items)) => {
                let strings: Vec<String> = items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect();
                if strings.is_empty() {
                    With::None
                } else {
                    With::Strings(strings)
                }
            }
            Some(Value::Object(with)) => match with.get("rawtext") {
                Some(list) => With::RawText(parse_list(list)?),
                None => return Err(error("withNotArrayOrRawText")),
            },
            Some(_) => return Err(error("withNotArrayOrRawText")),
        };
        return Ok(Component::Translate {
            key: key.to_owned(),
            with,
        });
    }
    if let Some(list) = object.get("rawtext") {
        return parse_list(list).map(Component::RawText);
    }
    Ok(Component::Empty)
}

fn resolve_list(ctx: &mut CommandContext<'_>, viewer: Entity, list: &[Component]) -> Vec<Value> {
    list.iter()
        .filter_map(|component| resolve(ctx, viewer, component))
        .collect()
}

fn resolve(ctx: &mut CommandContext<'_>, viewer: Entity, component: &Component) -> Option<Value> {
    match component {
        Component::Text(text) => Some(json!({ "text": text })),
        Component::Selector(selector) => {
            let entities = select(ctx, selector)?;
            Some(json!({ "text": ctx.names(&entities) }))
        }
        Component::Score { name, objective } => {
            let score = score(ctx, viewer, name, objective)?;
            Some(json!({ "text": score.to_string() }))
        }
        Component::Translate { key, with } => Some(match with {
            With::None => json!({ "translate": key }),
            With::Strings(strings) => {
                let with: Vec<Value> = strings.iter().map(|s| json!({ "text": s })).collect();
                json!({ "translate": key, "with": { "rawtext": with } })
            }
            With::RawText(list) => {
                let with = resolve_list(ctx, viewer, list);
                json!({ "translate": key, "with": { "rawtext": with } })
            }
        }),
        Component::RawText(list) => {
            let inner = resolve_list(ctx, viewer, list);
            (!inner.is_empty()).then(|| json!({ "rawtext": inner }))
        }
        Component::Empty => None,
    }
}

/// The entities a selector string matches (`None` for none or a bad selector).
fn select(ctx: &mut CommandContext<'_>, text: &str) -> Option<Vec<Entity>> {
    let mut cursor = Cursor::new(text);
    let selector = parse_selector(&mut cursor).ok()?;
    if !cursor.rest().trim().is_empty() {
        return None;
    }
    ctx.entities(Some(&Target::Selector(Box::new(selector))))
        .ok()
}

/// A score for `name`: `*` is the viewer, a selector its first match, any
/// other text a player or fake player of that name.
fn score(ctx: &mut CommandContext<'_>, viewer: Entity, name: &str, objective: &str) -> Option<i32> {
    let holder = if name == "*" {
        ScoreHolder::Entity(viewer)
    } else if name.starts_with('@') {
        ScoreHolder::Entity(*select(ctx, name)?.first()?)
    } else {
        ctx.entities(Some(&Target::Name(name.to_owned())))
            .ok()
            .and_then(|found| found.first().copied())
            .map_or_else(|| ScoreHolder::Fake(name.to_owned()), ScoreHolder::Entity)
    };
    ctx.world
        .get_resource::<Scoreboard>()?
        .score(&holder, objective)
}
