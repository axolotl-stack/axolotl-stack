use super::*;
use serde_json::json;
use unastar_data::actor_components::body;
use unastar_data::actor_components::body::ValueOrRange;
use unastar_packs::actor::{Loaded, load_value};

#[derive(Component)]
struct Baby;

fn source(components: Value, groups: Value) -> ActorDefinition {
    let document = json!({
        "format_version": "1.26.50",
        "minecraft:entity": {
            "description": {"identifier": "test:composition", "is_summonable": true},
            "components": components, "component_groups": groups
        }
    });
    let Loaded::Definition { definition, .. } = load_value(
        "composition control",
        &document,
        unastar_data::actor_definitions::parser(),
    )
    .unwrap() else {
        panic!("control definition skipped")
    };
    *definition
}

fn kinds() -> Arc<ComponentKinds> {
    let mut kinds = ComponentKindsBuilder::default();
    kinds
        .register(
            |_: body::IsBaby| Ok(()),
            |_, entity| {
                entity.insert(Baby);
            },
            |entity| {
                entity.remove::<Baby>();
            },
        )
        .unwrap();
    kinds.freeze()
}

#[test]
fn condition_marker_groups_update_and_remove_both_flag_words() {
    use crate::entity::definition::flags::{self, ActorFlags};
    use jolyne::valentine::actor_flags;

    let mut kinds = ComponentKindsBuilder::default();
    flags::register_component_kinds(&mut kinds).unwrap();
    let definition = CompiledDefinition::compile(
        &source(
            json!({}),
            json!({"conditions": {
                "minecraft:fire_immune": {},
                "minecraft:renders_when_invisible": {}
            }}),
        ),
        kinds.freeze(),
    )
    .unwrap();
    let group = definition.group("conditions").unwrap();
    let mut world = World::new();
    let actor = world.spawn_empty().id();
    attach_definition(&mut world, actor, definition).unwrap();
    queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
    apply_pending_definitions(&mut world);
    let flags = world.get::<ActorFlags>(actor).unwrap();
    assert!(flags.get(actor_flags::FIRE_IMMUNE).unwrap());
    assert!(flags.get(actor_flags::RENDERS_WHEN_INVISIBLE).unwrap());
    assert_ne!(flags.snapshot().words()[0], 0);
    assert_ne!(flags.snapshot().words()[1], 0);
    queue_group_change(&mut world, actor, GroupChange::Remove(group)).unwrap();
    apply_pending_definitions(&mut world);
    let flags = world.get::<ActorFlags>(actor).unwrap();
    assert!(!flags.get(actor_flags::FIRE_IMMUNE).unwrap());
    assert!(!flags.get(actor_flags::RENDERS_WHEN_INVISIBLE).unwrap());
}

#[test]
fn native_destructive_group_removal_reaches_actual_ecs_state() {
    let kinds = kinds();
    let baby = kinds.resolve(ActorComponent::IsBaby).unwrap();
    let definition = CompiledDefinition::compile(
        &source(
            json!({"minecraft:is_baby": {}}),
            json!({"override": {"minecraft:is_baby": {}}}),
        ),
        kinds,
    )
    .unwrap();
    let group = definition.group("override").unwrap();
    let mut world = World::new();
    let actor = world.spawn_empty().id();
    attach_definition(&mut world, actor, definition).unwrap();
    apply_pending_definitions(&mut world);
    assert!(world.get::<Baby>(actor).is_some());
    queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
    apply_pending_definitions(&mut world);
    queue_group_change(&mut world, actor, GroupChange::Remove(group)).unwrap();
    apply_pending_definitions(&mut world);
    assert!(world.get::<Baby>(actor).is_none());
    assert!(
        !world
            .get::<DefinitionState>(actor)
            .unwrap()
            .has_component(&baby)
            .unwrap()
    );
}

#[test]
fn transient_group_is_not_materialized_and_foreign_handles_do_not_mutate() {
    let kinds = kinds();
    let definition_source = source(json!({}), json!({"baby": {"minecraft:is_baby": {}}}));
    let first = CompiledDefinition::compile(&definition_source, Arc::clone(&kinds)).unwrap();
    let second = CompiledDefinition::compile(&definition_source, kinds).unwrap();
    let group = first.group("baby").unwrap();
    let foreign = second.group("baby").unwrap();
    let mut world = World::new();
    #[derive(Resource, Default)]
    struct SpawnNotifications(usize);
    world.init_resource::<SpawnNotifications>();
    world
        .register_component_hooks::<Baby>()
        .on_add(|mut world, _| {
            world.resource_mut::<SpawnNotifications>().0 += 1;
        });
    let actor = world.spawn_empty().id();
    attach_definition(&mut world, actor, first).unwrap();
    queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
    queue_group_change(&mut world, actor, GroupChange::Remove(group.clone())).unwrap();
    assert!(queue_group_change(&mut world, actor, GroupChange::Add(foreign)).is_err());
    apply_pending_definitions(&mut world);
    assert!(world.get::<Baby>(actor).is_none());
    assert_eq!(world.resource::<SpawnNotifications>().0, 0);
    let foreign_kind = self::kinds().resolve(ActorComponent::IsBaby).unwrap();
    assert!(
        world
            .get::<DefinitionState>(actor)
            .unwrap()
            .has_component(&foreign_kind)
            .is_err()
    );
    queue_group_change(&mut world, actor, GroupChange::Add(group)).unwrap();
    apply_pending_definitions(&mut world);
    assert!(world.get::<Baby>(actor).is_some());
    assert_eq!(world.resource::<SpawnNotifications>().0, 1);
}

