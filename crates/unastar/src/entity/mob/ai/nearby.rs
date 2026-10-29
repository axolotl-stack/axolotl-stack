//! The players and mobs goals search for, snapshotted once per tick and
//! bucketed by chunk column so a search only visits nearby chunks.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;

use crate::entity::components::{PhysicsBody, Player, Position, Rotation};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::state::{Dead, InLove};
use crate::registry::collision::Aabb;

/// A player as the goals see it this tick.
#[derive(Debug, Clone, Copy)]
pub struct PlayerView {
    pub entity: Entity,
    pub feet: DVec3,
    pub eye: DVec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl PlayerView {
    pub fn new(entity: Entity, eye: DVec3, rotation: &Rotation) -> Self {
        Self {
            entity,
            feet: eye - DVec3::Y * PLAYER_EYE_HEIGHT,
            eye,
            yaw: rotation.yaw,
            pitch: rotation.pitch,
        }
    }

    /// The current view of `entity`, if it is a player.
    pub fn of(world: &World, entity: Entity) -> Option<Self> {
        let player = world.get_entity(entity).ok()?;
        player.get::<Player>()?;
        Some(Self::new(
            entity,
            player.get::<Position>()?.0,
            player.get::<Rotation>()?,
        ))
    }
}

/// A mob as the goals see it this tick.
#[derive(Debug, Clone, Copy)]
pub struct MobView {
    pub entity: Entity,
    pub feet: DVec3,
    pub width: f64,
    pub height: f64,
    pub baby: bool,
    pub in_love: bool,
    pub dead: bool,
}

impl MobView {
    pub fn aabb(&self) -> Aabb {
        Aabb::from_feet(self.feet, self.width, self.height)
    }

    /// The view of `entity` at server tick `tick`, if it is a mob.
    pub fn of(world: &World, entity: Entity, tick: u64) -> Option<Self> {
        let mob = world.get_entity(entity).ok()?;
        let body = mob.get::<PhysicsBody>()?;
        Some(Self {
            entity,
            feet: mob.get::<Position>()?.0,
            width: body.width,
            height: body.height,
            baby: mob
                .get::<ActorFlags>()
                .is_some_and(|f| f.get(actor_flags::BABY).unwrap_or(false)),
            in_love: mob
                .get::<InLove>()
                .is_some_and(|love| tick < love.until_tick),
            dead: mob.contains::<Dead>(),
        })
    }
}

type Cell = (i32, i32);

fn cell(point: DVec3) -> Cell {
    ((point.x.floor() as i32) >> 4, (point.z.floor() as i32) >> 4)
}

/// Indices into a view list, sorted by chunk column.
#[derive(Debug, Default)]
struct Cells(Vec<(Cell, u32)>);

impl Cells {
    fn rebuild(&mut self, points: impl Iterator<Item = DVec3>) {
        self.0.clear();
        self.0
            .extend(points.enumerate().map(|(i, p)| (cell(p), i as u32)));
        self.0.sort_unstable();
    }

    /// Indices whose point lies in a chunk column overlapping `min..max`.
    fn within(&self, min: DVec3, max: DVec3) -> impl Iterator<Item = usize> + '_ {
        let (x0, z0) = cell(min);
        let (x1, z1) = cell(max);
        (x0..=x1).flat_map(move |x| {
            (z0..=z1).flat_map(move |z| {
                let start = self.0.partition_point(|e| e.0 < (x, z));
                let end = self.0.partition_point(|e| e.0 <= (x, z));
                self.0[start..end].iter().map(|e| e.1 as usize)
            })
        })
    }
}

/// This tick's players and mobs with their chunk-column index.
#[derive(Debug, Default)]
pub struct Nearby {
    players: Vec<PlayerView>,
    mobs: Vec<MobView>,
    player_cells: Cells,
    mob_cells: Cells,
}

impl Nearby {
    /// Replace the snapshot.
    pub fn rebuild(
        &mut self,
        players: impl Iterator<Item = PlayerView>,
        mobs: impl Iterator<Item = MobView>,
    ) {
        self.players.clear();
        self.players.extend(players);
        self.mobs.clear();
        self.mobs.extend(mobs);
        self.player_cells
            .rebuild(self.players.iter().map(|p| p.feet));
        self.mob_cells.rebuild(self.mobs.iter().map(|m| m.feet));
    }

    /// Players whose feet are within `radius` of `centre`.
    pub fn players_within(
        &self,
        centre: DVec3,
        radius: f64,
    ) -> impl Iterator<Item = &PlayerView> + '_ {
        let reach = DVec3::splat(radius);
        self.player_cells
            .within(centre - reach, centre + reach)
            .map(|i| &self.players[i])
            .filter(move |p| p.feet.distance_squared(centre) <= radius * radius)
    }

    /// Mobs whose box intersects `area`. The search grows the area by
    /// eight blocks so the feet of any box up to sixteen blocks wide are
    /// inside it.
    pub fn mobs_in(&self, area: Aabb) -> impl Iterator<Item = &MobView> + '_ {
        self.mob_cells
            .within(area.min - DVec3::splat(8.0), area.max + DVec3::splat(8.0))
            .map(|i| &self.mobs[i])
            .filter(move |m| area.intersects(&m.aabb()))
    }
}
