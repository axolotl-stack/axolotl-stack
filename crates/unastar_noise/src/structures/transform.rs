//! Structure geometry: `Rotation`, `Mirror`, `BoundingBox`, position and block
//! state transforms (Java `StructureTemplate.transform`, `BlockState.rotate`,
//! `BlockState.mirror`).
//!
//! Block state transforms are property driven: `facing`, `axis`, `rotation`
//! (0..15), horizontal connection properties (`north`/`east`/`south`/`west`),
//! jigsaw `orientation`, rail `shape`, stair `shape`, chest `type` and door
//! `hinge` cover every rotatable vanilla state used by templates.

use crate::features::random::{BitSource, WorldgenRandom};
use crate::features::{BlockPos, Direction, JavaState};

/// Java `Rotation` (declaration order matters for `getRandom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rotation {
    None = 0,
    Clockwise90 = 1,
    Clockwise180 = 2,
    CounterClockwise90 = 3,
}

impl Rotation {
    pub const ALL: [Rotation; 4] = [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::CounterClockwise90,
    ];

    /// `Rotation.getRandom(random)`.
    #[inline]
    pub fn random<S: BitSource, const RETAIN_GAUSSIAN: bool>(rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>) -> Rotation {
        Self::ALL[rng.next_int_bounded(4) as usize]
    }

    /// `Rotation.getShuffled(random)`: all rotations, shuffled.
    pub fn shuffled<S: BitSource, const RETAIN_GAUSSIAN: bool>(rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>) -> [Rotation; 4] {
        let mut list = Self::ALL;
        crate::structures::util::shuffle(&mut list, rng);
        list
    }

    /// `getRotated(rotation)`.
    #[inline]
    pub fn then(self, other: Rotation) -> Rotation {
        Self::ALL[((self as u8 + other as u8) % 4) as usize]
    }

    /// Rotate a horizontal direction (vertical ones are unchanged).
    #[inline]
    pub fn rotate_dir(self, d: Direction) -> Direction {
        if !d.is_horizontal() {
            return d;
        }
        match self {
            Rotation::None => d,
            Rotation::Clockwise90 => d.clockwise(),
            Rotation::Clockwise180 => d.opposite(),
            Rotation::CounterClockwise90 => d.counter_clockwise(),
        }
    }

    /// `rotate(rotation, count)` for 16-step rotation properties.
    #[inline]
    pub fn rotate_index(self, rotation: i32, count: i32) -> i32 {
        match self {
            Rotation::None => rotation,
            Rotation::Clockwise90 => (rotation + count / 4) % count,
            Rotation::Clockwise180 => (rotation + count / 2) % count,
            Rotation::CounterClockwise90 => (rotation + count * 3 / 4) % count,
        }
    }
}

/// Java `Mirror`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mirror {
    None = 0,
    LeftRight = 1,
    FrontBack = 2,
}

impl Mirror {
    /// `mirror(direction)`.
    #[inline]
    pub fn mirror_dir(self, d: Direction) -> Direction {
        match (self, d) {
            (Mirror::LeftRight, Direction::North) => Direction::South,
            (Mirror::LeftRight, Direction::South) => Direction::North,
            (Mirror::FrontBack, Direction::West) => Direction::East,
            (Mirror::FrontBack, Direction::East) => Direction::West,
            _ => d,
        }
    }

    /// `getRotation(direction)`.
    #[inline]
    pub fn rotation_for(self, d: Direction) -> Rotation {
        let axis = d.axis();
        match self {
            Mirror::LeftRight if axis == "z" => Rotation::Clockwise180,
            Mirror::FrontBack if axis == "x" => Rotation::Clockwise180,
            _ => Rotation::None,
        }
    }

    /// `mirror(rotation, count)` for 16-step rotation properties.
    #[inline]
    pub fn mirror_index(self, rotation: i32, count: i32) -> i32 {
        let half = count / 2;
        let j = if rotation > half {
            rotation - count
        } else {
            rotation
        };
        match self {
            Mirror::None => rotation,
            Mirror::LeftRight => (count - j) % count,
            Mirror::FrontBack => (half - j + count) % count,
        }
    }
}

/// Inclusive block bounding box (`BoundingBox`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundingBox {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

