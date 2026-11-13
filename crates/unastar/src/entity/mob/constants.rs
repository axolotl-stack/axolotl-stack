//! Built-in mob behaviour constants that behaviour-pack data does not carry.
//!
//! Every value here describes how the Bedrock Dedicated Server behaves; the
//! `docs/research/mob-runtime-constants.md` lists each one with its status ("confirmed
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
/// Speed cap while the wanted position is below the feet; a faster mob drops
/// to it at once (a tempted cow at 0.3125, also with Speed II at 0.4375,
/// accelerates at the capped speed from its first tick above a step it jumps
/// onto, and a panicking cow from its first tick in the air after a
/// knockback: BDS 1.26.51 knockback captures).
pub const MOVE_DESCEND_CAP: f32 = 0.25;
/// The travel step's forward input is the move control's speed × this
/// (BDS 1.26.51: a walking cow at speed `s` gains `0.98 s²` per tick on
/// grass, through every step of its 10 %/tick speed ramp).
pub const MOVE_INPUT_DECAY: f32 = 0.98;
/// Vertical velocity of a living mob with gravity when it spawns or is born
/// (80 of the 112 BDS 1.26.51 summon captures: every living actor with
/// gravity except the shulker; without gravity, or not living, 0). It is the
/// `AddActor` velocity and the mob's first move.
pub const MOB_SPAWN_VELOCITY_Y: f64 = -1.0e-5;
/// Acceleration per unit of forward input of a mob in the air (BDS 1.26.51:
/// a cow at speed `s` gains `0.02 × 0.98 s` per tick in the air, not the
/// ground acceleration).
pub const MOB_AIR_ACCELERATION: f64 = 0.02;
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
/// A waypoint on the mob's block level counts as reached once the feet are
/// horizontally closer to it than `max(width / 2, PATH_WAYPOINT_REACH)`; the
/// follower then moves past every reached waypoint of that level ahead of
/// the current one. Confirmed against BDS (26.30).
pub const PATH_WAYPOINT_REACH: f64 = 0.6;
/// Walk path search order: an open node's estimate is the distance walked
/// to it plus this multiple of its straight distance to the goal, over side
/// and diagonal steps (a diagonal only between two sides reachable without
/// climbing). Confirmed against BDS (26.30).
pub const PATH_ESTIMATE_WEIGHT: f64 = 1.5;
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
/// Push per unit of forward input in water of a mob without an
/// `minecraft:underwater_movement` attribute row (the native default).
pub const WATER_MOVE_SPEED: f32 = 0.02;
/// Native chicken wing-flap drag: travelling through air with a negative
/// vertical velocity after gravity and drag, that velocity is multiplied by
/// `1 - WING_FLAP_VERTICAL_DRAG` each tick.
pub const WING_FLAP_VERTICAL_DRAG: f64 = 0.4;
/// `FALL_DAMAGE_MULTIPLIER` actor data of the native chicken class: the
/// native adult chicken summon carries 0 where the cow's carries 1, so a
/// chicken's fall damage is always 0 before its damage sensor is consulted.
pub const CHICKEN_FALL_DAMAGE_MULTIPLIER: f32 = 0.0;
/// `AMBIENT_SOUND_INTERVAL` actor data (seconds) of a mob without
/// `minecraft:ambient_sound_interval`: the native animal classes start at
/// 6, every other class at 8 (measured from the retained 1.26.51 summons).
pub const ANIMAL_AMBIENT_SOUND_INTERVAL: f32 = 6.0;
pub const DEFAULT_AMBIENT_SOUND_INTERVAL: f32 = 8.0;

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

// --- Hostile mobs (target goals, melee) -------------------------------------------

/// Follow range of a mob without `behavior.nearest_attackable_target`'s
/// `within_radius` (confirmed).
pub const DEFAULT_FOLLOW_RANGE: f32 = 16.0;
/// `behavior.hurt_by_target` reacts to an attacker that hurt the mob within
/// this many ticks (confirmed).
pub const HURT_BY_TARGET_MEMORY_TICKS: u64 = 60;
/// Each melee tick the path to the target is recalculated early with this
/// probability (confirmed).
pub const MELEE_REPATH_CHANCE: f32 = 0.05;
/// A mob's eye sits at this fraction of its box height, for line-of-sight
/// checks; provisional: Java Edition default eye height.
pub const MOB_EYE_HEIGHT_RATIO: f64 = 0.85;
/// `must_reach` re-checks reachability every `BASE + random(0..SPREAD)`
/// ticks (confirmed) …
pub const TARGET_REACH_CHECK_BASE: i32 = 10;
pub const TARGET_REACH_CHECK_SPREAD: i32 = 5;
/// … and counts the target reachable when the path ends within this squared
/// horizontal block distance of it (confirmed).
pub const TARGET_REACH_DIST_SQ: f64 = 3.0;
/// An invisible target is seen within the entry's distance × the goal's
/// `target_invisible_multiplier` × the fraction of its armour slots worn,
/// that fraction being at least this (confirmed).
pub const INVISIBLE_MIN_ARMOR_COVER: f32 = 0.1;
/// A player wearing the head of the targeting mob's kind is seen within
/// this fraction of the entry's distance (confirmed; wither skeletons are
/// not fooled, confirmed) …
pub const DISGUISED_PLAYER_RANGE_FACTOR: f32 = 0.5;
/// … the head worn for each mob (provisional: Java Edition
/// `LivingEntity.getVisibilityPercent`).
pub const MOB_HEAD_DISGUISES: [(&str, &str); 5] = [
    ("minecraft:zombie", "minecraft:zombie_head"),
    ("minecraft:skeleton", "minecraft:skeleton_skull"),
    ("minecraft:creeper", "minecraft:creeper_head"),
    ("minecraft:piglin", "minecraft:piglin_head"),
    ("minecraft:piglin_brute", "minecraft:piglin_head"),
];
/// `behavior.stomp_attack`: the mob rears up (`STANDING`) this many ticks
/// before its cooldown runs out while its target is in the no-damage range
/// (measured: BDS 1.26.51 polar bears stood ten ticks before every hit).
pub const STOMP_WARNING_TICKS: i32 = 10;

