//! Cubic spline density functions.
//!
//! Splines are used to create smooth terrain transitions based on
//! climate parameters like continentalness, erosion, and ridges.

use super::context::FunctionContext;
use super::function::{DensityFunction, Visitor};
use std::sync::Arc;

/// Value at a spline point - either constant or nested spline.
#[derive(Clone)]
pub enum SplineValue {
    /// A constant value.
    Constant(f64),
    /// A nested spline that depends on another coordinate.
    Nested(Arc<Spline>),
}

impl SplineValue {
    /// Evaluate the spline value at a context.
    pub fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        match self {
            SplineValue::Constant(v) => *v,
            SplineValue::Nested(spline) => spline.compute(ctx),
        }
    }

    /// Get minimum value.
    pub fn min_value(&self) -> f64 {
        match self {
            SplineValue::Constant(v) => *v,
            SplineValue::Nested(spline) => spline.min_value(),
        }
    }

    /// Get maximum value.
    pub fn max_value(&self) -> f64 {
        match self {
            SplineValue::Constant(v) => *v,
            SplineValue::Nested(spline) => spline.max_value(),
        }
    }
}

/// A point on a spline curve.
#[derive(Clone)]
pub struct SplinePoint {
    /// The coordinate location of this point.
    pub location: f64,
    /// The value at this point.
    pub value: SplineValue,
    /// The derivative (slope) at this point for Hermite interpolation.
    pub derivative: f64,
}

impl SplinePoint {
    /// Create a new spline point with constant value.
    pub fn new(location: f64, value: f64, derivative: f64) -> Self {
        Self {
            location,
            value: SplineValue::Constant(value),
            derivative,
        }
    }

    /// Create a new spline point with nested spline.
    pub fn nested(location: f64, spline: Arc<Spline>, derivative: f64) -> Self {
        Self {
            location,
            value: SplineValue::Nested(spline),
            derivative,
        }
    }
}

/// Cubic spline density function.
///
/// Evaluates a cubic Hermite spline based on a coordinate function.
/// Used for terrain offset, factor, and jaggedness.
#[derive(Clone)]
pub struct Spline {
    /// The function that provides the coordinate value.
    pub coordinate: Arc<dyn DensityFunction>,
    /// The spline points.
    pub points: Vec<SplinePoint>,
    /// Cached minimum value.
    min_value: f64,
    /// Cached maximum value.
    max_value: f64,
}

impl Spline {
    /// Create a new spline.
    pub fn new(coordinate: Arc<dyn DensityFunction>, points: Vec<SplinePoint>) -> Self {
        let (min_value, max_value) = Self::compute_bounds(&points);
        Self {
            coordinate,
            points,
            min_value,
            max_value,
        }
    }

    /// Create a simple linear spline with two points.
    pub fn linear(
        coordinate: Arc<dyn DensityFunction>,
        from_loc: f64,
        from_val: f64,
        to_loc: f64,
        to_val: f64,
    ) -> Self {
        let slope = (to_val - from_val) / (to_loc - from_loc);
        Self::new(
            coordinate,
            vec![
                SplinePoint::new(from_loc, from_val, slope),
                SplinePoint::new(to_loc, to_val, slope),
            ],
        )
    }

    /// Compute min/max bounds from points.
    fn compute_bounds(points: &[SplinePoint]) -> (f64, f64) {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;

        for point in points {
            min = min.min(point.value.min_value());
            max = max.max(point.value.max_value());
        }

        // Account for possible overshoot from derivatives
        // This is conservative - actual range might be smaller
        (min - 1.0, max + 1.0)
    }

    /// Find the index of the point just before the given coordinate.
    fn find_index(&self, coord: f64) -> usize {
        // Binary search for the interval
        let mut low = 0;
        let mut high = self.points.len();

        while low < high {
            let mid = (low + high) / 2;
            if self.points[mid].location <= coord {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        low.saturating_sub(1)
    }

    /// Evaluate the spline at a coordinate.
    fn evaluate_at(&self, coord: f64, ctx: &dyn FunctionContext) -> f64 {
        if self.points.is_empty() {
            return 0.0;
        }

        if self.points.len() == 1 {
            return self.points[0].value.compute(ctx);
        }

        // Clamp to endpoints
        if coord <= self.points[0].location {
            return self.points[0].value.compute(ctx);
        }
        if coord >= self.points.last().unwrap().location {
            return self.points.last().unwrap().value.compute(ctx);
        }

        let i = self.find_index(coord);
        let j = i + 1;

        if j >= self.points.len() {
            return self.points[i].value.compute(ctx);
        }

        let p0 = &self.points[i];
        let p1 = &self.points[j];

        // Compute t (normalized position between points)
        let loc0 = p0.location;
        let loc1 = p1.location;
        let t = (coord - loc0) / (loc1 - loc0);

        // Get values at endpoints
        let v0 = p0.value.compute(ctx);
        let v1 = p1.value.compute(ctx);

        // Get derivatives (scaled by interval length)
        let m0 = p0.derivative * (loc1 - loc0);
        let m1 = p1.derivative * (loc1 - loc0);

        // Hermite cubic interpolation
        hermite_cubic(t, v0, v1, m0, m1)
    }
}

impl DensityFunction for Spline {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let coord = self.coordinate.compute(ctx);
        self.evaluate_at(coord, ctx)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_coordinate = self.coordinate.map_all(visitor);
        let new_points: Vec<SplinePoint> = self
            .points
            .iter()
            .map(|p| SplinePoint {
                location: p.location,
                value: match &p.value {
                    SplineValue::Constant(v) => SplineValue::Constant(*v),
                    SplineValue::Nested(spline) => {
                        // Map the nested spline's coordinate
                        let new_coord = spline.coordinate.map_all(visitor);
                        SplineValue::Nested(Arc::new(Spline::new(
                            new_coord,
                            spline.points.clone(),
                        )))
                    }
                },
                derivative: p.derivative,
            })
            .collect();
        visitor.apply(Arc::new(Spline::new(new_coordinate, new_points)))
    }

    fn min_value(&self) -> f64 {
        self.min_value
    }

    fn max_value(&self) -> f64 {
        self.max_value
    }
}

/// Hermite cubic interpolation.
///
/// Given two endpoint values (v0, v1) and their derivatives (m0, m1),
/// interpolates smoothly at parameter t (0 to 1).
#[inline]
fn hermite_cubic(t: f64, v0: f64, v1: f64, m0: f64, m1: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;

    // Hermite basis functions
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;

    h00 * v0 + h10 * m0 + h01 * v1 + h11 * m1
}

/// Builder for creating complex splines.
pub struct SplineBuilder {
    coordinate: Arc<dyn DensityFunction>,
    points: Vec<SplinePoint>,
}

impl SplineBuilder {
    /// Create a new spline builder.
    pub fn new(coordinate: Arc<dyn DensityFunction>) -> Self {
        Self {
            coordinate,
            points: Vec::new(),
        }
    }

