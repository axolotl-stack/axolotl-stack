//! Throwables, bows and crossbows.
//!
//! Shapes and timings measured from BDS 1.26.51 (capture
//! `item-use-20261029`): a bow drawn 24 ticks shoots an arrow at about 3
//! blocks per tick from 0.1 below the eye with the `bow` sound 0.9 above
//! the feet, wears by one and uses up one arrow; a crossbow loads after 25
//! ticks (loading sounds at 5, 15 and 25 ticks, `FinishedChargingItem`,
//! `CompletedUsingItem` -1, the arrow stored as `chargedItem`) and fires
//! the next use at about 3.15 blocks per tick with `crossbow.shoot`. The
//! draw-to-power curve `(f² + 2f) / 3` (f = seconds drawn, at least 0.1, at
//! most 1) is a public reference recorded in `bds-quirks.md`.
//!
//! Tridents are not thrown yet: the server only spawns actors whose
//! `AddActor` shape was measured from BDS, and `minecraft:thrown_trident`
//! is missing from the retained summon capture.

use bevy_ecs::prelude::*;
use glam::DVec3;
use unastar_data::actor_identifiers::ActorType;
use zuri_nbt::{NBTTag, tag};

use super::{ItemUsing, held, set_slot, start_using, stop_using, used_ticks};
use crate::entity::components::{GameMode, MainInventory, OffhandSlot, Position};
use crate::entity::projectile::{LaunchSpec, launch_with};
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level, item_tag, with_item_tag};
use crate::server::game::durability::wear_held;

const ARROW: &str = "minecraft:arrow";
/// Bow speed at full draw, blocks per tick.
const BOW_SPEED: f64 = 3.0;
/// Crossbow arrow speed, blocks per tick.
const CROSSBOW_SPEED: f64 = 3.15;
/// Ticks a crossbow takes to load.
pub const CROSSBOW_LOAD_TICKS: u64 = 25;

/// An item used by drawing and releasing (or, for the crossbow, charging).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    Bow,
    Crossbow,
}

pub fn drawn(identifier: &str) -> Option<Drawn> {
    match identifier {
        "minecraft:bow" => Some(Drawn::Bow),
        "minecraft:crossbow" => Some(Drawn::Crossbow),
        _ => None,
    }
}

/// The projectile a throwable item throws, if it is one.
pub fn thrown(item: &str) -> Option<ActorType> {
    crate::entity::projectile::thrown_by(item).map(|(actor, _)| actor)
}

/// Throw the held item (the projectile system takes it from the hand).
pub fn throw(world: &mut World, player: Entity) {
    world
        .get_resource_or_init::<crate::entity::projectile::ThrowQueue>()
        .0
        .push(player);
}

fn creative(world: &World, player: Entity) -> bool {
    world.get::<GameMode>(player) == Some(&GameMode::Creative)
}

/// Where a plain arrow is: the offhand first, then the main inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ammo {
    Offhand,
    Main(usize),
}

fn find_arrow(world: &World, player: Entity) -> Option<Ammo> {
    let plain = |s: &ItemStack| s.item_id() == ARROW && s.damage == 0 && s.count > 0;
    if world
        .get::<OffhandSlot>(player)
        .is_some_and(|offhand| plain(&offhand.0))
    {
        return Some(Ammo::Offhand);
    }
    world
        .get::<MainInventory>(player)?
        .0
        .slots()
        .iter()
        .position(plain)
        .map(Ammo::Main)
}

/// Take one arrow from `ammo`.
fn take_arrow(world: &mut World, player: Entity, ammo: Ammo) {
    match ammo {
        Ammo::Offhand => {
            if let Some(mut offhand) = world.get_mut::<OffhandSlot>(player) {
                offhand.0 = offhand.0.grow(-1);
            }
            crate::server::game::inventory::resync_inventory(world, player);
        }
        Ammo::Main(slot) => {
            let Some(left) = world
                .get::<MainInventory>(player)
                .and_then(|inv| inv.0.item(slot))
                .map(|s| s.grow(-1))
            else {
                return;
            };
            if let Some(mut inv) = world.get_mut::<MainInventory>(player) {
                let _ = inv.0.set_item(slot, left.clone());
            }
            crate::server::game::send_slot(world, player, slot, &left);
        }
    }
}

/// Feet + 0.9: where BDS plays shooting sounds.
fn shoot_sound_at(world: &World, player: Entity) -> Option<DVec3> {
    world
        .get::<Position>(player)
        .map(|p| p.0 - DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT - 0.9, 0.0))
}