impl BoundingBox {
    pub const fn new(
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// `fromCorners`.
    pub fn from_corners(a: BlockPos, b: BlockPos) -> Self {
        Self::new(
            a.x.min(b.x),
            a.y.min(b.y),
            a.z.min(b.z),
            a.x.max(b.x),
            a.y.max(b.y),
            a.z.max(b.z),
        )
    }

    /// `orientBox(x, y, z, offX, offY, offZ, width, height, depth, facing)`.
    #[allow(clippy::too_many_arguments)]
    pub fn orient_box(
        x: i32,
        y: i32,
        z: i32,
        off_x: i32,
        off_y: i32,
        off_z: i32,
        width: i32,
        height: i32,
        depth: i32,
        facing: Direction,
    ) -> Self {
        match facing {
            Direction::South => Self::new(
                x + off_x,
                y + off_y,
                z + off_z,
                x + width - 1 + off_x,
                y + height - 1 + off_y,
                z + depth - 1 + off_z,
            ),
            Direction::West => Self::new(
                x - depth + 1 + off_z,
                y + off_y,
                z + off_x,
                x + off_z,
                y + height - 1 + off_y,
                z + width - 1 + off_x,
            ),
            Direction::East => Self::new(
                x + off_z,
                y + off_y,
                z + off_x,
                x + depth - 1 + off_z,
                y + height - 1 + off_y,
                z + width - 1 + off_x,
            ),
            // North (and vertical directions, which Java treats like north).
            _ => Self::new(
                x + off_x,
                y + off_y,
                z - depth + 1 + off_z,
                x + width - 1 + off_x,
                y + height - 1 + off_y,
                z + off_z,
            ),
        }
    }

    #[inline]
    pub fn intersects(&self, o: &BoundingBox) -> bool {
        self.max_x >= o.min_x
            && self.min_x <= o.max_x
            && self.max_z >= o.min_z
            && self.min_z <= o.max_z
            && self.max_y >= o.min_y
            && self.min_y <= o.max_y
    }

    #[inline]
    pub fn intersects_xz(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.max_x >= min_x && self.min_x <= max_x && self.max_z >= min_z && self.min_z <= max_z
    }

    #[inline]
    pub fn is_inside(&self, p: BlockPos) -> bool {
        p.x >= self.min_x
            && p.x <= self.max_x
            && p.z >= self.min_z
            && p.z <= self.max_z
            && p.y >= self.min_y
            && p.y <= self.max_y
    }

    /// `encapsulate(other)`.
    pub fn encapsulate(&mut self, o: &BoundingBox) {
        self.min_x = self.min_x.min(o.min_x);
        self.min_y = self.min_y.min(o.min_y);
        self.min_z = self.min_z.min(o.min_z);
        self.max_x = self.max_x.max(o.max_x);
        self.max_y = self.max_y.max(o.max_y);
        self.max_z = self.max_z.max(o.max_z);
    }

    /// `move(dx, dy, dz)`.
    #[inline]
    pub fn moved(&self, dx: i32, dy: i32, dz: i32) -> BoundingBox {
        Self::new(
            self.min_x + dx,
            self.min_y + dy,
            self.min_z + dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    /// `inflatedBy(n)`.
    #[inline]
    pub fn inflated(&self, n: i32) -> BoundingBox {
        Self::new(
            self.min_x - n,
            self.min_y - n,
            self.min_z - n,
            self.max_x + n,
            self.max_y + n,
            self.max_z + n,
        )
    }

    #[inline]
    pub fn x_span(&self) -> i32 {
        self.max_x - self.min_x + 1
    }
    #[inline]
    pub fn y_span(&self) -> i32 {
        self.max_y - self.min_y + 1
    }
    #[inline]
    pub fn z_span(&self) -> i32 {
        self.max_z - self.min_z + 1
    }

    /// `getCenter()`.
    #[inline]
    pub fn center(&self) -> BlockPos {
        BlockPos::new(
            self.min_x + (self.max_x - self.min_x + 1) / 2,
            self.min_y + (self.max_y - self.min_y + 1) / 2,
            self.min_z + (self.max_z - self.min_z + 1) / 2,
        )
    }

    /// The whole-height box of chunk (`cx`, `cz`) (`getWritableArea`).
    pub fn chunk(cx: i32, cz: i32, min_y: i32, max_y: i32) -> BoundingBox {
        Self::new(cx * 16, min_y, cz * 16, cx * 16 + 15, max_y, cz * 16 + 15)
    }
}

/// `StructureTemplate.transform(pos, mirror, rotation, pivot)`.
pub fn transform(
    target: BlockPos,
    mirror: Mirror,
    rotation: Rotation,
    pivot: BlockPos,
) -> BlockPos {
    let (mut i, j, mut k) = (target.x, target.y, target.z);
    let mirrored = match mirror {
        Mirror::LeftRight => {
            k = -k;
            true
        }
        Mirror::FrontBack => {
            i = -i;
            true
        }
        Mirror::None => false,
    };
    let (l, i1) = (pivot.x, pivot.z);
    match rotation {
        Rotation::CounterClockwise90 => BlockPos::new(l - i1 + k, j, l + i1 - i),
        Rotation::Clockwise90 => BlockPos::new(l + i1 - k, j, i1 - l + i),
        Rotation::Clockwise180 => BlockPos::new(l + l - i, j, i1 + i1 - k),
        Rotation::None => {
            if mirrored {
                BlockPos::new(i, j, k)
            } else {
                target
            }
        }
    }
}

fn dir_from_name(s: &str) -> Option<Direction> {
    Some(match s {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return None,
    })
}

/// Rotate a jigsaw/crafter `orientation` value (`<front>_<top>`).
fn rotate_orientation(value: &str, rotation: Rotation) -> Option<String> {
    let (front, top) = value.split_once('_')?;
    let f = rotation.rotate_dir(dir_from_name(front)?);
    let t = rotation.rotate_dir(dir_from_name(top)?);
    Some(format!("{}_{}", f.name(), t.name()))
}

fn mirror_orientation(value: &str, mirror: Mirror) -> Option<String> {
    let (front, top) = value.split_once('_')?;
    let f = mirror.mirror_dir(dir_from_name(front)?);
    let t = mirror.mirror_dir(dir_from_name(top)?);
    Some(format!("{}_{}", f.name(), t.name()))
}

/// Rail `shape` under a rotation (`RailBlock.rotate`).
fn rotate_rail(shape: &str, rotation: Rotation) -> &'static str {
    let step = |s: &str| -> &'static str {
        // One clockwise quarter turn.
        match s {
            "north_south" => "east_west",
            "east_west" => "north_south",
            "ascending_east" => "ascending_south",
            "ascending_south" => "ascending_west",
            "ascending_west" => "ascending_north",
            "ascending_north" => "ascending_east",
            "south_east" => "south_west",
            "south_west" => "north_west",
            "north_west" => "north_east",
            "north_east" => "south_east",
            other => leak_shape(other),
        }
    };
    let turns = match rotation {
        Rotation::None => 0,
        Rotation::Clockwise90 => 1,
        Rotation::Clockwise180 => 2,
        Rotation::CounterClockwise90 => 3,
    };
    let mut s: &'static str = leak_shape(shape);
    for _ in 0..turns {
        s = step(s);
    }
    s
}

fn leak_shape(s: &str) -> &'static str {
    const SHAPES: [&str; 10] = [
        "north_south",
        "east_west",
        "ascending_east",
        "ascending_west",
        "ascending_north",
        "ascending_south",
        "south_east",
        "south_west",
        "north_west",
        "north_east",
    ];
    SHAPES
        .into_iter()
        .find(|x| *x == s)
        .expect("rail shape value")
}

