//! Building blocks shared by the component families: item matchers, self
//! events, filters, Molang, loot tables, blocks and the registration helpers.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{
    EventTrigger, FilterSubject, MolangString, MolangValue,
};
use unastar_data::actor_components::{ComponentBody, FilterJson};
use unastar_rules::filter::{Filter, FilterRegistry};
use unastar_rules::molang::{Expr, QueryContext, QueryRegistry, parse};

use super::ACCEPTED_WITHOUT_BEHAVIOUR;
use crate::entity::definition::{ComponentCompileError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::archetype::{UnsupportedFilterTest, check_filter};
use crate::entity::mob::constants;

/// Why an item name cannot be parsed.
#[derive(Debug, Error)]
pub enum ItemNameError {
    #[error("item aux value out of range in {0:?}")]
    AuxOutOfRange(String),
    #[error("empty item name in {0:?}")]
    Empty(String),
}

/// An item a component matches: `wheat`, `minecraft:wheat` or `bucket:0`.
/// A trailing numeric segment is the legacy aux value; omitted means any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMatcher {
    pub identifier: Arc<str>,
    pub aux: Option<i16>,
}

impl ItemMatcher {
    pub fn parse(text: &str) -> Result<Self, ItemNameError> {
        let (name, aux) = match text.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => {
                let aux = tail
                    .parse::<i16>()
                    .map_err(|_| ItemNameError::AuxOutOfRange(text.to_owned()))?;
                (head, Some(aux))
            }
            _ => (text, None),
        };
        if name.is_empty() {
            return Err(ItemNameError::Empty(text.to_owned()));
        }
        let identifier = if name.contains(':') {
            Arc::from(name)
        } else {
            Arc::from(format!("minecraft:{name}"))
        };
        Ok(Self { identifier, aux })
    }

    pub fn matches(&self, item_id: &str, aux: i16) -> bool {
        self.identifier.as_ref() == item_id && self.aux.is_none_or(|want| want == aux)
    }
}

/// Parse every name of an item list.
pub(super) fn items<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Result<Arc<[ItemMatcher]>, ItemNameError> {
    names
        .into_iter()
        .map(ItemMatcher::parse)
        .collect::<Result<Vec<_>, _>>()
        .map(Into::into)
}

/// Why an authored trigger cannot become a [`SelfEvent`].
#[derive(Debug, Error)]
pub enum SelfEventError {
    #[error("event {event} targets {target:?}; only self is supported")]
    OtherTarget {
        event: String,
        target: FilterSubject,
    },
    #[error("filtered trigger {0} is not supported")]
    Filtered(String),
}

/// An event this mob fires on itself, resolved by name when it fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfEvent(pub Arc<str>);

pub(super) fn self_event(trigger: &EventTrigger) -> Result<SelfEvent, SelfEventError> {
    match trigger {
        EventTrigger::Event(name) => Ok(SelfEvent(Arc::from(name.as_str()))),
        EventTrigger::Trigger(trigger) => {
            if trigger.target != FilterSubject::SelfValue {
                return Err(SelfEventError::OtherTarget {
                    event: trigger.event.clone(),
                    target: trigger.target,
                });
            }
            if trigger.filters.is_some() {
                return Err(SelfEventError::Filtered(trigger.event.clone()));
            }
            Ok(SelfEvent(Arc::from(trigger.event.as_str())))
        }
    }
}

