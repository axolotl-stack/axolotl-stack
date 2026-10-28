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
/// Ground jump velocity when the mob has no `minecraft:jump.static`.
pub const DEFAULT_JUMP_POWER: f32 = 0.42;
/// `movement.basic` `max_turn` when the component is absent (degrees/tick;
/// the published schema default).
pub const DEFAULT_MAX_TURN: f32 = 30.0;
/// Head yaw returns toward the body yaw at this rate (degrees/tick) while no
/// goal looks anywhere; provisional: Java Edition `LookControl`.
pub const LOOK_IDLE_HEAD_TURN: f32 = 10.0;

// --- Navigation -----------------------------------------------------------------

/// Path search range around the mob; provisional: Microsoft Learn
/// `minecraft:follow_range` default (16).
pub const PATH_MAX_RANGE: f64 = 16.0;
/// Highest drop a walk path takes; provisional: Java Edition mob maximum
/// fall distance (3).
pub const PATH_MAX_DROP: f64 = 3.0;
/// A waypoint counts as reached when the feet are within this horizontally
/// (`width / 2` for mobs wider than this, else `this - width / 2`) and within
/// [`PATH_WAYPOINT_REACH_Y`] vertically; provisional: Java Edition
/// `PathNavigation`.
pub const PATH_WAYPOINT_REACH: f64 = 0.75;
pub const PATH_WAYPOINT_REACH_Y: f64 = 1.0;
/// Every this many ticks of path following the mob must have moved at least
/// `speed' × 100 × PATH_STUCK_DISTANCE_FACTOR` blocks (`speed'` = speed when
/// ≥ 1, else speed²), or the path is dropped as stuck; provisional: Java
/// Edition `PathNavigation` stuck detection.
pub const PATH_STUCK_CHECK_TICKS: u32 = 100;
pub const PATH_STUCK_DISTANCE_FACTOR: f32 = 0.25;
/// Blocks a walk path avoids when `avoid_damage_blocks` is set; provisional:
/// Java Edition walk-node danger classes (fire, lava, magma, cactus, berry
/// bush, wither rose, lit campfires).
pub const PATH_DAMAGING_BLOCKS: &[&str] = &[
    "minecraft:fire",
    "minecraft:soul_fire",
    "minecraft:lava",
    "minecraft:flowing_lava",
    "minecraft:magma",
    "minecraft:cactus",
    "minecraft:sweet_berry_bush",
    "minecraft:wither_rose",
    "minecraft:campfire",
    "minecraft:soul_campfire",
];
/// Walk-target score of a stroll candidate standing on grass (others score
/// 0, so the first candidate on grass wins); provisional: Java Edition
/// animal walk-target value.
pub const GRASS_WALK_TARGET_SCORE: f32 = 10.0;

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
/// Head turn toward the breeding partner, degrees/tick.
pub const BREED_HEAD_TURN: f32 = 10.0;

// --- Entity pushing ---------------------------------------------------------------

/// Overlapping pushable actors are pushed apart by this much per tick along
/// the dominant horizontal axis direction; provisional: Java Edition
/// `Entity#push`.
pub const ENTITY_PUSH_STRENGTH: f64 = 0.05;
/// Player collision box (blocks), for pushing and tempt stop distance.
pub const PLAYER_BOX_WIDTH: f64 = 0.6;
pub const PLAYER_BOX_HEIGHT: f64 = 1.8;

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
/// Knockback adds `KNOCKBACK_VERTICAL × this` to the vertical velocity
/// (before the cap), so a grounded mob leaves at the 0.4 cap.
pub const KNOCKBACK_VERTICAL_SCALE: f64 = 1.2;
/// Farthest a player's eye may be from the nearest point of a mob's box to
/// land a melee hit (provisional: the client reaches 3 blocks in survival
/// and 5 in creative per the Minecraft Wiki "Reach" page; this bound adds
/// latency tolerance on top).
pub const MELEE_REACH: f64 = 6.0;
/// A mob counts as "last hit by a player" (loot `killed_by_player`,
/// `query.last_hit_by_player`) for this many ticks after a player's hit
/// (provisional: Java Edition `lastHurtByPlayerTime`).
pub const LAST_HIT_BY_PLAYER_TICKS: u64 = 100;