fn sound(world: &World, player: Entity, name: &str) {
    if let Some(at) = shoot_sound_at(world, player) {
        crate::entity::mob::net::sound::play_level_sound(world, Some(player), at, name, -1);
    }
}

/// The arrow a crossbow holds, if loaded.
fn charged(stack: &ItemStack) -> bool {
    item_tag(stack).is_some_and(|tag| tag.0.contains_key("chargedItem"))
}

/// `stack` with a plain arrow stored as its `chargedItem`, or without one.
fn with_charge(stack: &ItemStack, loaded: bool) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    if loaded {
        let mut arrow = std::collections::HashMap::new();
        arrow.insert("Count".to_owned(), NBTTag::Byte(tag::Byte(1)));
        arrow.insert("Damage".to_owned(), NBTTag::Short(tag::Short(0)));
        arrow.insert(
            "Name".to_owned(),
            NBTTag::String(tag::String(ARROW.to_owned())),
        );
        arrow.insert("WasPickedUp".to_owned(), NBTTag::Byte(tag::Byte(0)));
        compound.0.insert(
            "chargedItem".to_owned(),
            NBTTag::Compound(tag::Compound(arrow)),
        );
    } else {
        compound.0.remove("chargedItem");
    }
    with_item_tag(stack, compound)
}

/// An air use of a bow, crossbow or trident.
pub fn use_drawn(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    drawn: Drawn,
    using: Option<ItemUsing>,
) {
    if using.is_some() {
        return;
    }
    let Some(entry) = stack.item() else {
        return;
    };
    if drawn == Drawn::Crossbow && charged(stack) {
        fire_crossbow(world, player, slot, stack);
        return;
    }
    if !creative(world, player) && find_arrow(world, player).is_none() {
        return;
    }
    start_using(world, player, slot, entry.id);
}

/// Letting go of a drawn bow (a crossbow just stops charging).
pub fn release_drawn(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    drawn: Drawn,
    using: Option<ItemUsing>,
) {
    let Some(using) = using else {
        return;
    };
    let ticks = used_ticks(world, &using);
    stop_using(world, player);
    if drawn == Drawn::Bow {
        shoot_bow(world, player, slot, stack, ticks);
    }
}

/// Bow power after `ticks` of drawing: `(f² + 2f) / 3` for `f` seconds,
/// at most 1; `None` below 0.1 (no shot).
pub fn bow_power(ticks: u64) -> Option<f64> {
    let f = ticks as f64 / 20.0;
    let power = ((f * f + 2.0 * f) / 3.0).min(1.0);
    (power >= 0.1).then_some(power)
}

fn shoot_bow(world: &mut World, player: Entity, slot: u8, stack: &ItemStack, ticks: u64) {
    let Some(power) = bow_power(ticks) else {
        return;
    };
    let creative = creative(world, player);
    let infinity = enchantment_level(stack, Enchantment::Infinity) > 0;
    let ammo = find_arrow(world, player);
    if ammo.is_none() && !creative {
        return;
    }
    let returned = (!creative && !infinity)
        .then(|| crate::server::game::world_stack(world, ARROW, 1))
        .flatten();
    let spec = LaunchSpec {
        speed: Some(power * BOW_SPEED),
        inaccuracy: 1.0,
        critical: power >= 1.0,
        item: returned,
        throw_sound: false,
    };
    let Some(permit) = crate::server::game::item_actions::authorize(
        world,
        player,
        crate::server::game::item_actions::ItemActionKind::Launch,
    ) else {
        return;
    };
    let projectile = match launch_with(world, player, ActorType::Arrow, None, spec) {
        Ok(projectile) => projectile,
        Err(error) => {
            tracing::warn!(%error, "Shooting an arrow failed");
            return;
        }
    };
    if !permit.is_current(world) {
        world.despawn(projectile);
        return;
    }
    if let (Some(ammo), false, false) = (ammo, creative, infinity) {
        take_arrow(world, player, ammo);
    }
    let _ = slot;
    wear_held(world, player, 1);
    sound(world, player, "bow");
    permit.commit(
        world,
        crate::server::game::item_actions::ItemActionOutcome::Launched { projectile },
    );
}

/// System step (from `tick_using`): crossbow loading sounds and, at 25
/// ticks, loading the arrow.
pub fn tick_crossbow(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    using: &ItemUsing,
) {
    let ticks = used_ticks(world, using);
    match ticks {
        5 => sound(world, player, "crossbow.loading.start"),
        15 => sound(world, player, "crossbow.loading.middle"),
        CROSSBOW_LOAD_TICKS.. => {
            sound(world, player, "crossbow.loading.end");
            load_crossbow(world, player, slot, stack);
        }
        _ => {}
    }
}

