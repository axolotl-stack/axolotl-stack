//! Actual SDK decisions change physical slots without entering durable delivery quarantine.

use super::*;
use crate::entity::components::{ArmourInventory, HeldSlot, MainInventory, OffhandSlot, Position};
use crate::item::ItemStack;
use crate::item::enchant::{durability_damage, with_durability_damage, with_item_tag};
use bevy_ecs::prelude::On;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use zuri_nbt::{NBTTag, tag};

/// Build the real public SDK fixture into an independent target directory.
fn guest() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/inventory-mechanics/Cargo.toml");
        let target = tempfile::tempdir().unwrap();
        let mut command = Command::new(env!("CARGO"));
        command
            .args([
                "build",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(manifest)
            .env("CARGO_TARGET_DIR", target.path())
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        if let Some(toolchain) = option_env!("RUSTUP_TOOLCHAIN") {
            command.env("RUSTUP_TOOLCHAIN", toolchain);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "inventory mechanics fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/inventory_mechanics_fixture.wasm"),
        )
        .unwrap()
    })
}

impl Harness {
    /// Load real canonical imports with independently granted live-inventory permission.
    fn inventory_mechanics(permitted: bool, required: bool) -> Self {
        let mut capabilities = vec![
            PluginCapability::Commands,
            PluginCapability::ItemValues,
            PluginCapability::ReadInventory,
            PluginCapability::ScheduleTasks,
        ];
        if permitted {
            capabilities.push(PluginCapability::InventoryMechanics);
        }
        Self::component_factory(false, &capabilities, required, false, |engine| {
            wasmtime::component::Component::new(engine, guest()).unwrap()
        })
    }

    /// Exercise the ordinary command bridge and its valid default cancellation.
    fn inventory_command(&mut self, command: &str) {
        assert!(commands::dispatch(
            self.server.ecs.world_mut(),
            self.player,
            command,
            None
        ));
    }

    /// Prepare registered native items with unrelated metadata that mutations must preserve.
    fn physical_stack(&self, identifier: &str, count: u8, marker: i32) -> ItemStack {
        let stack =
            crate::server::game::world_stack(self.server.ecs.world(), identifier, count).unwrap();
        with_item_tag(
            &stack,
            tag::Compound(std::collections::HashMap::from([
                ("foreign".into(), NBTTag::Int(tag::Int(marker))),
                (
                    "unrelated".into(),
                    NBTTag::String(tag::String("preserve".into())),
                ),
            ])),
        )
    }

    /// Install exact physical stacks as native setup, never through a guest-created image.
    fn main_item(&mut self, slot: usize, stack: ItemStack) {
        self.server
            .ecs
            .world_mut()
            .get_mut::<MainInventory>(self.player)
            .unwrap()
            .0
            .set_item(slot, stack)
            .unwrap();
    }

    /// Copy the physical authoritative main inventory including its private network identities.
    fn main(&self) -> Vec<ItemStack> {
        self.server
            .ecs
            .world()
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .slots()
            .to_vec()
    }

    /// Observe only native wire inventory publications, independently of the guest result.
    fn inventory_packet_count(&mut self) -> usize {
        std::iter::from_fn(|| self.packets.try_recv().ok())
            .filter(|packet| matches!(packet.data, McpePacketData::InventoryContentPacket(_)))
            .count()
    }
}