    /// Add a constant point.
    pub fn add(mut self, location: f64, value: f64, derivative: f64) -> Self {
        self.points.push(SplinePoint::new(location, value, derivative));
        self
    }

    /// Add a point with nested spline.
    pub fn add_nested(mut self, location: f64, spline: Arc<Spline>, derivative: f64) -> Self {
        self.points.push(SplinePoint::nested(location, spline, derivative));
        self
    }

    /// Build the spline.
    pub fn build(self) -> Spline {
        Spline::new(self.coordinate, self.points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::context::SinglePointContext;
    use crate::world::generator::density::math::{Constant, YClampedGradient};

    #[test]
    fn test_hermite_cubic_endpoints() {
        // At t=0, should return v0
        assert!((hermite_cubic(0.0, 10.0, 20.0, 1.0, 1.0) - 10.0).abs() < 0.001);
        // At t=1, should return v1
        assert!((hermite_cubic(1.0, 10.0, 20.0, 1.0, 1.0) - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_hermite_cubic_midpoint() {
        // With zero derivatives, should be linear
        let mid = hermite_cubic(0.5, 0.0, 10.0, 0.0, 0.0);
        assert!((mid - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_spline_constant() {
        let coord = Arc::new(Constant::new(0.0)); // Midpoint coord
        let spline = Spline::new(
            coord,
            vec![
                SplinePoint::new(-1.0, 10.0, 0.0),
                SplinePoint::new(1.0, 20.0, 0.0),
            ],
        );

        let ctx = SinglePointContext::new(0, 0, 0);
        let value = spline.compute(&ctx);
        // At coord=0.0 (midpoint between -1 and 1), should be ~15
        assert!((value - 15.0).abs() < 1.0);
    }

    #[test]
    fn test_spline_clamp_below() {
        let coord = Arc::new(Constant::new(-2.0)); // Below range
        let spline = Spline::new(
            coord,
            vec![
                SplinePoint::new(-1.0, 10.0, 0.0),
                SplinePoint::new(1.0, 20.0, 0.0),
            ],
        );

        let ctx = SinglePointContext::new(0, 0, 0);
        assert_eq!(spline.compute(&ctx), 10.0); // Should clamp to first point
    }

    #[test]
    fn test_spline_clamp_above() {
        let coord = Arc::new(Constant::new(2.0)); // Above range
        let spline = Spline::new(
            coord,
            vec![
                SplinePoint::new(-1.0, 10.0, 0.0),
                SplinePoint::new(1.0, 20.0, 0.0),
            ],
        );

        let ctx = SinglePointContext::new(0, 0, 0);
        assert_eq!(spline.compute(&ctx), 20.0); // Should clamp to last point
    }

    #[test]
    fn test_spline_builder() {
        let coord = Arc::new(Constant::new(0.0));
        let spline = SplineBuilder::new(coord)
            .add(-1.0, 0.0, 0.5)
            .add(0.0, 5.0, 0.0)
            .add(1.0, 10.0, 0.5)
            .build();

        assert_eq!(spline.points.len(), 3);
    }

    #[test]
    fn test_spline_y_gradient() {
        // Use Y position as spline coordinate
        let coord = Arc::new(YClampedGradient::new(0, 256, -1.0, 1.0));
        let spline = Spline::new(
            coord,
            vec![
                SplinePoint::new(-1.0, 100.0, 0.0),
                SplinePoint::new(0.0, 50.0, 0.0),
                SplinePoint::new(1.0, 0.0, 0.0),
            ],
        );

        let ctx_bottom = SinglePointContext::new(0, 0, 0);
        let ctx_mid = SinglePointContext::new(0, 128, 0);
        let ctx_top = SinglePointContext::new(0, 256, 0);

        // At Y=0 (coord=-1), value should be ~100
        assert!((spline.compute(&ctx_bottom) - 100.0).abs() < 5.0);
        // At Y=128 (coord=0), value should be ~50
        assert!((spline.compute(&ctx_mid) - 50.0).abs() < 5.0);
        // At Y=256 (coord=1), value should be ~0
        assert!((spline.compute(&ctx_top) - 0.0).abs() < 5.0);
    }
}
