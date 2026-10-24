//! Java block identities used by the Java-style decoration code.
//!
//! Decoration runs in Java block-state space: every vanilla Java state has a
//! global id ([`JavaState`], the registry order of the pinned 1.21.11 data in
//! `java_data/blocks.json`), and the runtime converts to Bedrock runtime ids
//! only when it reads or writes the world. Block sets (tags and `matching_blocks`
//! lists) are build-time bitsets over [`JavaBlock`].

use super::java_blocks::{BLOCK_SETS, BLOCKS, BLOCKS_BY_NAME, SET_WORDS, STATE_BLOCK, STATE_FLAGS};

/// A Java block (registry index into `java_blocks::BLOCKS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JavaBlock(pub u16);

/// A Java block state (global state id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JavaState(pub u16);

/// A set of Java blocks (a tag or an inline block list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockSet(pub u16);

/// Static description of one property of a Java block.
#[derive(Debug)]
pub struct PropDef {
    pub name: &'static str,
    pub values: &'static [&'static str],
    /// Distance in state ids between consecutive values of this property.
    pub stride: u16,
}

/// Static description of a Java block.
#[derive(Debug)]
pub struct JavaBlockDef {
    pub name: &'static str,
    /// Block type from the report (`minecraft:leaves`, `minecraft:sapling`, ...).
    pub kind: &'static str,
    pub first_state: u16,
    pub state_count: u16,
    pub default_state: u16,
    pub props: &'static [PropDef],
}

/// State flag bits (see `java_data/README.md`).
pub mod flag {
    pub const SOLID: u8 = 1;
    pub const FULL_CUBE: u8 = 1 << 7;
    /// Sturdy-face bit for a [`super::Direction`].
    #[inline]
    pub const fn face(dir: super::Direction) -> u8 {
        1 << (dir as u8 + 1)
    }
}

impl JavaBlock {
    #[inline]
    pub fn def(self) -> &'static JavaBlockDef {
        &BLOCKS[self.0 as usize]
    }

    #[inline]
    pub fn name(self) -> &'static str {
        self.def().name
    }

    #[inline]
    pub fn default_state(self) -> JavaState {
        JavaState(self.def().default_state)
    }

    /// Look a block up by registry name (`minecraft:stone`).
    pub fn by_name(name: &str) -> Option<JavaBlock> {
        BLOCKS_BY_NAME
            .binary_search_by(|(n, _)| (*n).cmp(name))
            .ok()
            .map(|i| JavaBlock(BLOCKS_BY_NAME[i].1))
    }

    /// Whether this block is in `set`.
    #[inline]
    pub fn is_in(self, set: BlockSet) -> bool {
        let words = &BLOCK_SETS[set.0 as usize];
        let i = self.0 as usize;
        debug_assert!(i / 64 < SET_WORDS);
        words[i / 64] & (1u64 << (i % 64)) != 0
    }
}

impl JavaState {
    #[inline]
    pub fn block(self) -> JavaBlock {
        JavaBlock(STATE_BLOCK[self.0 as usize])
    }

    #[inline]
    pub fn flags(self) -> u8 {
        STATE_FLAGS[self.0 as usize]
    }

    /// `BlockState.isSolid()` (legacy solid).
    #[inline]
    pub fn is_solid(self) -> bool {
        self.flags() & flag::SOLID != 0
    }

    /// Full-square collision on `dir` (`isFaceSturdy(..., SupportType.FULL)`).
    #[inline]
    pub fn is_face_sturdy(self, dir: Direction) -> bool {
        self.flags() & flag::face(dir) != 0
    }

    /// Full-cube collision shape.
    #[inline]
    pub fn is_collision_full_cube(self) -> bool {
        self.flags() & flag::FULL_CUBE != 0
    }

    /// `BlockState.blocksMotion()`: solid, except cobweb and bamboo sapling.
    #[inline]
    pub fn blocks_motion(self) -> bool {
        let b = self.block();
        b != super::java_blocks::blocks::COBWEB
            && b != super::java_blocks::blocks::BAMBOO_SAPLING
            && self.is_solid()
    }

    /// `isAir()` (air, cave air, void air).
    #[inline]
    pub fn is_air(self) -> bool {
        use super::java_blocks::blocks::{AIR, CAVE_AIR, VOID_AIR};
        let b = self.block();
        b == AIR || b == CAVE_AIR || b == VOID_AIR
    }

