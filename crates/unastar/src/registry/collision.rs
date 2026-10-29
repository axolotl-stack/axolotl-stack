//! Block collision shapes and axis-aligned bounding boxes.
//!
//! Collision boxes come per block state from [`unastar_data::shapes`]: BDS
//! 1.26.51 geometry from CloudburstMC/Data (Apache-2.0), projected by
//! protocolgen and checked against the native palette for all 22,091 states.
//! Nothing is modelled by hand: slabs, stairs (including the
//! `minecraft:corner` state), doors, trapdoors, snow layers (`height` h
//! collides at h/8), fence gates, carpets, beds etc. read the table.
//!
//! # Connections
//!
//! Fences, walls, glass panes and (copper/iron) bars carry their connections
//! in their Bedrock states (`minecraft:connection_*` and
//! `wall_connection_type_*`), and the table gives each of those states its
//! own shape. Those properties depend on the neighbours (protocolgen's README:
//! "Neighbor-dependent block shapes describe the extraction context; live
//! worlds may need their own models to resolve connections"), and unastar does
//! not yet rewrite them on neighbour updates, so [`connected_state`] derives
//! them from the neighbours whenever a collision shape is needed. Property
//! names, connection families, face coverage and canonical state transitions
//! are resolved once when the registry loads; queries only read the immutable
//! numeric cache and shared native geometry. The rule follows
//! df-mc/dragonfly's `block/model` (`Fence.Connects`, `Thin.Connects`)
//! and `block/wall.go` (`calculateConnections`, `calculatePost`) (MIT):
//!
//! - A fence connects to fences of the same kind (wooden fences to each other,
//!   the nether brick fence only to itself), to a fence gate whose closed
//!   panel lies along the connection (gate facing perpendicular to it), and
//!   to blocks whose collision shape covers the whole face towards the fence.
//! - A pane or bars connects to panes, bars and walls, and to full faces.
//! - A wall connects to walls, panes and bars, fence gates as fences do, and
//!   full faces. A wall's collision depends on which sides connect and on
//!   `wall_post_bit` (a straight wall without a post is 6/16 thick instead
//!   of 8/16), not on `short` versus `tall`, so a newly connected side becomes
//!   `short` and a stored height is otherwise kept. The post is raised unless
//!   exactly two opposite sides connect, or when the block above forces it:
//!   a standing lantern, a standing sign, a torch on top, a post wall, or an
//!   open wooden trapdoor facing a connected side.
//! - Unloaded neighbours count as full blocks, as they do for physics.
//!
//! "Full face" is computed from the neighbour's collision shape; whether BDS
//! uses that or another solidity notion (glass, leaves, ice, soul sand) is
//! not yet confirmed: see `thoughts/shared/research/bds-quirks.md`.

use glam::{DVec3, IVec3};
use unastar_data::shapes::{self, ShapeBox, ShapeRef};

use super::BlockRegistry;
use super::block::{BlockEntry, StateValue};

/// Axis-aligned bounding box in world (or block-local) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub const fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    /// Box of `width` x `height` x `width` whose bottom face is centred on `feet`
    /// (the Bedrock actor position convention for items, mobs and falling blocks).
    pub fn from_feet(feet: DVec3, width: f64, height: f64) -> Self {
        let half = width / 2.0;
        Self {
            min: DVec3::new(feet.x - half, feet.y, feet.z - half),
            max: DVec3::new(feet.x + half, feet.y + height, feet.z + half),
        }
    }

    pub fn offset(self, by: DVec3) -> Self {
        Self {
            min: self.min + by,
            max: self.max + by,
        }
    }

    /// Grow the box in the direction of `delta` only.
    pub fn expand_towards(self, delta: DVec3) -> Self {
        Self {
            min: self.min + delta.min(DVec3::ZERO),
            max: self.max + delta.max(DVec3::ZERO),
        }
    }

    pub fn inflate(self, by: DVec3) -> Self {
        Self {
            min: self.min - by,
            max: self.max + by,
        }
    }

    /// Strict overlap test (touching faces do not intersect).
    pub fn intersects(&self, other: &Aabb) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
            && self.min.z < other.max.z
            && self.max.z > other.min.z
    }

    /// Bottom-centre point.
    pub fn feet(&self) -> DVec3 {
        DVec3::new(
            (self.min.x + self.max.x) / 2.0,
            self.min.y,
            (self.min.z + self.max.z) / 2.0,
        )
    }

    /// Clamp a movement of `mover` along X so it stops at this box.
    pub fn clip_x(&self, mover: &Aabb, mut dx: f64) -> f64 {
        if mover.max.y <= self.min.y
            || mover.min.y >= self.max.y
            || mover.max.z <= self.min.z
            || mover.min.z >= self.max.z
        {
            return dx;
        }
        if dx > 0.0 && mover.max.x <= self.min.x {
            dx = dx.min(self.min.x - mover.max.x);
        } else if dx < 0.0 && mover.min.x >= self.max.x {
            dx = dx.max(self.max.x - mover.min.x);
        }
        dx
    }

    /// Clamp a movement of `mover` along Y so it stops at this box.
    pub fn clip_y(&self, mover: &Aabb, mut dy: f64) -> f64 {
        if mover.max.x <= self.min.x
            || mover.min.x >= self.max.x
            || mover.max.z <= self.min.z
            || mover.min.z >= self.max.z
        {
            return dy;
        }
        if dy > 0.0 && mover.max.y <= self.min.y {
            dy = dy.min(self.min.y - mover.max.y);
        } else if dy < 0.0 && mover.min.y >= self.max.y {
            dy = dy.max(self.max.y - mover.min.y);
        }
        dy
    }

    /// Clamp a movement of `mover` along Z so it stops at this box.
    pub fn clip_z(&self, mover: &Aabb, mut dz: f64) -> f64 {
        if mover.max.x <= self.min.x
            || mover.min.x >= self.max.x
            || mover.max.y <= self.min.y
            || mover.min.y >= self.max.y
        {
            return dz;
        }
        if dz > 0.0 && mover.max.z <= self.min.z {
            dz = dz.min(self.min.z - mover.max.z);
        } else if dz < 0.0 && mover.min.z >= self.max.z {
            dz = dz.max(self.max.z - mover.min.z);
        }
        dz
    }
}

