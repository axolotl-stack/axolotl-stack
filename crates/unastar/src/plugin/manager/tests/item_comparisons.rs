//! Owned equality queries preserve every native item field without exporting encoded metadata.

use super::*;
use crate::entity::components::MainInventory;
use crate::item::ItemStack;
use crate::server::game::types::ItemRegistryResource;

/// Give a stack a legitimate opaque tag while keeping its public display name unchanged.
fn tagged(stack: &ItemStack, unknown: &str) -> ItemStack {
    crate::item::enchant::with_item_tag(
        stack,
        zuri_nbt::tag::Compound::builder()
            .with_string("opaque-example", unknown)
            .with_compound(
                "display",
                zuri_nbt::tag::Compound::builder().with_string("Name", "Example"),
            )
            .build(),
    )
}

/// Change only a real envelope restriction tail, preserving its exact preceding compound bytes.
fn restricted(stack: &ItemStack) -> ItemStack {
    let mut bytes = stack.user_data().to_vec();
    assert_eq!(&bytes[bytes.len() - 8..], &[0; 8]);
    bytes.truncate(bytes.len() - 8);
    bytes.extend_from_slice(&1_i32.to_le_bytes());
    bytes.extend_from_slice(&15_u16.to_le_bytes());
    bytes.extend_from_slice(b"minecraft:stone");
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    stack.clone().with_nbt(bytes)
}