    #[inline]
    pub fn is(self, block: JavaBlock) -> bool {
        self.block() == block
    }

    #[inline]
    pub fn is_in(self, set: BlockSet) -> bool {
        self.block().is_in(set)
    }

    /// `canBeReplaced()`: membership of the `minecraft:replaceable` tag.
    #[inline]
    pub fn can_be_replaced(self) -> bool {
        self.is_in(super::java_blocks::tags::REPLACEABLE)
    }

    fn prop(self, name: &str) -> Option<&'static PropDef> {
        self.block().def().props.iter().find(|p| p.name == name)
    }

    /// Value of property `name`, if the block has it.
    pub fn value(self, name: &str) -> Option<&'static str> {
        let def = self.block().def();
        let offset = self.0 - def.first_state;
        let p = self.prop(name)?;
        Some(p.values[((offset / p.stride) as usize) % p.values.len()])
    }

    /// Whether the block has property `name`.
    #[inline]
    pub fn has(self, name: &str) -> bool {
        self.prop(name).is_some()
    }

    /// `trySetValue`: this state with `name = value`, or unchanged if the block
    /// lacks the property or the value.
    pub fn try_with(self, name: &str, value: &str) -> JavaState {
        let Some(p) = self.prop(name) else {
            return self;
        };
        let Some(new_idx) = p.values.iter().position(|v| *v == value) else {
            return self;
        };
        let def = self.block().def();
        let offset = self.0 - def.first_state;
        let cur_idx = (offset / p.stride) as usize % p.values.len();
        let base = offset - cur_idx as u16 * p.stride;
        JavaState(def.first_state + base + new_idx as u16 * p.stride)
    }

    /// `setValue`: like [`Self::try_with`] but the property must exist.
    pub fn with(self, name: &str, value: &str) -> JavaState {
        debug_assert!(
            self.prop(name).is_some_and(|p| p.values.contains(&value)),
            "{} has no {name}={value}",
            self.block().name()
        );
        self.try_with(name, value)
    }

    /// Boolean property shorthand.
    #[inline]
    pub fn with_bool(self, name: &str, value: bool) -> JavaState {
        self.with(name, if value { "true" } else { "false" })
    }

    /// Integer property shorthand.
    pub fn with_int(self, name: &str, value: i32) -> JavaState {
        let mut buf = itoa_buf(value);
        self.with(name, buf.as_str())
    }

    /// Integer property value (`None` if absent or not numeric).
    pub fn int_value(self, name: &str) -> Option<i32> {
        self.value(name).and_then(|v| v.parse().ok())
    }

    /// Fluid state of this block state (`getFluidState()`).
    pub fn fluid(self) -> Fluid {
        use super::java_blocks::blocks as b;
        let block = self.block();
        if block == b::WATER || block == b::LAVA {
            let level = self.int_value("level").unwrap_or(0);
            let kind = if block == b::WATER {
                FluidKind::Water
            } else {
                FluidKind::Lava
            };
            // Levels 1..=7 flow; 8..=15 are falling flowing fluid of amount 8.
            return Fluid {
                kind,
                source: level == 0,
                amount: if level == 0 {
                    8
                } else if level >= 8 {
                    8
                } else {
                    8 - level as u8
                },
            };
        }
        if self.value("waterlogged") == Some("true")
            || block == b::BUBBLE_COLUMN
            || block == b::KELP
            || block == b::KELP_PLANT
            || block == b::SEAGRASS
            || block == b::TALL_SEAGRASS
        {
            return Fluid::WATER_SOURCE;
        }
        Fluid::EMPTY
    }
}

/// Small stack buffer for formatting property integers without allocating.
struct ItoaBuf {
    bytes: [u8; 12],
    len: usize,
}

impl ItoaBuf {
    fn as_str(&mut self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).expect("ascii digits")
    }
}

fn itoa_buf(value: i32) -> ItoaBuf {
    let mut out = ItoaBuf {
        bytes: [0; 12],
        len: 0,
    };
    let mut tmp = [0u8; 12];
    let mut n = value.unsigned_abs();
    let mut i = 0;
    loop {
        tmp[i] = b'0' + (n % 10) as u8;
        i += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if value < 0 {
        out.bytes[0] = b'-';
        out.len = 1;
    }
    while i > 0 {
        i -= 1;
        out.bytes[out.len] = tmp[i];
        out.len += 1;
    }
    out
}

/// Fluid type of a [`Fluid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidKind {
    Empty,
    Water,
    Lava,
}