fn load_crossbow(world: &mut World, player: Entity, slot: u8, stack: &ItemStack) {
    stop_using(world, player);
    let creative = creative(world, player);
    match find_arrow(world, player) {
        Some(ammo) if !creative => take_arrow(world, player, ammo),
        None if !creative => return,
        _ => {}
    }
    set_slot(world, player, slot, with_charge(stack, true));
    crate::entity::mob::combat::send_actor_event(
        world,
        player,
        jolyne::valentine::types::EnumsActorEvent::FinishedChargingItem,
        0,
    );
    if let Some(entry) = stack.item() {
        super::completed(world, player, entry.network_id(), super::UseMethod::None);
    }
}

fn fire_crossbow(world: &mut World, player: Entity, slot: u8, stack: &ItemStack) {
    let returned = (!creative(world, player))
        .then(|| crate::server::game::world_stack(world, ARROW, 1))
        .flatten();
    let spec = LaunchSpec {
        speed: Some(CROSSBOW_SPEED),
        inaccuracy: 1.0,
        critical: true,
        item: returned,
        throw_sound: false,
    };
    let Some(permit) = crate::server::game::item_actions::authorize(
        world,
        player,
        crate::server::game::item_actions::ItemActionKind::Launch,
    ) else {
        return;
    };
    let projectile = match launch_with(world, player, ActorType::Arrow, None, spec) {
        Ok(projectile) => projectile,
        Err(error) => {
            tracing::warn!(%error, "Firing a crossbow failed");
            return;
        }
    };
    if !permit.is_current(world) {
        world.despawn(projectile);
        return;
    }
    set_slot(world, player, slot, with_charge(stack, false));
    wear_held(world, player, 1);
    sound(world, player, "crossbow.shoot");
    permit.commit(
        world,
        crate::server::game::item_actions::ItemActionOutcome::Launched { projectile },
    );
}

