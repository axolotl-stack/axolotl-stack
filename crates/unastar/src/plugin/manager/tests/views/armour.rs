//! Armour masking keeps native hands and physical equipment authoritative.

use super::*;

/// Give the actual source complete physical gear rather than replacing packet expectations.
fn equip(h: &mut Harness) {
    let subject = h.players[0];
    let world = h.server.ecs.world_mut();
    world
        .get_mut::<MainInventory>(subject)
        .unwrap()
        .0
        .set_item(
            0,
            crate::item::test_support::stack("minecraft:diamond_sword", 1),
        )
        .unwrap();
    world.get_mut::<OffhandSlot>(subject).unwrap().0 =
        crate::item::test_support::stack("minecraft:totem_of_undying", 1);
    let mut armour = world.get_mut::<ArmourInventory>(subject).unwrap();
    for (slot, item) in [
        "minecraft:diamond_helmet",
        "minecraft:diamond_chestplate",
        "minecraft:diamond_leggings",
        "minecraft:diamond_boots",
    ]
    .into_iter()
    .enumerate()
    {
        armour
            .set_slot(slot, crate::item::test_support::stack(item, 1))
            .unwrap();
    }
}

/// Inspect the subject's actual wire-round-tripped equipment publication.
fn armour(packets: &[McpePacketData]) -> &MobArmorEquipmentPacket {
    packets
        .iter()
        .find_map(|packet| match packet {
            McpePacketData::MobArmorEquipmentPacket(armour)
                if armour.target_runtime_id.actor_runtime_id == 7 =>
            {
                Some(armour)
            }
            _ => None,
        })
        .expect("native subject armour packet")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn armour_only_mask_is_viewer_specific_and_keeps_both_hands_and_physical_slots() {
    let mut h = Harness::new(scoped(&project_armour(
        "Masked",
        false,
        true,
        true,
        Some(false),
    )));
    equip(&mut h);
    let subject = h.players[0];
    let original = h
        .server
        .ecs
        .world()
        .get::<ArmourInventory>(subject)
        .unwrap()
        .clone();
    h.jump(1);
    h.sync();
    let hidden = h.packets(1);
    let shown = h.packets(2);
    let masked = armour(&hidden);
    assert!(
        [&masked.head, &masked.torso, &masked.legs, &masked.feet]
            .iter()
            .all(|item| item.id == 0)
    );
    let baseline = armour(&shown);
    assert!(
        [
            &baseline.head,
            &baseline.torso,
            &baseline.legs,
            &baseline.feet
        ]
        .iter()
        .all(|item| item.id != 0)
    );
    for packets in [&hidden, &shown] {
        assert!(packets.iter().any(|packet| matches!(packet, McpePacketData::AddPlayerPacket(add) if add.target_runtime_id.actor_runtime_id == 7 && add.carried_item.id != 0)));
        for container in [0, 119] {
            assert!(packets.iter().any(|packet| matches!(packet, McpePacketData::MobEquipmentPacket(hand) if hand.target_runtime_id.actor_runtime_id == 7 && hand.container_id == container && hand.item.id != 0)));
        }
    }
    let current = h
        .server
        .ecs
        .world()
        .get::<ArmourInventory>(subject)
        .unwrap();
    for slot in 0..4 {
        assert_eq!(current.slot(slot), original.slot(slot));
        assert_eq!(
            current.slot(slot).map(|item| item.net_id),
            original.slot(slot).map(|item| item.net_id)
        );
    }
    assert!(
        !h.server
            .ecs
            .world()
            .get::<OffhandSlot>(subject)
            .unwrap()
            .0
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clearing_or_unloading_armour_masks_restores_latest_native_gear_without_readding_actor() {
    for unload in [false, true] {
        let body = format!(
            "i32.const 600 i32.load i32.const 1 i32.eq if {} else {} end",
            project_armour("Masked", false, true, true, Some(false)),
            clear()
        );
        let mut h = Harness::new(scoped(&body));
        equip(&mut h);
        h.jump(1);
        h.sync();
        h.packets(1);
        h.server
            .ecs
            .world_mut()
            .get_mut::<ArmourInventory>(h.players[0])
            .unwrap()
            .set_helmet(crate::item::test_support::stack("minecraft:iron_helmet", 1))
            .unwrap();
        h.sync();
        assert!(!h.packets(1).iter().any(|packet| matches!(packet, McpePacketData::MobArmorEquipmentPacket(armour) if armour.target_runtime_id.actor_runtime_id == 7 && armour.head.id != 0)));
        if unload {
            h.manager.lock().unwrap().plugins.clear();
        } else {
            h.jump(1);
        }
        h.sync();
        let restored = h.packets(1);
        assert_ne!(armour(&restored).head.id, 0);
        assert!(!restored.iter().any(|packet| matches!(packet, McpePacketData::AddPlayerPacket(add) if add.target_runtime_id.actor_runtime_id == 7)));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn armour_allowance_cannot_reveal_hands_or_armour_hidden_by_the_equipment_restriction() {
    let mut h = Harness::new(scoped(&project_armour(
        "Restricted",
        false,
        true,
        false,
        Some(true),
    )));
    equip(&mut h);
    h.jump(1);
    h.sync();
    let packets = h.packets(1);
    let masked = armour(&packets);
    assert!(
        [&masked.head, &masked.torso, &masked.legs, &masked.feet]
            .iter()
            .all(|item| item.id == 0)
    );
    assert!(packets.iter().any(|packet| matches!(packet, McpePacketData::AddPlayerPacket(add) if add.target_runtime_id.actor_runtime_id == 7 && add.carried_item.id == 0)));
}