/// `behavior.melee_box_attack` `can_spread_on_fire`: a burning attacker with
/// an empty main hand sets the target alight with probability
/// `U(0, 1) < this[difficulty]` (peaceful, easy, normal, hard) for
/// `2 × (int)this[difficulty]` seconds (confirmed).
pub const MELEE_FIRE_SPREAD_CHANCE: [f32; 4] = [0.0, 0.42, 1.14, 1.92];
/// A melee hit's damage is raised once per Strength level by
/// `d × SCALE + ADD` … (confirmed)
pub const STRENGTH_DAMAGE_SCALE: f32 = 1.3;
pub const STRENGTH_DAMAGE_ADD: f32 = 1.0;
/// … and lowered once per Weakness level by `d × SCALE + ADD`, a negative
/// result dealing nothing (confirmed).
pub const WEAKNESS_DAMAGE_SCALE: f32 = 0.8;
pub const WEAKNESS_DAMAGE_ADD: f32 = -0.5;

// --- Fire (`mob::combat::fire`) ---------------------------------------------------

/// `minecraft:burns_in_daylight` is checked on every tick that is a
/// multiple of this (confirmed).
pub const DAYLIGHT_BURN_CHECK_TICKS: u64 = 2;
/// Mobs only catch fire in daylight while the sky darkening is at most this
/// (confirmed).
pub const DAYLIGHT_BURN_MAX_SKY_DARKEN: u8 = 3;
/// The light deciding daylight burning is read at this fraction of the
/// mob's height above its feet … (confirmed)
pub const DAYLIGHT_BURN_LIGHT_HEIGHT: f64 = 0.66;
/// … and its brightness ([`OVERWORLD_BRIGHTNESS_RAMP`]) must exceed this
/// (confirmed; raw light 12 is the first level above it).
pub const DAYLIGHT_BURN_MIN_BRIGHTNESS: f32 = 0.5;
/// A mob catching fire in daylight burns this long (8 s, confirmed).
pub const DAYLIGHT_BURN_TICKS: u64 = 160;
/// A mob whose box touches this block does not catch fire in daylight
/// (the check is confirmed; the block is provisional: Java Edition, where
/// powder snow keeps undead from burning).
pub const DAYLIGHT_BURN_BLOCKER: &str = "minecraft:powder_snow";
/// The damage causes an actor with the `FIRE_IMMUNE` flag
/// (`minecraft:fire_immune`) does not take (measured: `/damage` with these
/// fails on the fire-immune zombie pigman and wither skeleton and succeeds on
/// the zombie and skeleton; every other cause, campfire and soul campfire
/// included, hurts all four; `testdata/bds-1.26.51-fire-immune-causes.json`).
pub const FIRE_IMMUNE_CAUSES: &[unastar_data::actor_components::body::ActorDamageCause] = {
    use unastar_data::actor_components::body::ActorDamageCause as Cause;
    &[Cause::Fire, Cause::FireTick, Cause::Lava, Cause::Magma]
};

// --- Entity pushing ---------------------------------------------------------------

/// A pushed actor moves this much per tick along the vector away from its
/// pusher, scaled by `1 / sqrt(max(|dx|, |dz|))` capped at 1.
pub const ENTITY_PUSH_STRENGTH: f64 = 0.05;
/// An actor pushes the actors whose boxes touch its own box grown by this
/// much on x and z (not on y).
pub const ENTITY_PUSH_REACH: f64 = 0.2;
/// Actors whose positions are closer than this on both horizontal axes do
/// not push each other.
pub const ENTITY_PUSH_MIN_DISTANCE: f64 = 0.1;
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
/// A native zombie spawns with a knockback resistance drawn uniformly from
/// `[0, this)`, which scales the push and the lift of every knockback it
/// takes (five BDS 1.26.51 zombies: 0.0150, 0.0306, 0.0434, 0.0472, 0.0493;
/// the 1,160 zombies, husks, drowned and zombified piglins of the spawn-bonus
/// sweep all saved one in `[0.0001, 0.0499]`).
pub const ZOMBIE_KNOCKBACK_RESISTANCE_BONUS: f64 = 0.05;
/// The zombie classes' follow-range bonus draw: uniform below this, kept
/// only above [`FOLLOW_RANGE_BONUS_THRESHOLD`] (BDS 1.26.51 spawn-bonus
/// sweep: 403 of 1,160 saved one, amounts 1.0014 to 1.4997;
/// `mob::attributes`).
pub const FOLLOW_RANGE_BONUS_DRAW_MAX: f32 = 1.5;
/// See [`FOLLOW_RANGE_BONUS_DRAW_MAX`].
pub const FOLLOW_RANGE_BONUS_THRESHOLD: f32 = 1.0;
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

// --- Fall damage --------------------------------------------------------------

/// Blocks a mob falls without damage (also the ground-dust threshold).
pub const FALL_SAFE_DISTANCE: f64 = 3.0;
/// The landing block is the one this far below the feet.
pub const FALL_LANDING_PROBE: f64 = 0.2;
/// Fall damage above this plays `fall.big`, else `fall.small`.
pub const FALL_BIG_SOUND_DAMAGE: f64 = 4.0;
/// Landing blocks that scale the fall distance before the damage formula
/// (the stored fall distance resets after every landing either way).
pub const FALL_LANDING_DISTANCE_SCALES: [(&str, f64); 6] = [
    ("minecraft:hay_block", 0.2),
    ("minecraft:honey_block", 0.5),
    ("minecraft:bed", 0.5),
    ("minecraft:slime", 0.0),
    ("minecraft:powder_snow", 0.0),
    ("minecraft:end_portal", 0.0),
];
/// Landing on an upward pointed dripstone tip adds this to the distance …
pub const STALAGMITE_EXTRA_DISTANCE: f64 = 2.0;
/// … and multiplies the damage (cause stalagmite).
pub const STALAGMITE_MULTIPLIER: f64 = 2.0;
/// Landing blocks that raise no ground dust.
pub const FALL_DUSTLESS_BLOCKS: [&str; 3] = [
    "minecraft:air",
    "minecraft:barrier",
    "minecraft:invisible_bedrock",
];

