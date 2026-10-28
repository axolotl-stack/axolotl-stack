//! Startup factories for the literal core state authored by cow/chicken packs.
//!
//! These components are inputs to generic derivation, not native physics or
//! attribute defaults. In particular, collision dimensions remain raw and
//! unscaled; no gravity, drag, step height, or attribute bounds are chosen here.
//! The current prototype API has no per-instance expression/RNG context, so
//! dynamic values and ranges are rejected. Unknown fields remain the shared pack
//! loader's responsibility; these factories consume only implemented fields.

use bevy_ecs::prelude::*;
use serde_json::{Map, Value};
use unastar_data::actor_definitions::component_field_default;

use super::components::Health;
use super::definition::ComponentKindsBuilder;

/// Authored base movement value, without native attribute bounds or modifiers.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MovementConfig {
    pub value: f32,
}

/// Unscaled authored collision dimensions in blocks, including explicit zero.
/// Consumers apply the published negative-dimension-to-zero rule when deriving
/// geometry, independently from any measured relationship to actor scale.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct RawCollisionBox {
    pub width: f32,
    pub height: f32,
}

/// Authored actor scale, kept separate from raw collision geometry.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ScaleConfig {
    pub value: f32,
}

/// Published physics switches only; no motion coefficients are implied.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsConfig {
    pub has_gravity: bool,
    pub has_collision: bool,
    pub push_towards_closest_space: bool,
}

/// Register before freezing the shared factory registry. Apply/remove uses typed
/// prototypes only and preserves the definition stack's replacement policy.
/// Marker/flag registrations belong to `flags::register_component_kinds`.
///
/// Health requires literal `value` and `max` unless the pinned published schema
/// supplies them. It never infers max from current health or calls `Default`.
/// Replacement reinstantiates authored health under the existing local stack
/// policy; this is not evidence of native runtime health reset behavior.
pub fn register_component_kinds(builder: &mut ComponentKindsBuilder) -> Result<(), String> {
    builder.register(
        "minecraft:health",
        compile_health,
        apply::<Health>,
        remove::<Health>,
    )?;
    builder.register(
        "minecraft:movement",
        compile_movement,
        apply::<MovementConfig>,
        remove::<MovementConfig>,
    )?;
    builder.register(
        "minecraft:collision_box",
        compile_collision_box,
        apply::<RawCollisionBox>,
        remove::<RawCollisionBox>,
    )?;
    builder.register(
        "minecraft:scale",
        compile_scale,
        apply::<ScaleConfig>,
        remove::<ScaleConfig>,
    )?;
    builder.register(
        "minecraft:physics",
        compile_physics,
        apply::<PhysicsConfig>,
        remove::<PhysicsConfig>,
    )
}

fn apply<T: Component + Clone>(value: &T, entity: &mut EntityWorldMut<'_>) {
    entity.insert(value.clone());
}

fn remove<T: Component>(entity: &mut EntityWorldMut<'_>) {
    entity.remove::<T>();
}

fn compile_health(body: &Value) -> Result<Health, String> {
    let body = fields(body)?;
    reject_bounds(body, &["min"])?;
    Ok(Health {
        current: number("minecraft:health", body, "value")?,
        max: number("minecraft:health", body, "max")?,
    })
}

fn compile_movement(body: &Value) -> Result<MovementConfig, String> {
    let body = fields(body)?;
    reject_bounds(body, &["min", "max"])?;
    Ok(MovementConfig {
        value: number("minecraft:movement", body, "value")?,
    })
}

fn compile_collision_box(body: &Value) -> Result<RawCollisionBox, String> {
    let body = fields(body)?;
    Ok(RawCollisionBox {
        width: number("minecraft:collision_box", body, "width")?,
        height: number("minecraft:collision_box", body, "height")?,
    })
}

fn compile_scale(body: &Value) -> Result<ScaleConfig, String> {
    let body = fields(body)?;
    reject_bounds(body, &["min", "max"])?;
    Ok(ScaleConfig {
        value: number("minecraft:scale", body, "value")?,
    })
}

