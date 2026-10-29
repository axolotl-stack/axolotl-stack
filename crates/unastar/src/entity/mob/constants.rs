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
/// Acceleration input while swimming (underwater movement attribute).
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
/// A projectile is spawned invisible and becomes visible on this tick of
/// its flight (the first being 1).
pub const PROJECTILE_VISIBLE_TICK: u64 = 2;
/// Level event of one `particle_on_hit` particle (data = the particle id).
pub const PROJECTILE_PARTICLE_LEVEL_EVENT: i32 = 2009;
/// Particle id of `iconcrack`; its level event data also carries the item's
/// network id shifted left by [`ICONCRACK_ITEM_SHIFT`].
pub const ICONCRACK_PARTICLE: i32 = 14;
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
