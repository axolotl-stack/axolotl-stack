//! Melee damage as BDS 1.26.51 deals it, for every hit a player makes (on a
//! mob or on another player). The caller looks up the held item's measured
//! damage (`unastar-data`'s melee table) and its Sharpness level.

/// Damage Sharpness adds per level, before rounding down.
pub const SHARPNESS_DAMAGE_PER_LEVEL: f32 = 1.25;

/// A player's melee damage: the held item's measured damage (the bare hand's
/// without one) plus `⌊1.25 × sharpness⌋`. Measured on BDS 1.26.51: a
/// diamond sword (8) with Sharpness I-V deals 9, 10, 11, 13 and 14 to a pig
/// (attack sweep) and to a player (PvP sweep), not 9.25 ... 14.25.
pub fn melee_damage(item_damage: f32, sharpness: u8) -> f32 {
    item_damage + (SHARPNESS_DAMAGE_PER_LEVEL * f32::from(sharpness)).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharpness_adds_whole_points_like_bds() {
        let hits: Vec<f32> = (0..=5).map(|level| melee_damage(8.0, level)).collect();
        assert_eq!(hits, [8.0, 9.0, 10.0, 11.0, 13.0, 14.0]);
        assert_eq!(melee_damage(1.0, 0), 1.0);
    }
}