fn compile_physics(body: &Value) -> Result<PhysicsConfig, String> {
    let body = fields(body)?;
    Ok(PhysicsConfig {
        has_gravity: boolean("minecraft:physics", body, "has_gravity")?,
        has_collision: boolean("minecraft:physics", body, "has_collision")?,
        push_towards_closest_space: boolean(
            "minecraft:physics",
            body,
            "push_towards_closest_space",
        )?,
    })
}

fn fields(body: &Value) -> Result<&Map<String, Value>, String> {
    body.as_object()
        .ok_or_else(|| "component body must be an object".to_owned())
}

// These are recognized dynamic-attribute bounds, not unknown pack fields.
// Without a typed bounds consumer, accepting them would discard authored
// semantics. Other extra fields are deliberately left to the shared loader.
fn reject_bounds(body: &Map<String, Value>, unsupported: &[&str]) -> Result<(), String> {
    if let Some(field) = unsupported.iter().find(|field| body.contains_key(**field)) {
        return Err(format!(
            "field `{field}` is an unsupported authored attribute bound"
        ));
    }
    Ok(())
}

fn field<'a>(
    component: &str,
    body: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Value, String> {
    body.get(field)
        .or_else(|| component_field_default(component, field))
        .ok_or_else(|| {
            format!("missing required field `{field}`; no published component field default")
        })
}

fn number(component: &str, body: &Map<String, Value>, name: &str) -> Result<f32, String> {
    let value = field(component, body, name)?
        .as_f64()
        .ok_or_else(|| {
            format!(
                "field `{name}` requires a literal number; dynamic expressions and ranges need per-instance evaluation unsupported by this prototype API"
            )
        })?;
    let value = value as f32;
    if !value.is_finite() {
        return Err(format!("field `{name}` must be a finite f32 number"));
    }
    Ok(value)
}