/// Rail `shape` under a mirror (`RailBlock.mirror`).
fn mirror_rail(shape: &str, mirror: Mirror) -> &'static str {
    match (mirror, shape) {
        (Mirror::LeftRight, "ascending_north") => "ascending_south",
        (Mirror::LeftRight, "ascending_south") => "ascending_north",
        (Mirror::LeftRight, "south_east") => "north_east",
        (Mirror::LeftRight, "south_west") => "north_west",
        (Mirror::LeftRight, "north_west") => "south_west",
        (Mirror::LeftRight, "north_east") => "south_east",
        (Mirror::FrontBack, "ascending_east") => "ascending_west",
        (Mirror::FrontBack, "ascending_west") => "ascending_east",
        (Mirror::FrontBack, "south_east") => "south_west",
        (Mirror::FrontBack, "south_west") => "south_east",
        (Mirror::FrontBack, "north_west") => "north_east",
        (Mirror::FrontBack, "north_east") => "north_west",
        (_, other) => leak_shape(other),
    }
}

/// `BlockState.rotate(rotation)`.
pub fn rotate_state(state: JavaState, rotation: Rotation) -> JavaState {
    if rotation == Rotation::None {
        return state;
    }
    let mut s = state;
    if let Some(f) = s.value("facing").and_then(dir_from_name) {
        s = s.with("facing", rotation.rotate_dir(f).name());
    }
    if matches!(
        rotation,
        Rotation::Clockwise90 | Rotation::CounterClockwise90
    ) && let Some(axis) = s.value("axis")
    {
        s = s.try_with(
            "axis",
            match axis {
                "x" => "z",
                "z" => "x",
                other => other,
            },
        );
    }
    if let Some(r) = s.int_value("rotation") {
        s = s.with_int("rotation", rotation.rotate_index(r, 16));
    }
    if let Some(o) = s.value("orientation")
        && let Some(v) = rotate_orientation(o, rotation)
    {
        s = s.try_with("orientation", &v);
    }
    if s.has("shape") && s.block().def().kind.ends_with("rail") {
        let shape = s.value("shape").expect("shape");
        let mut r = rotate_rail(shape, rotation);
        // Powered/detector/activator rails have no curved shapes.
        if !s
            .block()
            .def()
            .props
            .iter()
            .any(|p| p.name == "shape" && p.values.contains(&r))
        {
            r = shape_or_same(shape);
        }
        s = s.try_with("shape", r);
    }
    // Horizontal connection properties move with the rotation.
    let sides = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    if sides.iter().all(|d| s.has(d.name())) {
        let vals = sides.map(|d| state.value(d.name()).expect("side"));
        for (i, d) in sides.iter().enumerate() {
            let to = rotation.rotate_dir(*d);
            s = s.try_with(to.name(), vals[i]);
        }
    }
    s
}