fn to_aabb(b: &ShapeBox) -> Aabb {
    let [x0, y0, z0, x1, y1, z1] = b.map(f64::from);
    Aabb::new(DVec3::new(x0, y0, z0), DVec3::new(x1, y1, z1))
}

/// Collision boxes of `runtime_id`, straight from the shape table.
///
/// # Panics
/// If `runtime_id` is outside the vanilla palette or its collision shape is
/// unavailable. Neither happens for registry states: every one of the 22,091
/// vanilla states has collision geometry (`unastar-data`'s `tests/shapes.rs`).
fn collision_shape(runtime_id: u32) -> &'static [ShapeBox] {
    match shapes::collision(runtime_id) {
        Some(ShapeRef::Available(boxes)) => boxes,
        Some(ShapeRef::Unavailable) => {
            panic!("block state {runtime_id} has no collision shape in the BDS data")
        }
        None => panic!("block state {runtime_id} is outside the vanilla shape table"),
    }
}

/// Append the block-local collision boxes of state `runtime_id`.
///
/// Fences, walls, panes and bars collide as their stored connection state;
/// pass the result of [`connected_state`] to account for neighbours.
pub fn block_collision_boxes(runtime_id: u32, out: &mut Vec<Aabb>) {
    out.extend(block_collision_aabbs(runtime_id));
}

/// Borrow native per-state geometry and widen each box only as it is consumed.
/// This does not allocate or copy the shape into a temporary collection.
pub fn block_collision_aabbs(runtime_id: u32) -> impl Iterator<Item = Aabb> {
    collision_shape(runtime_id).iter().map(to_aabb)
}

/// Horizontal side of a block, with its connection state properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    North,
    East,
    South,
    West,
}

impl Side {
    const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    fn offset(self) -> IVec3 {
        match self {
            Self::North => IVec3::NEG_Z,
            Self::East => IVec3::X,
            Self::South => IVec3::Z,
            Self::West => IVec3::NEG_X,
        }
    }

    fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::East => Self::West,
            Self::South => Self::North,
            Self::West => Self::East,
        }
    }

    fn along_x(self) -> bool {
        matches!(self, Self::East | Self::West)
    }

    fn fence_property(self) -> &'static str {
        match self {
            Self::North => "minecraft:connection_north",
            Self::East => "minecraft:connection_east",
            Self::South => "minecraft:connection_south",
            Self::West => "minecraft:connection_west",
        }
    }

    fn wall_property(self) -> &'static str {
        match self {
            Self::North => "wall_connection_type_north",
            Self::East => "wall_connection_type_east",
            Self::South => "wall_connection_type_south",
            Self::West => "wall_connection_type_west",
        }
    }
}

/// Blocks whose connection state properties follow their neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Connectable {
    /// `*_fence`; wooden fences connect to each other, the nether brick fence
    /// only to itself.
    Fence { nether_brick: bool },
    /// Glass panes and iron/copper bars.
    Thin,
    /// `*_wall` (and the education `border_block`, which shares the wall
    /// state properties).
    Wall,
}

fn has_property(entry: &BlockEntry, name: &str) -> bool {
    entry.data.properties.iter().any(|p| p.name == name)
}

/// The connection family of `entry`, if it has one. `minecraft:trip_wire`
/// also has `minecraft:connection_*` properties, but they link tripwire, not
/// collision arms, so it is not a family here.
fn connectable(entry: &BlockEntry) -> Option<Connectable> {
    if has_property(entry, Side::North.wall_property()) {
        return Some(Connectable::Wall);
    }
    if !has_property(entry, Side::North.fence_property()) {
        return None;
    }
    let id = entry.string_id.as_str();
    if id.ends_with("_fence") {
        Some(Connectable::Fence {
            nether_brick: id == "minecraft:nether_brick_fence",
        })
    } else if id.ends_with("glass_pane") || id.ends_with("_bars") {
        Some(Connectable::Thin)
    } else {
        None
    }
}

/// Whether the collision shape covers the whole `face` of the unit block
/// (boxes on that face plane, clipped to the face, leave no gap).
fn covers_face(boxes: &[ShapeBox], face: Side) -> bool {
    // Face rectangles as [u0, u1, v0, v1] with v the height.
    let rects: Vec<[f32; 4]> = boxes
        .iter()
        .filter_map(|&[x0, y0, z0, x1, y1, z1]| {
            let on_face = match face {
                Side::North => z0 <= 0.0,
                Side::South => z1 >= 1.0,
                Side::West => x0 <= 0.0,
                Side::East => x1 >= 1.0,
            };
            let [u0, u1] = if face.along_x() { [z0, z1] } else { [x0, x1] };
            on_face.then_some([u0, u1, y0, y1].map(|v| v.clamp(0.0, 1.0)))
        })
        .collect();
    let cuts = |a: usize, b: usize| {
        let mut cuts: Vec<f32> = rects.iter().flat_map(|r| [r[a], r[b]]).collect();
        cuts.extend([0.0, 1.0]);
        cuts.sort_by(f32::total_cmp);
        cuts.dedup();
        cuts
    };
    let (us, vs) = (cuts(0, 1), cuts(2, 3));
    us.windows(2).all(|u| {
        vs.windows(2).all(|v| {
            rects
                .iter()
                .any(|r| r[0] <= u[0] && u[1] <= r[1] && r[2] <= v[0] && v[1] <= r[3])
        })
    })
}