// --- Projectiles (`minecraft:projectile`) ---------------------------------------

/// Each tick a projectile's rotation moves this fraction of the way to the
/// direction of its velocity.
pub const PROJECTILE_ROTATION_SMOOTHING: f32 = 0.2;
/// Target boxes grow by `(ticks in flight - DELAY) × STEP`, clamped to
/// `[0, MAX]`, for the entity hit test.
pub const PROJECTILE_HIT_INFLATE_DELAY: f64 = 2.0;
pub const PROJECTILE_HIT_INFLATE_STEP: f64 = 0.05;
pub const PROJECTILE_HIT_INFLATE_MAX: f64 = 0.3;
/// A projectile is spawned invisible and becomes visible at the end of this
/// tick of its flight (the first being 1).
pub const PROJECTILE_VISIBLE_TICK: u64 = 2;
/// Synchronised level sounds of an actor turning invisible and visible
/// again, played at the centre of its box with data -1 (BDS 1.26.51 sends
/// them for every projectile; only the wandering trader's sound set gives
/// them a sound).
pub const DISAPPEARED_SOUND: &str = "disappeared";
pub const REAPPEARED_SOUND: &str = "reappeared";
/// `grant_xp`: the potion-splash level event at the hit …
pub const POTION_SPLASH_LEVEL_EVENT: i32 = 2002;
/// … with the default potion colour (ARGB `0xFF385DC6`) as its data: BDS
/// 1.26.51 sends data -13083194 where an xp bottle breaks.
pub const DEFAULT_POTION_COLOUR_ARGB: i32 = 0xFF38_5DC6_u32 as i32;
/// Level event of one particle (data = the particle id): a projectile's
/// `particle_on_hit`, an interaction's `particle_on_start`.
pub const PROJECTILE_PARTICLE_LEVEL_EVENT: i32 = 2009;
/// Particle id of `iconcrack`; its level event data also carries the item's
/// network id shifted left by [`ICONCRACK_ITEM_SHIFT`].
pub const ICONCRACK_PARTICLE: i32 = 14;
/// The `snowballpoof` particle id: BDS 1.26.51 sends six
/// `LevelEvent(2009, data 15)` where a snowball hits (client-diff capture
/// `item-use-20261029`).
pub const SNOWBALLPOOF_PARTICLE: i32 = 15;
pub const ICONCRACK_ITEM_SHIFT: u32 = 16;
/// Level sound of a player throwing a projectile item.
pub const THROW_SOUND: &str = "throw";
/// The throw sound plays at the player's drop attach point, this far below
/// the eyes. Provisional: the height thrown item drops spawn at.
pub const THROW_SOUND_EYE_DROP: f64 = 0.5;
/// Items that throw a `minecraft:egg` actor when used (a native item
/// class, not item components), with the egg's climate variant.
pub const THROWN_EGG_ITEMS: [(&str, &str); 3] = [
    ("minecraft:egg", "temperate"),
    ("minecraft:brown_egg", "warm"),
    ("minecraft:blue_egg", "cold"),
];
/// The egg actor property the thrown item sets.
pub const CLIMATE_VARIANT_PROPERTY: &str = "minecraft:climate_variant";

// --- Experience orbs (provisional: Java Edition `ExperienceOrb`; the spawn
// velocity range is consistent with the native summon capture) -------------

/// Orbs are removed this many ticks after spawning.
pub const XP_ORB_LIFETIME_TICKS: u64 = 6000;
/// Gravity subtracted before the move (BDS 1.26.51: an orb summoned with an
/// upward velocity of 0.1204 rose 0.0804 in its first tick, then
/// `0.0804 × 0.98 − 0.04`; `net/testdata/bds-1.26.51-replication.json`,
/// trial `xp_orb|fall-32|natural`).
pub const XP_ORB_GRAVITY: f64 = 0.04;
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
/// `ActorEvent` Feed data: the fed item's network id shifted left by this,
/// over its aux value.
pub const FEED_EVENT_ITEM_SHIFT: u32 = 16;
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
/// Legacy loot item names that stand for one modern item; matched without
/// regard to case, as item names are. Provisional: pmmp's
/// BedrockItemUpgradeSchema, which agrees with the identity BDS reports for
/// its own `muttonRaw` furnace recipe input.
pub const LEGACY_ITEM_NAMES: [(&str, &str); 1] = [("minecraft:muttonRaw", "minecraft:mutton")];
/// The legacy loot item name that picks its wool item by data value
/// (the palette colour): `minecraft:wool` with data 14 is red wool.
pub const LEGACY_WOOL: &str = "minecraft:wool";

// --- Palette colours ------------------------------------------------------------