fn shape_or_same(shape: &str) -> &'static str {
    leak_shape(shape)
}

/// `BlockState.mirror(mirror)`.
pub fn mirror_state(state: JavaState, mirror: Mirror) -> JavaState {
    if mirror == Mirror::None {
        return state;
    }
    let kind = state.block().def().kind;
    // Stairs: inner/outer shapes swap handedness when mirrored along their axis.
    if kind == "minecraft:stair"
        && let Some(f) = state.value("facing").and_then(dir_from_name)
    {
        let along = match mirror {
            Mirror::LeftRight => f.axis() == "z",
            Mirror::FrontBack => f.axis() == "x",
            Mirror::None => false,
        };
        if along {
            let swapped = match state.value("shape") {
                Some("inner_left") => Some("inner_right"),
                Some("inner_right") => Some("inner_left"),
                Some("outer_left") => Some("outer_right"),
                Some("outer_right") => Some("outer_left"),
                _ => None,
            };
            let rotated = rotate_state(state, Rotation::Clockwise180);
            return match swapped {
                Some(v) => rotated.with("shape", v),
                None => rotated,
            };
        }
    }
    let mut s = state;
    if let Some(f) = s.value("facing").and_then(dir_from_name) {
        s = rotate_state(s, mirror.rotation_for(f));
        if s.has("hinge") {
            let h = if s.value("hinge") == Some("left") {
                "right"
            } else {
                "left"
            };
            s = s.with("hinge", h);
        }
        if s.value("type").is_some_and(|t| t == "left" || t == "right") && kind.contains("chest") {
            let t = if s.value("type") == Some("left") {
                "right"
            } else {
                "left"
            };
            s = s.with("type", t);
        }
        return s;
    }
    if let Some(r) = s.int_value("rotation") {
        s = s.with_int("rotation", mirror.mirror_index(r, 16));
    }
    if let Some(o) = s.value("orientation")
        && let Some(v) = mirror_orientation(o, mirror)
    {
        s = s.try_with("orientation", &v);
    }
    if s.has("shape") && kind.ends_with("rail") {
        let r = mirror_rail(s.value("shape").expect("shape"), mirror);
        s = s.try_with("shape", r);
    }
    let (a, b) = match mirror {
        Mirror::LeftRight => (Direction::North, Direction::South),
        _ => (Direction::East, Direction::West),
    };
    if s.has(a.name()) && s.has(b.name()) {
        let va = state.value(a.name()).expect("side");
        let vb = state.value(b.name()).expect("side");
        s = s.try_with(a.name(), vb).try_with(b.name(), va);
    }
    s
}