#[test]
fn group_change_queued_by_component_hook_waits_for_next_barrier() {
    #[derive(Resource)]
    struct RemoveSelf(ComponentGroupId);
    let mut kinds = ComponentKindsBuilder::default();
    kinds
        .register(
            |_: body::IsBaby| Ok(()),
            |_, entity| {
                entity.insert(Baby);
                let actor = entity.id();
                entity.world_scope(|world| {
                    let group = world.resource::<RemoveSelf>().0.clone();
                    queue_group_change(world, actor, GroupChange::Remove(group)).unwrap();
                });
            },
            |entity| {
                entity.remove::<Baby>();
            },
        )
        .unwrap();
    let kinds = kinds.freeze();
    let baby = kinds.resolve(ActorComponent::IsBaby).unwrap();
    let definition = CompiledDefinition::compile(
        &source(json!({}), json!({"baby": {"minecraft:is_baby": {}}})),
        kinds,
    )
    .unwrap();
    let group = definition.group("baby").unwrap();
    let mut world = World::new();
    world.insert_resource(RemoveSelf(group.clone()));
    let actor = world.spawn_empty().id();
    attach_definition(&mut world, actor, definition).unwrap();
    queue_group_change(&mut world, actor, GroupChange::Add(group)).unwrap();
    apply_pending_definitions(&mut world);
    assert!(world.get::<Baby>(actor).is_some());
    assert!(
        !world
            .get::<DefinitionState>(actor)
            .unwrap()
            .has_component(&baby)
            .unwrap()
    );
    apply_pending_definitions(&mut world);
    assert!(world.get::<Baby>(actor).is_none());
}

#[derive(Component)]
struct A(i64);
#[derive(Component)]
struct B(i64);

#[test]
fn every_previous_component_is_removed_before_replacement_callbacks() {
    let mut kinds = ComponentKindsBuilder::default();
    kinds
        .register(
            |scale: body::Scale| Ok(scale.value as i64),
            |value, entity| {
                entity.insert(A(*value));
            },
            |entity| {
                entity.remove::<A>();
            },
        )
        .unwrap();
    kinds
        .register(
            |health: body::Health| match health.value {
                ValueOrRange::Value(value) => Ok(i64::from(value)),
                ValueOrRange::Range(_) => Err(ComponentCompileError::Rejected(
                    "fixed health required".into(),
                )),
            },
            |value, entity| {
                if *value == 2 {
                    assert!(!entity.contains::<A>());
                }
                entity.insert(B(*value));
            },
            |entity| {
                entity.remove::<B>();
            },
        )
        .unwrap();
    let definition = CompiledDefinition::compile(&source(
        json!({"minecraft:scale": {"value": 1}, "minecraft:health": {"value": 1}}),
        json!({"replacement": {"minecraft:scale": {"value": 2}, "minecraft:health": {"value": 2}}}),
    ), kinds.freeze()).unwrap();
    let group = definition.group("replacement").unwrap();
    let mut world = World::new();
    let actor = world.spawn_empty().id();
    attach_definition(&mut world, actor, definition).unwrap();
    apply_pending_definitions(&mut world);
    assert_eq!(world.get::<A>(actor).unwrap().0, 1);
    assert_eq!(world.get::<B>(actor).unwrap().0, 1);
    queue_group_change(&mut world, actor, GroupChange::Add(group)).unwrap();
    apply_pending_definitions(&mut world);
    assert_eq!(world.get::<A>(actor).unwrap().0, 2);
    assert_eq!(world.get::<B>(actor).unwrap().0, 2);
}

#[test]
fn group_history_replays_to_the_same_composition() {
    let definition = CompiledDefinition::compile(
        &source(
            json!({}),
            json!({"baby": {"minecraft:is_baby": {}}, "grown": {"minecraft:is_baby": {}}}),
        ),
        kinds(),
    )
    .unwrap();
    let baby = definition.group("baby").unwrap();
    let grown = definition.group("grown").unwrap();
    let mut world = World::new();
    let actor = world.spawn_empty().id();
    attach_definition(&mut world, actor, Arc::clone(&definition)).unwrap();
    for change in [
        GroupChange::Add(baby.clone()),
        GroupChange::Add(grown.clone()),
        GroupChange::Remove(grown),
        GroupChange::Add(baby),
    ] {
        queue_group_change(&mut world, actor, change).unwrap();
        apply_pending_definitions(&mut world);
    }
    let history: Vec<(String, bool)> = world
        .get::<DefinitionState>(actor)
        .unwrap()
        .group_history()
        .map(|(name, added)| (name.to_owned(), added))
        .collect();
    assert_eq!(
        history,
        [("grown".to_owned(), false), ("baby".to_owned(), true)]
    );

    let replayed = world.spawn_empty().id();
    attach_definition(&mut world, replayed, Arc::clone(&definition)).unwrap();
    for (name, added) in &history {
        let group = definition.group(name).unwrap();
        let change = if *added {
            GroupChange::Add(group)
        } else {
            GroupChange::Remove(group)
        };
        queue_group_change(&mut world, replayed, change).unwrap();
    }
    apply_pending_definitions(&mut world);
    assert!(world.get::<Baby>(actor).is_some());
    assert!(world.get::<Baby>(replayed).is_some());
}
