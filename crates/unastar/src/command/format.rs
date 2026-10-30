//! How BDS writes numbers into command feedback parameters.

use glam::DVec3;

/// A coordinate or other decimal as BDS prints it (`0.50`, `100.00`).
pub fn decimal(value: f64) -> String {
    format!("{value:.2}")
}

/// A position as BDS prints it (`0.50, 100.00, 0.50`), e.g. in
/// `commands.tp.successVictim`.
pub fn position(p: DVec3) -> String {
    format!("{}, {}, {}", decimal(p.x), decimal(p.y), decimal(p.z))
}

/// A position's three components as separate parameters, e.g. for
/// `commands.tp.success.coordinates`.
pub fn position_params(p: DVec3) -> [String; 3] {
    [decimal(p.x), decimal(p.y), decimal(p.z)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_match_the_bds_capture() {
        // `/tp 0 100 0`: "0.50, 100.00, 0.50".
        assert_eq!(position(DVec3::new(0.5, 100.0, 0.5)), "0.50, 100.00, 0.50");
        assert_eq!(
            position_params(DVec3::new(10.5, 80.0, 10.5)),
            ["10.50", "80.00", "10.50"]
        );
    }
}