/// Wool item of each palette colour, by colour index (0 white … 15 black).
/// Indices 0, 1, 3, 4, 6, 7, 8, 9, 11, 12, 14 and 15 follow from the colour
/// the vanilla sheep definition gives each named colour group; 2 (magenta),
/// 5 (lime), 10 (purple) and 13 (green) are provisional: Minecraft Wiki
/// "Wool" Bedrock data values.
pub const WOOL_ITEMS: [&str; 16] = [
    "minecraft:white_wool",
    "minecraft:orange_wool",
    "minecraft:magenta_wool",
    "minecraft:light_blue_wool",
    "minecraft:yellow_wool",
    "minecraft:lime_wool",
    "minecraft:pink_wool",
    "minecraft:gray_wool",
    "minecraft:light_gray_wool",
    "minecraft:cyan_wool",
    "minecraft:purple_wool",
    "minecraft:blue_wool",
    "minecraft:brown_wool",
    "minecraft:green_wool",
    "minecraft:red_wool",
    "minecraft:black_wool",
];
/// Dye item of each palette colour, by colour index. Using a dye on an
/// `is_dyeable` actor sets its colour to `15 − the dye's item colour`, which
/// is the index of the colour the dye is named after (the dye item colours
/// follow the legacy dye data values, provisional: Minecraft Wiki "Dye").
/// Ink sacs, lapis lazuli, cocoa beans and bone meal only count as dyes when
/// a world feature this server does not enable is on, so they do not dye.
pub const DYE_ITEMS: [&str; 16] = [
    "minecraft:white_dye",
    "minecraft:orange_dye",
    "minecraft:magenta_dye",
    "minecraft:light_blue_dye",
    "minecraft:yellow_dye",
    "minecraft:lime_dye",
    "minecraft:pink_dye",
    "minecraft:gray_dye",
    "minecraft:light_gray_dye",
    "minecraft:cyan_dye",
    "minecraft:purple_dye",
    "minecraft:blue_dye",
    "minecraft:brown_dye",
    "minecraft:green_dye",
    "minecraft:red_dye",
    "minecraft:black_dye",
];

// --- Interaction ----------------------------------------------------------------

/// An interaction row's `spawn_items` drop at the mob's feet raised by the
/// row's `y_offset`, with the pop velocity of a dropped item plus
/// `U(0, 1) × LIFT` upwards and `(U − U) × SPREAD` on x and z.
pub const INTERACT_SPAWN_ITEM_LIFT: f64 = 0.05;
pub const INTERACT_SPAWN_ITEM_SPREAD: f64 = 0.1;

/// `particle_on_start` particle ids: BDS 1.26.51 sends
/// `LevelEvent(2009, data 5)` when a brown mooshroom eats a flower
/// (`smoke`) and data 16 when a mooshroom is sheared (`largeexplode`)
/// (`sweep.py mooshroom`).
pub const SMOKE_PARTICLE: i32 = 5;
pub const LARGEEXPLODE_PARTICLE: i32 = 16;
/// An interaction's particle appears at the middle of the mob's box raised
/// by the row's `particle_y_offset`. With `particle_offset_towards_interactor`
/// it moves this far towards the interacting player's eyes, along the
/// direction from [`INTERACT_PARTICLE_AIM_LIFT`] above that point. Measured
/// on BDS 1.26.51 mooshrooms (y offset 0.25) within 1e-5: an adult level
/// with the player, an adult two blocks diagonally below a raised player
/// and a baby.
pub const INTERACT_PARTICLE_TOWARDS: f64 = 1.0;
pub const INTERACT_PARTICLE_AIM_LIFT: f64 = 0.1;

// --- Eating blocks (`minecraft:behavior.eat_block`) -----------------------------

/// Level event the eaten block's destroy particles use (at the block centre,
/// data = the eaten block's runtime id).
pub const LEVEL_EVENT_PARTICLES_DESTROY_BLOCK: i32 = 2001;
/// Legacy block names in `eat_and_replace_block_pairs`, resolved to the
/// block they name today. Provisional: pmmp's BedrockBlockUpgradeSchema
/// (`grass` was renamed `grass_block`; `tallgrass` split into `short_grass`
/// and `fern`, its default state being short grass).
pub const LEGACY_BLOCK_NAMES: [(&str, &str); 2] = [
    ("minecraft:grass", "minecraft:grass_block"),
    ("minecraft:tallgrass", "minecraft:short_grass"),
];
/// Controls `behavior.eat_block` holds when it authors no `control_flags`:
/// move, look and jump.
pub(crate) const EAT_BLOCK_DEFAULT_CONTROLS: crate::entity::goal_selector::GoalControls =
    crate::entity::goal_selector::GoalControls::MOVE
        .union(crate::entity::goal_selector::GoalControls::LOOK)
        .union(crate::entity::goal_selector::GoalControls::JUMP);

// --- Doors (`minecraft:annotation.break_door`, door path nodes) ------------------

/// A block state with this property is a door half (wooden, copper or iron).
pub const DOOR_HINGE_STATE: &str = "door_hinge_bit";
/// The door state property that holds whether the door is open.
pub const DOOR_OPEN_STATE: &str = "open_bit";
/// The door state property set on the upper half.
pub const DOOR_UPPER_STATE: &str = "upper_block_bit";
/// The only door a door-breaking mob never breaks (confirmed: every other
/// door block qualifies).
pub const IRON_DOOR_BLOCK: &str = "minecraft:iron_door";
/// While remembering a door it is not breaking, a breaker restarts with
/// probability 1 in this per tick (confirmed).
pub const BREAK_DOOR_RESTART_ROLL: u32 = 7;
/// Each breaking tick plays the door bash sound with probability 1 in this
/// (confirmed).
pub const BREAK_DOOR_BASH_ROLL: u32 = 20;
/// `LevelEvent` StartBlockCracking data is `(1 / break ticks) × this`,
/// truncated (confirmed).
pub const BLOCK_CRACK_DATA_SCALE: f32 = 65535.0;
/// `LevelEvent` ids the door breaker sends at the door cell's centre
/// (confirmed values; names provisional: gophertunnel `packet.LevelEvent*`).
pub const LEVEL_EVENT_START_BLOCK_CRACKING: i32 = 3600;
pub const LEVEL_EVENT_STOP_BLOCK_CRACKING: i32 = 3601;
pub const LEVEL_EVENT_SOUND_ZOMBIE_WOODEN_DOOR: i32 = 1010;
pub const LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH: i32 = 1012;
/// Destroy particles without the break sound; data is the broken block's
/// default state runtime ID.
pub const LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND: i32 = 2021;

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

// --- Natural spawning (`entity::spawning`) --------------------------------------

