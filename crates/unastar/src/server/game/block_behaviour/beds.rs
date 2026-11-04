//! Beds: two blocks, the foot and the head (`head_piece_bit`), lying along
//! `direction` (0 south, 1 west, 2 north, 3 east), as BDS 1.26.51 does.
//!
//! - **Placement**: the foot goes in the target cell and the head one block
//!   further in the direction the player looks, which becomes the bed's
//!   `direction`. Both cells must be free and not lava, and the blocks below
//!   both must hold the bed on their top face (a full top, or the top of a
//!   fence or wall).
//! - **Survival**: each half needs its partner (the same bed block, the
//!   other `head_piece_bit`, the same `direction`). Support below is only
//!   checked when the bed is placed. A half left alone is removed by the
//!   neighbour update at once, without drops (quiet destroy particles).
//! - **Breaking** either half by a player removes the partner without drops
//!   and yields the broken half's own drop, so a bed drops one bed item
//!   whichever half breaks.
//! - **Using** a bed ([`use_bed`]) always counts as an interaction. A use
//!   of the foot acts on the head (nothing happens when the head is
//!   missing). Then, in order:
//!   1. outside the overworld, with `respawnBlocksExplode` on, both halves
//!      become air and the bed explodes at the midpoint of their centres
//!      (the head's centre alone when the foot is missing) with power 5,
//!      setting fire and breaking blocks of any resistance; with the rule
//!      off nothing happens;
//!   2. a bed a player sleeps in gets the user `tile.bed.occupied`; an
//!      `occupied_bit` head nobody sleeps in is cleared;
//!   3. a player whose position is more than 3 blocks from the head's
//!      corner along x or z, or more than 4 along y, gets
//!      `tile.bed.tooFar`;
//!   4. a bed without a spot to get up at ([`super::sleep::wake_spot`])
//!      gets the player `tile.bed.obstructed`;
//!   5. the head becomes the player's respawn block
//!      ([`super::utility::respawn_anchor::set_spawn_block`]:
//!      `SetSpawnPosition` with the player's cell and the head), so the
//!      spawn is set even when sleeping then fails; when it changed, the
//!      player gets `tile.bed.respawnSet`;
//!   6. during the day (sky darkened by less than 4) the player gets
//!      `tile.bed.noSleep`; a thunderstorm counts as night (provisional:
//!      Minecraft Wiki);
//!   7. outside creative and peaceful, a monster within ±8 (x, z) and ±5
//!      (y) of the head's corner gets the player `tile.bed.notSafe`;
//!   8. otherwise the player falls asleep ([`super::sleep`]).
//!
//! Messages are grey rawtext translations (the BDS 1.26.51 capture of a
//! noon use: SetSpawnPosition, `tile.bed.respawnSet`, `tile.bed.noSleep`).
//! Not modelled: the water-breathing check of a waterlogged bed, monsters
//! counted by their box rather than their feet.
//!
//! The bed's colour lives in its block entity, which the server does not
//! keep yet: placed beds show the default colour and drop a plain bed.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};

use super::utility::respawn_anchor::{
    OVERWORLD, dimension, respawn_block_explosion, respawn_blocks_explode, set_spawn_block, tell,
};
use super::{Place, SupportKind, air, bit, centre, deferred, int, name_at, provides_support, with};
use crate::entity::components::{GameMode, Position};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::placement::Horizontal;
use crate::server::game::types::BlockRegistryResource;

/// How far (x and z, then y) from the head's corner a player may use a bed.
const USE_REACH: DVec3 = DVec3::new(3.0, 4.0, 3.0);
/// How far (x and z, then y) from the head's corner monsters keep a player
/// awake.
const MONSTER_RANGE: DVec3 = DVec3::new(8.0, 5.0, 8.0);

/// Bed state properties.
pub const DIRECTION: &str = "direction";
pub const HEAD: &str = "head_piece_bit";
pub const OCCUPIED: &str = "occupied_bit";

/// The direction a bed state lies along (foot to head).
pub fn direction(entry: &BlockEntry, state: u32) -> Option<Horizontal> {
    int(entry, state, DIRECTION).and_then(Horizontal::from_legacy)
}

