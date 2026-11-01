//! Actual canonical inventory images retain values, never a live inventory borrow.

use super::*;
use crate::entity::components::*;
use crate::server::game::inventory_effects::InventoryState;

/// Grant only the observation and detached-value capabilities under test.
fn permit(manager: &Arc<Mutex<PluginManager>>) {
    manager.lock().unwrap().pending[0]
        .manifest
        .capabilities
        .extend([
            PluginCapability::ReadInventory,
            PluginCapability::ItemValues,
        ]);
}

/// Read each semantic area through the real canonical ABI without retaining temporary stacks.
fn inspect_areas() -> String {
    let mut body = String::from(
        r#"
        i32.const 512 i32.load i32.const 256 call @[method]inventory-image.identity@
        i32.const 256 i32.load8_u if unreachable end
        i32.const 264 i32.load8_u i32.const 1 i32.ne if unreachable end
        i32.const 272 i64.load i64.const 1 i64.ne if unreachable end
        i32.const 284 i32.load i32.const 36 i32.ne if unreachable end
        i32.const 292 i32.load i32.const 7 i32.ne if unreachable end
        i32.const 296 i32.load8_u i32.const 1 i32.ne if unreachable end
        i32.const 304 i64.load i64.const 1 i64.ne if unreachable end
        i32.const 512 i32.load i32.const 64 call @[method]inventory-image.dimension@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 68 i32.load if unreachable end
        i32.const 512 i32.load i32.const 64 call @[method]inventory-image.held-slot@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 65 i32.load8_u i32.const 7 i32.ne if unreachable end
        i32.const 512 i32.load i32.const 64 call @[method]inventory-image.revision@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 72 i64.load i64.const 0 i64.ne if unreachable end
    "#,
    );
    for (area, length) in [36, 4, 1, PLAYER_UI_SIZE, ENDER_CHEST_SIZE]
        .into_iter()
        .enumerate()
    {
        body.push_str(&format!(
            r#"
            i32.const 512 i32.load i32.const {area} i32.const 64 call @[method]inventory-image.slot-count@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 68 i32.load i32.const {length} i32.ne if unreachable end
        "#
        ));
        for slot in 0..length {
            let count = slot + 1;
            let damage = area + 1;
            body.push_str(&format!(
                r#"
                i32.const 512 i32.load i32.const {area} i32.const {slot} i32.const 64 call @[method]inventory-image.slot@
                i32.const 64 i32.load8_u if unreachable end
                i32.const 68 i32.load i32.const 80 call @[method]stack.count@
                i32.const 80 i32.load8_u if unreachable end
                i32.const 81 i32.load8_u i32.const {count} i32.ne if unreachable end
                i32.const 68 i32.load i32.const 96 call @[method]stack.damage@
                i32.const 96 i32.load8_u if unreachable end
                i32.const 98 i32.load16_u i32.const {damage} i32.ne if unreachable end
                i32.const 68 i32.load i32.const 112 call @[method]stack.name@
                i32.const 112 i32.load8_u if unreachable end
                i32.const 116 i32.load8_u i32.const 1 i32.ne if unreachable end
                i32.const 124 i32.load i32.const 5 i32.ne if unreachable end
                i32.const 120 i32.load i32.load8_u i32.const 86 i32.ne if unreachable end
                i32.const 68 i32.load call @stack_drop@
            "#
            ));
        }
    }
    body.push_str("i32.const 512 i32.load call @inventory-image_drop@\n");
    body.push_str(witness());
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reading_an_image_does_not_grant_the_separate_item_value_capability() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[(
            "on-player-jump",
            format!(
                r#"
        local.get 1 i32.const 64 call @[method]player.inventory@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 68 i32.load i32.const 0 i32.const 0 i32.const 80 call @[method]inventory-image.slot@
        i32.const 80 i32.load8_u i32.const 1 i32.ne if unreachable end
        i32.const 84 i32.load8_u i32.const 6 i32.ne if unreachable end
        i32.const 68 i32.load call @inventory-image_drop@
        {}
    "#,
                witness()
            ),
        )],
    );
    manager.lock().unwrap().pending[0]
        .manifest
        .capabilities
        .insert(PluginCapability::ReadInventory);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert_eq!(
        manager
            .lock()
            .unwrap()
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

/// Populate every authoritative area with independently observable count, damage and metadata.
fn populate(router: &mut DimensionRouter, player: Entity) -> InventoryState {
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    let registry = world
        .resource::<crate::server::game::types::ItemRegistryResource>()
        .0
        .clone();
    let items = |area: i16, length: usize| {
        (0..length)
            .map(|slot| {
                let stack = registry
                    .stack("minecraft:diamond", (slot + 1) as u8)
                    .unwrap()
                    .with_damage(area);
                crate::item::enchant::with_item_tag(
                    &stack,
                    zuri_nbt::tag::Compound::builder()
                        .with_compound(
                            "display",
                            zuri_nbt::tag::Compound::builder().with_string("Name", "Value"),
                        )
                        .build(),
                )
            })
            .collect::<Vec<_>>()
    };
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .slots_mut()
        .clone_from_slice(&items(1, 36));
    world
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .inventory_mut()
        .slots_mut()
        .clone_from_slice(&items(2, 4));
    world.get_mut::<OffhandSlot>(player).unwrap().0 = items(3, 1).remove(0);
    world
        .get_mut::<PlayerUi>(player)
        .unwrap()
        .0
        .slots_mut()
        .clone_from_slice(&items(4, PLAYER_UI_SIZE));
    world
        .get_mut::<EnderChestInventory>(player)
        .unwrap()
        .0
        .slots_mut()
        .clone_from_slice(&items(5, ENDER_CHEST_SIZE));
    world.get_mut::<HeldSlot>(player).unwrap().set(7);
    InventoryState::capture(world, player).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn complete_owned_image_survives_actor_removal_and_reads_all_areas_in_a_fresh_callback() {
    let (mut router, clients) = router();
    populate(&mut router, clients[0].0);
    let manager = attach_scoped(
        &mut router,
        &[
            (
                "on-player-jump",
                r#"
                local.get 1 i32.const 64 call @[method]player.inventory@
                i32.const 64 i32.load8_u if unreachable end
                i32.const 512 i32.const 68 i32.load i32.store
            "#
                .into(),
            ),
            ("on-tick", inspect_areas()),
        ],
    );
    permit(&manager);
    scope_callback(&mut router, clients[0].0);
    router.register_tick_handler(Depart);
    router.tick().await.unwrap();
    assert!(router.locate_session(1).is_none());
    assert_eq!(logs(&manager), ["x"]);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn candidate_slot_and_held_slot_edits_preserve_the_original_image_and_live_inventory() {
    let (mut router, clients) = router();
    let before = populate(&mut router, clients[0].0);
    let manager = attach_scoped(&mut router, &[
        ("on-player-jump", r#"
            local.get 1 i32.const 64 call @[method]player.inventory@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 512 i32.const 68 i32.load i32.store
            i32.const 68 i32.load i32.const 0 i32.const 0 i32.const 80 call @[method]inventory-image.slot@
            i32.const 80 i32.load8_u if unreachable end
            i32.const 84 i32.load i32.const 2 i32.const 96 call @[method]stack.with-count@
            i32.const 96 i32.load8_u if unreachable end
            i32.const 512 i32.load i32.const 0 i32.const 0 i32.const 100 i32.load i32.const 112 call @[method]inventory-image.with-slot@
            i32.const 112 i32.load8_u if unreachable end
            i32.const 116 i32.load i32.const 8 i32.const 128 call @[method]inventory-image.with-held-slot@
            i32.const 128 i32.load8_u if unreachable end
            i32.const 516 i32.const 132 i32.load i32.store
            i32.const 116 i32.load call @inventory-image_drop@
            i32.const 84 i32.load call @stack_drop@
            i32.const 100 i32.load call @stack_drop@
        "#.into()),
        ("on-tick", format!(r#"
            i32.const 516 i32.load i32.const 0 i32.const 0 i32.const 64 call @[method]inventory-image.slot@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 68 i32.load i32.const 80 call @[method]stack.count@
            i32.const 80 i32.load8_u if unreachable end
            i32.const 81 i32.load8_u i32.const 2 i32.ne if unreachable end
            i32.const 68 i32.load call @stack_drop@
            i32.const 516 i32.load i32.const 64 call @[method]inventory-image.held-slot@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 65 i32.load8_u i32.const 8 i32.ne if unreachable end
            i32.const 516 i32.load i32.const 9 i32.const 64 call @[method]inventory-image.with-held-slot@
            i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end
            i32.const 68 i32.load8_u i32.const 5 i32.ne if unreachable end
            i32.const 516 i32.load i32.const 0 i32.const 36 i32.const 64 call @[method]inventory-image.slot@
            i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end
            i32.const 68 i32.load8_u i32.const 5 i32.ne if unreachable end
            i32.const 516 i32.load call @inventory-image_drop@
            {}
        "#, inspect_areas())),
    ]);
    permit(&manager);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert_eq!(
        InventoryState::capture(router.world(0).unwrap().ecs.world(), clients[0].0).unwrap(),
        before
    );
    assert_eq!(
        manager
            .lock()
            .unwrap()
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capability_and_incomplete_state_fail_without_allocating_a_partial_image() {
    for missing in 0..8 {
        let (mut router, clients) = router();
        let world = router.world_mut(0).unwrap().ecs.world_mut();
        match missing {
            1 => {
                world.entity_mut(clients[0].0).remove::<MainInventory>();
            }
            2 => {
                world.entity_mut(clients[0].0).remove::<ArmourInventory>();
            }
            3 => {
                world.entity_mut(clients[0].0).remove::<OffhandSlot>();
            }
            4 => {
                world.entity_mut(clients[0].0).remove::<PlayerUi>();
            }
            5 => {
                world
                    .entity_mut(clients[0].0)
                    .remove::<EnderChestInventory>();
            }
            6 => {
                world.entity_mut(clients[0].0).remove::<HeldSlot>();
            }
            _ => (),
        }
        let error = if missing == 0 {
            6
        } else if missing == 7 {
            7
        } else {
            8
        };
        let manager = attach_scoped(
            &mut router,
            &[(
                "on-player-jump",
                format!(
                    r#"
            local.get 1 i32.const 64 call @[method]player.inventory@
            i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end
            i32.const 68 i32.load8_u i32.const {error} i32.ne if unreachable end
            {}
        "#,
                    witness()
                ),
            )],
        );
        if missing != 0 {
            permit(&manager);
        }
        if missing == 7 {
            let mut manager = manager.lock().unwrap();
            let limits = PluginLimits {
                max_owned_resources: 2,
                ..Default::default()
            };
            manager.pending[0].store = store(&manager.engine, &limits);
            manager.pending[0].manifest.limits = limits;
        }
        scope_callback(&mut router, clients[0].0);
        router.tick().await.unwrap();
        assert_eq!(logs(&manager), ["x"], "case {missing}");
        assert_eq!(
            manager
                .lock()
                .unwrap()
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aggregate_byte_quota_is_atomic_and_dropping_images_restores_capacity() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[(
            "on-player-jump",
            format!(
                r#"
        (local i32 i32)
        block $full
            loop $again
                local.get 1 i32.const 64 call @[method]player.inventory@
                i32.const 64 i32.load8_u if
                    i32.const 68 i32.load8_u i32.const 7 i32.ne if unreachable end
                    br $full
                end
                i32.const 8192 local.get 2 i32.const 4 i32.mul i32.add i32.const 68 i32.load i32.store
                local.get 2 i32.const 1 i32.add local.tee 2 i32.const 4000 i32.ge_u if unreachable end
                br $again
            end
        end
        local.get 2 i32.eqz if unreachable end
        loop $drop
            i32.const 8192 local.get 3 i32.const 4 i32.mul i32.add i32.load call @inventory-image_drop@
            local.get 3 i32.const 1 i32.add local.tee 3 local.get 2 i32.lt_u br_if $drop
        end
        local.get 1 i32.const 64 call @[method]player.inventory@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 68 i32.load call @inventory-image_drop@
        {}
    "#,
                witness()
            ),
        )],
    );
    permit(&manager);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert_eq!(
        manager
            .lock()
            .unwrap()
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inventory_byte_quota_includes_retained_identity_and_connection_strings() {
    let (mut router, clients) = router();
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    let name = "x".repeat(4 * 1024 * 1024);
    world.get_mut::<PlayerName>(clients[0].0).unwrap().0 = name.clone();
    world
        .get_mut::<PlayerSession>(clients[0].0)
        .unwrap()
        .display_name = name;
    world
        .entity_mut(clients[0].0)
        .remove::<crate::plugin::manager::players::PlayerLineage>();
    let manager = attach_scoped(
        &mut router,
        &[(
            "on-player-jump",
            format!(
                r#"
        local.get 1 i32.const 64 call @[method]player.inventory@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 512 i32.const 68 i32.load i32.store
        local.get 1 i32.const 80 call @[method]player.inventory@
        i32.const 80 i32.load8_u i32.const 1 i32.ne if unreachable end
        i32.const 84 i32.load8_u i32.const 7 i32.ne if unreachable end
        i32.const 512 i32.load call @inventory-image_drop@
        {}
    "#,
                witness()
            ),
        )],
    );
    permit(&manager);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert_eq!(
        manager
            .lock()
            .unwrap()
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_live_player_cannot_capture_inventory_after_its_scope_expires() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            (
                "on-player-jump",
                r#"
            local.get 1 i32.const 64 call @[method]player.to-handle@
            i32.const 64 i32.load8_u if unreachable end
            local.get 0 i32.const 68 i32.load i32.const 80 call @[method]world-tx.player@
            i32.const 80 i32.load8_u if unreachable end
            i32.const 512 i32.const 84 i32.load i32.store
            i32.const 68 i32.load call @player-handle_drop@
        "#
                .into(),
            ),
            (
                "on-tick",
                format!(
                    r#"
            i32.const 512 i32.load i32.const 96 call @[method]player.inventory@
            i32.const 96 i32.load8_u i32.const 1 i32.ne if unreachable end
            i32.const 100 i32.load8_u if unreachable end
            i32.const 512 i32.load call @player_drop@
            {}
        "#,
                    witness()
                ),
            ),
        ],
    );
    permit(&manager);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert_eq!(
        manager
            .lock()
            .unwrap()
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
