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
/// Ticks a fire or soul fire block sets the player alight for, 8 seconds
/// (confirmed); a longer fire already burning is kept.
pub const FIRE_BLOCK_IGNITE_TICKS: u64 = 160;
/// Ticks lava sets the player alight for (provisional: Minecraft Wiki
/// "Lava", 15 seconds).
pub const LAVA_IGNITE_TICKS: u64 = 300;
/// A burning player takes [`BURN_DAMAGE`] every this many ticks
/// (provisional: Java Edition `Entity.baseTick`).
pub const BURN_INTERVAL_TICKS: u64 = 20;
/// Damage of one burn tick, dealt as `fire_tick` (the cause is confirmed;
/// the amount is provisional: Java Edition `Entity.baseTick`).
pub const BURN_DAMAGE: f32 = 1.0;
