//! TNT and underwater TNT: lighting and priming.
//!
//! As BDS 1.26.51 does:
//! - Nothing lights TNT while the `tntExplodes` game rule is off; it is then
//!   an ordinary block (a use with flint and steel is not handled).
//! - Lighting ([`ignite`]) replaces the block with air and spawns the
//!   primed `minecraft:tnt` actor with its feet at the bottom centre of the
//!   cell `(x + 0.5, y, z + 0.5)`; viewers get `LevelEvent` 1005 (the fuse
//!   sound) at the actor with data 0. The actor starts moving 0.02 blocks
//!   per tick horizontally in a random direction and 0.2 upwards. Its own
//!   `minecraft:explode` (80-tick fuse, power 4) counts down and explodes
//!   through the shared explosion 81 ticks after priming.
//! - A player's use holding flint and steel or a fire charge lights it;
//!   flint and steel takes 1 durability, a fire charge is used up (neither
//!   in creative).
//! - Redstone power reaching it lights it, also when it is placed into
//!   power ([`power_changed`] runs right after placement).
//! - An explosion reaching it ([`prime_from_explosion`]): the block becomes
//!   air (the explosion does that) and the actor gets its `from_explosion`
//!   event (a random 0.5–2 s fuse), without the fuse sound.
//! - A player breaking TNT whose `explode_bit` is set primes it.
//! - A dispenser firing TNT ([`dispense`]): the actor stands at
//!   `floor(p) + 0.5` on every axis of the dispense position `p`, with
//!   `LevelSoundEvent` `block.click` (data −1) and `LevelEvent` 1005 there.
//!
//! Fire burning a TNT block lights it through [`ignite`] (the fire rules
//! call it). `allow_underwater_bit` only matters with education features,
//! which are not modelled.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::super::{Place, bit};
use crate::entity::components::{GameMode, Velocity};
use crate::entity::mob::{SpawnCause, spawn_mob, spawn_mob_with_event};
use crate::entity::systems::BlockSource;
use crate::registry::block::BlockEntry;

/// `LevelEvent` of the fuse sound a newly primed TNT plays.
pub const LEVEL_EVENT_FUSE: i32 = 1005;
/// The primed TNT actor.
pub const PRIMED_TNT: &str = "minecraft:tnt";
/// The actor event that gives a chain-reaction TNT its short fuse.
const FROM_EXPLOSION: &str = "from_explosion";
/// Horizontal speed of a newly primed TNT, in blocks per tick.
const PRIME_HORIZONTAL_SPEED: f64 = 0.02;
/// Upward speed of a newly primed TNT.
const PRIME_UPWARD_SPEED: f64 = 0.2;

/// Whether `entry` is TNT (either kind).
pub fn is_tnt(entry: &BlockEntry) -> bool {
    matches!(
        entry.string_id.as_str(),
        "minecraft:tnt" | "minecraft:underwater_tnt"
    )
}

/// The `tntExplodes` game rule (on unless set off).
pub fn tnt_explodes(world: &World) -> bool {
    !matches!(
        world
            .get_resource::<crate::world::rules::GameRules>()
            .and_then(|rules| rules.value("tntExplodes"))
            .map(|(_, value)| value),
        Some(unastar_data::game_rules::GameRuleValue::Bool(false))
    )
}

/// Spawn a primed TNT with its feet at `feet`, moving off in a random
/// direction; `event` replaces the spawn event.
fn spawn_primed(world: &mut World, feet: DVec3, event: Option<&str>) -> Option<Entity> {
    let spawned = match event {
        Some(event) => spawn_mob_with_event(world, PRIMED_TNT, feet, 0.0, event),
        None => spawn_mob(world, PRIMED_TNT, feet, 0.0, SpawnCause::Spawned),
    };
    let tnt = match spawned {
        Ok(tnt) => tnt,
        Err(error) => {
            tracing::warn!(%error, "primed TNT could not spawn");
            return None;
        }
    };
    let angle = super::super::random(world).r#gen::<f64>() * std::f64::consts::TAU;
    let motion = DVec3::new(
        -angle.sin() * PRIME_HORIZONTAL_SPEED,
        PRIME_UPWARD_SPEED,
        -angle.cos() * PRIME_HORIZONTAL_SPEED,
    );
    world.entity_mut(tnt).insert(Velocity(motion));
    Some(tnt)
}

/// The bottom centre of the cell `pos`, where a primed TNT stands.
fn feet(pos: IVec3) -> DVec3 {
    DVec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    )
}