/// Numeric facts for one canonical state. Unknown states have no cache row.
#[derive(Debug, Clone, Copy)]
struct ConnectionState {
    kind: Option<Connectable>,
    full_faces: u8,
    gate_spans_x: Option<bool>,
    wall_post: WallPost,
    transition: usize,
}

#[derive(Debug, Clone, Copy)]
enum WallPost {
    Never,
    Always,
    Facing(Side),
}

impl WallPost {
    fn raises(self, connected: [bool; 4]) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Facing(side) => connected[side as usize],
        }
    }
}

/// Each transition changes exactly one property, retaining every other native
/// property, including stored wall arm heights and unrelated base-state values.
#[derive(Debug, Clone)]
struct ConnectionTransitions {
    sides: [[u32; 2]; 4],
    post: [u32; 2],
}

/// Immutable registry-owned lookup data. String/property queries and face
/// coverage allocations are confined to construction.
#[derive(Debug, Default)]
pub(super) struct ConnectionCache {
    states: Vec<Option<ConnectionState>>,
    transitions: Vec<ConnectionTransitions>,
}

impl ConnectionCache {
    pub(super) fn new(registry: &BlockRegistry) -> Self {
        let len = registry
            .iter()
            .map(|entry| entry.max_state_id)
            .max()
            .map_or(0, |last| last as usize + 1);
        let mut cache = Self {
            states: vec![None; len],
            transitions: Vec::new(),
        };
        for entry in registry.iter() {
            let kind = connectable(entry);
            let gate = has_property(entry, "in_wall_bit");
            for state in entry.min_state_id..=entry.max_state_id {
                let transition = if let Some(kind) = kind {
                    let set = |property, value| {
                        entry
                            .with_state_value(state, property, value)
                            .unwrap_or_else(|| {
                                panic!("{} has no state {property}={value:?}", entry.string_id)
                            })
                    };
                    let sides = Side::ALL.map(|side| {
                        if kind != Connectable::Wall {
                            return [0, 1]
                                .map(|bit| set(side.fence_property(), StateValue::Byte(bit)));
                        }
                        let property = side.wall_property();
                        let connected = match entry.state_value(state, property) {
                            Some(StateValue::Str("none")) | None => StateValue::Str("short"),
                            Some(stored) => stored,
                        };
                        [
                            set(property, StateValue::Str("none")),
                            set(property, connected),
                        ]
                    });
                    let post = if kind == Connectable::Wall {
                        [0, 1].map(|bit| set("wall_post_bit", StateValue::Byte(bit)))
                    } else {
                        [state; 2]
                    };
                    let index = cache.transitions.len();
                    cache
                        .transitions
                        .push(ConnectionTransitions { sides, post });
                    index
                } else {
                    usize::MAX
                };
                cache.states[state as usize] = Some(ConnectionState {
                    kind,
                    full_faces: Side::ALL.into_iter().fold(0, |mask, side| {
                        mask | (u8::from(covers_face(collision_shape(state), side)) << side as u8)
                    }),
                    gate_spans_x: gate.then(|| {
                        matches!(
                            entry.state_value(state, "minecraft:cardinal_direction"),
                            Some(StateValue::Str("north" | "south"))
                        )
                    }),
                    wall_post: wall_post_rule(entry, state, kind),
                    transition,
                });
            }
        }
        cache
    }

    fn state(&self, state: u32) -> Option<&ConnectionState> {
        self.states.get(state as usize)?.as_ref()
    }

    fn transitions(&self, state: u32) -> &ConnectionTransitions {
        &self.transitions[self.states[state as usize].as_ref().unwrap().transition]
    }

    fn connects(&self, kind: Connectable, side: Side, neighbour: Option<u32>) -> bool {
        let Some(state) = neighbour else {
            return true;
        };
        let Some(neighbour) = self.state(state) else {
            return false;
        };
        match (kind, neighbour.kind) {
            (
                Connectable::Fence { nether_brick },
                Some(Connectable::Fence {
                    nether_brick: other,
                }),
            ) => return nether_brick == other,
            (
                Connectable::Thin | Connectable::Wall,
                Some(Connectable::Thin | Connectable::Wall),
            ) => {
                return true;
            }
            _ => {}
        }
        // Preserve the existing gate-axis rule, including open gates.
        if kind != Connectable::Thin
            && let Some(spans_x) = neighbour.gate_spans_x
        {
            return spans_x == side.along_x();
        }
        neighbour.full_faces & (1 << side.opposite() as u8) != 0
    }
}

