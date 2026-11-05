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
//! most 1) is a public reference recorded in `docs/quirks/bds.md`.
//!
//! Tridents drawn for at least 10 ticks are thrown at their definition's
//! power with `item.trident.throw`; Loyalty brings a thrown trident back to
//! its owner, Riptide launches the player instead of throwing (only in
//! water: this server has no weather).

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
/// Minimum trident draw, ticks.
const TRIDENT_MIN_DRAW: u64 = 10;

/// An item used by drawing and releasing (or, for the crossbow, charging).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    Bow,
    Crossbow,
    Trident,
}

pub fn drawn(identifier: &str) -> Option<Drawn> {
    match identifier {
        "minecraft:bow" => Some(Drawn::Bow),
        "minecraft:crossbow" => Some(Drawn::Crossbow),
        "minecraft:trident" => Some(Drawn::Trident),
        _ => None,
    }
}

/// The projectile a throwable item throws, if it is one.
pub fn thrown(item: &str) -> Option<ActorType> {
    crate::entity::projectile::thrown_by(item).map(|(actor, _)| actor)
}

/// The cooldown a thrown item starts, measured on BDS 1.26.51: ender pearls
/// send `PlayerStartItemCooldown("minecraft:ender_pearl", 20)` before the
/// throw and cannot be thrown again for 20 ticks.
fn throw_cooldown(item: &str) -> Option<(&'static str, u32)> {
    (item == "minecraft:ender_pearl").then_some(("minecraft:ender_pearl", 20))
}

/// Throw the held item (the projectile system takes it from the hand),
/// unless its cooldown is running.
pub fn throw(world: &mut World, player: Entity, item: &str) {
    if let Some((category, ticks)) = throw_cooldown(item) {
        if super::cooldown::cooling(world, player, category) {
            return;
        }
        super::cooldown::start_cooldown(world, player, category, ticks);
    }
    world
        .get_resource_or_init::<crate::entity::projectile::ThrowQueue>()
        .0
        .push(player);
}

fn creative(world: &World, player: Entity) -> bool {
    world.get::<GameMode>(player) == Some(&GameMode::Creative)
}

/// Where an arrow (plain or tipped) is: the offhand first, then the main
/// inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ammo {
    Offhand,
    Main(usize),
}

fn find_arrow(world: &World, player: Entity) -> Option<Ammo> {
    let arrow = |s: &ItemStack| s.item_id() == ARROW && s.count > 0;
    if world
        .get::<OffhandSlot>(player)
        .is_some_and(|offhand| arrow(&offhand.0))
    {
        return Some(Ammo::Offhand);
    }
    world
        .get::<MainInventory>(player)?
        .0
        .slots()
        .iter()
        .position(arrow)
        .map(Ammo::Main)
}

/// The aux of the arrow at `ammo` (0 plain, potion aux + 1 tipped).
fn arrow_aux(world: &World, player: Entity, ammo: Ammo) -> i16 {
    match ammo {
        Ammo::Offhand => world.get::<OffhandSlot>(player).map(|o| o.0.damage),
        Ammo::Main(slot) => world
            .get::<MainInventory>(player)
            .and_then(|inv| inv.0.item(slot))
            .map(|s| s.damage),
    }
    .unwrap_or(0)
}

/// A shot arrow: a tipped arrow (aux > 0) carries potion `aux - 1`.
fn shoot_arrow(
    world: &mut World,
    player: Entity,
    aux: i16,
    spec: LaunchSpec,
) -> Result<(), crate::entity::projectile::LaunchError> {
    let arrow = launch_with(world, player, ActorType::Arrow, None, spec)?;
    if aux > 0 {
        crate::entity::projectile::carry_potion(world, arrow, aux - 1, true);
    }
    Ok(())
}

/// One arrow of `aux` to pick back up.
fn arrow_stack(world: &World, aux: i16) -> Option<ItemStack> {
    crate::server::game::world_stack(world, ARROW, 1).map(|mut s| {
        s.damage = aux;
        s
    })
}