fn boolean(component: &str, body: &Map<String, Value>, name: &str) -> Result<bool, String> {
    field(component, body, name)?
        .as_bool()
        .ok_or_else(|| format!("field `{name}` requires a literal boolean"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use serde_json::json;
    use unastar_data::actor_definitions::ActorDefinition;
    use unastar_packs::actor::{Loaded, load_value};

    use crate::entity::definition::{
        CompiledDefinition, ComponentKinds, DefinitionState, GroupChange,
        apply_pending_definitions, attach_definition, queue_group_change,
    };

    fn source(components: Value, groups: Value) -> ActorDefinition {
        let document = json!({
            "format_version": "1.26.50",
            "minecraft:entity": {
                "description": {"identifier": "test:core", "is_summonable": true},
                "components": components,
                "component_groups": groups
            }
        });
        let Loaded::Definition { definition, .. } = load_value(
            "core factory control",
            &document,
            unastar_data::actor_definitions::parser(),
        )
        .unwrap() else {
            panic!("control definition skipped")
        };
        *definition
    }

    fn kinds() -> Arc<ComponentKinds> {
        let mut builder = ComponentKindsBuilder::default();
        register_component_kinds(&mut builder).unwrap();
        builder.freeze()
    }

    fn spawn(world: &mut World, definition: Arc<CompiledDefinition>) -> Entity {
        let actor = world.spawn_empty().id();
        attach_definition(world, actor, definition).unwrap();
        apply_pending_definitions(world);
        actor
    }

    #[test]
    fn published_omissions_do_not_overwrite_explicit_false_or_zero() {
        let kinds = kinds();
        let omitted = CompiledDefinition::compile(
            &source(
                json!({"minecraft:collision_box": {}, "minecraft:physics": {}}),
                json!({}),
            ),
            Arc::clone(&kinds),
        )
        .unwrap();
        let mixed = CompiledDefinition::compile(
            &source(
                json!({
                    "minecraft:collision_box": {"width": 0},
                    "minecraft:physics": {"has_gravity": false}
                }),
                json!({}),
            ),
            Arc::clone(&kinds),
        )
        .unwrap();
        let explicit = CompiledDefinition::compile(
            &source(
                json!({
                    "minecraft:health": {"value": 0, "max": 0},
                    "minecraft:movement": {"value": 0},
                    "minecraft:scale": {"value": 0},
                    "minecraft:collision_box": {"width": 0, "height": 0},
                    "minecraft:physics": {
                        "has_gravity": false,
                        "has_collision": false,
                        "push_towards_closest_space": true
                    }
                }),
                json!({}),
            ),
            kinds,
        )
        .unwrap();
        let mut world = World::new();
        let default_actor = spawn(&mut world, omitted);
        let zero_actor = spawn(&mut world, explicit);
        let mixed_actor = spawn(&mut world, mixed);
        let default_box = world.get::<RawCollisionBox>(default_actor).unwrap();
        assert_eq!((default_box.width, default_box.height), (0.6_f32, 1.8_f32));
        let physics = world.get::<PhysicsConfig>(default_actor).unwrap();
        assert!(physics.has_gravity && physics.has_collision);
        assert!(!physics.push_towards_closest_space);
        assert!(world.get::<ScaleConfig>(default_actor).is_none());
        let mixed_box = world.get::<RawCollisionBox>(mixed_actor).unwrap();
        assert_eq!(mixed_box.width, 0.0);
        assert_eq!(mixed_box.height, default_box.height);
        let mixed_physics = world.get::<PhysicsConfig>(mixed_actor).unwrap();
        assert!(!mixed_physics.has_gravity);
        assert!(mixed_physics.has_collision);
        assert!(!mixed_physics.push_towards_closest_space);
        assert!(world.get::<Health>(zero_actor).unwrap().is_dead());
        assert_eq!(world.get::<Health>(zero_actor).unwrap().max, 0.0);
        assert_eq!(world.get::<MovementConfig>(zero_actor).unwrap().value, 0.0);
        assert_eq!(world.get::<ScaleConfig>(zero_actor).unwrap().value, 0.0);
        let zero_box = world.get::<RawCollisionBox>(zero_actor).unwrap();
        assert_eq!((zero_box.width, zero_box.height), (0.0, 0.0));
        let physics = world.get::<PhysicsConfig>(zero_actor).unwrap();
        assert!(!physics.has_gravity && !physics.has_collision);
        assert!(physics.push_towards_closest_space);
    }

    fn compile_error(components: Value) -> String {
        match CompiledDefinition::compile(&source(components, json!({})), kinds()) {
            Ok(_) => panic!("unsupported definition compiled"),
            Err(error) => error,
        }
    }

    #[test]
    fn unpublished_health_fields_are_errors_not_inferred_constructor_values() {
        for body in [json!({"value": 4}), json!({"max": 4})] {
            let _ = compile_error(json!({"minecraft:health": body}));
        }
        for component in ["minecraft:movement", "minecraft:scale"] {
            let _ = compile_error(json!({(component): {}}));
        }
    }

    #[test]
    fn expressions_ranges_and_unimplemented_bounds_are_not_silently_narrowed() {
        for body in [
            json!({"value": {"range_min": 1, "range_max": 4}, "max": 4}),
            json!({"value": "query.health", "max": 4}),
            json!({"value": 4, "max": {"range_min": 4, "range_max": 8}}),
        ] {
            let _ = compile_error(json!({"minecraft:health": body}));
        }
        for component in ["minecraft:movement", "minecraft:scale"] {
            let _ = compile_error(json!({(component): {"value": [0.5, 1.0]}}));
            let _ = compile_error(json!({(component): {"value": 1, "max": 2}}));
        }
        let _ = compile_error(json!({"minecraft:physics": {"has_gravity": null}}));
    }

    #[test]
    fn unknown_pack_fields_do_not_change_implemented_state() {
        let definition = CompiledDefinition::compile(
            &source(
                json!({
                    "minecraft:health": {"value": 4, "max": 4, "test:unknown": 99},
                    "minecraft:movement": {"value": 0, "test:unknown": {"value": 99}},
                    "minecraft:scale": {"value": 0.5, "test:unknown": false},
                    "minecraft:collision_box": {"width": 0, "test:unknown": [99]},
                    "minecraft:physics": {"has_gravity": false, "test:unknown": null}
                }),
                json!({}),
            ),
            kinds(),
        )
        .unwrap();
        let mut world = World::new();
        let actor = spawn(&mut world, definition);
        assert_eq!(world.get::<Health>(actor).unwrap().current, 4.0);
        assert_eq!(world.get::<MovementConfig>(actor).unwrap().value, 0.0);
        assert_eq!(world.get::<ScaleConfig>(actor).unwrap().value, 0.5);
        let collision = world.get::<RawCollisionBox>(actor).unwrap();
        assert_eq!((collision.width, collision.height), (0.0, 1.8_f32));
        let physics = world.get::<PhysicsConfig>(actor).unwrap();
        assert!(!physics.has_gravity);
        assert!(physics.has_collision);
    }

    #[test]
    fn composition_replaces_runtime_health_and_removes_without_restoring_base() {
        let kinds = kinds();
        let health_kind = kinds.resolve("minecraft:health").unwrap();
        let definition = CompiledDefinition::compile(
            &source(
                json!({
                    "minecraft:health": {"value": 10, "max": 10},
                    "minecraft:movement": {"value": 0.25},
                    "minecraft:collision_box": {"width": 0.9, "height": 1.3},
                    "minecraft:scale": {"value": 1},
                    "minecraft:physics": {}
                }),
                json!({
                    "baby": {"minecraft:scale": {"value": 0.5}},
                    "replacement": {
                        "minecraft:health": {"value": 4, "max": 4},
                        "minecraft:movement": {"value": 0},
                        "minecraft:collision_box": {"width": 0.6, "height": 0.8},
                        "minecraft:scale": {"value": 0.25},
                        "minecraft:physics": {"has_gravity": false}
                    }
                }),
            ),
            kinds,
        )
        .unwrap();
        let baby = definition.group("baby").unwrap();
        let replacement = definition.group("replacement").unwrap();
        let mut world = World::new();
        let first = spawn(&mut world, Arc::clone(&definition));
        let second = spawn(&mut world, definition);
        world.get_mut::<Health>(first).unwrap().damage(3.0);
        assert_eq!(world.get::<Health>(second).unwrap().current, 10.0);
        let raw = *world.get::<RawCollisionBox>(first).unwrap();
        queue_group_change(&mut world, first, GroupChange::Add(baby)).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(world.get::<RawCollisionBox>(first).unwrap(), &raw);
        assert_eq!(world.get::<ScaleConfig>(first).unwrap().value, 0.5);
        assert_eq!(world.get::<Health>(first).unwrap().current, 7.0);

        queue_group_change(&mut world, first, GroupChange::Add(replacement.clone())).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(world.get::<Health>(first).unwrap().current, 4.0);
        world.get_mut::<Health>(first).unwrap().damage(1.0);
        queue_group_change(&mut world, first, GroupChange::Add(replacement.clone())).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(world.get::<Health>(first).unwrap().current, 3.0);

        // The stack's local remove+add policy resets even when the final
        // prototype identity is unchanged; it is not a native-reset claim.
        queue_group_change(&mut world, first, GroupChange::Remove(replacement.clone())).unwrap();
        queue_group_change(&mut world, first, GroupChange::Add(replacement.clone())).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(world.get::<Health>(first).unwrap().current, 4.0);
        queue_group_change(&mut world, first, GroupChange::Remove(replacement)).unwrap();
        apply_pending_definitions(&mut world);
        assert!(world.get::<Health>(first).is_none());
        assert!(world.get::<MovementConfig>(first).is_none());
        assert!(world.get::<RawCollisionBox>(first).is_none());
        assert!(world.get::<ScaleConfig>(first).is_none());
        assert!(world.get::<PhysicsConfig>(first).is_none());
        assert!(
            !world
                .get::<DefinitionState>(first)
                .unwrap()
                .has_component(&health_kind)
                .unwrap()
        );
        assert_eq!(world.get::<Health>(second).unwrap().current, 10.0);
        assert_eq!(world.get::<RawCollisionBox>(second).unwrap(), &raw);
    }
}