/// The state of `state` at `pos`, with neighbour-derived fence/wall/pane/bar
/// connections. All query-time work uses numeric immutable registry data.
pub fn connected_state(
    registry: &BlockRegistry,
    pos: IVec3,
    state: u32,
    block_at: impl Fn(IVec3) -> Option<u32>,
) -> u32 {
    let cache = &registry.connections;
    let Some(kind) = cache.state(state).and_then(|state| state.kind) else {
        return state;
    };
    let connected = Side::ALL.map(|side| cache.connects(kind, side, block_at(pos + side.offset())));
    let mut resolved = state;
    for side in Side::ALL {
        resolved =
            cache.transitions(resolved).sides[side as usize][usize::from(connected[side as usize])];
    }
    if kind == Connectable::Wall {
        let straight = matches!(
            connected,
            [true, false, true, false] | [false, true, false, true]
        );
        let above = block_at(pos + IVec3::Y);
        let post = !straight
            || above
                .and_then(|state| cache.state(state))
                .is_some_and(|state| state.wall_post.raises(connected));
        resolved = cache.transitions(resolved).post[usize::from(post)];
    }
    resolved
}

/// Resolve post-forcing categories at registry load, preserving the original
/// lantern/sign/torch/wall/wooden-trapdoor precedence and native state values.
fn wall_post_rule(entry: &BlockEntry, state: u32, kind: Option<Connectable>) -> WallPost {
    let id = entry.string_id.as_str();
    let value = |property| entry.state_value(state, property);
    let raises = if id.ends_with("lantern") {
        value("hanging") == Some(StateValue::Byte(0))
    } else if id.ends_with("standing_sign") {
        true
    } else if has_property(entry, "torch_facing_direction") {
        value("torch_facing_direction") == Some(StateValue::Str("top"))
    } else if kind == Some(Connectable::Wall) {
        value("wall_post_bit") == Some(StateValue::Byte(1))
    } else {
        let wooden_trapdoor =
            id.ends_with("trapdoor") && id != "minecraft:iron_trapdoor" && !id.contains("copper");
        if wooden_trapdoor && value("open_bit") == Some(StateValue::Byte(1)) {
            return WallPost::Facing(match value("direction") {
                Some(StateValue::Int(0)) => Side::East,
                Some(StateValue::Int(1)) => Side::West,
                Some(StateValue::Int(2)) => Side::South,
                Some(StateValue::Int(3)) => Side::North,
                other => panic!("{id} direction {other:?}"),
            });
        }
        false
    };
    if raises {
        WallPost::Always
    } else {
        WallPost::Never
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const fn local(min: [f64; 3], max: [f64; 3]) -> Aabb {
        Aabb::new(
            DVec3::new(min[0], min[1], min[2]),
            DVec3::new(max[0], max[1], max[2]),
        )
    }

    const FULL: Aabb = local([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);

    fn registry() -> &'static BlockRegistry {
        BlockRegistry::vanilla()
    }

    /// Pre-cache property-based resolver, kept only as a behavior oracle.
    fn reference_connected(pos: IVec3, state: u32, block_at: impl Fn(IVec3) -> Option<u32>) -> u32 {
        let Some(entry) = registry().get_by_runtime_id(state) else {
            return state;
        };
        let Some(kind) = connectable(entry) else {
            return state;
        };
        let connected =
            Side::ALL.map(|side| reference_connects(kind, side, block_at(pos + side.offset())));
        let mut resolved = state;
        for (side, connected) in Side::ALL.into_iter().zip(connected) {
            let (property, value) = if kind != Connectable::Wall {
                (side.fence_property(), StateValue::Byte(u8::from(connected)))
            } else {
                let property = side.wall_property();
                let value = match entry.state_value(state, property) {
                    _ if !connected => StateValue::Str("none"),
                    Some(StateValue::Str("none")) | None => StateValue::Str("short"),
                    Some(stored) => stored,
                };
                (property, value)
            };
            resolved = entry.with_state_value(resolved, property, value).unwrap();
        }
        if kind == Connectable::Wall {
            let straight = matches!(
                connected,
                [true, false, true, false] | [false, true, false, true]
            );
            let post = !straight
                || block_at(pos + IVec3::Y)
                    .is_some_and(|above| reference_raises_post(connected, above));
            resolved = entry
                .with_state_value(resolved, "wall_post_bit", StateValue::Byte(u8::from(post)))
                .unwrap();
        }
        resolved
    }

    fn reference_connects(kind: Connectable, side: Side, neighbour: Option<u32>) -> bool {
        let Some(state) = neighbour else {
            return true;
        };
        let Some(entry) = registry().get_by_runtime_id(state) else {
            return false;
        };
        match (kind, connectable(entry)) {
            (
                Connectable::Fence { nether_brick },
                Some(Connectable::Fence {
                    nether_brick: other,
                }),
            ) => return nether_brick == other,
            (
                Connectable::Thin | Connectable::Wall,
                Some(Connectable::Thin | Connectable::Wall),
            ) => return true,
            _ => {}
        }
        if kind != Connectable::Thin && has_property(entry, "in_wall_bit") {
            let spans_x = matches!(
                entry.state_value(state, "minecraft:cardinal_direction"),
                Some(StateValue::Str("north" | "south"))
            );
            return spans_x == side.along_x();
        }
        covers_face(collision_shape(state), side.opposite())
    }

    fn reference_raises_post(connected: [bool; 4], above: u32) -> bool {
        let Some(entry) = registry().get_by_runtime_id(above) else {
            return false;
        };
        let id = entry.string_id.as_str();
        let value = |property| entry.state_value(above, property);
        if id.ends_with("lantern") {
            return value("hanging") == Some(StateValue::Byte(0));
        }
        if id.ends_with("standing_sign") {
            return true;
        }
        if has_property(entry, "torch_facing_direction") {
            return value("torch_facing_direction") == Some(StateValue::Str("top"));
        }
        if connectable(entry) == Some(Connectable::Wall) {
            return value("wall_post_bit") == Some(StateValue::Byte(1));
        }
        let wooden_trapdoor =
            id.ends_with("trapdoor") && id != "minecraft:iron_trapdoor" && !id.contains("copper");
        if wooden_trapdoor && value("open_bit") == Some(StateValue::Byte(1)) {
            let facing = match value("direction") {
                Some(StateValue::Int(0)) => Side::East,
                Some(StateValue::Int(1)) => Side::West,
                Some(StateValue::Int(2)) => Side::South,
                Some(StateValue::Int(3)) => Side::North,
                other => panic!("{id} direction {other:?}"),
            };
            return connected[facing as usize];
        }
        false
    }

    #[test]
    fn all_native_connection_states_match_property_resolver() {
        let air = plain("minecraft:air");
        // A standing sign raises a straight wall post; air does not.
        let sign = registry()
            .iter()
            .find(|entry| entry.string_id.ends_with("standing_sign"))
            .unwrap()
            .default_state_id;
        for entry in registry()
            .iter()
            .filter(|entry| connectable(entry).is_some())
        {
            for state in entry.min_state_id..=entry.max_state_id {
                for mask in 0u8..16 {
                    for above in [air, sign] {
                        let at = |pos| {
                            if pos == IVec3::Y {
                                return Some(above);
                            }
                            let side = Side::ALL
                                .into_iter()
                                .find(|side| side.offset() == pos)
                                .unwrap();
                            // Unloaded neighbours connect for every family.
                            (mask & (1 << side as u8) == 0).then_some(air)
                        };
                        assert_eq!(
                            connected_state(registry(), IVec3::ZERO, state, at),
                            reference_connected(IVec3::ZERO, state, at),
                            "{} state={state} mask={mask} above={above}",
                            entry.string_id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn all_native_neighbours_match_property_predicates() {
        let kinds = [
            Connectable::Fence {
                nether_brick: false,
            },
            Connectable::Fence { nether_brick: true },
            Connectable::Thin,
            Connectable::Wall,
        ];
        let cache = &registry().connections;
        for neighbour in registry()
            .iter()
            .flat_map(|entry| entry.min_state_id..=entry.max_state_id)
            .map(Some)
            .chain([None, Some(u32::MAX)])
        {
            for kind in kinds {
                for side in Side::ALL {
                    assert_eq!(
                        cache.connects(kind, side, neighbour),
                        reference_connects(kind, side, neighbour),
                        "{kind:?} side={side:?} neighbour={neighbour:?}"
                    );
                }
            }
            for mask in 0u8..16 {
                let connected = Side::ALL.map(|side| mask & (1 << side as u8) != 0);
                let actual = neighbour
                    .and_then(|state| cache.state(state))
                    .is_some_and(|state| state.wall_post.raises(connected));
                let expected =
                    neighbour.is_some_and(|state| reference_raises_post(connected, state));
                assert_eq!(actual, expected, "above={neighbour:?} mask={mask}");
            }
        }
    }

    #[test]
    fn straight_wall_retains_tall_arm_and_uses_trapdoor_facing_for_post() {
        let wall = state(
            "minecraft:cobblestone_wall",
            &[
                ("wall_connection_type_north", StateValue::Str("tall")),
                ("wall_connection_type_east", StateValue::Str("tall")),
                ("wall_connection_type_south", StateValue::Str("none")),
                ("wall_post_bit", StateValue::Byte(1)),
            ],
        );
        let stone = plain("minecraft:stone");
        for direction in 0..4 {
            let trapdoor = state(
                "minecraft:trapdoor",
                &[
                    ("open_bit", StateValue::Byte(1)),
                    ("direction", StateValue::Int(direction)),
                ],
            );
            let world = World::new(&[
                (IVec3::ZERO, wall),
                (N, stone),
                (S, stone),
                (IVec3::Y, trapdoor),
            ]);
            let resolved = world.resolve(IVec3::ZERO);
            let entry = registry().get_by_runtime_id(resolved).unwrap();
            let types = Side::ALL.map(|side| entry.state_value(resolved, side.wall_property()));
            assert_eq!(
                types,
                [
                    Some(StateValue::Str("tall")),
                    Some(StateValue::Str("none")),
                    Some(StateValue::Str("short")),
                    Some(StateValue::Str("none")),
                ]
            );
            assert_eq!(
                entry.state_value(resolved, "wall_post_bit"),
                Some(StateValue::Byte(u8::from(direction >= 2))),
                "trapdoor direction={direction}"
            );
        }
    }

    #[test]
    fn open_gate_connections_keep_the_existing_axis_rule() {
        let fence = plain("minecraft:oak_fence");
        for open in 0..=1 {
            let gate = state(
                "minecraft:fence_gate",
                &[
                    ("minecraft:cardinal_direction", StateValue::Str("north")),
                    ("open_bit", StateValue::Byte(open)),
                ],
            );
            let resolved =
                World::new(&[(IVec3::ZERO, fence), (E, gate), (N, gate)]).resolve(IVec3::ZERO);
            assert_eq!(arms(resolved), [0, 1, 0, 0]);
        }
    }

    #[test]
    fn unknown_states_stay_unchanged_and_do_not_connect() {
        assert_eq!(
            connected_state(registry(), IVec3::ZERO, u32::MAX, |_| {
                panic!("an unknown base state must not query neighbours")
            }),
            u32::MAX
        );
        let fence = plain("minecraft:oak_fence");
        let resolved = connected_state(registry(), IVec3::ZERO, fence, |_| Some(u32::MAX));
        assert_eq!(arms(resolved), [0, 0, 0, 0]);
    }

    fn state(name: &str, props: &[(&str, StateValue)]) -> u32 {
        let entry = registry().get_by_name(name).expect(name);
        props
            .iter()
            .fold(entry.default_state_id, |state, (property, value)| {
                entry
                    .with_state_value(state, property, *value)
                    .unwrap_or_else(|| panic!("{name} {property}"))
            })
    }

    fn boxes_of(state: u32) -> Vec<Aabb> {
        let mut out = Vec::new();
        block_collision_boxes(state, &mut out);
        out
    }

    fn boxes(name: &str, props: &[(&str, StateValue)]) -> Vec<Aabb> {
        boxes_of(state(name, props))
    }

    /// A table value: `f32` data widened to `f64`.
    fn f(v: f32) -> f64 {
        f64::from(v)
    }

    #[test]
    fn full_and_empty_blocks() {
        assert_eq!(boxes("minecraft:stone", &[]), vec![FULL]);
        assert!(boxes("minecraft:air", &[]).is_empty());
        assert!(boxes("minecraft:short_grass", &[]).is_empty());
        assert!(boxes("minecraft:water", &[]).is_empty());
        assert_eq!(boxes("minecraft:farmland", &[])[0].max.y, 0.9375);
        assert_eq!(boxes("minecraft:bed", &[])[0].max.y, 0.5625);
    }

    #[test]
    fn slabs_follow_vertical_half() {
        let half = |v| [("minecraft:vertical_half", StateValue::Str(v))];
        assert_eq!(
            boxes("minecraft:oak_slab", &half("bottom")),
            vec![local([0.0, 0.0, 0.0], [1.0, 0.5, 1.0])]
        );
        assert_eq!(
            boxes("minecraft:oak_slab", &half("top")),
            vec![local([0.0, 0.5, 0.0], [1.0, 1.0, 1.0])]
        );
    }

    #[test]
    fn stairs_have_a_half_slab_and_a_raised_half() {
        let stairs = |direction, upside_down, corner| {
            boxes(
                "minecraft:oak_stairs",
                &[
                    ("weirdo_direction", StateValue::Int(direction)),
                    ("upside_down_bit", StateValue::Byte(upside_down)),
                    ("minecraft:corner", StateValue::Str(corner)),
                ],
            )
        };
        // weirdo_direction 3 ascends towards the north.
        assert_eq!(
            stairs(3, 0, "none"),
            vec![
                local([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]),
                local([0.0, 0.5, 0.0], [1.0, 1.0, 0.5]),
            ]
        );
        assert_eq!(
            stairs(0, 1, "none"),
            vec![
                local([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]),
                local([0.5, 0.0, 0.0], [1.0, 0.5, 1.0]),
            ]
        );
        // Corners are part of the Bedrock state, with a third box.
        assert_eq!(
            stairs(3, 0, "inner_left"),
            vec![
                local([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]),
                local([0.0, 0.5, 0.0], [1.0, 1.0, 0.5]),
                local([0.0, 0.5, 0.5], [0.5, 1.0, 1.0]),
            ]
        );
    }

    #[test]
    fn snow_layers_collide_one_layer_below_their_height() {
        let snow = |height| {
            boxes(
                "minecraft:snow_layer",
                &[("height", StateValue::Int(height))],
            )
        };
        assert!(snow(0).is_empty());
        assert_eq!(snow(2), vec![local([0.0, 0.0, 0.0], [1.0, 0.25, 1.0])]);
        assert_eq!(snow(3), vec![local([0.0, 0.0, 0.0], [1.0, 0.375, 1.0])]);
        assert_eq!(snow(7), vec![local([0.0, 0.0, 0.0], [1.0, 0.875, 1.0])]);
    }

    #[test]
    fn trapdoors_are_0_1825_thick_panels() {
        let trapdoor = |direction, open, upside_down| {
            boxes(
                "minecraft:trapdoor",
                &[
                    ("direction", StateValue::Int(direction)),
                    ("open_bit", StateValue::Byte(open)),
                    ("upside_down_bit", StateValue::Byte(upside_down)),
                ],
            )
        };
        assert_eq!(
            trapdoor(0, 0, 0),
            vec![local([0.0, 0.0, 0.0], [1.0, f(0.1825), 1.0])]
        );
        assert_eq!(
            trapdoor(0, 0, 1),
            vec![local([0.0, f(0.817499), 0.0], [1.0, 1.0, 1.0])]
        );
        assert_eq!(
            trapdoor(0, 1, 0),
            vec![local([0.0, 0.0, 0.0], [f(0.1825), 1.0, 1.0])]
        );
        assert_eq!(
            trapdoor(3, 1, 1),
            vec![local([0.0, 0.0, f(0.817499)], [1.0, 1.0, 1.0])]
        );
    }

    #[test]
    fn fence_gates_block_when_closed_only() {
        let gate = |open| {
            boxes(
                "minecraft:fence_gate",
                &[
                    ("minecraft:cardinal_direction", StateValue::Str("north")),
                    ("open_bit", StateValue::Byte(open)),
                ],
            )
        };
        assert_eq!(gate(0), vec![local([0.0, 0.0, 0.375], [1.0, 1.5, 0.625])]);
        assert!(gate(1).is_empty());
    }

    /// A small world of block states; unset positions are air.
    struct World(HashMap<IVec3, u32>);

    impl World {
        fn new(blocks: &[(IVec3, u32)]) -> Self {
            Self(blocks.iter().copied().collect())
        }

        fn resolve(&self, pos: IVec3) -> u32 {
            let air = registry()
                .default_state_id_by_name("minecraft:air")
                .unwrap();
            connected_state(registry(), pos, self.0[&pos], |p| {
                Some(self.0.get(&p).copied().unwrap_or(air))
            })
        }
    }

    /// Fence/pane connection bits in `Side::ALL` order (N, E, S, W).
    fn arms(state: u32) -> [u8; 4] {
        let entry = registry().get_by_runtime_id(state).unwrap();
        Side::ALL.map(
            |side| match entry.state_value(state, side.fence_property()) {
                Some(StateValue::Byte(v)) => v,
                other => panic!("{other:?}"),
            },
        )
    }

    fn plain(name: &str) -> u32 {
        state(name, &[])
    }

    const N: IVec3 = IVec3::NEG_Z;
    const E: IVec3 = IVec3::X;
    const S: IVec3 = IVec3::Z;
    const W: IVec3 = IVec3::NEG_X;

    #[test]
    fn fence_arms_follow_neighbours_and_the_table_shapes() {
        let fence = plain("minecraft:oak_fence");
        let post = World::new(&[(IVec3::ZERO, fence)]).resolve(IVec3::ZERO);
        assert_eq!(arms(post), [0, 0, 0, 0]);
        assert_eq!(
            boxes_of(post),
            vec![local([0.375, 0.0, 0.375], [0.625, 1.5, 0.625])]
        );

        let one = World::new(&[(IVec3::ZERO, fence), (N, fence)]).resolve(IVec3::ZERO);
        assert_eq!(arms(one), [1, 0, 0, 0]);
        assert_eq!(
            boxes_of(one),
            vec![local([0.375, 0.0, 0.0], [0.625, 1.5, 0.625])]
        );

        // Wooden fences of different woods connect to each other.
        let birch = plain("minecraft:birch_fence");
        let two = World::new(&[(IVec3::ZERO, fence), (N, fence), (E, birch)]).resolve(IVec3::ZERO);
        assert_eq!(arms(two), [1, 1, 0, 0]);
        assert_eq!(
            boxes_of(two),
            vec![
                local([0.375, 0.0, 0.0], [0.625, 1.5, 0.625]),
                local([0.375, 0.0, 0.375], [1.0, 1.5, 0.625]),
            ]
        );

        let four = World::new(&[
            (IVec3::ZERO, fence),
            (N, fence),
            (E, fence),
            (S, fence),
            (W, fence),
        ])
        .resolve(IVec3::ZERO);
        assert_eq!(arms(four), [1, 1, 1, 1]);
        assert_eq!(
            boxes_of(four),
            vec![
                local([0.375, 0.0, 0.0], [0.625, 1.5, 1.0]),
                local([0.0, 0.0, 0.375], [1.0, 1.5, 0.625]),
            ]
        );
    }

    #[test]
    fn fences_connect_to_full_faces_only() {
        let fence = plain("minecraft:oak_fence");
        let bottom_slab = state(
            "minecraft:oak_slab",
            &[("minecraft:vertical_half", StateValue::Str("bottom"))],
        );
        // Stairs ascending north: their south face is half open, their
        // north (back) face is full.
        let stairs = state(
            "minecraft:oak_stairs",
            &[
                ("weirdo_direction", StateValue::Int(3)),
                ("upside_down_bit", StateValue::Byte(0)),
                ("minecraft:corner", StateValue::Str("none")),
            ],
        );
        let world = World::new(&[
            (IVec3::ZERO, fence),
            (W, plain("minecraft:stone")),
            (E, plain("minecraft:torch")),
            (N, bottom_slab),
            (S, plain("minecraft:nether_brick_fence")),
        ]);
        assert_eq!(arms(world.resolve(IVec3::ZERO)), [0, 0, 0, 1]);
        let back = World::new(&[
            (IVec3::ZERO, fence),
            (N, stairs),
            (S + S, stairs),
            (S, fence),
        ]);
        // The stairs north of the origin face it with their open south side.
        assert_eq!(arms(back.resolve(IVec3::ZERO)), [0, 0, 1, 0]);
        // The stairs south of the fence at S face it with their full back.
        assert_eq!(arms(back.resolve(S)), [1, 0, 1, 0]);
    }

    #[test]
    fn fences_and_walls_connect_to_gates_along_the_gate_only() {
        let gate = |facing| {
            state(
                "minecraft:fence_gate",
                &[("minecraft:cardinal_direction", StateValue::Str(facing))],
            )
        };
        let fence = plain("minecraft:oak_fence");
        // A north-facing gate spans X: its west end meets the fence.
        let along = World::new(&[(IVec3::ZERO, fence), (E, gate("north"))]);
        assert_eq!(arms(along.resolve(IVec3::ZERO)), [0, 1, 0, 0]);
        let across = World::new(&[(IVec3::ZERO, fence), (E, gate("east"))]);
        assert_eq!(arms(across.resolve(IVec3::ZERO)), [0, 0, 0, 0]);
        let wall = plain("minecraft:cobblestone_wall");
        let wall_world = World::new(&[(IVec3::ZERO, wall), (N, gate("east")), (S, gate("north"))]);
        let resolved = wall_world.resolve(IVec3::ZERO);
        let entry = registry().get_by_runtime_id(resolved).unwrap();
        assert_eq!(
            entry.state_value(resolved, "wall_connection_type_north"),
            Some(StateValue::Str("short"))
        );
        assert_eq!(
            entry.state_value(resolved, "wall_connection_type_south"),
            Some(StateValue::Str("none"))
        );
        // Panes do not connect to gates.
        let pane = World::new(&[
            (IVec3::ZERO, plain("minecraft:glass_pane")),
            (E, gate("north")),
        ]);
        assert_eq!(arms(pane.resolve(IVec3::ZERO)), [0, 0, 0, 0]);
    }

    #[test]
    fn walls_connect_to_walls_panes_and_full_faces() {
        let wall = plain("minecraft:cobblestone_wall");
        let world = World::new(&[
            (IVec3::ZERO, wall),
            (N, plain("minecraft:andesite_wall")),
            (E, plain("minecraft:iron_bars")),
            (S, plain("minecraft:stone")),
            (W, plain("minecraft:oak_fence")),
        ]);
        let resolved = world.resolve(IVec3::ZERO);
        let entry = registry().get_by_runtime_id(resolved).unwrap();
        let types = Side::ALL.map(|side| entry.state_value(resolved, side.wall_property()));
        let short = Some(StateValue::Str("short"));
        assert_eq!(types, [short, short, short, Some(StateValue::Str("none"))]);
        // The table shape of that state: one box reaching N, E and S.
        assert_eq!(
            boxes_of(resolved),
            vec![local([0.25, 0.0, 0.0], [1.0, 1.5, 1.0])]
        );
        let isolated = World::new(&[(IVec3::ZERO, wall)]).resolve(IVec3::ZERO);
        assert_eq!(
            boxes_of(isolated),
            vec![local([0.25, 0.0, 0.25], [0.75, 1.5, 0.75])]
        );
    }

    #[test]
    fn panes_and_bars_connect_to_each_other_walls_and_full_faces() {
        let pane = plain("minecraft:glass_pane");
        let world = World::new(&[
            (IVec3::ZERO, pane),
            (N, plain("minecraft:copper_bars")),
            (E, plain("minecraft:cobblestone_wall")),
            (S, plain("minecraft:oak_fence")),
            (W, plain("minecraft:glass")),
        ]);
        assert_eq!(arms(world.resolve(IVec3::ZERO)), [1, 1, 0, 1]);
        let isolated = World::new(&[(IVec3::ZERO, pane)]).resolve(IVec3::ZERO);
        assert_eq!(
            boxes_of(isolated),
            vec![local([0.4375, 0.0, 0.4375], [0.5625, 1.0, 0.5625])]
        );
        let one = World::new(&[(IVec3::ZERO, pane), (E, plain("minecraft:iron_bars"))])
            .resolve(IVec3::ZERO);
        assert_eq!(
            boxes_of(one),
            vec![local([0.5, 0.0, 0.4375], [1.0, 1.0, 0.5625])]
        );
    }

    #[test]
    fn unloaded_neighbours_count_as_full_blocks() {
        let fence = plain("minecraft:oak_fence");
        let resolved = connected_state(registry(), IVec3::ZERO, fence, |_| None);
        assert_eq!(arms(resolved), [1, 1, 1, 1]);
        let stone = plain("minecraft:stone");
        assert_eq!(
            connected_state(registry(), IVec3::ZERO, stone, |_| None),
            stone
        );
    }

    #[test]
    fn every_connection_block_has_a_family() {
        for entry in registry().iter() {
            if has_property(entry, Side::North.fence_property())
                && entry.string_id != "minecraft:trip_wire"
            {
                assert!(
                    matches!(
                        connectable(entry),
                        Some(Connectable::Fence { .. } | Connectable::Thin)
                    ),
                    "{}",
                    entry.string_id
                );
            }
        }
    }

    /// Arm height affects outline, not collision. Collision still depends
    /// on the post bit: a straight postless wall is narrower.
    #[test]
    fn wall_collision_depends_on_connected_sides_and_post() {
        let mut walls = 0;
        for entry in registry()
            .iter()
            .filter(|e| connectable(e) == Some(Connectable::Wall))
        {
            walls += 1;
            let mut by_mask: HashMap<([bool; 4], bool), Vec<Aabb>> = HashMap::new();
            for state in entry.min_state_id..=entry.max_state_id {
                let mask = Side::ALL.map(|side| {
                    entry.state_value(state, side.wall_property()) != Some(StateValue::Str("none"))
                });
                let post = entry.state_value(state, "wall_post_bit") == Some(StateValue::Byte(1));
                let shape = boxes_of(state);
                assert_eq!(
                    by_mask.entry((mask, post)).or_insert_with(|| shape.clone()),
                    &shape,
                    "{} state {state}",
                    entry.string_id
                );
            }
        }
        assert_eq!(walls, 33);
    }

    #[test]
    fn full_face_coverage_needs_the_whole_square() {
        assert!(covers_face(&[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]], Side::North));
        // Two boxes together cover the south face; the north face only half.
        let halves = [
            [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
            [0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        ];
        assert!(covers_face(&halves, Side::South));
        assert!(!covers_face(&halves, Side::North));
        assert!(!covers_face(&[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]], Side::East));
        assert!(!covers_face(&[], Side::West));
    }

    #[test]
    fn clip_stops_a_falling_box_on_a_floor() {
        let mover = Aabb::from_feet(DVec3::new(0.5, 1.25, 0.5), 0.25, 0.25);
        assert_eq!(FULL.clip_y(&mover, -1.0), -0.25);
        assert_eq!(FULL.clip_y(&mover, 0.5), 0.5);
        let beside = Aabb::from_feet(DVec3::new(2.0, 1.25, 0.5), 0.25, 0.25);
        assert_eq!(FULL.clip_y(&beside, -1.0), -1.0);
    }
}