/// Each tick every ticking chunk rolls `nextInt(SPAWN_TICK_ROLL)`; a spawn
/// attempt runs when the roll is at most [`SPAWN_TICK_ROLL_MAX`] (11 in 2000).
pub const SPAWN_TICK_ROLL: i32 = 2000;
pub const SPAWN_TICK_ROLL_MAX: i32 = 10;
/// Mobs are counted for the population caps in the square of chunks within
/// this many chunks of the spawning chunk (9 × 9).
pub const SPAWN_CENSUS_RADIUS_CHUNKS: i32 = 4;
/// A cluster never takes the spawnable mob count past this many.
pub const SPAWN_GLOBAL_MOB_LIMIT: u32 = 200;
/// Population cap per category in the census area for mobs spawned on the
/// surface, indexed by [`crate::entity::spawning::Category`] (animal,
/// monster, water_animal, villager, ambient, cat, pillager). A cap of 0
/// never spawns.
pub const MOBS_PER_CHUNK_SURFACE: [f32; 7] = [4.0, 8.0, 36.0, 0.0, 0.0, 4.0, 8.0];
/// The same caps for mobs spawned underground.
pub const MOBS_PER_CHUNK_UNDERGROUND: [f32; 7] = [4.0, 16.0, 36.0, 0.0, 2.0, 0.0, 8.0];
/// Naturally spawned mobs stand at the spawn block's corner plus this on x
/// and z (worlds of base game version 1.21.0 or later).
pub const SPAWN_POSITION_OFFSET: f64 = 0.49;
/// `minecraft:weight` when a condition has none.
pub const DEFAULT_SPAWN_WEIGHT: u32 = 100;
/// `minecraft:distance_filter` when a condition has none: no player closer
/// than `min`, the nearest player within `max` blocks.
pub const DEFAULT_SPAWN_DISTANCE_MIN: i32 = 24;
pub const DEFAULT_SPAWN_DISTANCE_MAX: i32 = 128;
/// An authored `distance_filter.max` is clamped to this (`min` to ≥ 0).
pub const SPAWN_DISTANCE_CAP: i32 = 128;
/// From this chunk tick range up, a spawn needs its chunk and the 8 around
/// it ticking. With exactly [`SPAWN_SHORT_TICK_RANGE`] the neighbours are not
/// checked but the player distance is capped at
/// [`SPAWN_SHORT_TICK_RANGE_DISTANCE`]; below that nothing spawns.
pub const SPAWN_FULL_TICK_RANGE: i32 = 5;
pub const SPAWN_SHORT_TICK_RANGE: i32 = 4;
pub const SPAWN_SHORT_TICK_RANGE_DISTANCE: i32 = 44;
/// Block material ordinals (`material.type`) the spawn block search treats
/// as open space: air, water, lava and bubble columns.
pub const SPAWN_OPEN_MATERIALS: [u8; 4] = [0, 5, 6, 18];
/// Material ordinals of water and lava (in-liquid spawn conditions).
pub const MATERIAL_WATER: u8 = 5;
pub const MATERIAL_LAVA: u8 = 6;
/// Material ordinal of ice, which counts as a spawn block although it is not
/// a solid block.
pub const MATERIAL_ICE: u8 = 13;
/// Material ordinal of leaves: a spawn column whose top spawn block is
/// leaves keeps its next spawn block below on the surface.
pub const MATERIAL_LEAVES: u8 = 7;
// --- Flee sun (`behavior.flee_sun`, confirmed) --------------------------------

/// Candidate cover blocks drawn per attempt to start …
pub const FLEE_SUN_ATTEMPTS: u32 = 10;
/// … each `random(2 × range) - range` from the feet block on x/z and y.
pub const FLEE_SUN_RANGE_XZ: i32 = 10;
pub const FLEE_SUN_RANGE_Y: i32 = 3;

