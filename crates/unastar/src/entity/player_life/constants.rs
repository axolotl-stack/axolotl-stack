//! Native constants of player damage, death and respawn. Each value says
//! whether it was confirmed against BDS or is provisional, with its public
//! source.

/// A fall death of more than this many blocks is reported as
/// `death.fell.accident.generic`, a shorter one as `death.attack.fall`
/// (confirmed).
pub const FALL_ACCIDENT_DISTANCE: f64 = 5.0;

/// A player whose position (its eyes) is more than this many blocks below
/// the world's minimum height is in the void (confirmed).
pub const VOID_DEPTH: f64 = 16.0;
/// Void damage per tick, under the hurt timer; it also resets the fall
/// (confirmed).
pub const VOID_DAMAGE: f32 = 4.0;

/// Damage per tick of standing in fire and soul fire, under the hurt timer
/// (confirmed).
pub const BURNING_BLOCKS: [(&str, f32); 2] =
    [("minecraft:fire", 1.0), ("minecraft:soul_fire", 2.0)];
/// The block whose overlap resets a player's fall (confirmed: BDS 1.26.51,
/// 10 blocks into 2-deep powder snow did no damage).
pub const POWDER_SNOW: &str = "minecraft:powder_snow";
/// A glide that descends less than this in a tick restarts the fall from
/// 1 block (provisional: Java Edition's limit; BDS 1.26.51 confirms that a
/// 30-block glide slowed to 0.28 a tick lands unhurt).
pub const GLIDE_SAFE_DESCENT: f64 = 0.5;
/// Protection points per Feather Falling level against fall damage, and
/// the share of damage one point takes off (confirmed together: BDS
/// 1.26.51, level IV boots took a 10-block fall from 7 to 3.64).
pub const FEATHER_FALLING_POINTS: f32 = 3.0;
pub const PROTECTION_POINT: f32 = 0.04;
/// Ticks a fire or soul fire block sets the player alight for, 8 seconds
/// (confirmed); a longer fire already burning is kept.
pub const FIRE_BLOCK_IGNITE_TICKS: u64 = 160;
/// Ticks lava sets the player alight for (provisional: Minecraft Wiki
/// "Lava", 15 seconds).
pub const LAVA_IGNITE_TICKS: u64 = 300;
/// A burning player takes [`BURN_DAMAGE`] whenever its remaining fire time
/// is a multiple of this many ticks (confirmed: BDS 1.26.51, seven burns
/// 20 ticks apart after leaving a fire block, the last 20 ticks before the
/// fire went out).
pub const BURN_INTERVAL_TICKS: u64 = 20;
/// Damage of one burn tick, dealt as `fire_tick` (confirmed: each burn of
/// the BDS 1.26.51 capture took 1 health).
pub const BURN_DAMAGE: f32 = 1.0;

/// Damage of one drowning hit (confirmed: each of the ten hits in the BDS
/// 1.26.51 breath capture took 2 health).
pub const DROWNING_DAMAGE: f32 = 2.0;