/// Java `FluidState` reduced to what decoration reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fluid {
    pub kind: FluidKind,
    pub source: bool,
    /// Fluid amount 1..=8 (8 for sources), 0 when empty.
    pub amount: u8,
}

impl Fluid {
    pub const EMPTY: Fluid = Fluid {
        kind: FluidKind::Empty,
        source: false,
        amount: 0,
    };
    pub const WATER_SOURCE: Fluid = Fluid {
        kind: FluidKind::Water,
        source: true,
        amount: 8,
    };

    #[inline]
    pub fn is_empty(self) -> bool {
        self.kind == FluidKind::Empty
    }

    /// `is(FluidTags.WATER)`.
    #[inline]
    pub fn is_water(self) -> bool {
        self.kind == FluidKind::Water
    }

    /// `is(FluidTags.LAVA)`.
    #[inline]
    pub fn is_lava(self) -> bool {
        self.kind == FluidKind::Lava
    }

    /// Bit for [`FluidSet`] membership.
    #[inline]
    pub fn set_bit(self) -> u8 {
        match (self.kind, self.source) {
            (FluidKind::Empty, _) => FluidSet::EMPTY,
            (FluidKind::Water, true) => FluidSet::WATER,
            (FluidKind::Water, false) => FluidSet::FLOWING_WATER,
            (FluidKind::Lava, true) => FluidSet::LAVA,
            (FluidKind::Lava, false) => FluidSet::FLOWING_LAVA,
        }
    }
}

/// Set of fluid types (`HolderSet<Fluid>`) as bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidSet(pub u8);

impl FluidSet {
    pub const EMPTY: u8 = 1;
    pub const WATER: u8 = 2;
    pub const FLOWING_WATER: u8 = 4;
    pub const LAVA: u8 = 8;
    pub const FLOWING_LAVA: u8 = 16;

    #[inline]
    pub fn contains(self, fluid: Fluid) -> bool {
        self.0 & fluid.set_bit() != 0
    }
}

/// Java `Direction` in its declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Direction {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Direction {
    /// `Direction.values()`.
    pub const ALL: [Direction; 6] = [
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ];
    /// `Direction.Plane.HORIZONTAL` iteration order.
    pub const HORIZONTAL: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    #[inline]
    pub const fn offset(self) -> (i32, i32, i32) {
        match self {
            Direction::Down => (0, -1, 0),
            Direction::Up => (0, 1, 0),
            Direction::North => (0, 0, -1),
            Direction::South => (0, 0, 1),
            Direction::West => (-1, 0, 0),
            Direction::East => (1, 0, 0),
        }
    }

    #[inline]
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::Down => Direction::Up,
            Direction::Up => Direction::Down,
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::East => Direction::West,
        }
    }

    /// Lowercase Java name (`"north"`), also the block property name.
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Direction::Down => "down",
            Direction::Up => "up",
            Direction::North => "north",
            Direction::South => "south",
            Direction::West => "west",
            Direction::East => "east",
        }
    }

    /// `getClockWise()` around Y (horizontal directions only).
    #[inline]
    pub const fn clockwise(self) -> Direction {
        match self {
            Direction::North => Direction::East,
            Direction::East => Direction::South,
            Direction::South => Direction::West,
            Direction::West => Direction::North,
            other => other,
        }
    }

    /// `getCounterClockWise()` around Y (horizontal directions only).
    #[inline]
    pub const fn counter_clockwise(self) -> Direction {
        match self {
            Direction::North => Direction::West,
            Direction::West => Direction::South,
            Direction::South => Direction::East,
            Direction::East => Direction::North,
            other => other,
        }
    }

    /// Axis name (`"x"`, `"y"`, `"z"`).
    #[inline]
    pub const fn axis(self) -> &'static str {
        match self {
            Direction::Down | Direction::Up => "y",
            Direction::North | Direction::South => "z",
            Direction::West | Direction::East => "x",
        }
    }

    #[inline]
    pub const fn is_horizontal(self) -> bool {
        !matches!(self, Direction::Down | Direction::Up)
    }
}