// --- Experience orbs (provisional: Java Edition `ExperienceOrb`; the spawn
// velocity range is consistent with the native summon capture) -------------

/// Orbs are removed this many ticks after spawning.
pub const XP_ORB_LIFETIME_TICKS: u64 = 6000;
/// Gravity subtracted before the move.
pub const XP_ORB_GRAVITY: f64 = 0.03;
/// Per-tick velocity multiplier (block friction also applies on the ground).
pub const XP_ORB_DRAG: f64 = 0.98;
/// Spawn velocity: x/z uniform in `±XP_ORB_SPAWN_HORIZONTAL`, y uniform in
/// `[0, XP_ORB_SPAWN_VERTICAL)`.
pub const XP_ORB_SPAWN_HORIZONTAL: f64 = 0.2;
pub const XP_ORB_SPAWN_VERTICAL: f64 = 0.4;
/// Orbs fly towards the nearest player within this distance …
pub const XP_ORB_FOLLOW_RANGE: f64 = 8.0;
/// … accelerating by `(1 − distance / range)² × this` per tick.
pub const XP_ORB_FOLLOW_ACCELERATION: f64 = 0.1;

// --- Breeding / ageing --------------------------------------------------------

pub const LOVE_TICKS: u32 = 600;
/// Growth fraction per feed when `feed_items` gives none (provisional:
/// Microsoft Learn `minecraft:ageable`).
pub const AGEABLE_FEED_GROWTH: f32 = 0.1;
/// Authored `ageable.duration` and `breedable.breed_cooldown` are seconds;
/// the runtime counters are ticks at this rate.
pub const TICKS_PER_SECOND: f32 = 20.0;

// --- Legacy item names --------------------------------------------------------

/// Modern item behind a legacy `bucket:<aux>` name in behaviour-pack data
/// (the cow's milking row turns `bucket:0` into `bucket:1`). Provisional:
/// Minecraft Wiki "Bucket" Bedrock data values, which pmmp's
/// BedrockItemUpgradeSchema maps to the same flattened identifiers.
pub const LEGACY_BUCKET_ITEMS: [(i16, &str); 11] = [
    (0, "minecraft:bucket"),
    (1, "minecraft:milk_bucket"),
    (2, "minecraft:cod_bucket"),
    (3, "minecraft:salmon_bucket"),
    (4, "minecraft:tropical_fish_bucket"),
    (5, "minecraft:pufferfish_bucket"),
    (8, "minecraft:water_bucket"),
    (10, "minecraft:lava_bucket"),
    (11, "minecraft:powder_snow_bucket"),
    (12, "minecraft:axolotl_bucket"),
    (13, "minecraft:tadpole_bucket"),
];

// --- Despawn ------------------------------------------------------------------

/// A chunk tick range (simulation distance, chunks) below this caps the
/// instant-despawn distance at [`DESPAWN_SHORT_TICK_RANGE_DISTANCE`].
pub const DESPAWN_SHORT_TICK_RANGE: i32 = 5;
/// Instant-despawn distance when the chunk tick range is below 5.
pub const DESPAWN_SHORT_TICK_RANGE_DISTANCE: f64 = 44.0;

// --- Replication --------------------------------------------------------------

/// No move packet while summed rotation change is at most this (degrees) …
pub const NET_ROTATION_THRESHOLD: f32 = 4.0;
/// … and squared position change is at most this.
pub const NET_POSITION_THRESHOLD_SQ: f64 = 1.0e-4;
/// A motion packet is sent when velocity changed by more than this (squared).
pub const NET_VELOCITY_THRESHOLD_SQ: f64 = 0.04;