/// Why an authored filter cannot be used by a component.
#[derive(Debug, Error)]
pub enum FilterError {
    #[error(transparent)]
    Parse(#[from] unastar_rules::ParseError),
    #[error("filter diagnostics {0:?}")]
    Diagnostics(Vec<unastar_rules::Diagnostic>),
    #[error(transparent)]
    Unsupported(#[from] UnsupportedFilterTest),
}

/// Parse a component's filter and check the runtime can answer every test.
pub fn filter(json: &FilterJson) -> Result<Filter, FilterError> {
    let mut diagnostics = unastar_rules::Diagnostics::default();
    let filter = Filter::parse(json, FilterRegistry::vanilla(), "filters", &mut diagnostics)?;
    if !diagnostics.items.is_empty() {
        return Err(FilterError::Diagnostics(diagnostics.items));
    }
    check_filter(&filter)?;
    Ok(filter)
}

/// Why authored Molang cannot be parsed or bound (the message is the Molang
/// parser's or binder's report).
#[derive(Debug, Error)]
#[error("molang {source_text:?}: {message}")]
pub struct MolangError {
    pub source_text: String,
    pub message: String,
}

/// A Molang value; an empty string (the published default) is `None`.
pub(super) fn molang(value: &MolangValue) -> Result<Option<Arc<Expr>>, MolangError> {
    let source = match value {
        MolangValue::Float(n) => n.to_string(),
        MolangValue::MolangString(MolangString::String(s)) => s.clone(),
        MolangValue::MolangString(MolangString::MolangExpressionObject(o)) => o.expression.clone(),
    };
    if source.trim().is_empty() {
        return Ok(None);
    }
    let fail = |message| MolangError {
        source_text: source.clone(),
        message,
    };
    let mut expr = parse(&source).map_err(fail)?;
    expr.bind(QueryRegistry::vanilla(), QueryContext::Runtime)
        .map_err(fail)?;
    Ok(Some(Arc::new(expr)))
}

/// A loot table a component names is not embedded.
#[derive(Debug, Error)]
#[error("loot table {0} is not in the embedded loot registry")]
pub struct UnknownLootTable(pub String);

/// A behaviour-pack loot table path as the loot registry keys it
/// (`entities/cow` for `loot_tables/entities/cow.json`); unknown tables
/// reject the definition.
pub(super) fn loot_table(path: &str) -> Result<Arc<str>, UnknownLootTable> {
    let table = path
        .strip_prefix("loot_tables/")
        .unwrap_or(path)
        .trim_end_matches(".json");
    if unastar_data::loot::get(table).is_none() {
        return Err(UnknownLootTable(path.to_owned()));
    }
    Ok(Arc::from(table))
}

/// A block a component names is not in the block registry.
#[derive(Debug, Error)]
#[error("unknown block {0}")]
pub struct UnknownBlock(pub String);

/// The block a behaviour-pack block name means: `minecraft:` is implied and
/// legacy names ([`constants::LEGACY_BLOCK_NAMES`]) resolve to today's block.
pub(super) fn block_entry(
    name: &str,
) -> Result<&'static crate::registry::block::BlockEntry, UnknownBlock> {
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    let name = constants::LEGACY_BLOCK_NAMES
        .iter()
        .find(|(legacy, _)| *legacy == name)
        .map_or(name.as_str(), |(_, modern)| *modern);
    crate::registry::BlockRegistry::vanilla()
        .get_by_name(name)
        .ok_or_else(|| UnknownBlock(name.to_owned()))
}

/// A plain config component: compile from the typed body, insert a clone,
/// remove on composition change.
pub fn insert<B: ComponentBody, T: Component + Clone, E: Into<ComponentCompileError>>(
    builder: &mut ComponentKindsBuilder,
    compile: impl Fn(B) -> Result<T, E> + Send + Sync + 'static,
) -> Result<(), DefinitionError> {
    builder.register(
        move |body| compile(body).map_err(Into::into),
        |value: &T, entity| {
            entity.insert(value.clone());
        },
        |entity| {
            entity.remove::<T>();
        },
    )
}

/// [`insert`] for a body every value of which compiles.
pub fn insert_infallible<B: ComponentBody, T: Component + Clone>(
    builder: &mut ComponentKindsBuilder,
    compile: impl Fn(B) -> T + Send + Sync + 'static,
) -> Result<(), DefinitionError> {
    insert(builder, move |body| {
        Ok::<_, ComponentCompileError>(compile(body))
    })
}

/// See [`ACCEPTED_WITHOUT_BEHAVIOUR`].
pub(super) fn inert<B: ComponentBody>(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    debug_assert!(
        ACCEPTED_WITHOUT_BEHAVIOUR
            .iter()
            .any(|(component, _)| *component == B::COMPONENT)
    );
    builder.register(|_: B| Ok(()), |_, _| {}, |_| {})
}