/// Block position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    #[inline]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
    #[inline]
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }
    #[inline]
    pub const fn rel(self, dir: Direction) -> Self {
        let (dx, dy, dz) = dir.offset();
        self.offset(dx, dy, dz)
    }
    #[inline]
    pub const fn rel_n(self, dir: Direction, n: i32) -> Self {
        let (dx, dy, dz) = dir.offset();
        self.offset(dx * n, dy * n, dz * n)
    }
    #[inline]
    pub const fn above(self) -> Self {
        self.offset(0, 1, 0)
    }
    #[inline]
    pub const fn below(self) -> Self {
        self.offset(0, -1, 0)
    }
    #[inline]
    pub const fn with_y(self, y: i32) -> Self {
        Self::new(self.x, y, self.z)
    }
    /// `distSqr` between block corners.
    #[inline]
    pub fn dist_sqr(self, other: BlockPos) -> f64 {
        let dx = (self.x - other.x) as f64;
        let dy = (self.y - other.y) as f64;
        let dz = (self.z - other.z) as f64;
        dx * dx + dy * dy + dz * dz
    }
}

/// Java `Heightmap.Types`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Heightmap {
    WorldSurfaceWg = 0,
    WorldSurface = 1,
    OceanFloorWg = 2,
    OceanFloor = 3,
    MotionBlocking = 4,
    MotionBlockingNoLeaves = 5,
}

impl Heightmap {
    /// Whether `state` counts as "opaque" for this heightmap.
    pub fn is_opaque(self, state: JavaState) -> bool {
        match self {
            Heightmap::WorldSurfaceWg | Heightmap::WorldSurface => !state.is_air(),
            Heightmap::OceanFloorWg | Heightmap::OceanFloor => state.blocks_motion(),
            Heightmap::MotionBlocking => state.blocks_motion() || !state.fluid().is_empty(),
            Heightmap::MotionBlockingNoLeaves => {
                (state.blocks_motion() || !state.fluid().is_empty())
                    && !state.is_in(super::java_blocks::tags::LEAVES)
            }
        }
    }
}

/// Java `BlockPredicate` (data form, evaluated by [`super::eval::test_predicate`]).
#[derive(Debug, Clone, Copy)]
pub enum BlockPredicate {
    MatchingBlocks { set: BlockSet, offset: [i32; 3] },
    MatchingFluids { fluids: FluidSet, offset: [i32; 3] },
    Solid { offset: [i32; 3] },
    Replaceable { offset: [i32; 3] },
    WouldSurvive { state: JavaState, offset: [i32; 3] },
    HasSturdyFace { direction: Direction, offset: [i32; 3] },
    InsideWorldBounds { offset: [i32; 3] },
    AllOf(&'static [BlockPredicate]),
    AnyOf(&'static [BlockPredicate]),
    Not(&'static BlockPredicate),
    True,
}

/// Java `RuleTest`.
#[derive(Debug, Clone, Copy)]
pub enum RuleTest {
    AlwaysTrue,
    BlockMatch(JavaBlock),
    BlockStateMatch(JavaState),
    TagMatch(BlockSet),
    RandomBlockMatch(JavaBlock, f32),
    RandomBlockStateMatch(JavaState, f32),
}

/// Inline noise parameters (`firstOctave`, `amplitudes`).
#[derive(Debug, Clone, Copy)]
pub struct NoiseSpec {
    pub first_octave: i32,
    pub amplitudes: &'static [f64],
}

/// Java `BlockStateProvider`.
#[derive(Debug, Clone, Copy)]
pub enum StateProvider {
    Simple(JavaState),
    /// Default state of the block with a random `axis`.
    Rotated(JavaState),
    Weighted {
        entries: &'static [(JavaState, i32)],
        total_weight: i32,
    },
    RandomizedInt {
        source: &'static StateProvider,
        property: &'static str,
        values: super::values::IntProvider,
    },
    Noise {
        noise: super::noise::NoiseRef,
        scale: f32,
        states: &'static [JavaState],
    },
    DualNoise {
        noise: super::noise::NoiseRef,
        slow_noise: super::noise::NoiseRef,
        scale: f32,
        slow_scale: f32,
        variety: (i32, i32),
        states: &'static [JavaState],
    },
    NoiseThreshold {
        noise: super::noise::NoiseRef,
        scale: f32,
        threshold: f32,
        high_chance: f32,
        default_state: JavaState,
        low_states: &'static [JavaState],
        high_states: &'static [JavaState],
    },
}

/// `RuleBasedBlockStateProvider`.
#[derive(Debug, Clone, Copy)]
pub struct RuleBasedStateProvider {
    pub fallback: StateProvider,
    pub rules: &'static [(BlockPredicate, StateProvider)],
}