/// The overworld brightness ramp: the brightness each raw light level
/// `0..=15` stands for, `(1 - f) / (3f + 1)` with `f = 1 - level / 15`, as
/// the f32 values the game stores (confirmed).
pub const OVERWORLD_BRIGHTNESS_RAMP: [f32; 16] = [
    f32::from_bits(0x0000_0000),
    f32::from_bits(0x3c8f_b823),
    f32::from_bits(0x3d17_b426),
    f32::from_bits(0x3d70_f0f0),
    f32::from_bits(0x3daa_aaaa),
    f32::from_bits(0x3de3_8e3b),
    f32::from_bits(0x3e12_4923),
    f32::from_bits(0x3e37_cb7e),
    f32::from_bits(0x3e63_8e3b),
    f32::from_bits(0x3e8b_a2ea),
    f32::from_bits(0x3eaa_aaab),
    f32::from_bits(0x3ed0_97b5),
    f32::from_bits(0x3f00_0001),
    f32::from_bits(0x3f1e_79e8),
    f32::from_bits(0x3f47_1c71),
    f32::from_bits(0x3f80_0000),
];
/// A plain mob's walk-target value everywhere (confirmed). A naturally
/// spawned mob whose walk-target value at its feet is negative is refused.
pub const MOB_WALK_TARGET: f32 = 0.0;
/// A monster's walk-target value is this minus the brightness ramp at the
/// block (confirmed).
pub const MONSTER_WALK_TARGET_BASE: f32 = 0.5;
/// An animal's walk-target value with a grass block below (confirmed).
pub const ANIMAL_GRASS_WALK_TARGET: f32 = 10.0;
/// An animal's walk-target value elsewhere is the brightness ramp at the
/// block plus this (confirmed).
pub const ANIMAL_WALK_TARGET_OFFSET: f32 = -0.5;
/// An animal's or monster's walk-target value above a magma block, unless
/// it is fire immune (a grass block below an animal wins; confirmed).
pub const MAGMA_WALK_TARGET: f32 = -0.5;
pub const MAGMA_BLOCK: &str = "minecraft:magma";
/// A naturally spawned monster needs raw brightness below this at its feet
/// (confirmed).
pub const MONSTER_SPAWN_BRIGHTNESS_LIMIT: u8 = 8;
/// Ground no mob without fire immunity spawns on (confirmed).
pub const SPAWN_UNSAFE_GROUND: [&str; 3] = [
    "minecraft:magma",
    "minecraft:campfire",
    "minecraft:soul_campfire",
];
/// Blocks no mob spawns with its feet in: rails, buttons, pressure plates,
/// levers, tripwire hooks, campfires and the redstone components refuse
/// spawns (confirmed).
pub const SPAWN_REFUSING_BLOCKS: [&str; 48] = [
    "minecraft:acacia_button",
    "minecraft:acacia_pressure_plate",
    "minecraft:activator_rail",
    "minecraft:bamboo_button",
    "minecraft:bamboo_pressure_plate",
    "minecraft:birch_button",
    "minecraft:birch_pressure_plate",
    "minecraft:campfire",
    "minecraft:cherry_button",
    "minecraft:cherry_pressure_plate",
    "minecraft:crimson_button",
    "minecraft:crimson_pressure_plate",
    "minecraft:dark_oak_button",
    "minecraft:dark_oak_pressure_plate",
    "minecraft:detector_rail",
    "minecraft:golden_rail",
    "minecraft:heavy_weighted_pressure_plate",
    "minecraft:jungle_button",
    "minecraft:jungle_pressure_plate",
    "minecraft:lever",
    "minecraft:light_weighted_pressure_plate",
    "minecraft:mangrove_button",
    "minecraft:mangrove_pressure_plate",
    "minecraft:pale_oak_button",
    "minecraft:pale_oak_pressure_plate",
    "minecraft:polished_blackstone_button",
    "minecraft:polished_blackstone_pressure_plate",
    "minecraft:poplar_button",
    "minecraft:poplar_pressure_plate",
    "minecraft:powered_comparator",
    "minecraft:powered_repeater",
    "minecraft:rail",
    "minecraft:redstone_block",
    "minecraft:redstone_torch",
    "minecraft:redstone_wire",
    "minecraft:soul_campfire",
    "minecraft:spruce_button",
    "minecraft:spruce_pressure_plate",
    "minecraft:stone_button",
    "minecraft:stone_pressure_plate",
    "minecraft:tripwire_hook",
    "minecraft:unlit_redstone_torch",
    "minecraft:unpowered_comparator",
    "minecraft:unpowered_repeater",
    "minecraft:warped_button",
    "minecraft:warped_pressure_plate",
    "minecraft:wooden_button",
    "minecraft:wooden_pressure_plate",
];
/// A mob that the wither effect can affect does not spawn with its feet in
/// this block (confirmed).
pub const SPAWN_WITHER_ROSE: &str = "minecraft:wither_rose";

// --- Environment filters (`mob::events`) ---------------------------------------

/// `is_underwater`: the actor's breathing point (a mob's sits at
/// [`MOB_EYE_HEIGHT_RATIO`] of its height above the feet, a player's at the
/// eye) is under water when the block holding it is water and the point is
/// below `block_y + 1 - (level - 1) / UNDERWATER_LEVEL_STEPS`, where `level`
/// is `liquid_depth + 1` (falling water, depth 8 and up, counts as level 1,
/// a full block) (confirmed; the mob breathing-point height is provisional,
/// see [`MOB_EYE_HEIGHT_RATIO`]).
pub const UNDERWATER_LEVEL_STEPS: f64 = 9.0;

// --- Equipment (`mob::gear`, `minecraft:behavior.pickup_items`) -----------------

/// The only item a mob holds in the off hand when it equips a picked-up
/// item; every other held item goes to the main hand (confirmed).
pub const SHIELD_ITEM: &str = "minecraft:shield";
/// Death drop chance of an equipment slot without an authored
/// `slot_drop_chance` (confirmed).
pub const GEAR_DEFAULT_DROP_CHANCE: f32 = 0.25;
/// Drop chance added per Looting level of the killer's weapon (confirmed).
pub const GEAR_LOOTING_DROP_BONUS: f32 = 0.01;
/// A non-guaranteed damageable equipment drop is worn down to wear
/// (`Damage`) `max_damage - r`, `r` uniform in `0..=k` with `k` uniform in
/// `0..max(max_damage, this) - 3`, so it keeps `r` durability (confirmed).
pub const GEAR_DROP_MIN_MAX_DAMAGE: i32 = 4;
/// Bedrock's Curse of Binding enchantment id: armour carrying it is never
/// swapped for a picked-up piece (confirmed).
pub const BINDING_CURSE_ID: i16 = 27;
/// `MobEquipment` container id of a mob's off hand in the inventory send
/// (confirmed).
pub const GEAR_OFFHAND_CONTAINER: u8 = 119;
/// `MobEquipment` container id of an armour slot equipped by
/// `behavior.equip_item` (confirmed; its selected-slot field, sent equal to
/// the slot, is provisional: no capture of the packet was retained).
pub const GEAR_ARMOUR_CONTAINER: u8 = 120;
/// `pickup_based_on_chance`: the mob may pick items up when a spawn-time
/// roll is below this times the clamped regional difficulty (confirmed).
pub const PICKUP_LOOT_CHANCE: f32 = 0.55;
/// Ticks before `behavior.pickup_items` first re-paths (confirmed).
pub const PICKUP_REPATH_START_TICKS: i32 = 10;
/// Re-path delay: uniform `0..PICKUP_REPATH_RANDOM` plus a base by the
/// squared distance to the item (confirmed).
pub const PICKUP_REPATH_RANDOM: i32 = 7;
pub const PICKUP_REPATH_NEAR_TICKS: i32 = 4;
pub const PICKUP_REPATH_MID_TICKS: i32 = 9;
pub const PICKUP_REPATH_FAR_TICKS: i32 = 14;
/// Squared distances beyond which the mid / far bases apply (confirmed).
pub const PICKUP_REPATH_MID_DIST_SQ: f64 = 256.0;
pub const PICKUP_REPATH_FAR_DIST_SQ: f64 = 1024.0;
/// Extra delay after a failed re-path (confirmed).
pub const PICKUP_REPATH_FAIL_TICKS: i32 = 15;
/// Head turn speed toward the item, degrees per tick (confirmed: 30, the
/// same both axes).
pub const PICKUP_HEAD_TURN: f32 = 30.0;

