//! Built-in mob behaviour constants that behaviour-pack data does not carry.
//!
//! Every value here describes how the Bedrock Dedicated Server behaves; the
//! decisions doc's D12 table lists each one with its status ("confirmed
//! against BDS" or "provisional" with a public source). Keep the two in sync.

// --- Move control -----------------------------------------------------------

/// Current speed rises by this fraction of the target speed per tick (a lower
/// target is taken immediately).
pub const MOVE_SPEED_RAMP: f32 = 0.1;
/// Squared 3D distance at which the move control drops its target.
pub const MOVE_ARRIVE_DIST_SQ: f64 = 0.1;
/// Height above the feet below which an arrived target counts as reached.
pub const MOVE_ARRIVE_HEIGHT: f64 = 0.25;
/// Floor of the final-node slow-down: `max(target * d², ARRIVAL_FLOOR * target)`.
pub const MOVE_ARRIVAL_FLOOR: f32 = 0.5;
/// Speed cap while the wanted position is below the feet.
pub const MOVE_DESCEND_CAP: f32 = 0.25;
/// Land mobs step up this high without jumping.
pub const MOB_STEP_HEIGHT: f64 = 0.5625;
/// Target height difference (when the mob was in water) that triggers a jump.
pub const MOVE_JUMP_HEIGHT_IN_WATER: f64 = 0.009;
/// Minimum ticks between two jumps.
pub const JUMP_COOLDOWN_TICKS: u32 = 10;
/// Upward velocity added per tick while jumping in liquid.
pub const LIQUID_JUMP_IMPULSE: f64 = 0.04;

// --- Living physics -----------------------------------------------------------

pub const LIVING_GRAVITY: f64 = 0.08;
pub const LIVING_VERTICAL_DRAG: f64 = 0.98;
/// Horizontal multiplier per tick: block friction × this (air: this alone).
pub const LIVING_FRICTION_FACTOR: f64 = 0.91;
/// Block friction used when the sampled support block is air.
pub const DEFAULT_BLOCK_FRICTION: f64 = 0.6;
/// Ground acceleration is `speed × (NORMAL_GROUND_FRICTION / friction)³`.
pub const NORMAL_GROUND_FRICTION: f64 = 0.546;
/// Support block is sampled this far below the feet.
pub const FRICTION_SAMPLE_DEPTH: f64 = 0.1;
pub const WATER_DRAG: f64 = 0.8;
pub const LIQUID_GRAVITY: f64 = 0.02;
/// Lava drag; provisional (the Java Edition value).
pub const LAVA_DRAG: f64 = 0.5;
/// Acceleration input while swimming (underwater movement attribute).
pub const WATER_MOVE_SPEED: f32 = 0.02;

// --- Goals --------------------------------------------------------------------

/// Random target candidates tried per stroll/panic activation.
pub const RANDOM_POS_ATTEMPTS: u32 = 10;
/// Panic runs only within this many ticks of the last damage.
pub const PANIC_RECENT_HURT_TICKS: u64 = 40;
pub const PANIC_RANGE_XZ: i32 = 5;
pub const PANIC_RANGE_Y: i32 = 4;
pub const PANIC_REPATH_TICKS: u32 = 8;
/// Tempt search radius when `within_radius` is not positive.
pub const TEMPT_DEFAULT_RADIUS: f32 = 16.0;
/// Tempt stop distance when the authored value is negative.
pub const TEMPT_DEFAULT_STOP_DISTANCE: f32 = 1.5;
pub const TEMPT_COOLDOWN_TICKS: u32 = 100;
/// Re-path every `TEMPT_REPATH_TICKS + random(0..=3)` ticks.
pub const TEMPT_REPATH_TICKS: u32 = 8;
pub const TEMPT_HEAD_TURN: f32 = 30.0;
/// `can_get_scared`: player within this distance moving/turning stops tempt.
pub const TEMPT_SCARE_DISTANCE: f64 = 6.0;
pub const TEMPT_SCARE_MOVE: f64 = 0.1;
pub const TEMPT_SCARE_TURN: f32 = 5.0;
/// Follow-parent search box half extents.
pub const FOLLOW_PARENT_SEARCH: (f64, f64, f64) = (8.0, 4.0, 8.0);
pub const FOLLOW_PARENT_MIN_DIST: f64 = 3.0;
pub const FOLLOW_PARENT_MAX_DIST: f64 = 16.0;
pub const FOLLOW_PARENT_REPATH_TICKS: u32 = 10;
pub const LOOK_AT_PLAYER_HEAD_TURN: f32 = 10.0;
pub const BREED_SEARCH: f64 = 8.0;
pub const BREED_CHECK_TICKS: u32 = 60;
/// Partners mate when their boxes are within this distance.
pub const BREED_TOUCH_DIST: f64 = 0.4;
pub const BREED_MAX_PARTNER_DIST: f64 = 3.0;

// --- Combat and death ---------------------------------------------------------

/// Ticks after a hit during which only greater damage applies.
pub const HURT_TICKS: u32 = 10;
pub const KNOCKBACK_HORIZONTAL: f64 = 0.4;
pub const KNOCKBACK_VERTICAL: f64 = 0.4;
pub const KNOCKBACK_VERTICAL_CAP: f64 = 0.4;
/// Existing velocity is multiplied by this before knockback is added.
pub const KNOCKBACK_RETAIN: f64 = 0.5;
/// The body is removed (and XP spawned) this many ticks after death.
pub const DEATH_TICKS: u32 = 20;
/// Experience orb sizes, largest first.
pub const XP_ORB_SIZES: [u32; 11] = [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1];

// --- Breeding / ageing --------------------------------------------------------

pub const LOVE_TICKS: u32 = 600;
/// Growth fraction per feed when `feed_items` gives none (provisional:
/// Microsoft Learn `minecraft:ageable`).
pub const AGEABLE_FEED_GROWTH: f32 = 0.1;

// --- Despawn ------------------------------------------------------------------

/// Instant-despawn distance when the chunk tick range is below 5.
pub const DESPAWN_SHORT_TICK_RANGE_DISTANCE: f64 = 44.0;

// --- Replication --------------------------------------------------------------

/// No move packet while summed rotation change is at most this (degrees) …
pub const NET_ROTATION_THRESHOLD: f32 = 4.0;
/// … and squared position change is at most this.
pub const NET_POSITION_THRESHOLD_SQ: f64 = 1.0e-4;
/// A motion packet is sent when velocity changed by more than this (squared).
pub const NET_VELOCITY_THRESHOLD_SQ: f64 = 0.04;