/// Light the TNT block at `pos` (a use, power, fire): it becomes air and a
/// primed TNT with the fuse sound takes its place. Returns whether it was
/// lit.
pub fn ignite(world: &mut World, pos: IVec3) -> bool {
    if !tnt_explodes(world) {
        return false;
    }
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    if !world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(is_tnt)
    {
        return false;
    }
    let air = super::super::air(world.registry());
    crate::server::game::blocks::set_block(world, pos, air, false);
    if spawn_primed(world, feet(pos), None).is_some() {
        crate::entity::mob::net::sound::level_event(world, feet(pos), LEVEL_EVENT_FUSE, 0);
    }
    true
}

/// An explosion removed the TNT block at `pos`: a primed TNT with the
/// chain-reaction fuse takes its place, silently.
pub fn prime_from_explosion(world: &mut World, pos: IVec3) {
    if tnt_explodes(world) {
        spawn_primed(world, feet(pos), Some(FROM_EXPLOSION));
    }
}

/// A dispenser fires TNT from the dispense position `at`. Returns whether
/// a primed TNT was spawned (the dispenser then uses one item up).
pub fn dispense(world: &mut World, at: DVec3) -> bool {
    if !tnt_explodes(world) {
        return false;
    }
    let cell = at.floor() + DVec3::splat(0.5);
    if spawn_primed(world, cell, None).is_none() {
        return false;
    }
    crate::entity::mob::net::sound::play_level_sound(world, None, at, "block.click", -1);
    crate::entity::mob::net::sound::level_event(world, at, LEVEL_EVENT_FUSE, 0);
    true
}

/// TNT is lit through the item path: a use reaches [`use_block`] only with
/// flint and steel or a fire charge in hand, so a click with a block still
/// places it against the TNT.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    false
}

/// A player's use holding flint and steel or a fire charge lights the TNT;
/// anything else is not handled.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, _state: u32) -> bool {
    use crate::server::game::item_use::{held, set_slot};
    let Some((slot, stack)) = held(world, player) else {
        return false;
    };
    let flint = match stack.item_id() {
        "minecraft:flint_and_steel" => true,
        "minecraft:fire_charge" => false,
        _ => return false,
    };
    if !ignite(world, pos) {
        return false;
    }
    if flint {
        crate::server::game::durability::wear_held(world, player, 1);
    } else if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, stack.grow(-1));
    }
    true
}

/// TNT keeps the generic placement state.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// TNT stands anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// Power at placement arrives through [`power_changed`].
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Neighbour changes reach TNT only as power ([`power_changed`]).
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// TNT schedules no ticks.
pub fn scheduled_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// TNT has no random ticks.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Any power lights it.
pub fn power_changed(world: &mut World, pos: IVec3, _state: u32, powered: bool) {
    if powered {
        ignite(world, pos);
    }
}