/// What `tick_using` does for a drawn item each tick.
pub fn tick_drawn(world: &mut World, player: Entity, drawn: Drawn, using: &ItemUsing) {
    if drawn != Drawn::Crossbow {
        return;
    }
    if let Some((slot, stack)) = held(world, player) {
        tick_crossbow(world, player, slot, &stack, using);
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::*;
    use glam::DVec3;

    use super::*;
    use crate::entity::components::{
        Health, HeldSlot, Player, PlayerInput, PlayerSession, PlayerState, Rotation,
        RuntimeEntityId,
    };
    use crate::entity::mob::{SpawnCause, spawn_mob};
    use crate::entity::projectile::{ProjectileItem, Stuck};
    use crate::item::enchant::durability_damage;
    use crate::item::test_support::stack;
    use crate::server::game::item_use::{ItemUseQueue, UseRequest};
    use crate::server::game::{GameServer, ServerConfig};
    use crate::world::ChunkManager;
    use crate::world::chunk::{Chunk, blocks};
    use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};

    struct Range {
        server: GameServer,
        player: Entity,
        _rx: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    }

    /// A stone floor at y = 63 over chunk (0, 0) and a survival player at
    /// (4.5, 64, 2.5) looking south (+z) with `hotbar`.
    fn range(hotbar: &[(&str, u8)]) -> Range {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(crate::entity::EntityRandom::seeded(5));
        let mut column = Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                column.set_block(x, 63, z, *blocks::STONE);
            }
        }
        let chunk = world.spawn(ChunkData::new(column)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let (tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let mut main = MainInventory::default();
        for (slot, (name, count)) in hotbar.iter().enumerate() {
            main.0.set_item(slot, stack(name, *count)).unwrap();
        }
        let player = world
            .spawn((
                Player,
                Position(DVec3::new(
                    4.5,
                    64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                    2.5,
                )),
                Rotation::default(),
                GameMode::Survival,
                main,
                OffhandSlot::default(),
                HeldSlot(0),
                Health::default(),
                PlayerInput::default(),
                PlayerState::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "archer".into(), None, None, tx),
            ))
            .id();
        let mut viewers = ChunkViewers::default();
        viewers.insert(player);
        world.entity_mut(chunk).insert(viewers);
        Range {
            server,
            player,
            _rx: rx,
        }
    }

    impl Range {
        fn world(&mut self) -> &mut World {
            self.server.ecs.world_mut()
        }

        fn send(&mut self, request: UseRequest) {
            let player = self.player;
            self.world()
                .resource_mut::<ItemUseQueue>()
                .0
                .push((player, request));
            self.server.tick();
        }

        fn ticks(&mut self, n: usize) {
            for _ in 0..n {
                self.server.tick();
            }
        }

        fn slot(&mut self, slot: usize) -> ItemStack {
            let player = self.player;
            self.world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(slot)
                .unwrap()
                .clone()
        }

        fn cow(&mut self, feet: DVec3) -> Entity {
            let world = self.world();
            loop {
                let cow =
                    spawn_mob(world, "minecraft:cow", feet, 0.0, SpawnCause::Spawned).unwrap();
                if world
                    .get::<crate::entity::mob::components::LootConfig>(cow)
                    .is_some()
                {
                    break cow;
                }
                world.despawn(cow);
            }
        }
    }

    #[test]
    fn a_drawn_bow_shoots_an_arrow_that_hurts_a_cow() {
        let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 8)]);
        let cow = r.cow(DVec3::new(4.5, 64.0, 9.5));
        r.ticks(1);
        r.send(UseRequest::UseAir);
        r.ticks(24);
        r.send(UseRequest::Release);
        assert_eq!(durability_damage(&r.slot(0)), 1, "the bow wears");
        assert_eq!(r.slot(1).count, 7, "one arrow is used up");
        r.ticks(10);
        let health = r.world().get::<Health>(cow).map_or(0.0, |h| h.current);
        assert!(health < 10.0, "cow health {health}");
    }

    #[test]
    fn a_bow_without_arrows_does_not_draw() {
        let mut r = range(&[("minecraft:bow", 1)]);
        r.send(UseRequest::UseAir);
        let player = r.player;
        assert!(r.world().get::<ItemUsing>(player).is_none());
    }

    #[test]
    fn arrows_stick_in_the_ground_and_come_back() {
        let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 2)]);
        let player = r.player;
        // Look down at the floor in front.
        r.world().get_mut::<Rotation>(player).unwrap().pitch = 60.0;
        r.send(UseRequest::UseAir);
        r.ticks(24);
        r.send(UseRequest::Release);
        r.ticks(20);
        let stuck: Vec<DVec3> = r
            .world()
            .query_filtered::<&Position, (With<Stuck>, With<ProjectileItem>)>()
            .iter(r.world())
            .map(|p| p.0)
            .collect();
        assert_eq!(stuck.len(), 1, "one stuck arrow");
        assert_eq!(r.slot(1).count, 1);
        // Walk onto it.
        let at = stuck[0];
        r.world().get_mut::<Position>(player).unwrap().0 =
            at + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
        r.ticks(1);
        assert_eq!(r.slot(1).count, 2, "the arrow is picked up");
    }

    #[test]
    fn crossbows_load_after_25_ticks_then_fire() {
        let mut r = range(&[("minecraft:crossbow", 1), ("minecraft:arrow", 3)]);
        r.send(UseRequest::UseAir);
        r.ticks(23);
        assert!(!charged(&r.slot(0)));
        r.ticks(2);
        assert!(charged(&r.slot(0)), "loaded at 25 ticks");
        assert_eq!(r.slot(1).count, 2);
        r.send(UseRequest::UseAir);
        assert!(!charged(&r.slot(0)));
        assert_eq!(durability_damage(&r.slot(0)), 1);
        let arrows = r
            .world()
            .query_filtered::<&crate::entity::projectile::ProjectileFlight, ()>()
            .iter(r.world())
            .filter(|f| f.critical)
            .count();
        assert_eq!(arrows, 1);
    }

    #[test]
    fn snowballs_are_thrown() {
        let mut r = range(&[("minecraft:snowball", 4)]);
        r.send(UseRequest::UseAir);
        assert_eq!(r.slot(0).count, 3);
        let kinds: Vec<_> = r
            .world()
            .query::<&crate::entity::mob::MobKind>()
            .iter(r.world())
            .map(|k| k.0.identifier.to_string())
            .collect();
        assert!(
            kinds.contains(&"minecraft:snowball".to_owned()),
            "{kinds:?}"
        );
    }

    #[test]
    fn bow_power_follows_the_draw_curve() {
        assert_eq!(bow_power(1), None);
        assert!((bow_power(10).unwrap() - (0.25 + 1.0) / 3.0).abs() < 1e-9);
        assert_eq!(bow_power(20), Some(1.0));
        assert_eq!(bow_power(24), Some(1.0));
    }

    #[test]
    fn crossbow_charge_round_trips_through_the_item_tag() {
        let crossbow = crate::item::test_support::stack("minecraft:crossbow", 1);
        assert!(!charged(&crossbow));
        let loaded = with_charge(&crossbow, true);
        assert!(charged(&loaded));
        assert!(!charged(&with_charge(&loaded, false)));
    }
}