// --- Ranged attacks (`mob::ai::ranged`, `mob::combat::ranged`) ---------------------

/// Held items `has_ranged_weapon` counts (measured: a BDS 1.26.51 skeleton
/// whose hand is replaced keeps shooting with a bow only; with a crossbow,
/// trident, snowball, stick or nothing it never shoots and its ranged goal
/// never raises `FACING_TARGET_TO_RANGE_ATTACK`, `sweep.py ranged --items
/// weapon`).
pub const RANGED_WEAPONS: [&str; 1] = ["minecraft:bow"];
/// A shot waits one tick more than its `attack_interval`: BDS 1.26.51
/// skeletons, strays, bogged and parched shoot every 61 / 71 ticks on easy
/// and normal and every 41 / 51 on hard (intervals 3 / 3.5 / 2 / 2.5 s),
/// the first shot one tick later still after the target is taken
/// (measured: `sweep.py ranged`, every difficulty).
pub const RANGED_RELOAD_EXTRA_TICKS: i32 = 1;
/// The projectile starts this far from the shooter's feet, horizontally,
/// toward the target's feet (measured: the BDS 1.26.51 off-axis, diagonal
/// and raised shots land on it to 1e-6) …
pub const RANGED_SHOT_FORWARD: f64 = 1.0;
/// … at this fraction of the shooter's height above its feet, plus the
/// projectile's own `offset` (measured: arrows of the 1.9-tall skeleton
/// family start 1.61 above the feet with the arrow's -0.1 offset).
pub const RANGED_SHOT_EYE_RATIO: f64 = 0.9;
/// The projectile is created at the shooter's feet with the centre of its
/// box this far up, and only then moved to its start: its `disappeared`
/// sound plays there (measured on the 1.9-tall skeleton family).
pub const RANGED_SHOT_CREATED_CENTRE: f64 = 0.9;
// A shot aims at the centre of the target's box along the drag-free
// parabola of the projectile's power and gravity, measured from the
// shooter's horizontal position (`combat::ranged::aim_direction`; measured:
// BDS 1.26.51 hard shots at a pinned player 4 to 12 blocks away and up to 6
// blocks above or below the skeleton fit it with chi²/dof 0.44 over 8
// geometries and 121 shots).
/// Each axis of a shot's unit direction gets a gaussian times this times
/// the projectile's uncertainty (measured: the BDS 1.26.51 spread is
/// 0.0070 to 0.0080 per uncertainty on easy, normal and hard, gaussian:
/// kurtosis 3.2 over 91 shots, past the bound of a triangle draw).
pub const RANGED_SPREAD_SCALE: f64 = 0.0075;
/// `difficulty_randomization: multiplicative` (a mob's arrows): the speed
/// damage multiplier is scaled by `1 + this × difficulty` (easy 1 … hard
/// 3) …
pub const SHOT_DAMAGE_PER_DIFFICULTY: f32 = 0.05;
/// … plus a uniform draw within ± this (measured: 291 BDS 1.26.51 skeleton
/// arrow hits on a pinned player, the hit damage `ceil(speed × 2 × f)`
/// fitted by maximum likelihood: mean factor 1.035 / 1.095 / 1.155 on easy
/// / normal / hard, uniform width 0.45; a gaussian of sd 0.13 fits 1.5 log
/// units worse, so the shape of the draw is provisional).
pub const SHOT_DAMAGE_SPREAD: f32 = 0.225;
/// Tipped arrows mobs shoot, by aux value (potion id + 1): the effect a hit
/// gives, level 0, for an eighth of the potion's duration (measured: BDS
/// 1.26.51 stray, bogged and parched hits on the sweep player sent these
/// `MobEffect` adds on every difficulty).
pub const TIPPED_ARROW_EFFECTS: [(u8, crate::entity::components::EffectType, u32); 3] = [
    // Long slowness (stray).
    (19, crate::entity::components::EffectType::SLOWNESS, 600),
    // Poison (bogged).
    (26, crate::entity::components::EffectType::POISON, 112),
    // Weakness (parched).
    (35, crate::entity::components::EffectType::WEAKNESS, 225),
];
/// A shot plays this unnamed sound (no actor) at the centre of the
/// shooter's box, before the shooter's own `sound` (measured: BDS 1.26.51
/// sends `undefined` with an empty actor identifier on every shot).
pub const RANGED_SHOT_SOUND: &str = "undefined";

// --- Silverfish (`mob::ai::silverfish`, `mob::life::infested`) ---------------------

/// The merge goal tries to merge on one roll in this many per tick it may
/// start, then draws one of the feet block's six faces (BDS 1.26.51: 101
/// silverfish in 1 × 1 pits with five host faces waited 8.9 ticks on
/// average; the most likely whole roll is 8, with 9 close and 10 about two
/// standard errors off).
pub const SILVERFISH_MERGE_ROLL: u32 = 8;
/// Ticks from the wake-up goal's start to its first scan, and between
/// scans (BDS 1.26.51: hurt on tick 40, blocks broke on 60, 80, 100, …).
pub const SILVERFISH_WAKE_INTERVAL_TICKS: u32 = 20;
/// How far the wake-up scan reaches along x and z (BDS 1.26.51: blocks 10
/// away broke, 11 and 12 away never did).
pub const SILVERFISH_WAKE_RANGE_XZ: i32 = 10;
/// How far the wake-up scan reaches along y (BDS 1.26.51: 5 above broke, 6
/// above never did).
pub const SILVERFISH_WAKE_RANGE_Y: i32 = 5;
/// A silverfish let out of an infested block stands at the block's corner
/// moved this far along x (BDS 1.26.51: every released silverfish's
/// `AddActor`).
pub const SILVERFISH_RELEASE_OFFSET_X: f64 = -0.2;