/// Exact comparisons include IDs so a move cannot silently clone a stack's identity.
fn exact(left: &[ItemStack], right: &[ItemStack]) {
    assert_eq!(left, right);
    assert_eq!(
        left.iter().map(|stack| stack.net_id).collect::<Vec<_>>(),
        right.iter().map(|stack| stack.net_id).collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hotbar_bijection_preserves_every_stack_and_leaves_health_and_physics_active() {
    let mut h = Harness::inventory_mechanics(true, true);
    for index in 0..9 {
        let stack = h.physical_stack("minecraft:stone", index as u8 + 1, index as i32);
        h.main_item(index, stack);
    }
    let extra = h.physical_stack("minecraft:paper", 7, 70);
    h.main_item(12, extra);
    let before = h.main();
    let health = h
        .server
        .ecs
        .world()
        .get::<Health>(h.player)
        .unwrap()
        .current;
    while h.packets.try_recv().is_ok() {}
    h.inventory_command("mechanic reverse");
    let after = h.main();
    exact(
        &after[..9],
        &before[..9].iter().rev().cloned().collect::<Vec<_>>(),
    );
    exact(&after[9..], &before[9..]);
    assert_eq!(h.server.ecs.world().get::<HeldSlot>(h.player).unwrap().0, 0);
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<Health>(h.player)
            .unwrap()
            .current,
        health
    );
    assert_eq!(
        h.server.ecs.world().get::<Position>(h.player).unwrap().0,
        glam::DVec3::new(0.5, 17.62, 0.5)
    );
    assert!(
        h.server
            .ecs
            .world()
            .get::<crate::entity::components::PlayerInput>(h.player)
            .is_some()
    );
    assert!(!crate::server::game::inventory_effects::is_quarantined(
        h.server.ecs.world(),
        h.player
    ));
    assert_eq!(h.inventory_packet_count(), 4);
    h.inventory_command("mechanic poll");
    assert_eq!(h.logs(), ["staged", "applied"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_consumption_applies_once_and_does_not_create_a_durable_receipt() {
    let mut h = Harness::inventory_mechanics(true, true);
    let stack = h.physical_stack("minecraft:paper", 3, 1);
    h.main_item(0, stack);
    let original = h.main()[0].clone();
    h.inventory_command("mechanic double");
    let after = &h.main()[0];
    assert_eq!(after, &original.clone().with_count(2));
    assert_eq!(after.net_id, original.net_id);
    h.inventory_command("mechanic poll");
    assert_eq!(h.logs(), ["staged", "staged", "applied", "rejected"]);
    assert!(!crate::server::game::inventory_effects::is_quarantined(
        h.server.ecs.world(),
        h.player
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consumption_rejects_replaced_identical_stacks_and_changed_selection() {
    for changed_selection in [false, true] {
        let mut h = Harness::inventory_mechanics(true, true);
        let stack = h.physical_stack("minecraft:paper", 3, 1);
        h.main_item(0, stack);
        h.inventory_command("mechanic capture");
        let original = h.main()[0].clone();
        if changed_selection {
            h.main_item(1, original.clone().with_net_id(crate::item::fresh_net_id()));
            h.server
                .ecs
                .world_mut()
                .get_mut::<HeldSlot>(h.player)
                .unwrap()
                .0 = 1;
        } else {
            h.main_item(0, original.with_net_id(crate::item::fresh_net_id()));
        }
        let before = h.main();
        h.inventory_command("mechanic consume-captured");
        exact(&h.main(), &before);
        assert_eq!(h.logs(), ["stale"]);
        assert!(h.manager.lock().unwrap().readiness.check().is_ok());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repair_changes_only_native_wear_in_each_supported_physical_area() {
    for area in ["repair", "armour", "offhand"] {
        let mut h = Harness::inventory_mechanics(true, true);
        let identifier = if area == "armour" {
            "minecraft:diamond_helmet"
        } else {
            "minecraft:diamond_sword"
        };
        let stack = crate::item::enchant::with_block_lists(
            &with_durability_damage(&h.physical_stack(identifier, 1, 77), 12),
            &["minecraft:stone".into()],
            &["minecraft:dirt".into()],
        )
        .into_slot();
        match area {
            "armour" => {
                h.server
                    .ecs
                    .world_mut()
                    .get_mut::<ArmourInventory>(h.player)
                    .unwrap()
                    .set_slot(0, stack)
                    .unwrap();
            }
            "offhand" => {
                h.server
                    .ecs
                    .world_mut()
                    .get_mut::<OffhandSlot>(h.player)
                    .unwrap()
                    .0 = stack
            }
            _ => h.main_item(0, stack),
        }
        let get = |h: &Harness| match area {
            "armour" => h
                .server
                .ecs
                .world()
                .get::<ArmourInventory>(h.player)
                .unwrap()
                .slot(0)
                .unwrap()
                .clone(),
            "offhand" => h
                .server
                .ecs
                .world()
                .get::<OffhandSlot>(h.player)
                .unwrap()
                .0
                .clone(),
            _ => h.main()[0].clone(),
        };
        let before = get(&h);
        h.inventory_command(&format!("mechanic {area}"));
        let after = get(&h);
        assert_eq!(durability_damage(&after), 8);
        assert_eq!(after.item_id(), before.item_id());
        assert_eq!(after.count, before.count);
        assert_eq!(after.damage, before.damage);
        assert_eq!(after.runtime_id, before.runtime_id);
        let mut old_tag = crate::item::enchant::item_tag(&before).unwrap();
        let mut new_tag = crate::item::enchant::item_tag(&after).unwrap();
        old_tag.0.remove("Damage");
        new_tag.0.remove("Damage");
        assert_eq!(new_tag, old_tag);
        assert_eq!(
            crate::item::enchant::block_lists(&after),
            crate::item::enchant::block_lists(&before)
        );
        assert_eq!(after.net_id, before.net_id);
        h.inventory_command("mechanic poll");
        assert_eq!(h.logs(), ["staged", "applied"]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_orders_costs_and_private_area_repairs_never_change_items() {
    let mut h = Harness::inventory_mechanics(true, true);
    let stack = h.physical_stack("minecraft:diamond_sword", 1, 2);
    h.main_item(0, stack);
    let before = h.main();
    for command in [
        "mechanic invalid",
        "mechanic invalid-consume",
        "mechanic invalid-repair",
    ] {
        h.inventory_command(command);
        exact(&h.main(), &before);
    }
    assert_eq!(h.logs(), ["invalid", "invalid-consume", "invalid-repair"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_guest_ticket_does_not_discard_an_accepted_live_change() {
    let mut h = Harness::inventory_mechanics(true, true);
    let stack = h.physical_stack("minecraft:paper", 3, 4);
    h.main_item(0, stack);
    let before = h.main();
    h.inventory_command("mechanic drop");
    exact(
        &h.main()[..9],
        &before[..9].iter().rev().cloned().collect::<Vec<_>>(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_traps_discard_all_live_inventory_changes_and_release_reserved_bytes() {
    let mut h = Harness::inventory_mechanics(true, false);
    let stack = h.physical_stack("minecraft:paper", 3, 4);
    h.main_item(0, stack);
    let before = h.main();
    h.inventory_command("mechanic trap");
    exact(&h.main(), &before);
    assert!(h.manager.lock().unwrap().readiness.check().is_ok());
    assert_eq!(h.logs(), ["staged"]);
    let manager = h.manager.lock().unwrap();
    let context = manager.plugins.values().next().unwrap().store.data();
    assert!(matches!(
        context.test_inventory_mechanics_statuses().as_slice(),
        [wit_types::InventoryMechanicsStatus::Cancelled]
    ));
    assert_eq!(context.resources.value_budget.used(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_inventory_requires_its_own_capability_and_a_living_complete_actor() {
    let mut h = Harness::inventory_mechanics(false, true);
    let before = h.main();
    h.inventory_command("mechanic denied");
    exact(&h.main(), &before);
    assert_eq!(h.logs(), ["denied"]);
    for incomplete in [false, true] {
        let mut h = Harness::inventory_mechanics(true, true);
        if incomplete {
            h.server
                .ecs
                .world_mut()
                .entity_mut(h.player)
                .remove::<crate::entity::components::PlayerUi>();
        } else {
            h.server
                .ecs
                .world_mut()
                .get_mut::<Health>(h.player)
                .unwrap()
                .current = 0.0;
        }
        h.inventory_command("mechanic unavailable");
        assert_eq!(h.logs(), ["unavailable"]);
        assert!(h.manager.lock().unwrap().readiness.check().is_ok());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_positive_melee_completion_consumes_the_original_weapon_before_native_wear() {
    let mut h = Harness::inventory_mechanics(true, true);
    let weapon = with_durability_damage(&h.physical_stack("minecraft:diamond_sword", 1, 6), 12);
    h.main_item(0, weapon);
    h.inventory_command("mechanic arm");
    let (outbound_tx, _target_packets) = tokio::sync::mpsc::channel(4096);
    let target = h
        .server
        .spawn_player(PlayerSpawnData {
            session_id: 43,
            runtime_id: 43,
            display_name: "Target".into(),
            xuid: Some("43".into()),
            uuid: Some(uuid::Uuid::from_u128(43).to_string()),
            position: glam::DVec3::new(2.5, 17.62, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    h.server
        .ecs
        .world_mut()
        .get_mut::<Health>(target)
        .unwrap()
        .current = 20.0;
    crate::entity::mob::combat::player_attack(
        h.server.ecs.world_mut(),
        h.player,
        target,
        &Default::default(),
    );
    assert!(h.server.ecs.world().get::<Health>(target).unwrap().current < 20.0);
    assert!(
        h.main()[0].is_empty(),
        "native wear must never recreate the consumed original weapon"
    );
    h.inventory_command("mechanic poll");
    assert_eq!(h.logs(), ["staged", "applied"]);
    assert_eq!(
        h.server.ecs.world().get::<Position>(h.player).unwrap().0,
        glam::DVec3::new(0.5, 17.62, 0.5)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_native_melee_veto_cannot_charge_a_positive_loss_handler() {
    let mut h = Harness::inventory_mechanics(true, true);
    let weapon = h.physical_stack("minecraft:diamond_sword", 1, 6);
    h.main_item(0, weapon);
    h.inventory_command("mechanic arm");
    h.server.ecs.world_mut().add_observer(
        |mut event: On<crate::entity::player_life::hooks::PlayerDamage>| event.cancel(),
    );
    let (outbound_tx, _target_packets) = tokio::sync::mpsc::channel(4096);
    let target = h
        .server
        .spawn_player(PlayerSpawnData {
            session_id: 43,
            runtime_id: 43,
            display_name: "Target".into(),
            xuid: Some("43".into()),
            uuid: Some(uuid::Uuid::from_u128(43).to_string()),
            position: glam::DVec3::new(2.5, 17.62, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    let before = h.main();
    crate::entity::mob::combat::player_attack(
        h.server.ecs.world_mut(),
        h.player,
        target,
        &Default::default(),
    );
    assert_eq!(
        h.server.ecs.world().get::<Health>(target).unwrap().current,
        20.0
    );
    exact(&h.main(), &before);
    assert!(h.logs().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_tasks_allow_genuine_retained_actors_and_reject_frozen_or_incomplete_sources() {
    use crate::server::game::inventory_effects as native;
    for mode in [
        "retained",
        "quarantined",
        "recovering",
        "incomplete",
        "dead",
    ] {
        let mut h = Harness::inventory_mechanics(true, true);
        let stack = h.physical_stack("minecraft:paper", 3, 4);
        h.main_item(0, stack);
        let before = h.main();
        h.inventory_command("mechanic schedule");
        let directory = tempfile::tempdir().unwrap();
        match mode {
            "retained" => h.server.despawn_player(42),
            "quarantined" => {
                h.server.ecs.world_mut().insert_resource(
                    crate::server::game::types::PlayerProviderResource(Arc::new(
                        crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap(),
                    )),
                );
                native::install(&mut h.server).unwrap();
                let expected =
                    native::InventoryState::capture(h.server.ecs.world(), h.player).unwrap();
                let mut replacement = expected.clone();
                replacement.held_slot = 1;
                assert!(matches!(
                    native::submit(
                        h.server.ecs.world_mut(),
                        h.player,
                        native::InventoryEffect {
                            id: "sample:hold".into(),
                            expected,
                            replacement
                        }
                    )
                    .unwrap(),
                    native::Submission::Queued
                ));
            }
            "recovering" => {
                let source =
                    native::RecoverySource::capture(h.server.ecs.world(), h.player).unwrap();
                native::fence_for_recovery(h.server.ecs.world_mut(), &source).unwrap();
            }
            "incomplete" => {
                h.server
                    .ecs
                    .world_mut()
                    .entity_mut(h.player)
                    .remove::<crate::entity::components::EnderChestInventory>();
            }
            "dead" => {
                h.server
                    .ecs
                    .world_mut()
                    .get_mut::<Health>(h.player)
                    .unwrap()
                    .current = 0.0
            }
            _ => unreachable!(),
        }
        let player = h.player;
        let manager = h.manager;
        let mut router = crate::server::worlds::DimensionRouter::new(h.server);
        router.set_plugin_manager(manager.clone());
        manager.lock().unwrap().drain_scheduled_tasks(&mut router);
        let world = router.world(0).unwrap().ecs.world();
        let context = manager.lock().unwrap();
        let logs = &context
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_logs;
        if mode == "retained" {
            exact(
                &world.get::<MainInventory>(player).unwrap().0.slots()[..9],
                &before[..9].iter().rev().cloned().collect::<Vec<_>>(),
            );
            assert_eq!(logs, &["staged"]);
            assert!(world.get::<PlayerSession>(player).is_none());
            assert!(world.get::<Health>(player).unwrap().current > 0.0);
        } else {
            assert_eq!(logs, &["unavailable"]);
            if let Some(main) = world.get::<MainInventory>(player) {
                exact(main.0.slots(), &before);
            }
        }
        assert!(context.readiness.check().is_ok());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn physical_completion_remains_an_owned_fact_after_the_actor_is_removed() {
    let mut h = Harness::inventory_mechanics(true, true);
    h.inventory_command("mechanic reverse");
    h.inventory_command("mechanic schedule-poll");
    h.server.ecs.world_mut().despawn(h.player);
    let manager = h.manager;
    let mut router = crate::server::worlds::DimensionRouter::new(h.server);
    router.set_plugin_manager(manager.clone());
    manager.lock().unwrap().drain_scheduled_tasks(&mut router);
    let manager = manager.lock().unwrap();
    assert_eq!(
        manager
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_logs,
        ["staged", "owned-applied"]
    );
    assert!(manager.readiness.check().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_permutation_cannot_transfer_native_weapon_wear_to_an_identical_replacement() {
    let mut h = Harness::inventory_mechanics(true, true);
    let weapon = with_durability_damage(&h.physical_stack("minecraft:diamond_sword", 1, 6), 12);
    h.main_item(0, weapon.clone());
    h.main_item(8, weapon.with_net_id(crate::item::fresh_net_id()));
    let before = h.main();
    assert_ne!(before[0].net_id, before[8].net_id);
    h.inventory_command("mechanic arm-reverse");
    let (outbound_tx, _target_packets) = tokio::sync::mpsc::channel(4096);
    let target = h
        .server
        .spawn_player(PlayerSpawnData {
            session_id: 43,
            runtime_id: 43,
            display_name: "Target".into(),
            xuid: Some("43".into()),
            uuid: Some(uuid::Uuid::from_u128(43).to_string()),
            position: glam::DVec3::new(2.5, 17.62, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    crate::entity::mob::combat::player_attack(
        h.server.ecs.world_mut(),
        h.player,
        target,
        &Default::default(),
    );
    assert!(h.server.ecs.world().get::<Health>(target).unwrap().current < 20.0);
    exact(
        &h.main()[..9],
        &before[..9].iter().rev().cloned().collect::<Vec<_>>(),
    );
    h.inventory_command("mechanic poll");
    assert_eq!(h.logs(), ["staged", "applied"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_boundary_revalidates_exact_transport_selection_and_slot_identities() {
    use crate::plugin::manager::wit_bindings::unastar::plugin::types::{
        HostInventoryMechanicsOperation, HostPlayer,
    };
    for mode in ["selection", "slot", "channel"] {
        let mut h = Harness::inventory_mechanics(true, true);
        let stack = h.physical_stack("minecraft:paper", 3, 4);
        h.main_item(0, stack);
        let engine = engine();
        let mut store = store(&engine, &PluginLimits::default());
        store.data_mut().capabilities = [
            PluginCapability::ReadPlayers,
            PluginCapability::InventoryMechanics,
        ]
        .into();
        let operation = with_world(&mut store, h.server.ecs.world_mut(), |store| {
            let player = make_player_resource(store, h.player).unwrap();
            HostPlayer::reorder_hotbar(store.data_mut(), player, vec![8, 7, 6, 5, 4, 3, 2, 1, 0])
                .unwrap()
        });
        let _replacement_receiver;
        match mode {
            "selection" => {
                h.server
                    .ecs
                    .world_mut()
                    .get_mut::<HeldSlot>(h.player)
                    .unwrap()
                    .0 = 1
            }
            "slot" => {
                let mut original = h.main()[0].clone();
                original.net_id = crate::item::fresh_net_id();
                h.main_item(0, original);
            }
            "channel" => {
                let (tx, rx) = tokio::sync::mpsc::channel(4096);
                _replacement_receiver = rx;
                h.server
                    .ecs
                    .world_mut()
                    .entity_mut(h.player)
                    .insert(PlayerSession::new(
                        42,
                        "Player".into(),
                        Some("42".into()),
                        Some(uuid::Uuid::from_u128(42).to_string()),
                        tx,
                    ));
            }
            _ => unreachable!(),
        }
        let expected = h.main();
        finish_invocation(store.data_mut(), h.server.ecs.world_mut(), true);
        exact(&h.main(), &expected);
        assert!(matches!(
            HostInventoryMechanicsOperation::status(store.data_mut(), operation).unwrap(),
            wit_types::InventoryMechanicsStatus::Rejected(_)
        ));
        assert_eq!(store.data().resources.value_budget.used(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_tickets_cannot_bypass_callback_capacity_and_failure_releases_all_values() {
    use crate::plugin::manager::wit_bindings::unastar::plugin::types::{
        HostInventoryMechanicsOperation, HostPlayer,
    };
    let mut h = Harness::inventory_mechanics(true, true);
    let engine = engine();
    let mut store = store(&engine, &PluginLimits::default());
    store.data_mut().capabilities = [
        PluginCapability::ReadPlayers,
        PluginCapability::InventoryMechanics,
    ]
    .into();
    let before = h.main();
    with_world(&mut store, h.server.ecs.world_mut(), |store| {
        let player = make_player_resource(store, h.player).unwrap();
        for _ in 0..64 {
            let operation = HostPlayer::reorder_hotbar(
                store.data_mut(),
                wasmtime::component::Resource::new_borrow(player.rep()),
                vec![8, 7, 6, 5, 4, 3, 2, 1, 0],
            )
            .unwrap();
            HostInventoryMechanicsOperation::drop(store.data_mut(), operation).unwrap();
        }
        assert!(matches!(
            HostPlayer::reorder_hotbar(store.data_mut(), player, vec![8, 7, 6, 5, 4, 3, 2, 1, 0]),
            Err(wit_types::AccessError::LimitExceeded)
        ));
    });
    assert!(store.data().resources.value_budget.used() > 0);
    finish_invocation(store.data_mut(), h.server.ecs.world_mut(), false);
    assert_eq!(store.data().resources.value_budget.used(), 0);
    exact(&h.main(), &before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_operation_resource_allocation_releases_the_entire_slot_reservation() {
    use crate::plugin::manager::wit_bindings::unastar::plugin::types::HostPlayer;
    let mut h = Harness::inventory_mechanics(true, true);
    let engine = engine();
    let limits = PluginLimits {
        max_owned_resources: 1,
        ..PluginLimits::default()
    };
    let mut store = store(&engine, &limits);
    store.data_mut().capabilities = [
        PluginCapability::ReadPlayers,
        PluginCapability::InventoryMechanics,
    ]
    .into();
    let before = h.main();
    with_world(&mut store, h.server.ecs.world_mut(), |store| {
        let player = make_player_resource(store, h.player).unwrap();
        assert!(matches!(
            HostPlayer::reorder_hotbar(store.data_mut(), player, vec![8, 7, 6, 5, 4, 3, 2, 1, 0]),
            Err(wit_types::AccessError::LimitExceeded)
        ));
    });
    assert_eq!(store.data().resources.value_budget.used(), 0);
    assert!(store.data().pending_inventory_mechanics.is_empty());
    finish_invocation(store.data_mut(), h.server.ecs.world_mut(), true);
    exact(&h.main(), &before);
}
