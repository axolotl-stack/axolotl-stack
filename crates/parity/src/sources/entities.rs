//! Entity rows: every vanilla actor, with a sub-row per behaviour-pack
//! component it uses and one for its spawn rules.
//!
//! Actors are the union of the vanilla behaviour-pack definitions
//! (`unastar_data::actor_definitions`) and the BDS 1.26.51
//! `AvailableActorIdentifiers` capture (`unastar_data::actor_identifiers`),
//! which also lists built-in actors that have no pack definition.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use unastar_data::{actor_definitions, actor_identifiers, spawn_rules};

use crate::catalog::Row;

pub fn rows() -> Vec<Row> {
    let mut actors: BTreeMap<String, Option<&'static actor_definitions::ActorDefinition>> =
        BTreeMap::new();
    for definition in actor_definitions::vanilla() {
        actors.insert(
            definition.identifier().to_owned(),
            Some(definition.as_ref()),
        );
    }
    for captured in actor_identifiers::vanilla() {
        actors.entry(captured.id.clone()).or_insert(None);
    }
    let spawn_ruled: BTreeSet<&str> = spawn_rules::vanilla()
        .iter()
        .map(|file| file.identifier())
        .collect();

    let mut rows = Vec::new();
    for (identifier, definition) in actors {
        let actor = match definition {
            Some(definition) => Row::new(&identifier, family(&identifier, definition)),
            None => Row::new(&identifier, "built_in")
                .with_label("built-in actor without a behaviour-pack definition"),
        };
        if let Some(definition) = definition {
            let mut components: BTreeSet<&str> =
                definition.components.keys().map(String::as_str).collect();
            for group in definition.component_groups.values() {
                components.extend(group.keys().map(String::as_str));
            }
            for component in components {
                let name = component.strip_prefix("minecraft:").unwrap_or(component);
                rows.push(Row::child(&actor, name));
            }
        }
        if spawn_ruled.contains(identifier.as_str()) {
            rows.push(Row::child(&actor, "spawn_rules"));
        }
        rows.push(actor);
    }
    rows
}

/// Behaviour family from the definition itself: projectile and boss
/// components first, then the `minecraft:type_family` families every vanilla
/// definition declares.
fn family(identifier: &str, definition: &actor_definitions::ActorDefinition) -> &'static str {
    let has = |component: &str| {
        definition.components.contains_key(component)
            || definition
                .component_groups
                .values()
                .any(|group| group.contains_key(component))
    };
    if identifier == "minecraft:player" {
        return "player";
    }
    if has("minecraft:boss") {
        return "boss";
    }
    if has("minecraft:projectile") {
        return "projectile";
    }
    let families = type_families(definition);
    let is = |family: &str| families.contains(family);
    // Not every mob lists the `mob` family (goats, fish), so objects are
    // inanimate families or actors without health.
    if is("inanimate") || is("boat") || is("minecart") || !has("minecraft:health") {
        "object"
    } else if is("monster") || is("illager") {
        "hostile"
    } else if is("villager") || is("wandering_trader") || is("irongolem") || is("npc") {
        "village"
    } else if has("minecraft:breathable")
        && definition
            .components
            .get("minecraft:breathable")
            .is_some_and(breathes_water_only)
    {
        "aquatic"
    } else {
        "passive"
    }
}

fn type_families(definition: &actor_definitions::ActorDefinition) -> BTreeSet<&str> {
    let mut families = BTreeSet::new();
    let bodies = std::iter::once(&definition.components)
        .chain(definition.component_groups.values())
        .filter_map(|components| components.get("minecraft:type_family"));
    for body in bodies {
        if let Some(Value::Array(list)) = body.get("family") {
            families.extend(list.iter().filter_map(Value::as_str));
        }
    }
    families
}

fn breathes_water_only(body: &Value) -> bool {
    body.get("breathes_water").and_then(Value::as_bool) == Some(true)
        && body.get("breathes_air").and_then(Value::as_bool) == Some(false)
}