/// The partner cell of the bed half `state` at `pos`.
fn partner(entry: &BlockEntry, pos: IVec3, state: u32) -> Option<IVec3> {
    let step = direction(entry, state)?.offset();
    Some(if bit(entry, state, HEAD) {
        pos - step
    } else {
        pos + step
    })
}

fn is_lava(blocks: &impl BlockSource, pos: IVec3) -> bool {
    matches!(
        name_at(blocks, pos),
        Some("minecraft:lava" | "minecraft:flowing_lava")
    )
}

pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, state: u32, entry: &BlockEntry) -> bool {
    let Some(other_pos) = partner(entry, pos, state) else {
        return false;
    };
    let head = bit(entry, state, HEAD);
    let facing = direction(entry, state);
    blocks.block_at(other_pos).is_some_and(|other| {
        (entry.min_state_id..=entry.max_state_id).contains(&other)
            && bit(entry, other, HEAD) != head
            && direction(entry, other) == facing
    })
}

pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let blocks = place.blocks;
    let facing = Horizontal::from_yaw(place.input.yaw);
    let foot = place.position;
    let head = foot + facing.offset();
    let supported = [foot, head].into_iter().all(|cell| {
        !is_lava(blocks, cell)
            && provides_support(blocks, cell - IVec3::Y, IVec3::Y, SupportKind::Centre)
    });
    if !supported {
        return None;
    }
    let block = place.block;
    let state = with(
        block,
        place.state,
        DIRECTION,
        StateValue::Int(facing.legacy()),
    );
    let state = with(block, state, HEAD, StateValue::Byte(0));
    Some(with(block, state, OCCUPIED, StateValue::Byte(0)))
}

/// The head placed with the foot `state` at `position`.
pub fn parts(entry: &BlockEntry, position: IVec3, state: u32) -> Vec<(IVec3, u32)> {
    if bit(entry, state, HEAD) {
        return Vec::new();
    }
    partner(entry, position, state)
        .map(|head| (head, with(entry, state, HEAD, StateValue::Byte(1))))
        .into_iter()
        .collect()
}

/// The other half of the bed half `state` at `pos`, and the state whose
/// drops a player's break yields (the broken half itself).
pub fn structure(entry: &BlockEntry, pos: IVec3, state: u32) -> (Vec<IVec3>, u32) {
    (partner(entry, pos, state).into_iter().collect(), state)
}

/// Whether `state` is a half of the bed block `entry`.
fn is_bed(entry: &BlockEntry, state: Option<u32>) -> bool {
    state.is_some_and(|state| (entry.min_state_id..=entry.max_state_id).contains(&state))
}

/// A player uses the bed half `state` at `pos` (see the module docs).
/// Always an interaction.
pub fn use_bed(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return true;
    };
    let head = if bit(entry, state, HEAD) {
        Some(pos)
    } else {
        partner(entry, pos, state)
    };
    let Some(head) = head.filter(|head| is_bed(entry, world.block_at(*head))) else {
        return true;
    };
    let mut head_state = world.block_at(head).unwrap_or(state);
    if dimension(world) != OVERWORLD {
        if respawn_blocks_explode(world) {
            explode_bed(world, entry, head, head_state);
        }
        return true;
    }
    if bit(entry, head_state, OCCUPIED) {
        if super::sleep::sleeper_in(world, head).is_some() {
            tell(world, player, "tile.bed.occupied");
            return true;
        }
        head_state = with(entry, head_state, OCCUPIED, StateValue::Byte(0));
        set_block(world, head, head_state, false);
    }
    let Some(eye) = world.get::<Position>(player).map(|position| position.0) else {
        return true;
    };
    let distance = (eye - head.as_dvec3()).abs();
    if distance.cmpgt(USE_REACH).any() {
        tell(world, player, "tile.bed.tooFar");
        return true;
    }
    let direction = int(entry, head_state, DIRECTION).unwrap_or(0);
    if super::sleep::wake_spot(world, head, direction, None).is_none() {
        tell(world, player, "tile.bed.obstructed");
        return true;
    }
    if set_spawn_block(world, player, head) {
        tell(world, player, "tile.bed.respawnSet");
    }
    let thunder = world
        .get_resource::<crate::world::weather::Weather>()
        .is_some_and(|weather| weather.state == crate::world::weather::WeatherState::Thunder);
    if crate::world::clock::is_daytime(world) && !thunder {
        tell(world, player, "tile.bed.noSleep");
        return true;
    }
    let creative = world.get::<GameMode>(player) == Some(&GameMode::Creative);
    let peaceful = world
        .get_resource::<crate::world::rules::Difficulty>()
        .is_some_and(|difficulty| *difficulty == crate::world::rules::Difficulty::Peaceful);
    if !creative && !peaceful && monsters_near(world, head) {
        tell(world, player, "tile.bed.notSafe");
        return true;
    }
    if world.get::<super::sleep::Sleeping>(player).is_none() {
        super::sleep::start(world, player, head, head_state);
    }
    true
}