/// Capture real stored slots, retaining detached values beyond the original actor's lifetime.
fn capture(count: usize) -> String {
    let mut body = String::from(
        r#"
        local.get 1 i32.const 64 call @[method]player.inventory@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 512 i32.const 68 i32.load i32.store
    "#,
    );
    for index in 0..count {
        let address = 520 + index * 4;
        body.push_str(&format!(r#"
            i32.const 512 i32.load i32.const 0 i32.const {index} i32.const 64 call @[method]inventory-image.slot@
            i32.const 64 i32.load8_u if unreachable end
            i32.const {address} i32.const 68 i32.load i32.store
        "#));
    }
    body.push_str(
        r#"
        i32.const 512 i32.load call @inventory-image_drop@
        i32.const 1536 i32.const 1 i32.store8
    "#,
    );
    body
}

/// Check the real canonical result layout and expected Boolean, without inspecting known tags.
fn comparison(method: &str, left: usize, right: usize, expected: bool) -> String {
    let left = 520 + left * 4;
    let right = 520 + right * 4;
    let expected = u8::from(expected);
    format!(
        r#"
        i32.const {left} i32.load i32.const {right} i32.load i32.const 64 call @[method]stack.{method}@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 65 i32.load8_u i32.const {expected} i32.ne if unreachable end
    "#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_owned_comparisons_distinguish_opaque_native_contents_and_merge_policy_after_departure()
 {
    let (mut router, clients) = router();
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    let registry = world.resource::<ItemRegistryResource>().0.clone();
    let diamond = registry.stack("minecraft:diamond", 2).unwrap();
    let named = tagged(&diamond, "original");
    let sword = registry.stack("minecraft:diamond_sword", 1).unwrap();
    let values = vec![
        diamond.clone(),
        diamond.clone().with_count(7),
        diamond.clone().with_damage(1),
        named.clone(),
        restricted(&named),
        named.with_count(7),
        tagged(&diamond, "different"),
        sword.clone(),
        sword.with_net_id(55),
        ItemStack::empty(),
        registry.stack("minecraft:emerald", 2).unwrap(),
        diamond.clone().with_runtime_id(77),
    ];
    for (index, value) in values.iter().enumerate() {
        assert!(crate::item::enchant::valid_user_data(value) || value.is_empty());
        world
            .get_mut::<MainInventory>(clients[0].0)
            .unwrap()
            .0
            .set_item(index, value.clone())
            .unwrap();
    }
    let mut query = String::from(
        r#"
        i32.const 1536 i32.load8_u i32.const 1 i32.eq
        if i32.const 1536 i32.const 2 i32.store8 return end
        i32.const 1536 i32.load8_u i32.const 2 i32.eq if
    "#,
    );
    for (left, right, same, stackable) in [
        (0, 1, true, true),
        (0, 2, false, false),
        (3, 4, false, false),
        (3, 5, true, true),
        (3, 6, false, false),
        (7, 8, true, false),
        (9, 9, false, false),
        (0, 9, false, false),
        (0, 10, false, false),
        (0, 11, false, false),
    ] {
        query.push_str(&comparison("same-item", left, right, same));
        query.push_str(&comparison("stackable-with", left, right, stackable));
    }
    for index in 0..values.len() {
        let address = 520 + index * 4;
        query.push_str(&format!("i32.const {address} i32.load call @stack_drop@\n"));
    }
    query.push_str("i32.const 1536 i32.const 0 i32.store8\n");
    query.push_str(witness());
    query.push_str("\nend");
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-jump", capture(values.len())),
            ("on-tick", query),
        ],
    );
    manager.lock().unwrap().pending[0]
        .manifest
        .capabilities
        .extend([
            PluginCapability::ReadInventory,
            PluginCapability::ItemValues,
        ]);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(
        router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(clients[0].0)
            .unwrap()
            .0
            .slots()[..values.len()],
        values
    );
    {
        let manager = manager.lock().unwrap();
        assert!(
            manager
                .plugins
                .values()
                .next()
                .unwrap()
                .store
                .data()
                .resources
                .value_budget
                .used()
                > 0
        );
    }
    router.despawn_player(1);
    assert!(
        router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(clients[0].0)
            .is_err()
    );
    router.tick().await.unwrap();
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    let manager = manager.lock().unwrap();
    let context = manager.plugins.values().next().unwrap().store.data();
    assert_eq!(context.resources.value_budget.used(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_comparison_capability_and_operation_denials_preserve_values_until_the_guest_drops_them()
 {
    for (revoke_capability, expected) in [(true, 6), (false, 7)] {
        let (mut router, clients) = router();
        let mut query = String::from(
            r#"
        i32.const 1536 i32.load8_u i32.const 1 i32.eq
        if i32.const 1536 i32.const 2 i32.store8 return end
        i32.const 1536 i32.load8_u i32.const 2 i32.eq if
    "#,
        );
        for method in ["same-item", "stackable-with"] {
            query.push_str(&format!(r#"
                i32.const 520 i32.load i32.const 524 i32.load i32.const 64 call @[method]stack.{method}@
                i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end
                i32.const 65 i32.load8_u i32.const {expected} i32.ne if unreachable end
            "#));
        }
        query.push_str(
            r#"
            i32.const 520 i32.load call @stack_drop@
            i32.const 524 i32.load call @stack_drop@
            i32.const 1536 i32.const 0 i32.store8
        "#,
        );
        query.push_str(witness());
        query.push_str("\nend");
        let manager = attach_scoped(
            &mut router,
            &[("on-player-jump", capture(2)), ("on-tick", query)],
        );
        manager.lock().unwrap().pending[0]
            .manifest
            .capabilities
            .extend([
                PluginCapability::ReadInventory,
                PluginCapability::ItemValues,
            ]);
        scope_callback(&mut router, clients[0].0);
        router.tick().await.unwrap();
        {
            let mut manager = manager.lock().unwrap();
            let context = manager
                .plugins
                .values_mut()
                .next()
                .unwrap()
                .store
                .data_mut();
            assert!(context.resources.value_budget.used() > 0);
            if revoke_capability {
                context.capabilities.remove(&PluginCapability::ItemValues);
            } else {
                context.api_limits.max_host_operations_per_tick = 0;
            }
        }
        router.tick().await.unwrap();
        router.tick().await.unwrap();
        if revoke_capability {
            assert_eq!(logs(&manager), ["x"]);
        } else {
            // Logging shares the exhausted operation allowance; dropping values still completes.
            assert!(logs(&manager).is_empty());
        }
        let manager = manager.lock().unwrap();
        assert_eq!(
            manager
                .plugins
                .values()
                .next()
                .unwrap()
                .store
                .data()
                .resources
                .value_budget
                .used(),
            0
        );
    }
}