// --- Slimes and magma cubes (`mob::ai::hop`, `mob::life::split`) ------------------

/// A dying slime or magma cube bigger than the smallest size leaves this
/// many children at least and at most (BDS 1.26.51, `sweep.py slime
/// --items split`: 18, 26 and 16 of 60 splits left 2, 3 and 4 children, and
/// none any other number).
pub const SPLIT_MIN_CHILDREN: u32 = 2;
pub const SPLIT_MAX_CHILDREN: u32 = 4;
/// Split children stand this far above the floor of the block the body
/// ended in, at its centre on x and z (BDS 1.26.51: every child `AddActor`
/// of the split sweeps).
pub const SPLIT_CHILD_RISE: f64 = 0.5;
/// A chunk's slime-chunk generator is seeded with
/// `chunk_x × SLIME_CHUNK_SEED_MULTIPLIER ^ chunk_z`, and the chunk is a
/// slime chunk when its first output is a multiple of
/// [`SLIME_CHUNK_CHANCE`] (`entity::spawning::slime_chunk`; every slime of
/// the BDS 1.26.51 `sweep.py slime-spawn` runs stood in such a chunk).
pub const SLIME_CHUNK_SEED_MULTIPLIER: u32 = 0x1f1f_1f1f;
pub const SLIME_CHUNK_CHANCE: u32 = 10;
/// The slime's native spawn check passes in a slime chunk only for feet
/// below this height (provisional: the superflat floor the spawn sweep
/// measured is far below it; the bound itself is not measured).
pub const SLIME_CHUNK_MAX_FEET_Y: i32 = 40;
/// A magma cube's jump impulse grows by this per size step (`VARIANT`) above
/// the authored `jump.static` power (BDS 1.26.51, `sweep.py slime --items
/// hop`: first-tick rises 0.52, 0.62 and 0.82 for sizes 1, 2 and 4).
pub const MAGMA_CUBE_JUMP_PER_SIZE: f64 = 0.1;
/// `CLIENT_EVENT` values a slime class sends: on the tick it leaves the
/// ground, and on the tick it lands (BDS 1.26.51: every hop of the hop
/// sweeps; 0 in the summon).
pub const SLIME_CLIENT_EVENT_LEFT_GROUND: i8 = 2;
pub const SLIME_CLIENT_EVENT_LANDED: i8 = 1;
/// The magma cube class starts with this `FALL_DAMAGE_MULTIPLIER` (BDS
/// 1.26.51: the native magma cube summon; its definition's fall damage
/// sensor deals none either way).
pub const MAGMA_CUBE_FALL_DAMAGE_MULTIPLIER: f32 = 0.0;

// --- Flyers (`mob::ai::fly`, the bat) -------------------------------------------

/// The bat class's vertical air drag is this times the 0.91 every axis of a
/// float-navigating flyer keeps (BDS 1.26.51, `sweep.py bat`: the bat's
/// vertical motion kept 0.546 a tick on every tick it did not push).
pub const BAT_FLIGHT_VERTICAL_FACTOR: f64 = 0.6;
/// The chance per tick that a flying bat under a solid block starts resting
/// (BDS 1.26.51, `sweep.py bat --items low`: 21 bats in a room one block
/// tall settled after 3 to 288 ticks, and two had not after 280; the rate
/// they fit is about 1 in 90).
pub const BAT_REST_CHANCE: f32 = 0.01;
/// A resting bat wakes when a player's eyes come closer than this to its
/// feet (BDS 1.26.51, `sweep.py bat --items wake,canopy`).
pub const BAT_WAKE_RANGE: f64 = 4.0;
// --- The vex (`mob::ai::vex`) -----------------------------------------------

/// The vex's move control pushes movement x speed multiplier x this a tick
/// toward its point (BDS 1.26.51, `sweep.py vex`: 0.0125 while it wanders,
/// 0.05 while it charges, with `minecraft:movement` 1).
pub const VEX_PUSH_SCALE: f64 = 0.05;
/// The speed multiplier of the vex's native wander (0.0125 / 0.05).
pub const VEX_WANDER_SPEED: f64 = 0.25;
/// The chance per tick that a vex with no point starts a wander (BDS
/// 1.26.51: 44 waits after an arrival averaged 7.6 samples, from 1 to 35).
pub const VEX_WANDER_CHANCE: f32 = 1.0 / 7.0;
/// A wander point is the centre of a block within this many blocks of the
/// vex's block on x and z (BDS 1.26.51: every measured point within 7).
pub const VEX_WANDER_XZ: i32 = 7;
/// ... and on y (every measured point within 5).
pub const VEX_WANDER_Y: i32 = 5;
/// The vex's move control lets its velocity drop to this share on the tick
/// it arrives (BDS 1.26.51: 0.455 = 0.5 x 0.91 of the last move).
pub const VEX_ARRIVAL_SLOWDOWN: f64 = 0.5;
/// A charging vex steers again at its target's eyes while their feet are
/// within this distance (3 blocks).
pub const VEX_CHARGE_STEER_DIST: f64 = 3.0;
/// A charging vex flies at this far below its target's eyes (BDS 1.26.51:
/// 1.52 above a player's feet in 33 charges, `sweep.py vex --items charge`).
pub const VEX_CHARGE_AIM_DROP: f64 = 0.1;

/// The bat class starts with this `FALL_DAMAGE_MULTIPLIER` (BDS 1.26.51:
/// the native bat summon; its definition's fall damage sensor deals none
/// either way).
pub const BAT_FALL_DAMAGE_MULTIPLIER: f32 = 0.0;
