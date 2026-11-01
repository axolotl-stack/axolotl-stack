//! Living actors remain authoritative while exact value delivery waits for storage.

use super::*;
use crate::entity::components::{GameMode, HeldSlot, Hunger, Position};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use unastar_data::actor_components::body::ActorDamageCause;

fn hit(damage: f32, cause: ActorDamageCause) -> Hit {
    Hit {
        damage,
        cause,
        attacker: None,
        by_player: true,
        knockback_from: None,
        knockback_power: KnockbackPower::DEFAULT,
    }
}

fn equipped(harness: &mut Harness) {
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    *world.get_mut::<GameMode>(harness.player).unwrap() = GameMode::Survival;
    world
        .get_mut::<MainInventory>(harness.player)
        .unwrap()
        .0
        .set_item(
            0,
            crate::item::test_support::stack("minecraft:snowball", 16),
        )
        .unwrap();
    world.get_mut::<HeldSlot>(harness.player).unwrap().0 = 0;
    world
        .get_mut::<ArmourInventory>(harness.player)
        .unwrap()
        .set_slot(
            1,
            crate::item::test_support::stack("minecraft:diamond_chestplate", 1),
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delayed_grant_keeps_actual_damage_armour_wear_consumption_and_movement_live() {
    for mode in [54, 55, 65] {
        let directory = tempfile::tempdir().unwrap();
        let provider = gated(directory.path(), false);
        let mut harness = Harness::new(mode, provider.clone(), limits(), true);
        ready(&mut harness).await;
        if mode == 54 {
            fill(&mut harness);
        }
        equipped(&mut harness);
        harness.action().await.unwrap();
        provider.started.acquire().await.unwrap().forget();
        let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
        let applied = crate::entity::player_life::hurt_player(
            world,
            harness.player,
            &hit(8.0, ActorDamageCause::EntityAttack),
        );
        assert!(
            applied > 0.0,
            "an ordinary delayed value grant cannot make its living recipient immune"
        );
        let health = world.get::<Health>(harness.player).unwrap().current;
        let wear = crate::item::enchant::durability_damage(
            &world
                .get::<ArmourInventory>(harness.player)
                .unwrap()
                .slots()[1],
        );
        let exhaustion = world.get::<Hunger>(harness.player).unwrap().exhaustion;
        assert!(wear > 0);
        assert!(exhaustion > 0.0);
        assert!(!native::is_quarantined(world, harness.player));
        world
            .resource_mut::<crate::entity::projectile::ThrowQueue>()
            .0
            .push(harness.player);
        crate::entity::projectile::apply_throws(world);
        assert_eq!(
            world
                .get::<MainInventory>(harness.player)
                .unwrap()
                .0
                .slots()[0]
                .count,
            15,
            "real native selected consumption remains available"
        );
        harness.move_to(1.5).await.unwrap();
        assert_eq!(
            harness
                .router
                .world(0)
                .unwrap()
                .ecs
                .world()
                .get::<Position>(harness.player)
                .unwrap()
                .0
                .x,
            1.5
        );
        assert!(
            drops(&mut harness).is_empty(),
            "no incoming value appears before its durable write"
        );
        let remaining = if mode == 65 { 14 } else { 15 };
        provider.release.add_permits(2);
        harness.acknowledge(1).await;
        let world = harness.router.world(0).unwrap().ecs.world();
        assert_eq!(
            world.get::<Health>(harness.player).unwrap().current,
            health,
            "ACK never restores a captured earlier health value"
        );
        assert_eq!(
            crate::item::enchant::durability_damage(
                &world
                    .get::<ArmourInventory>(harness.player)
                    .unwrap()
                    .slots()[1],
            ),
            wear,
            "unchanged worn slots are not replaced by their earlier image"
        );
        assert_eq!(
            world
                .get::<MainInventory>(harness.player)
                .unwrap()
                .0
                .slots()[0]
                .count,
            remaining
        );
        let saved = provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.inventory[0].count, remaining,
            "the catch-up checkpoint preserves consumed value"
        );
        assert_eq!(saved.health, health);
        let registry = &world
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0;
        assert_eq!(
            crate::item::enchant::durability_damage(
                &saved.armour[1].clone().restore(registry).unwrap()
            ),
            wear
        );
        assert!(
            harness
                .logs()
                .iter()
                .any(|line| line == "live-inventory-busy")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn death_during_delayed_grant_cannot_publish_loot_or_respawn_before_persistence() {
    for (mode, immediate) in [(54, false), (55, false), (55, true)] {
        let directory = tempfile::tempdir().unwrap();
        let provider = gated(directory.path(), false);
        provider.hold_terminal.store(true, Ordering::Release);
        let mut harness = Harness::new(mode, provider.clone(), limits(), true);
        ready(&mut harness).await;
        if mode == 54 {
            fill(&mut harness);
        }
        equipped(&mut harness);
        harness
            .router
            .world_mut(0)
            .unwrap()
            .ecs
            .world_mut()
            .resource_mut::<crate::world::rules::GameRules>()
            .do_immediate_respawn = immediate;
        harness.action().await.unwrap();
        provider.started.acquire().await.unwrap().forget();
        let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
        assert_eq!(
            crate::entity::player_life::hurt_player(
                world,
                harness.player,
                &hit(100.0, ActorDamageCause::Override)
            ),
            20.0
        );
        assert!(
            world
                .get::<crate::entity::player_life::Dead>(harness.player)
                .is_some()
        );
        assert_eq!(world.get::<Health>(harness.player).unwrap().current, 0.0);
        crate::entity::player_life::respawn::respawn(world, harness.player);
        assert_eq!(
            world.get::<Health>(harness.player).unwrap().current,
            0.0,
            "unacknowledged terminal work cannot be escaped with a respawn packet"
        );
        assert!(drops(&mut harness).is_empty());
        assert!(
            !std::iter::from_fn(|| harness._packets.try_recv().ok()).any(|packet| matches!(
                packet.data,
                jolyne::valentine::McpePacketData::DeathInfoPacket(_)
            ))
        );
        provider.release.add_permits(4);
        harness.acknowledge(1).await;
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                harness.router.tick().await.unwrap();
                if let Ok(permit) = provider.terminal_started.try_acquire() {
                    permit.forget();
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("actual post-drop canonical writer starts after the grant ACK");
        let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
        assert!(crate::server::game::player_state::source_actors::pending(
            world,
            harness.player
        ));
        crate::entity::player_life::respawn::respawn(world, harness.player);
        assert!(
            world
                .get::<crate::entity::player_life::Dead>(harness.player)
                .is_some()
        );
        assert!(
            world
                .get::<Health>(harness.player)
                .is_none_or(|health| health.current == 0.0)
        );
        assert_eq!(
            world
                .query::<(
                    &ItemEntityStack,
                    Option<&crate::entity::components::PersistentWorldItem>
                )>()
                .iter(world)
                .filter(|(_, managed)| managed.is_none())
                .count(),
            0
        );
        assert!(
            !std::iter::from_fn(|| harness._packets.try_recv().ok()).any(|packet| matches!(
                packet.data,
                jolyne::valentine::McpePacketData::DeathInfoPacket(_)
            ))
        );
        let before_terminal = provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            before_terminal.health, 0.0,
            "the catch-up never normalizes an actually dead recipient to healthy"
        );
        assert!(before_terminal.native_terminal.is_none());
        provider.terminal_release.add_permits(1);
        await_terminal(&mut harness).await;
        let world = harness.router.world(0).unwrap().ecs.world();
        assert_eq!(
            world
                .get::<crate::entity::player_life::Dead>(harness.player)
                .is_none(),
            immediate
        );
        assert_eq!(
            world.get::<Health>(harness.player).unwrap().current,
            if immediate { 20.0 } else { 0.0 }
        );
        let saved = provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        assert!(
            saved.inventory.iter().all(|item| item.count == 0),
            "terminal state supersedes the earlier live grant image before loot is exposed"
        );
        assert_eq!(saved.health, 0.0);
        assert!(saved.native_terminal.is_some());
        assert!(
            std::iter::from_fn(|| harness._packets.try_recv().ok()).any(|packet| matches!(
                packet.data,
                jolyne::valentine::McpePacketData::DeathInfoPacket(_)
            ))
        );
        assert_eq!(
            drops(&mut harness)
                .iter()
                .map(|(_, _, count)| u32::from(*count))
                .sum::<u32>(),
            if mode == 54 { 130 } else { 2 }
        );
        crate::entity::player_life::respawn::respawn(
            harness.router.world_mut(0).unwrap().ecs.world_mut(),
            harness.player,
        );
        assert!(
            harness
                .router
                .world(0)
                .unwrap()
                .ecs
                .world()
                .get::<crate::entity::player_life::Dead>(harness.player)
                .is_none()
        );
        assert!(harness.router.save_player(1).await);
        let respawned = provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        assert!(respawned.native_terminal.is_none());
        assert!(respawned.restoration_checkpoint.is_none());
        assert_eq!(respawned.health, respawned.max_health);
        drop(harness);
        provider.close().await.unwrap();
        drop(provider);
        let reopened = Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap());
        let data = reopened
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        assert!(data.native_terminal.is_none());
        assert!(data.restoration_checkpoint.is_none());
        let mut cold = Harness::with_saved(0, reopened, limits(), true, Some(data));
        ready(&mut cold).await;
        let world = cold.router.world(0).unwrap().ecs.world();
        assert_eq!(world.get::<Health>(cold.player).unwrap().current, 20.0);
        assert!(
            world
                .get::<crate::entity::player_life::Dead>(cold.player)
                .is_none()
        );
        assert!(
            world
                .get::<MainInventory>(cold.player)
                .unwrap()
                .0
                .slots()
                .iter()
                .all(|item| item.is_empty())
        );
    }
}

async fn next_write(harness: &mut Harness, provider: &Gate) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            native::poll(harness.router.world_mut(0).unwrap().ecs.world_mut());
            if let Ok(permit) = provider.started.try_acquire() {
                permit.forget();
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("next actual canonical delivery phase");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_destination_change_spills_only_owed_value_and_cold_replay_never_recreates_it() {
    let directory = tempfile::tempdir().unwrap();
    {
        let provider = gated(directory.path(), false);
        let mut harness = Harness::new(55, provider.clone(), limits(), true);
        ready(&mut harness).await;
        equipped(&mut harness);
        harness.action().await.unwrap();
        provider.started.acquire().await.unwrap().forget();
        let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
        let replacement = crate::item::test_support::stack("minecraft:iron_helmet", 1);
        world
            .get_mut::<ArmourInventory>(harness.player)
            .unwrap()
            .set_slot(0, replacement.clone())
            .unwrap();
        provider.release.add_permits(1);
        next_write(&mut harness, &provider).await;
        let world = harness.router.world(0).unwrap().ecs.world();
        assert_eq!(
            world
                .get::<ArmourInventory>(harness.player)
                .unwrap()
                .slots()[0],
            replacement
        );
        assert!(!native::is_quarantined(world, harness.player));
        assert!(native::inventory_busy(world, harness.player));
        assert!(
            drops(&mut harness).is_empty(),
            "spill ownership must acknowledge before native publication"
        );
        assert_eq!(
            provider
                .inner
                .world_items(uuid::Uuid::from_u128(9), 0)
                .await
                .unwrap()
                .len(),
            1
        );
        let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
        assert!(
            crate::entity::player_life::hurt_player(
                world,
                harness.player,
                &hit(4.0, ActorDamageCause::EntityAttack)
            ) > 0.0
        );
        let health = world.get::<Health>(harness.player).unwrap().current;
        provider.release.add_permits(1);
        harness.acknowledge(1).await;
        let world = harness.router.world(0).unwrap().ecs.world();
        assert_eq!(world.get::<Health>(harness.player).unwrap().current, health);
        assert_eq!(
            world
                .get::<ArmourInventory>(harness.player)
                .unwrap()
                .slots()[0]
                .item_id(),
            "minecraft:iron_helmet"
        );
        let actual = drops(&mut harness);
        assert_eq!(actual.len(), 2);
        assert!(
            actual.iter().all(
                |(_, row, count)| row.stack.item_id == "minecraft:diamond_helmet" && *count == 1
            )
        );
        let saved = provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.armour[0].item_id, "minecraft:iron_helmet");
        drop(harness);
        provider.close().await.unwrap();
    }
    let reopened = Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap());
    let saved = reopened
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    let mut harness = Harness::with_saved(61, reopened.clone(), limits(), true, Some(saved));
    ready(&mut harness).await;
    assert_eq!(drops(&mut harness).len(), 2);
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|line| line == "grant-replayed"));
    assert_eq!(drops(&mut harness).len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn merging_grant_adds_only_new_quantity_after_real_selected_consumption() {
    let directory = tempfile::tempdir().unwrap();
    let provider = gated(directory.path(), false);
    let mut harness = Harness::new(62, provider.clone(), limits(), true);
    ready(&mut harness).await;
    harness.action().await.unwrap();
    harness.acknowledge(1).await;
    assert_eq!(harness.count(), 10);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<GameMode>(harness.player)
        .map(|mut mode| *mode = GameMode::Survival)
        .unwrap();
    harness.action().await.unwrap();
    provider.started.acquire().await.unwrap().forget();
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    world
        .resource_mut::<crate::entity::projectile::ThrowQueue>()
        .0
        .push(harness.player);
    crate::entity::projectile::apply_throws(world);
    assert_eq!(harness.count(), 9);
    provider.release.add_permits(2);
    harness.acknowledge(2).await;
    assert_eq!(
        harness.count(),
        15,
        "ACK adds six owed items to the current nine, without restoring the captured ten"
    );
    let world = harness.router.world(0).unwrap().ecs.world();
    let total: u32 = world
        .get::<MainInventory>(harness.player)
        .unwrap()
        .0
        .slots()
        .iter()
        .filter(|item| item.item_id() == "minecraft:snowball")
        .map(|item| u32::from(item.count))
        .sum();
    assert_eq!(total, 29);
    let stored = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.inventory[0].count, 15);
    assert_eq!(
        stored
            .inventory
            .iter()
            .filter(|item| item.item_id == "minecraft:snowball")
            .map(|item| u32::from(item.count))
            .sum::<u32>(),
        29
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_retained_recipient_stays_vulnerable_and_receives_only_its_owed_items() {
    let directory = tempfile::tempdir().unwrap();
    let provider = gated(directory.path(), false);
    let mut harness = Harness::new(55, provider.clone(), limits(), true);
    ready(&mut harness).await;
    equipped(&mut harness);
    harness.action().await.unwrap();
    provider.started.acquire().await.unwrap().forget();
    harness.router.despawn_player(1);
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    world.flush();
    assert!(
        world
            .get::<crate::server::game::player_events::DetachedPlayer>(harness.player)
            .is_some()
    );
    assert!(world.get::<PlayerSession>(harness.player).is_none());
    assert!(native::inventory_busy(world, harness.player));
    assert!(!native::is_quarantined(world, harness.player));
    assert!(
        crate::entity::player_life::hurt_player(
            world,
            harness.player,
            &hit(8.0, ActorDamageCause::EntityAttack)
        ) > 0.0
    );
    let health = world.get::<Health>(harness.player).unwrap().current;
    let wear = crate::item::enchant::durability_damage(
        &world
            .get::<ArmourInventory>(harness.player)
            .unwrap()
            .slots()[1],
    );
    assert!(wear > 0);
    provider.release.add_permits(2);
    harness.acknowledge(1).await;
    let world = harness.router.world(0).unwrap().ecs.world();
    assert_eq!(world.get::<Health>(harness.player).unwrap().current, health);
    assert_eq!(
        crate::item::enchant::durability_damage(
            &world
                .get::<ArmourInventory>(harness.player)
                .unwrap()
                .slots()[1]
        ),
        wear
    );
    assert_eq!(
        world
            .get::<ArmourInventory>(harness.player)
            .unwrap()
            .slots()[0]
            .item_id(),
        "minecraft:diamond_helmet"
    );
    assert!(world.get::<PlayerSession>(harness.player).is_none());
    assert!(!native::inventory_busy(world, harness.player));
    assert_eq!(drops(&mut harness).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_spill_memory_preserves_the_full_opaque_metadata_limit() {
    use zuri_nbt::{NBTTag, tag};
    let prototype = |length| {
        crate::item::enchant::with_item_tag(
            &crate::item::test_support::stack("minecraft:snowball", 1),
            tag::Compound(
                [(
                    "opaque".into(),
                    NBTTag::ByteArray(tag::ByteArray(vec![255; length])),
                )]
                .into_iter()
                .collect(),
            ),
        )
    };
    let overhead = prototype(1).user_data().len() - 1;
    let original = prototype(131_072 - overhead);
    assert_eq!(original.user_data().len(), 131_072);
    assert!(crate::item::enchant::valid_user_data(&original));
    for mode in [63, 64] {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap());
        let mut harness = Harness::new(mode, provider.clone(), limits(), true);
        ready(&mut harness).await;
        harness
            .router
            .world_mut(0)
            .unwrap()
            .ecs
            .world_mut()
            .get_mut::<MainInventory>(harness.player)
            .unwrap()
            .0
            .set_item(0, original.clone())
            .unwrap();
        harness.action().await.unwrap();
        if mode == 63 {
            assert!(
                harness
                    .logs()
                    .iter()
                    .any(|line| line == "grant-fragment-memory-limited")
            );
            assert!(
                provider
                    .load(uuid::Uuid::from_u128(1))
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(!native::inventory_busy(
                harness.router.world(0).unwrap().ecs.world(),
                harness.player
            ));
            assert_eq!(harness.count(), 1);
        } else {
            harness.acknowledge(1).await;
            assert_eq!(harness.count(), 2);
            let stored = provider
                .load(uuid::Uuid::from_u128(1))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                stored.inventory[0].nbt.as_deref(),
                Some(original.user_data().as_ref())
            );
            assert_eq!(stored.inventory[0].count, 2);
        }
        assert!(drops(&mut harness).is_empty());
    }
}

async fn await_terminal(harness: &mut Harness) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            harness.router.tick().await.unwrap();
            let world = harness.router.world(0).unwrap().ecs.world();
            if !crate::server::game::player_state::source_actors::pending(world, harness.player)
                && world.get::<Health>(harness.player).is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("real terminal acknowledgement reaches the actual corpse");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_item_quota_rejection_settles_the_fatal_hit_without_minting_a_grant_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let provider = gated(directory.path(), false);
    let owner = uuid::Uuid::from_u128(99);
    let data = PlayerData {
        uuid: owner.to_string(),
        ..Default::default()
    };
    for batch in 0..16 {
        let items = (0..256)
            .map(|index| PersistentItem {
                id: uuid::Uuid::from_u128(10_000 + batch * 256 + index),
                world: uuid::Uuid::from_u128(99),
                dimension: 2,
                feet: [1.5, 64.0, 1.5],
                stack: crate::storage::StoredItemStack::from(&crate::item::test_support::stack(
                    "minecraft:diamond",
                    1,
                )),
            })
            .collect();
        provider
            .inner
            .save_with_items(owner, &data, &ItemMutation::Create(items))
            .await
            .unwrap();
    }
    let mut harness = Harness::new(54, provider.clone(), limits(), true);
    ready(&mut harness).await;
    fill(&mut harness);
    equipped(&mut harness);
    harness.action().await.unwrap();
    provider.started.acquire().await.unwrap().forget();
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    assert_eq!(
        crate::entity::player_life::hurt_player(
            world,
            harness.player,
            &hit(100.0, ActorDamageCause::Override)
        ),
        20.0
    );
    provider.release.add_permits(1);
    harness.acknowledge(1).await;
    await_terminal(&mut harness).await;
    let world = harness.router.world(0).unwrap().ecs.world();
    assert!(
        world
            .get::<crate::entity::player_life::Dead>(harness.player)
            .is_some()
    );
    assert_eq!(world.get::<Health>(harness.player).unwrap().current, 0.0);
    assert!(drops(&mut harness).is_empty());
    assert!(
        provider
            .inner
            .world_items(uuid::Uuid::from_u128(9), 0)
            .await
            .unwrap()
            .is_empty()
    );
    let saved = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert!(saved.native_terminal.is_some());
    assert!(saved.inventory.iter().all(|item| item.count == 0));
    assert_eq!(
        saved.inventory_effects.len(),
        1,
        "only the genuine native terminal receipt exists"
    );
    harness.router.tick().await.unwrap();
    assert!(
        harness
            .logs()
            .iter()
            .any(|line| line == "grant-not-applied")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_spill_after_an_acknowledged_mint_is_uncertain_never_whole_request_not_applied() {
    let directory = tempfile::tempdir().unwrap();
    let provider = gated(directory.path(), false);
    let owner = uuid::Uuid::from_u128(99);
    let data = PlayerData {
        uuid: owner.to_string(),
        ..Default::default()
    };
    for batch in 0..16 {
        let count = if batch == 15 { 255 } else { 256 };
        let items = (0..count)
            .map(|index| PersistentItem {
                id: uuid::Uuid::from_u128(10_000 + batch * 256 + index),
                world: uuid::Uuid::from_u128(99),
                dimension: 2,
                feet: [1.5, 64.0, 1.5],
                stack: crate::storage::StoredItemStack::from(&crate::item::test_support::stack(
                    "minecraft:diamond",
                    1,
                )),
            })
            .collect();
        provider
            .inner
            .save_with_items(owner, &data, &ItemMutation::Create(items))
            .await
            .unwrap();
    }
    let mut harness = Harness::new(55, provider.clone(), limits(), true);
    ready(&mut harness).await;
    equipped(&mut harness);
    harness.action().await.unwrap();
    provider.started.acquire().await.unwrap().forget();
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(harness.player)
        .unwrap()
        .set_slot(
            0,
            crate::item::test_support::stack("minecraft:iron_helmet", 1),
        )
        .unwrap();
    provider.release.add_permits(2);
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    let logs = harness.logs();
    assert!(
        logs.iter().any(|line| line == "grant-uncertain"),
        "{logs:?}"
    );
    assert!(!logs.iter().any(|line| line == "grant-not-applied"));
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    assert!(native::inventory_busy(world, harness.player));
    assert!(!native::is_quarantined(world, harness.player));
    assert_eq!(
        world
            .get::<ArmourInventory>(harness.player)
            .unwrap()
            .slots()[0]
            .item_id(),
        "minecraft:iron_helmet"
    );
    assert!(
        crate::entity::player_life::hurt_player(
            world,
            harness.player,
            &hit(4.0, ActorDamageCause::EntityAttack)
        ) > 0.0
    );
    let saved = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.inventory_effects.len(), 1);
    assert_eq!(saved.armour[0].item_id, "minecraft:diamond_helmet");
    assert_eq!(
        provider
            .inner
            .world_items(uuid::Uuid::from_u128(9), 0)
            .await
            .unwrap()
            .len(),
        1
    );
    harness.move_to(0.5).await.unwrap();
    assert!(
        harness
            .logs()
            .iter()
            .any(|line| line == "uncertain-grant-readback-hidden")
    );
}
