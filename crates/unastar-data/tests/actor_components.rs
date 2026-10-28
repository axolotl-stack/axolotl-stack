use std::collections::BTreeSet;

use serde_json::Value;
use unastar_data::actor_components::{ActorComponent, decode_component};
use unastar_data::actor_definitions::{self, ActorDefinition};

/// `(group, component name, body)` for base components (`group` empty) and
/// every component group.
fn bodies(definition: &ActorDefinition) -> impl Iterator<Item = (&str, &str, &Value)> {
    let base = definition
        .components
        .iter()
        .map(|(name, body)| ("", name.as_str(), body));
    let groups = definition
        .component_groups
        .iter()
        .flat_map(|(group, components)| {
            components
                .iter()
                .map(move |(name, body)| (group.as_str(), name.as_str(), body))
        });
    base.chain(groups)
}

#[test]
fn component_names_round_trip_and_cover_every_vanilla_definition() {
    for &component in ActorComponent::ALL {
        assert_eq!(ActorComponent::from_name(component.name()), Some(component));
    }
    let mut unknown = BTreeSet::new();
    for definition in actor_definitions::vanilla() {
        for (_, name, _) in bodies(definition) {
            if ActorComponent::from_name(name).is_none() {
                unknown.insert(name.to_owned());
            }
        }
    }
    assert!(unknown.is_empty(), "unregistered vanilla components: {unknown:?}");
}

#[test]
fn every_typed_vanilla_component_body_decodes() {
    let mut decoded = 0usize;
    let mut untyped = 0usize;
    let mut failures = Vec::new();
    for definition in actor_definitions::vanilla() {
        for (group, name, body) in bodies(definition) {
            let component = ActorComponent::from_name(name).expect("registered component");
            match decode_component(component, body) {
                Ok(true) => decoded += 1,
                Ok(false) => untyped += 1,
                Err(error) => failures.push(format!(
                    "{} [{group}]: {error}",
                    definition.identifier()
                )),
            }
        }
    }
    eprintln!("{decoded} typed component instances decoded, {untyped} without a typed body");
    assert!(
        failures.is_empty(),
        "{} vanilla component bodies failed to decode:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(decoded > 0);
}

#[test]
fn cow_and_chicken_components_are_all_typed() {
    for identifier in ["minecraft:cow", "minecraft:chicken"] {
        let definition = actor_definitions::get(identifier).expect("vanilla definition");
        let untyped: BTreeSet<_> = bodies(definition)
            .filter(|(_, name, body)| {
                let component = ActorComponent::from_name(name).expect("registered component");
                !decode_component(component, body).expect("decodes")
            })
            .map(|(_, name, _)| name)
            .collect();
        assert!(untyped.is_empty(), "{identifier} untyped components: {untyped:?}");
    }
}