/// TNT whose `explode_bit` is set is primed by the player breaking it.
pub fn breaking(world: &mut World, pos: IVec3, state: u32) {
    let unstable = world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|entry| bit(entry, state, "explode_bit"));
    if unstable && tnt_explodes(world) && spawn_primed(world, feet(pos), None).is_some() {
        crate::entity::mob::net::sound::level_event(world, feet(pos), LEVEL_EVENT_FUSE, 0);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! TNT through a real `GameServer` (the primed actor's fuse and
    //! explosion run in the entity systems). [`Server`] is shared with the
    //! dispenser and redstone consumer tests.

    use bevy_ecs::prelude::*;
    use glam::{DVec3, IVec3};
    use jolyne::valentine::{McpePacket, McpePacketData};
    use tokio::sync::mpsc;

    use super::super::super::tests::{FLOOR, state};
    use super::{LEVEL_EVENT_FUSE, PRIMED_TNT};
    use crate::entity::EntityRandom;
    use crate::entity::components::{
        ArmourInventory, GameMode, Health, HeldSlot, MainInventory, Player, PlayerSession,
        PlayerState, Position, Rotation, RuntimeEntityId,
    };
    use crate::entity::mob::MobKind;
    use crate::entity::mob::components::explode::Fuse;
    use crate::entity::systems::BlockSource;
    use crate::registry::block::StateValue;
    use crate::server::game::{GameServer, ServerConfig};
    use crate::world::chunk::Chunk;
    use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};

    /// A server with a stone floor at y = [`FLOOR`] over chunks −1..=1 and
    /// one survival player far from the action at (20.5, 64, 20.5).
    pub(crate) struct Server {
        pub server: GameServer,
        pub player: Entity,
        pub rx: mpsc::Receiver<McpePacket>,
    }

    impl Server {
        pub(crate) fn new() -> Self {
            let mut server = GameServer::with_config(ServerConfig {
                world: crate::world::WorldConfig::void_spawn_platform(1),
                ..ServerConfig::default()
            });
            let world = server.ecs.world_mut();
            world.insert_resource(EntityRandom::seeded(5));
            world.insert_resource(super::super::super::BlockRandom::seeded(5));
            let (tx, rx) = mpsc::channel(1 << 16);
            let player = world
                .spawn((
                    Player,
                    Position(DVec3::new(
                        20.5,
                        64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                        20.5,
                    )),
                    Rotation::default(),
                    GameMode::Survival,
                    MainInventory::default(),
                    ArmourInventory::default(),
                    HeldSlot(0),
                    PlayerState::default(),
                    Health::default(),
                    RuntimeEntityId(1),
                    PlayerSession::new(1, "blocks".into(), None, None, tx),
                ))
                .id();
            let stone = *crate::world::chunk::blocks::STONE;
            for cx in -1..=1 {
                for cz in -1..=1 {
                    let mut column = Chunk::new(cx, cz);
                    for x in 0..16 {
                        for z in 0..16 {
                            column.set_block(x, FLOOR as i16, z, stone);
                        }
                    }
                    let mut viewers = ChunkViewers::default();
                    viewers.insert(player);
                    let chunk = world.spawn((ChunkData::new(column), viewers)).id();
                    world
                        .resource_mut::<ChunkManager>()
                        .insert(ChunkPosition::new(cx, cz), chunk);
                }
            }
            Self { server, player, rx }
        }

        pub(crate) fn world(&mut self) -> &mut World {
            self.server.ecs.world_mut()
        }

        /// Set a block through the change path.
        pub(crate) fn set(&mut self, pos: IVec3, state: u32) {
            crate::server::game::blocks::set_block(self.world(), pos, state, false);
        }

        pub(crate) fn name(&mut self, pos: IVec3) -> String {
            super::super::super::name_at(self.world(), pos)
                .expect("loaded")
                .to_owned()
        }

        pub(crate) fn value(&mut self, pos: IVec3, property: &str) -> Option<StateValue> {
            let world = self.world();
            let state = world.block_at(pos)?;
            world
                .registry()
                .get_by_runtime_id(state)?
                .state_value(state, property)
        }

        pub(crate) fn tick(&mut self, n: u64) {
            for _ in 0..n {
                self.server.tick();
            }
        }

        /// Put `count` of `item` in the player's selected slot.
        pub(crate) fn hold(&mut self, item: &str, count: u8) {
            let stack = crate::server::game::world_stack(self.world(), item, count).unwrap();
            let player = self.player;
            self.world()
                .get_mut::<MainInventory>(player)
                .unwrap()
                .0
                .set_item(0, stack)
                .unwrap();
        }

        pub(crate) fn held(&mut self) -> crate::item::ItemStack {
            let player = self.player;
            self.world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .clone()
        }

        /// The positions of the actors of `identifier`.
        pub(crate) fn actors(&mut self, identifier: &str) -> Vec<(Entity, DVec3)> {
            let world = self.world();
            world
                .query::<(Entity, &MobKind, &Position)>()
                .iter(world)
                .filter(|(_, kind, _)| &*kind.0.identifier == identifier)
                .map(|(entity, _, position)| (entity, position.0))
                .collect()
        }

        pub(crate) fn packets(&mut self) -> Vec<McpePacketData> {
            std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect()
        }

        /// A lever standing on the floor at `pos`, off.
        pub(crate) fn lever(&mut self, pos: IVec3) {
            self.set(
                pos,
                state(
                    "minecraft:lever",
                    &[("lever_direction", StateValue::Str("up_north_south"))],
                ),
            );
        }

        /// Flip the lever at `pos`.
        pub(crate) fn flip(&mut self, pos: IVec3) {
            let player = self.player;
            assert!(super::super::super::use_block(self.world(), player, pos));
        }
    }

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);

    fn tnt() -> u32 {
        state("minecraft:tnt", &[])
    }

    /// The primed TNT burns its whole fuse, then explodes and breaks the
    /// floor under it.
    fn burns_and_explodes(s: &mut Server, fuse: u64) {
        s.tick(fuse - 10);
        assert_eq!(s.actors(PRIMED_TNT).len(), 1, "still burning");
        s.tick(15);
        assert!(s.actors(PRIMED_TNT).is_empty(), "exploded");
        assert_eq!(s.name(AT - IVec3::Y), "minecraft:air", "the floor broke");
    }

    #[test]
    fn flint_and_steel_primes_tnt_which_explodes_after_its_fuse() {
        let mut s = Server::new();
        s.set(AT, tnt());
        s.hold("minecraft:flint_and_steel", 1);
        s.packets();
        let player = s.player;
        assert!(super::super::super::use_block(s.world(), player, AT));
        assert_eq!(s.name(AT), "minecraft:air");
        let primed = s.actors(PRIMED_TNT);
        assert_eq!(primed.len(), 1);
        assert_eq!(primed[0].1, DVec3::new(2.5, 64.0, 2.5), "bottom centre");
        assert!(s.packets().iter().any(|p| matches!(p,
            McpePacketData::LevelEventPacket(e) if e.event_id == LEVEL_EVENT_FUSE && e.data == 0)));
        assert_eq!(
            crate::item::enchant::durability_damage(&s.held()),
            1,
            "flint and steel wears"
        );
        burns_and_explodes(&mut s, 80);
    }

    #[test]
    fn anything_else_in_hand_does_not_light_tnt() {
        let mut s = Server::new();
        s.set(AT, tnt());
        s.hold("minecraft:stick", 1);
        let player = s.player;
        assert!(!super::super::super::use_block(s.world(), player, AT));
        assert_eq!(s.name(AT), "minecraft:tnt");
    }

    #[test]
    fn a_fire_charge_is_used_up_lighting_tnt() {
        let mut s = Server::new();
        s.set(AT, tnt());
        s.hold("minecraft:fire_charge", 2);
        let player = s.player;
        assert!(super::super::super::use_block(s.world(), player, AT));
        assert_eq!(s.actors(PRIMED_TNT).len(), 1);
        assert_eq!(s.held().count, 1);
    }

    #[test]
    fn a_lever_primes_tnt_beside_it() {
        let mut s = Server::new();
        s.set(AT, tnt());
        let lever = AT + IVec3::X;
        s.lever(lever);
        s.flip(lever);
        s.tick(2);
        assert_eq!(s.name(AT), "minecraft:air");
        assert_eq!(s.actors(PRIMED_TNT).len(), 1);
        burns_and_explodes(&mut s, 78);
    }

    #[test]
    fn tnt_placed_into_power_is_primed() {
        let mut s = Server::new();
        s.set(AT + IVec3::X, state("minecraft:redstone_block", &[]));
        s.set(AT, tnt());
        s.tick(2);
        assert_eq!(s.actors(PRIMED_TNT).len(), 1);
    }

    #[test]
    fn with_tnt_explodes_off_nothing_lights() {
        let mut s = Server::new();
        s.world()
            .resource_mut::<crate::world::rules::GameRules>()
            .set_value(
                "tntExplodes",
                unastar_data::game_rules::GameRuleValue::Bool(false),
            )
            .unwrap();
        s.set(AT, tnt());
        s.hold("minecraft:flint_and_steel", 1);
        let player = s.player;
        assert!(!super::super::super::use_block(s.world(), player, AT));
        s.set(AT + IVec3::X, state("minecraft:redstone_block", &[]));
        s.tick(4);
        assert_eq!(s.name(AT), "minecraft:tnt");
        assert!(s.actors(PRIMED_TNT).is_empty());
    }

    #[test]
    fn an_explosion_primes_tnt_with_the_short_fuse() {
        let mut s = Server::new();
        let other = AT + IVec3::new(3, 0, 0);
        s.set(other, tnt());
        let outcome = crate::world::explosion::explode(
            s.world(),
            &crate::world::explosion::Explosion {
                centre: DVec3::new(2.5, 64.0, 2.5),
                power: 4.0,
                source: None,
                breaks_blocks: true,
                causes_fire: false,
                allow_underwater: false,
                max_resistance: f32::MAX,
                damage_scaling: 1.0,
                knockback_scaling: 1.0,
                sound: "explode".into(),
            },
        );
        assert!(outcome.destroyed.contains(&other));
        assert_eq!(s.name(other), "minecraft:air");
        let primed = s.actors(PRIMED_TNT);
        assert_eq!(primed.len(), 1, "the TNT was primed, not dropped");
        assert_eq!(primed[0].1, DVec3::new(5.5, 64.0, 2.5));
        s.tick(1);
        let fuse = s.world().get::<Fuse>(primed[0].0).map(|fuse| fuse.ticks);
        assert!(
            fuse.is_some_and(|ticks| (10..=40).contains(&ticks)),
            "chain-reaction fuse: {fuse:?}"
        );
        s.tick(41);
        assert!(s.actors(PRIMED_TNT).is_empty(), "exploded");
    }
}