/// The aux of the arrow a loaded crossbow holds.
fn charged_aux(stack: &ItemStack) -> i16 {
    item_tag(stack)
        .and_then(|tag| match tag.0.get("chargedItem") {
            Some(NBTTag::Compound(arrow)) => match arrow.0.get("Damage") {
                Some(NBTTag::Short(damage)) => Some(damage.0),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or(0)
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
pub fn charged(stack: &ItemStack) -> bool {
    item_tag(stack).is_some_and(|tag| tag.0.contains_key("chargedItem"))
}

/// `stack` with an arrow of `aux` stored as its `chargedItem`, or without
/// one.
fn with_charge(stack: &ItemStack, loaded: Option<i16>) -> ItemStack {
    let mut compound = item_tag(stack).unwrap_or_default();
    if let Some(aux) = loaded {
        let mut arrow = std::collections::HashMap::new();
        arrow.insert("Count".to_owned(), NBTTag::Byte(tag::Byte(1)));
        arrow.insert("Damage".to_owned(), NBTTag::Short(tag::Short(aux)));
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
    if drawn == Drawn::Trident {
        // Riptide only works in water.
        if enchantment_level(stack, Enchantment::Riptide) > 0 && !in_water(world, player) {
            return;
        }
        start_using(world, player, slot, entry.id);
        return;
    }
    if !creative(world, player) && find_arrow(world, player).is_none() {
        return;
    }
    start_using(world, player, slot, entry.id);
}

/// Letting go of a drawn bow or trident (a crossbow just stops charging).
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
    match drawn {
        Drawn::Bow => shoot_bow(world, player, slot, stack, ticks),
        Drawn::Trident if ticks >= TRIDENT_MIN_DRAW => {
            if enchantment_level(stack, Enchantment::Riptide) > 0 {
                riptide(world, player, stack);
            } else {
                throw_trident(world, player, slot, stack);
            }
        }
        Drawn::Trident | Drawn::Crossbow => {}
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
    let aux = ammo.map_or(0, |ammo| arrow_aux(world, player, ammo));
    // Infinity keeps plain arrows only; tipped arrows are still used up.
    let infinity = infinity && aux == 0;
    let returned = (!creative && !infinity)
        .then(|| arrow_stack(world, aux))
        .flatten();
    let spec = LaunchSpec {
        speed: Some(power * BOW_SPEED),
        inaccuracy: 1.0,
        critical: power >= 1.0,
        item: returned,
        throw_sound: false,
    };
    if let Err(error) = shoot_arrow(world, player, aux, spec) {
        tracing::warn!(%error, "Shooting an arrow failed");
        return;
    }
    if let (Some(ammo), false, false) = (ammo, creative, infinity) {
        take_arrow(world, player, ammo);
    }
    let _ = slot;
    wear_held(world, player, 1);
    sound(world, player, "bow");
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
    let ammo = find_arrow(world, player);
    let aux = ammo.map_or(0, |ammo| arrow_aux(world, player, ammo));
    match ammo {
        Some(ammo) if !creative => take_arrow(world, player, ammo),
        None if !creative => return,
        _ => {}
    }
    set_slot(world, player, slot, with_charge(stack, Some(aux)));
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
    let aux = charged_aux(stack);
    let returned = (!creative(world, player))
        .then(|| arrow_stack(world, aux))
        .flatten();
    let spec = LaunchSpec {
        speed: Some(CROSSBOW_SPEED),
        inaccuracy: 1.0,
        critical: true,
        item: returned,
        throw_sound: false,
    };
    if let Err(error) = shoot_arrow(world, player, aux, spec) {
        tracing::warn!(%error, "Firing a crossbow failed");
        return;
    }
    set_slot(world, player, slot, with_charge(stack, None));
    wear_held(world, player, 1);
    sound(world, player, "crossbow.shoot");
}

fn in_water(world: &World, player: Entity) -> bool {
    world.get::<Position>(player).is_some_and(|eye| {
        crate::server::game::body_in_water(
            world,
            eye.0 - DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT, 0.0),
        )
    })
}

/// A trident thrown in survival carries its worn item back; Loyalty makes
/// it return to the thrower.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loyalty {
    pub level: u8,
    pub owner: Entity,
}

fn throw_trident(world: &mut World, player: Entity, slot: u8, stack: &ItemStack) {
    let creative = creative(world, player);
    let worn = if creative {
        Some(stack.clone())
    } else {
        crate::server::game::durability::wear(
            stack,
            1,
            crate::server::game::durability::WearKind::Tool,
            &mut world
                .get_resource_or_init::<crate::entity::EntityRandom>()
                .0,
        )
        .or_else(|| Some(stack.clone()))
    }
    .filter(|worn| !worn.is_empty());
    let Some(worn) = worn else {
        set_slot(world, player, slot, ItemStack::empty());
        return;
    };
    let loyalty = enchantment_level(stack, Enchantment::Loyalty);
    let spec = LaunchSpec {
        speed: None,
        inaccuracy: 1.0,
        critical: false,
        // A creative thrower gets nothing back.
        item: (!creative).then(|| worn.clone().with_net_id(0)),
        throw_sound: false,
    };
    let trident = match launch_with(world, player, ActorType::ThrownTrident, None, spec) {
        Ok(trident) => trident,
        Err(error) => {
            tracing::warn!(%error, "Throwing a trident failed");
            return;
        }
    };
    // An enchanted trident glints (BDS: only this flag differs for Loyalty).
    if has_enchantments(stack) {
        set_actor_flag(
            world,
            trident,
            jolyne::valentine::actor_flags::ENCHANTED,
            true,
        );
    }
    let impaling = enchantment_level(stack, Enchantment::Impaling);
    if impaling > 0 {
        world
            .entity_mut(trident)
            .insert(crate::entity::projectile::Impaling(impaling));
    }
    if loyalty > 0 {
        world.entity_mut(trident).insert(Loyalty {
            level: loyalty,
            owner: player,
        });
    }
    if !creative {
        set_slot(world, player, slot, ItemStack::empty());
    }
    sound(world, player, "item.trident.throw");
}

/// Riptide, in water only (this server has no weather): the player is
/// launched along the look direction at `3 × (1 + level) / 4` blocks per
/// tick and `item.trident.riptide_N` plays where they started, as measured
/// on BDS 1.26.51; the trident does not wear. BDS moves the player itself
/// (`CorrectPlayerMovePrediction`); this server pushes the client with
/// `SetActorMotion` and lets it fly the launch.
fn riptide(world: &mut World, player: Entity, stack: &ItemStack) {
    let level = enchantment_level(stack, Enchantment::Riptide);
    let Some(rotation) = world
        .get::<crate::entity::components::Rotation>(player)
        .copied()
    else {
        return;
    };
    let (yaw, pitch) = (
        f64::from(rotation.yaw).to_radians(),
        f64::from(rotation.pitch).to_radians(),
    );
    let look = DVec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    let speed = 3.0 * (1.0 + f64::from(level)) / 4.0;
    let sound_name = match level {
        1 => "item.trident.riptide_1",
        2 => "item.trident.riptide_2",
        _ => "item.trident.riptide_3",
    };
    sound(world, player, sound_name);
    super::push_player(world, player, look * speed);
}

/// Ticks a Loyalty trident lies in the ground before it returns (measured
/// on BDS 1.26.51: about 0.3 s after landing).
const LOYALTY_GROUND_TICKS: u64 = 6;

fn set_actor_flag(world: &mut World, actor: Entity, flag: u32, on: bool) {
    if let Some(mut flags) = world.get_mut::<crate::entity::definition::flags::ActorFlags>(actor) {
        flags.update(flag, on).expect("catalogue flag index");
    }
}

fn has_enchantments(stack: &ItemStack) -> bool {
    item_tag(stack).is_some_and(|tag| tag.0.contains_key("ench"))
}

/// System (exclusive): Loyalty tridents fly back to their owner after
/// lying in the ground for [`LOYALTY_GROUND_TICKS`] or after hurting an
/// actor, as measured on BDS 1.26.51: the return flag is set and
/// `item.trident.return` plays at the owner's eyes, then each tick the
/// velocity becomes `velocity × 0.95 + 0.05 × level` towards the owner's
/// eyes, through blocks. The owner takes it on touch (creative owners get
/// nothing). Without a living owner it stays where it is.
pub fn return_loyal_tridents(
    world: &mut World,
    tridents: &mut QueryState<(Entity, &Loyalty, &Position)>,
    mut due: Local<Vec<(Entity, Loyalty, DVec3)>>,
) {
    use crate::entity::projectile::{DealtDamage, ProjectileItem, Returning, Stuck};
    let now = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |t| t.current);
    due.extend(tridents.iter(world).map(|(e, l, p)| (e, *l, p.0)));
    for (trident, loyalty, at) in due.drain(..) {
        let owner_eye = world
            .get::<Position>(loyalty.owner)
            .map(|p| p.0)
            .filter(|_| {
                world
                    .get::<crate::entity::components::Health>(loyalty.owner)
                    .is_some_and(|h| !h.is_dead())
            });
        let Some(owner_eye) = owner_eye else {
            world.entity_mut(trident).remove::<(Loyalty, Returning)>();
            continue;
        };
        if world.get::<Returning>(trident).is_none() {
            let landed = world
                .get::<Stuck>(trident)
                .is_some_and(|stuck| now.saturating_sub(stuck.since) >= LOYALTY_GROUND_TICKS);
            if !landed && world.get::<DealtDamage>(trident).is_none() {
                continue;
            }
            world
                .entity_mut(trident)
                .remove::<Stuck>()
                .insert(Returning);
            set_actor_flag(
                world,
                trident,
                jolyne::valentine::actor_flags::RETURNTRIDENT,
                true,
            );
            crate::entity::mob::net::sound::play_level_sound(
                world,
                None,
                owner_eye,
                "item.trident.return",
                -1,
            );
        }
        if crate::entity::projectile::pickup::touches(owner_eye, at) {
            match world.get::<ProjectileItem>(trident).map(|i| i.0.clone()) {
                Some(item) if !creative(world, loyalty.owner) => {
                    if !crate::entity::projectile::pickup::collect(
                        world,
                        trident,
                        loyalty.owner,
                        item.clone(),
                    ) {
                        // A full inventory: it drops at the owner's feet.
                        super::give_or_drop(world, loyalty.owner, item);
                        world.despawn(trident);
                    }
                }
                _ => {
                    world.despawn(trident);
                }
            }
            continue;
        }
        let velocity = world
            .get::<crate::entity::components::Velocity>(trident)
            .map_or(DVec3::ZERO, |v| v.0);
        let towards = (owner_eye - at).normalize_or_zero();
        let next = velocity * 0.95 + towards * 0.05 * f64::from(loyalty.level);
        world.entity_mut(trident).insert((
            Position(at + next),
            crate::entity::components::Velocity(next),
        ));
    }
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

    fn effect(r: &mut Range, entity: Entity, id: u8) -> Option<(u8, u32)> {
        r.world()
            .get::<crate::entity::components::Effects>(entity)
            .and_then(|e| e.active.get(&crate::entity::components::EffectType(id)))
            .map(|e| (e.level, e.duration_ticks))
    }

    fn mobs_named(r: &mut Range, name: &str) -> Vec<Entity> {
        r.world()
            .query::<(Entity, &crate::entity::mob::MobKind)>()
            .iter(r.world())
            .filter(|(_, k)| &*k.0.identifier == name)
            .map(|(e, _)| e)
            .collect()
    }

    fn with_aux(name: &str, count: u8, aux: i16) -> ItemStack {
        let mut s = stack(name, count);
        s.damage = aux;
        s
    }

    fn enchanted(name: &str, enchantment: Enchantment, level: u8) -> ItemStack {
        crate::item::enchant::with_enchantments(&stack(name, 1), &[(enchantment, level)])
    }

    #[test]
    fn an_ender_pearl_teleports_its_thrower_with_fall_damage_and_cools_down() {
        let mut r = range(&[("minecraft:ender_pearl", 3)]);
        let player = r.player;
        r.world().get_mut::<Rotation>(player).unwrap().pitch = 30.0;
        r.send(UseRequest::UseAir);
        assert_eq!(r.slot(0).count, 2);
        // A second throw inside the 20-tick cooldown does nothing.
        r.send(UseRequest::UseAir);
        assert_eq!(r.slot(0).count, 2, "cooling down");
        r.ticks(30);
        let eye = r.world().get::<Position>(player).unwrap().0;
        assert!(eye.z > 4.0, "teleported forward to {eye}");
        assert!(
            (eye.y - (64.0 + crate::entity::PLAYER_EYE_HEIGHT)).abs() < 0.01,
            "lands on the floor, eyes at {eye}"
        );
        let health = r.world().get::<Health>(player).unwrap().current;
        assert_eq!(health, 15.0, "5 fall damage");
        assert!(mobs_named(&mut r, "minecraft:ender_pearl").is_empty());
        assert!(mobs_named(&mut r, "minecraft:endermite").is_empty());
        r.send(UseRequest::UseAir);
        assert_eq!(r.slot(0).count, 1, "the cooldown is over");
    }

    #[test]
    fn a_drunk_potion_gives_its_measured_effect_and_leaves_a_bottle() {
        let mut r = range(&[]);
        let player = r.player;
        r.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, with_aux("minecraft:potion", 1, 5))
            .unwrap();
        r.send(UseRequest::UseAir);
        r.ticks(30);
        assert_eq!(r.slot(0).item_id(), "minecraft:potion", "still drinking");
        r.ticks(3);
        assert_eq!(r.slot(0).item_id(), "minecraft:glass_bottle");
        // Night vision (16), the drink duration minus the ticks since.
        let (level, ticks) = effect(&mut r, player, 16).expect("night vision");
        assert_eq!(level, 0);
        assert!((3590..=3600).contains(&ticks), "{ticks}");
    }

    #[test]
    fn instant_harming_potions_hurt_at_once() {
        let mut r = range(&[]);
        let player = r.player;
        r.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, with_aux("minecraft:potion", 1, 24))
            .unwrap();
        r.send(UseRequest::UseAir);
        r.ticks(33);
        assert_eq!(r.world().get::<Health>(player).unwrap().current, 8.0);
        assert!(
            effect(&mut r, player, 7).is_none(),
            "instant effects are not kept"
        );
    }

    #[test]
    fn a_splash_potion_at_the_feet_gives_the_full_duration() {
        let mut r = range(&[]);
        let player = r.player;
        r.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, with_aux("minecraft:splash_potion", 1, 5))
            .unwrap();
        r.world().get_mut::<Rotation>(player).unwrap().pitch = 90.0;
        r.send(UseRequest::UseAir);
        assert!(r.slot(0).is_empty());
        r.ticks(10);
        let (_, ticks) = effect(&mut r, player, 16).expect("night vision");
        assert!(ticks > 3580, "{ticks}");
    }

    #[test]
    fn a_lingering_potion_leaves_a_shrinking_cloud() {
        let mut r = range(&[]);
        let player = r.player;
        r.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, with_aux("minecraft:lingering_potion", 1, 5))
            .unwrap();
        r.world().get_mut::<Rotation>(player).unwrap().pitch = 90.0;
        r.send(UseRequest::UseAir);
        r.ticks(15);
        let clouds = mobs_named(&mut r, "minecraft:area_effect_cloud");
        assert_eq!(clouds.len(), 1);
        let (_, ticks) = effect(&mut r, player, 16).expect("night vision from the cloud");
        assert!((880..=900).contains(&ticks), "a quarter of 3600: {ticks}");
        let radius = |r: &mut Range| {
            r.world()
                .get::<crate::entity::projectile::cloud::AreaEffectCloud>(clouds[0])
                .unwrap()
                .radius
        };
        assert_eq!(radius(&mut r), 2.5);
        // Reapplied every 40 ticks, shrinking by 0.5, until 0.5 is left.
        r.ticks(400);
        assert_eq!(radius(&mut r), 0.5);
        r.ticks(200);
        assert!(
            r.world().get_entity(clouds[0]).is_err(),
            "gone after 600 ticks"
        );
    }

    #[test]
    fn a_tipped_arrow_gives_an_eighth_of_its_potion() {
        let mut r = range(&[("minecraft:bow", 1)]);
        let player = r.player;
        r.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(1, with_aux("minecraft:arrow", 4, 6))
            .unwrap();
        let cow = r.cow(DVec3::new(4.5, 64.0, 9.5));
        r.ticks(1);
        r.send(UseRequest::UseAir);
        r.ticks(24);
        r.send(UseRequest::Release);
        assert_eq!(r.slot(1).count, 3);
        r.ticks(10);
        let (_, ticks) = effect(&mut r, cow, 16).expect("night vision on the cow");
        assert!((440..=450).contains(&ticks), "3600 / 8: {ticks}");
    }

    #[test]
    fn a_thrown_trident_wears_and_is_picked_up() {
        let mut r = range(&[("minecraft:trident", 1)]);
        let player = r.player;
        r.world().get_mut::<Rotation>(player).unwrap().pitch = 40.0;
        r.send(UseRequest::UseAir);
        r.ticks(5);
        r.send(UseRequest::Release);
        assert_eq!(
            r.slot(0).item_id(),
            "minecraft:trident",
            "a short draw keeps it"
        );
        r.send(UseRequest::UseAir);
        r.ticks(12);
        r.send(UseRequest::Release);
        assert!(r.slot(0).is_empty());
        r.ticks(20);
        let stuck: Vec<DVec3> = r
            .world()
            .query_filtered::<&Position, (With<Stuck>, With<ProjectileItem>)>()
            .iter(r.world())
            .map(|p| p.0)
            .collect();
        assert_eq!(stuck.len(), 1);
        r.world().get_mut::<Position>(player).unwrap().0 =
            stuck[0] + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
        r.ticks(1);
        assert_eq!(r.slot(0).item_id(), "minecraft:trident");
        assert_eq!(
            durability_damage(&r.slot(0)),
            1,
            "throwing costs one durability"
        );
    }

    #[test]
    fn a_loyal_trident_comes_back() {
        let mut r = range(&[]);
        let player = r.player;
        let trident = enchanted("minecraft:trident", Enchantment::Loyalty, 3);
        r.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, trident)
            .unwrap();
        r.world().get_mut::<Rotation>(player).unwrap().pitch = 40.0;
        r.send(UseRequest::UseAir);
        r.ticks(12);
        r.send(UseRequest::Release);
        assert!(r.slot(0).is_empty());
        r.ticks(80);
        assert_eq!(
            r.slot(0).item_id(),
            "minecraft:trident",
            "caught by its owner"
        );
        assert!(mobs_named(&mut r, "minecraft:thrown_trident").is_empty());
    }

    #[test]
    fn bow_power_follows_the_draw_curve() {
        assert_eq!(bow_power(1), None);
        assert!((bow_power(10).unwrap() - (0.25 + 1.0) / 3.0).abs() < 1e-9);
        assert_eq!(bow_power(20), Some(1.0));
        assert_eq!(bow_power(24), Some(1.0));
    }

    #[test]
    fn crossbow_charge_keeps_the_tipped_arrow_through_the_item_tag() {
        let crossbow = crate::item::test_support::stack("minecraft:crossbow", 1);
        assert!(!charged(&crossbow));
        let loaded = with_charge(&crossbow, Some(26));
        assert!(charged(&loaded));
        assert_eq!(charged_aux(&loaded), 26);
        assert!(!charged(&with_charge(&loaded, None)));
    }
}
