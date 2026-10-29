use super::*;
use crate::entity::components::{ArmourInventory, Experience, MainInventory, OffhandSlot};
use crate::item::ItemStack;

/// Diamonds in the hotbar, an iron chestplate, a shield in the offhand and
/// level 5 (35 experience to drop).
fn equip(h: &mut Harness) {
    let player = h.player;
    let world = h.world();
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, ItemStack::new("minecraft:diamond", 3))
        .unwrap();
    world
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .set_chestplate(ItemStack::new("minecraft:iron_chestplate", 1))
        .unwrap();
    world.get_mut::<OffhandSlot>(player).unwrap().0 = ItemStack::new("minecraft:shield", 1);
    world.get_mut::<Experience>(player).unwrap().level = 5;
}

fn carried(h: &mut Harness) -> Vec<String> {
    let player = h.player;
    let world = h.world();
    let mut items: Vec<String> = world
        .get::<MainInventory>(player)
        .unwrap()
        .0
        .slots()
        .iter()
        .chain(world.get::<ArmourInventory>(player).unwrap().slots())
        .chain(std::iter::once(
            &world.get::<OffhandSlot>(player).unwrap().0,
        ))
        .filter(|stack| !stack.is_empty())
        .map(|stack| stack.item_id.clone())
        .collect();
    items.sort();
    items
}

#[test]
fn a_player_killed_by_a_mob_dies_drops_everything_and_sees_the_respawn_screen() {
    let mut h = harness(Difficulty::Normal);
    equip(&mut h);
    let mut viewer = h.viewer();
    let cow = h.cow();
    h.hit_from(cow, 100.0);
    assert!(h.dead());
    assert_eq!(h.health(), 0.0);

    let packets = h.drain();
    assert_eq!(
        actor_events(&packets, SESSION),
        [EnumsActorEvent::Hurt, EnumsActorEvent::Death]
    );
    let message = (
        "death.attack.mob".to_owned(),
        vec!["Steve".to_owned(), "%entity.cow.name".to_owned()],
    );
    assert_eq!(death_infos(&packets), std::slice::from_ref(&message));
    assert_eq!(translations(&packets), std::slice::from_ref(&message));
    let spawn = world_spawn(&mut h);
    assert_eq!(
        respawns(&packets),
        [
            (EnumsPlayerRespawnState::Searchingforspawn, spawn),
            (EnumsPlayerRespawnState::Readytospawn, spawn),
        ]
    );
    let seen = drain(&mut viewer);
    assert_eq!(
        actor_events(&seen, SESSION),
        [EnumsActorEvent::Hurt, EnumsActorEvent::Death]
    );
    assert_eq!(translations(&seen), [message], "everyone reads the death");
    assert!(
        death_infos(&seen).is_empty(),
        "the death screen is the victim's"
    );

    assert_eq!(
        h.drops(),
        [
            ("minecraft:diamond".to_owned(), 3),
            ("minecraft:iron_chestplate".to_owned(), 1),
            ("minecraft:shield".to_owned(), 1),
        ]
    );
    assert!(carried(&mut h).is_empty());
    assert_eq!(h.experience_dropped(), 35);
    let player = h.player;
    assert_eq!(h.world().get::<Experience>(player).unwrap().level, 0);

    // The dead take no more damage.
    assert_eq!(h.hit_from(cow, 5.0), 0.0);
}

#[test]
fn keep_inventory_keeps_items_and_experience() {
    let mut h = harness(Difficulty::Normal);
    h.rules().keep_inventory = true;
    equip(&mut h);
    let cow = h.cow();
    h.hit_from(cow, 100.0);
    assert!(h.dead());
    assert!(h.drops().is_empty());
    assert_eq!(h.experience_dropped(), 0);
    assert_eq!(
        carried(&mut h),
        [
            "minecraft:diamond",
            "minecraft:iron_chestplate",
            "minecraft:shield"
        ]
    );
    let player = h.player;
    assert_eq!(h.world().get::<Experience>(player).unwrap().level, 5);
}

#[test]
fn deaths_by_falling_and_lava_have_their_own_messages() {
    let steve = || vec!["Steve".to_owned()];

    // A long fall is an accident.
    let mut h = harness(Difficulty::Normal);
    h.fall(HOME, 30);
    assert!(h.dead());
    assert_eq!(
        death_infos(&h.drain()),
        [("death.fell.accident.generic".to_owned(), steve())]
    );

    // Five blocks or fewer is hitting the ground too hard.
    let mut h = harness(Difficulty::Normal);
    h.set_health(1.0);
    h.fall(HOME, 5);
    assert!(h.dead());
    assert_eq!(
        translations(&h.drain()),
        [("death.attack.fall".to_owned(), steve())]
    );

    let mut h = harness(Difficulty::Normal);
    h.set_health(4.0);
    h.set_block(HOME.floor().as_ivec3(), *blocks::LAVA);
    h.tick();
    assert!(h.dead());
    assert_eq!(
        death_infos(&h.drain()),
        [("death.attack.lava".to_owned(), steve())]
    );
}

#[test]
fn hidden_death_messages_still_show_the_death_screen() {
    let mut h = harness(Difficulty::Normal);
    h.rules().show_death_messages = false;
    let cow = h.cow();
    h.hit_from(cow, 100.0);
    let packets = h.drain();
    assert_eq!(death_infos(&packets).len(), 1);
    assert!(translations(&packets).is_empty());
}