/// Remove the bed whose head is at `head` and explode it.
fn explode_bed(world: &mut World, entry: &BlockEntry, head: IVec3, head_state: u32) {
    let foot = partner(entry, head, head_state).filter(|foot| is_bed(entry, world.block_at(*foot)));
    let air = air(&world.resource::<BlockRegistryResource>().0);
    deferred(world, |world| {
        set_block(world, head, air, false);
        if let Some(foot) = foot {
            set_block(world, foot, air, false);
        }
    });
    let at = match foot {
        Some(foot) => (centre(head) + centre(foot)) / 2.0,
        None => centre(head),
    };
    respawn_block_explosion(world, at);
}

/// Whether a living monster stands within [`MONSTER_RANGE`] of the corner
/// of `head`.
fn monsters_near(world: &mut World, head: IVec3) -> bool {
    if !world.contains_resource::<crate::entity::mob::MobRegistry>() {
        return false;
    }
    let corner = head.as_dvec3();
    let (min, max) = (corner - MONSTER_RANGE, corner + MONSTER_RANGE);
    let mut query = world.query_filtered::<(Entity, &Position), (
        With<crate::entity::definition::families::ActorFamilies>,
        Without<crate::entity::mob::state::Dead>,
    )>();
    let near: Vec<Entity> = query
        .iter(world)
        .filter(|(_, position)| position.0.cmpge(min).all() && position.0.cmple(max).all())
        .map(|(entity, _)| entity)
        .collect();
    near.into_iter()
        .any(|entity| crate::entity::mob::events::is_family(world, entity, "monster"))
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::server::game::placement::{Placement, PlacementInput, resolve_placement};

    const FOOT: IVec3 = IVec3::new(0, FLOOR + 1, 0);

    fn bed() -> &'static BlockEntry {
        BlockRegistry::vanilla()
            .get_by_name("minecraft:bed")
            .expect("bed")
    }

    /// Place a bed on the top face of the floor under [`FOOT`], looking
    /// along `yaw`.
    fn place(blocks: &TestBlocks, yaw: f32) -> Option<Placement> {
        let input = PlacementInput {
            clicked: FOOT - IVec3::Y,
            face: 1,
            click: DVec3::new(0.5, 1.0, 0.5),
            yaw,
            pitch: 30.0,
        };
        resolve_placement(blocks, bed(), &input)
    }

    fn floor() -> TestBlocks {
        TestBlocks::floor("minecraft:stone", FLOOR)
    }

    fn bed_state(facing: Horizontal, head: bool) -> u32 {
        state(
            "minecraft:bed",
            &[
                (DIRECTION, StateValue::Int(facing.legacy())),
                (HEAD, StateValue::Byte(u8::from(head))),
            ],
        )
    }

    #[test]
    fn the_head_goes_one_block_further_in_the_look_direction() {
        for (yaw, facing) in [
            (0.0, Horizontal::South),
            (90.0, Horizontal::West),
            (180.0, Horizontal::North),
            (270.0, Horizontal::East),
        ] {
            let placement = place(&floor(), yaw).unwrap();
            assert_eq!(placement.position, FOOT);
            assert_eq!(placement.state, bed_state(facing, false), "yaw {yaw}");
            assert_eq!(
                placement.parts,
                vec![(FOOT + facing.offset(), bed_state(facing, true))],
                "yaw {yaw}"
            );
        }
    }

    #[test]
    fn beds_are_refused_without_room_or_support_for_the_head() {
        let mut blocked = floor();
        blocked.set(FOOT + IVec3::Z, "minecraft:stone");
        assert_eq!(place(&blocked, 0.0), None);
        let mut hole = floor();
        hole.set(FOOT + IVec3::new(0, -1, 1), "minecraft:air");
        assert_eq!(place(&hole, 0.0), None);
        let mut slab = floor();
        slab.set(FOOT + IVec3::new(0, -1, 1), "minecraft:smooth_stone_slab");
        assert_eq!(place(&slab, 0.0), None);
        let mut lava = floor();
        lava.set(FOOT + IVec3::Z, "minecraft:lava");
        assert_eq!(place(&lava, 0.0), None);
    }

    #[test]
    fn a_half_without_its_partner_goes_without_drops() {
        let mut h = Harness::new();
        let head = FOOT + IVec3::NEG_X;
        h.put(FOOT, bed_state(Horizontal::West, false));
        h.put(head, bed_state(Horizontal::West, true));
        h.set(head, state("minecraft:air", &[]));
        assert_eq!(h.name(FOOT), "minecraft:air");
        assert!(h.drops().is_empty());
    }

    #[test]
    fn a_bed_needs_its_partner_in_the_same_direction() {
        let mut blocks = floor();
        let foot = bed_state(Horizontal::North, false);
        blocks.set_state(FOOT, foot);
        blocks.set_state(FOOT + IVec3::NEG_Z, bed_state(Horizontal::North, true));
        assert!(can_survive(&blocks, FOOT, foot, bed()));
        blocks.set_state(FOOT + IVec3::NEG_Z, bed_state(Horizontal::South, true));
        assert!(!can_survive(&blocks, FOOT, foot, bed()));
        blocks.set_state(FOOT + IVec3::NEG_Z, bed_state(Horizontal::North, false));
        assert!(!can_survive(&blocks, FOOT, foot, bed()));
    }

    #[test]
    fn breaking_either_half_yields_one_bed() {
        for broken_head in [false, true] {
            let mut h = Harness::new();
            let head = FOOT + IVec3::Z;
            h.put(FOOT, bed_state(Horizontal::South, false));
            h.put(head, bed_state(Horizontal::South, true));
            let (broken, other) = if broken_head {
                (head, FOOT)
            } else {
                (FOOT, head)
            };
            let broken_state = h.state(broken);
            let drop_state = super::super::break_structure(&mut h.world, broken, broken_state);
            assert_eq!(drop_state, broken_state);
            assert_eq!(h.name(other), "minecraft:air");
            let drops: Vec<_> =
                crate::server::game::block_drops::block_drops(&mut h.world, drop_state, None)
                    .into_iter()
                    .map(|stack| (stack.item_id().to_string(), stack.count))
                    .collect();
            assert_eq!(drops, vec![("minecraft:bed".to_string(), 1)]);
        }
    }

    /// A bed lying south from [`FOOT`]; returns the head.
    fn put_bed(h: &mut Harness) -> IVec3 {
        let head = FOOT + IVec3::Z;
        h.put(FOOT, bed_state(Horizontal::South, false));
        h.put(head, bed_state(Horizontal::South, true));
        head
    }

    /// The translation keys of the chat messages the player got.
    fn messages(h: &mut Harness) -> Vec<String> {
        use jolyne::valentine::McpePacketData;
        use jolyne::valentine::types::TextPacketBody;
        h.packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::TextPacket(text) => match text.body {
                    TextPacketBody::MessageOnly(body) => {
                        let rawtext: serde_json::Value =
                            serde_json::from_str(&body.message).unwrap();
                        Some(rawtext["rawtext"][1]["translate"].as_str()?.to_string())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    fn use_at(h: &mut Harness, pos: IVec3) -> bool {
        let state = h.state(pos);
        use_bed(&mut h.world, h.player, pos, state)
    }

    fn spawn_block(h: &Harness, block: IVec3) -> bool {
        super::super::utility::respawn_anchor::is_spawn_block(&h.world, h.player, block)
    }

    #[test]
    fn using_a_bed_by_day_sets_the_spawn_at_its_head() {
        let mut h = Harness::new();
        let head = put_bed(&mut h);
        h.packets();
        assert!(use_at(&mut h, FOOT));
        assert!(spawn_block(&h, head));
        assert_eq!(
            messages(&mut h),
            ["tile.bed.respawnSet", "tile.bed.noSleep"]
        );
        assert!(use_at(&mut h, head));
        assert_eq!(
            messages(&mut h),
            ["tile.bed.noSleep"],
            "the same spawn again"
        );
    }

    #[test]
    fn using_a_bed_at_night_sets_the_spawn_and_sleeps() {
        let mut h = Harness::new();
        h.world
            .resource_mut::<crate::world::rules::WorldTime>()
            .ticks = 18000;
        let head = put_bed(&mut h);
        // A stale occupied bit nobody sleeps under is cleared first.
        h.put(
            head,
            with(bed(), h.state(head), OCCUPIED, StateValue::Byte(1)),
        );
        h.packets();
        assert!(use_at(&mut h, head));
        assert!(spawn_block(&h, head));
        assert!(
            h.world
                .get::<super::super::sleep::Sleeping>(h.player)
                .is_some()
        );
        let occupied = with(
            bed(),
            bed_state(Horizontal::South, true),
            OCCUPIED,
            StateValue::Byte(1),
        );
        assert_eq!(h.state(head), occupied);
        assert_eq!(messages(&mut h), ["tile.bed.respawnSet"]);
        // Someone sleeps in it now.
        assert!(use_at(&mut h, FOOT));
        assert_eq!(messages(&mut h), ["tile.bed.occupied"]);
    }

    #[test]
    fn a_bed_walled_in_is_obstructed() {
        let mut h = Harness::new();
        h.world
            .resource_mut::<crate::world::rules::WorldTime>()
            .ticks = 18000;
        let head = put_bed(&mut h);
        let stone = state("minecraft:stone", &[]);
        for x in -1..=1 {
            for z in -1..=2 {
                for y in 0..=2 {
                    let cell = FOOT + IVec3::new(x, y, z);
                    if cell != FOOT && cell != head {
                        h.put(cell, stone);
                    }
                }
            }
        }
        h.packets();
        assert!(use_at(&mut h, FOOT));
        assert_eq!(messages(&mut h), ["tile.bed.obstructed"]);
        assert!(!spawn_block(&h, head));
    }

    #[test]
    fn a_bed_too_far_away_sets_nothing() {
        let mut h = Harness::new();
        put_bed(&mut h);
        h.stand(DVec3::new(4.5, 65.62, 1.5), 0.0, 0.0);
        h.packets();
        assert!(use_at(&mut h, FOOT));
        assert!(
            h.world
                .get::<crate::entity::player_life::respawn::PlayerSpawnPoint>(h.player)
                .is_none()
        );
        assert_eq!(messages(&mut h), ["tile.bed.tooFar"]);
    }

    #[test]
    fn a_bed_explodes_outside_the_overworld() {
        let mut h = Harness::new();
        h.world
            .resource_mut::<crate::server::game::types::ServerConfigResource>()
            .0
            .world
            .dimension = super::super::utility::respawn_anchor::NETHER;
        let head = put_bed(&mut h);
        h.stand(DVec3::new(40.5, 80.0, 40.5), 0.0, 0.0);
        assert!(use_at(&mut h, FOOT));
        assert_eq!(h.name(FOOT), "minecraft:air");
        assert_eq!(h.name(head), "minecraft:air");
        assert_eq!(
            h.name(FOOT - IVec3::Y),
            "minecraft:air",
            "the blast breaks the floor"
        );
        assert!(
            h.world
                .get::<crate::entity::player_life::respawn::PlayerSpawnPoint>(h.player)
                .is_none()
        );
    }
}
