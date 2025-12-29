//! Mathematical density function operations.

use super::context::{ContextProvider, FunctionContext};
use super::function::{DensityFunction, Visitor};
use std::sync::Arc;

/// Constant value density function.
///
/// Always returns the same value regardless of position.
#[derive(Debug, Clone)]
pub struct Constant {
    /// The constant value to return.
    pub value: f64,
}

impl Constant {
    /// Create a new constant density function.
    pub fn new(value: f64) -> Self {
        Self { value }
    }
}

impl DensityFunction for Constant {
    fn compute(&self, _ctx: &dyn FunctionContext) -> f64 {
        self.value
    }

    fn fill_array(&self, values: &mut [f64], _provider: &dyn ContextProvider) {
        values.fill(self.value);
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        self.value
    }

    fn max_value(&self) -> f64 {
        self.value
    }

}

/// Two-argument operation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoArgType {
    /// Addition: a + b
    Add,
    /// Multiplication: a * b
    Mul,
    /// Minimum: min(a, b)
    Min,
    /// Maximum: max(a, b)
    Max,
}

/// Two-argument mathematical operation.
///
/// Combines two density functions using a binary operation.
#[derive(Clone)]
pub struct TwoArg {
    /// The operation type.
    pub op: TwoArgType,
    /// First operand.
    pub arg1: Arc<dyn DensityFunction>,
    /// Second operand.
    pub arg2: Arc<dyn DensityFunction>,
    /// Cached minimum value.
    min_value: f64,
    /// Cached maximum value.
    max_value: f64,
}

impl TwoArg {
    /// Create a new two-argument operation.
    pub fn new(
        op: TwoArgType,
        arg1: Arc<dyn DensityFunction>,
        arg2: Arc<dyn DensityFunction>,
    ) -> Self {
        let (min_value, max_value) = Self::compute_bounds(op, &*arg1, &*arg2);
        Self {
            op,
            arg1,
            arg2,
            min_value,
            max_value,
        }
    }

    /// Compute min/max bounds based on operand ranges.
    fn compute_bounds(
        op: TwoArgType,
        arg1: &dyn DensityFunction,
        arg2: &dyn DensityFunction,
    ) -> (f64, f64) {
        let a_min = arg1.min_value();
        let a_max = arg1.max_value();
        let b_min = arg2.min_value();
        let b_max = arg2.max_value();

        match op {
            TwoArgType::Add => (a_min + b_min, a_max + b_max),
            TwoArgType::Mul => {
                // For multiplication, consider all corner cases
                let corners = [a_min * b_min, a_min * b_max, a_max * b_min, a_max * b_max];
                let min = corners.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = corners.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                (min, max)
            }
            TwoArgType::Min => (a_min.min(b_min), a_max.min(b_max)),
            TwoArgType::Max => (a_min.max(b_min), a_max.max(b_max)),
        }
    }
}

impl DensityFunction for TwoArg {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let a = self.arg1.compute(ctx);
        let b = self.arg2.compute(ctx);
        match self.op {
            TwoArgType::Add => a + b,
            TwoArgType::Mul => a * b,
            TwoArgType::Min => a.min(b),
            TwoArgType::Max => a.max(b),
        }
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_arg1 = self.arg1.map_all(visitor);
        let new_arg2 = self.arg2.map_all(visitor);
        visitor.apply(Arc::new(TwoArg::new(self.op, new_arg1, new_arg2)))
    }

    fn min_value(&self) -> f64 {
        self.min_value
    }

    fn max_value(&self) -> f64 {
        self.max_value
    }

}

/// Unary transformation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedType {
    /// Absolute value: |x|
    Abs,
    /// Square: x²
    Square,
    /// Cube: x³
    Cube,
    /// Half negative: x > 0 ? x : x * 0.5
    HalfNegative,
    /// Quarter negative: x > 0 ? x : x * 0.25
    QuarterNegative,
    /// Invert: x == 0 ? 0 : 1/x (Java: sign(x) / |x| clamped)
    Invert,
    /// Squeeze: x/2 - x³/24 (terrain squish function)
    Squeeze,
}

/// Unary transformation of a density function.
#[derive(Clone)]
pub struct Mapped {
    /// The transformation type.
    pub op: MappedType,
    /// The input function.
    pub input: Arc<dyn DensityFunction>,
    /// Cached minimum value.
    min_value: f64,
    /// Cached maximum value.
    max_value: f64,
}

impl Mapped {
    /// Create a new mapped density function.
    pub fn new(op: MappedType, input: Arc<dyn DensityFunction>) -> Self {
        let (min_value, max_value) = Self::compute_bounds(op, &*input);
        Self {
            op,
            input,
            min_value,
            max_value,
        }
    }

    /// Apply the transformation to a value.
    #[inline]
    fn apply_op(op: MappedType, d: f64) -> f64 {
        match op {
            MappedType::Abs => d.abs(),
            MappedType::Square => d * d,
            MappedType::Cube => d * d * d,
            MappedType::HalfNegative => {
                if d > 0.0 {
                    d
                } else {
                    d * 0.5
                }
            }
            MappedType::QuarterNegative => {
                if d > 0.0 {
                    d
                } else {
                    d * 0.25
                }
            }
            MappedType::Invert => {
                // Java Edition implementation returns 1/x but clamps results
                if d.abs() < 0.0001 {
                    0.0
                } else {
                    1.0 / d
                }
            }
            MappedType::Squeeze => {
                // Java formula: clamp(d, -1, 1) THEN x/2 - x³/24
                // This is critical - without the clamp, large values cause incorrect terrain
                let e = d.clamp(-1.0, 1.0);
                e * 0.5 - e.powi(3) / 24.0
            }
        }
    }

    /// Compute min/max bounds for the transformation.
    fn compute_bounds(op: MappedType, input: &dyn DensityFunction) -> (f64, f64) {
        let in_min = input.min_value();
        let in_max = input.max_value();

        match op {
            MappedType::Abs => {
                // If range spans zero, min is 0; otherwise it's the smaller absolute value
                if in_min <= 0.0 && in_max >= 0.0 {
                    (0.0, in_min.abs().max(in_max.abs()))
                } else {
                    let a = in_min.abs();
                    let b = in_max.abs();
                    (a.min(b), a.max(b))
                }
            }
            MappedType::Square => {
                let a = in_min * in_min;
                let b = in_max * in_max;
                if in_min <= 0.0 && in_max >= 0.0 {
                    (0.0, a.max(b))
                } else {
                    (a.min(b), a.max(b))
                }
            }
            MappedType::Cube => {
                (in_min.powi(3), in_max.powi(3))
            }
            MappedType::HalfNegative => {
                let out_min = if in_min > 0.0 { in_min } else { in_min * 0.5 };
                let out_max = if in_max > 0.0 { in_max } else { in_max * 0.5 };
                (out_min, out_max)
            }
            MappedType::QuarterNegative => {
                let out_min = if in_min > 0.0 { in_min } else { in_min * 0.25 };
                let out_max = if in_max > 0.0 { in_max } else { in_max * 0.25 };
                (out_min, out_max)
            }
            MappedType::Invert => {
                // Conservative bounds for invert
                (-10000.0, 10000.0)
            }
            MappedType::Squeeze => {
                // With clamp(-1, 1), squeeze output is bounded:
                // squeeze(-1) = -1/2 - (-1)³/24 = -0.5 + 1/24 ≈ -0.458
                // squeeze(1) = 1/2 - 1/24 ≈ 0.458
                // squeeze(0) = 0
                // The function is monotonic in [-1, 1]
                let clamped_min = in_min.clamp(-1.0, 1.0);
                let clamped_max = in_max.clamp(-1.0, 1.0);
                let a = clamped_min * 0.5 - clamped_min.powi(3) / 24.0;
                let b = clamped_max * 0.5 - clamped_max.powi(3) / 24.0;
                (a.min(b), a.max(b))
            }
        }
    }
}

impl DensityFunction for Mapped {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        Self::apply_op(self.op, self.input.compute(ctx))
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_input = self.input.map_all(visitor);
        visitor.apply(Arc::new(Mapped::new(self.op, new_input)))
    }

    fn min_value(&self) -> f64 {
        self.min_value
    }

    fn max_value(&self) -> f64 {
        self.max_value
    }

}

/// Clamp density function.
///
/// Clamps the input to a specified range.
#[derive(Clone)]
pub struct Clamp {
    /// The input function.
    pub input: Arc<dyn DensityFunction>,
    /// Minimum clamp value.
    pub min: f64,
    /// Maximum clamp value.
    pub max: f64,
}

impl Clamp {
    /// Create a new clamp density function.
    pub fn new(input: Arc<dyn DensityFunction>, min: f64, max: f64) -> Self {
        Self { input, min, max }
    }
}

impl DensityFunction for Clamp {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        self.input.compute(ctx).clamp(self.min, self.max)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_input = self.input.map_all(visitor);
        visitor.apply(Arc::new(Clamp::new(new_input, self.min, self.max)))
    }

    fn min_value(&self) -> f64 {
        self.input.min_value().clamp(self.min, self.max)
    }

    fn max_value(&self) -> f64 {
        self.input.max_value().clamp(self.min, self.max)
    }

}

/// Y-clamped gradient density function.
///
/// Returns a linear interpolation based on Y coordinate,
/// clamped at the boundaries.
#[derive(Debug, Clone)]
pub struct YClampedGradient {
    /// Y coordinate at which `from_value` is returned.
    pub from_y: i32,
    /// Y coordinate at which `to_value` is returned.
    pub to_y: i32,
    /// Value at `from_y`.
    pub from_value: f64,
    /// Value at `to_y`.
    pub to_value: f64,
}

impl YClampedGradient {
    /// Create a new Y-clamped gradient.
    pub fn new(from_y: i32, to_y: i32, from_value: f64, to_value: f64) -> Self {
        Self {
            from_y,
            to_y,
            from_value,
            to_value,
        }
    }
}

impl DensityFunction for YClampedGradient {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let y = ctx.block_y();
        if y <= self.from_y {
            return self.from_value;
        }
        if y >= self.to_y {
            return self.to_value;
        }
        // Linear interpolation
        let t = (y - self.from_y) as f64 / (self.to_y - self.from_y) as f64;
        self.from_value + t * (self.to_value - self.from_value)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        self.from_value.min(self.to_value)
    }

    fn max_value(&self) -> f64 {
        self.from_value.max(self.to_value)
    }

}

/// Range choice density function.
///
/// Returns `when_in_range` if `input` is within the range,
/// otherwise returns `when_out_of_range`.
#[derive(Clone)]
pub struct RangeChoice {
    /// The input function to test.
    pub input: Arc<dyn DensityFunction>,
    /// Minimum of the range (inclusive).
    pub min_inclusive: f64,
    /// Maximum of the range (exclusive).
    pub max_exclusive: f64,
    /// Function to evaluate when input is in range.
    pub when_in_range: Arc<dyn DensityFunction>,
    /// Function to evaluate when input is out of range.
    pub when_out_of_range: Arc<dyn DensityFunction>,
}

impl RangeChoice {
    /// Create a new range choice density function.
    pub fn new(
        input: Arc<dyn DensityFunction>,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Arc<dyn DensityFunction>,
        when_out_of_range: Arc<dyn DensityFunction>,
    ) -> Self {
        Self {
            input,
            min_inclusive,
            max_exclusive,
            when_in_range,
            when_out_of_range,
        }
    }
}

impl DensityFunction for RangeChoice {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let input_val = self.input.compute(ctx);
        if input_val >= self.min_inclusive && input_val < self.max_exclusive {
            self.when_in_range.compute(ctx)
        } else {
            self.when_out_of_range.compute(ctx)
        }
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_input = self.input.map_all(visitor);
        let new_in_range = self.when_in_range.map_all(visitor);
        let new_out_range = self.when_out_of_range.map_all(visitor);
        visitor.apply(Arc::new(RangeChoice::new(
            new_input,
            self.min_inclusive,
            self.max_exclusive,
            new_in_range,
            new_out_range,
        )))
    }

    fn min_value(&self) -> f64 {
        self.when_in_range
            .min_value()
            .min(self.when_out_of_range.min_value())
    }

    fn max_value(&self) -> f64 {
        self.when_in_range
            .max_value()
            .max(self.when_out_of_range.max_value())
    }

}

/// Optimized multiply-or-add operation.
///
/// If `arg2` is a constant, this becomes either `arg1 * constant` or `arg1 + constant`.
/// Otherwise it's `arg1 * arg2` or `arg1 + arg2` depending on the type.
#[derive(Clone)]
pub struct MulOrAdd {
    /// The operation type.
    pub op: TwoArgType,
    /// First operand (the variable one).
    pub arg1: Arc<dyn DensityFunction>,
    /// Second operand (often a constant for optimization).
    pub arg2: Arc<dyn DensityFunction>,
    /// Cached minimum value.
    min_value: f64,
    /// Cached maximum value.
    max_value: f64,
}

impl MulOrAdd {
    /// Create a new multiply-or-add operation.
    pub fn new(
        op: TwoArgType,
        arg1: Arc<dyn DensityFunction>,
        arg2: Arc<dyn DensityFunction>,
    ) -> Self {
        let (min_value, max_value) = TwoArg::compute_bounds(op, &*arg1, &*arg2);
        Self {
            op,
            arg1,
            arg2,
            min_value,
            max_value,
        }
    }

    /// Create an add operation.
    pub fn add(arg1: Arc<dyn DensityFunction>, arg2: Arc<dyn DensityFunction>) -> Self {
        Self::new(TwoArgType::Add, arg1, arg2)
    }

    /// Create a multiply operation.
    pub fn mul(arg1: Arc<dyn DensityFunction>, arg2: Arc<dyn DensityFunction>) -> Self {
        Self::new(TwoArgType::Mul, arg1, arg2)
    }
}

impl DensityFunction for MulOrAdd {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let a = self.arg1.compute(ctx);
        let b = self.arg2.compute(ctx);
        match self.op {
            TwoArgType::Add => a + b,
            TwoArgType::Mul => a * b,
            TwoArgType::Min => a.min(b),
            TwoArgType::Max => a.max(b),
        }
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_arg1 = self.arg1.map_all(visitor);
        let new_arg2 = self.arg2.map_all(visitor);
        visitor.apply(Arc::new(MulOrAdd::new(self.op, new_arg1, new_arg2)))
    }

    fn min_value(&self) -> f64 {
        self.min_value
    }

    fn max_value(&self) -> f64 {
        self.max_value
    }
}

/// Y coordinate density function.
///
/// Simply returns the Y coordinate of the current position.
/// Used in cave systems for range checks (e.g., only generate caves at certain Y levels).
#[derive(Debug, Clone, Copy)]
pub struct YCoord;

impl YCoord {
    /// Create a new Y coordinate function.
    pub fn new() -> Self {
        Self
    }
}

impl Default for YCoord {
    fn default() -> Self {
        Self::new()
    }
}

impl DensityFunction for YCoord {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        ctx.block_y() as f64
    }

    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider) {
        for (i, value) in values.iter_mut().enumerate() {
            let ctx = provider.for_index(i);
            *value = ctx.block_y() as f64;
        }
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(*self))
    }

    fn min_value(&self) -> f64 {
        // World Y range
        -64.0
    }

    fn max_value(&self) -> f64 {
        // World Y range
        320.0
    }
}

/// Linear interpolation helper.
#[inline]
pub fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

/// Trilinear interpolation.
#[inline]
pub fn lerp3(
    tx: f64,
    ty: f64,
    tz: f64,
    c000: f64,
    c100: f64,
    c010: f64,
    c110: f64,
    c001: f64,
    c101: f64,
    c011: f64,
    c111: f64,
) -> f64 {
    lerp(
        tz,
        lerp(ty, lerp(tx, c000, c100), lerp(tx, c010, c110)),
        lerp(ty, lerp(tx, c001, c101), lerp(tx, c011, c111)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant() {
        let c = Constant::new(5.0);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(c.compute(&ctx), 5.0);
        assert_eq!(c.min_value(), 5.0);
        assert_eq!(c.max_value(), 5.0);
    }

    #[test]
    fn test_y_gradient() {
        let g = YClampedGradient::new(0, 100, 1.0, -1.0);

        // At Y=0, should return from_value
        let ctx0 = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(g.compute(&ctx0), 1.0);

        // At Y=50, should return 0.0 (midpoint)
        let ctx50 = super::super::context::SinglePointContext::new(0, 50, 0);
        assert!((g.compute(&ctx50) - 0.0).abs() < 0.0001);

        // At Y=100, should return to_value
        let ctx100 = super::super::context::SinglePointContext::new(0, 100, 0);
        assert_eq!(g.compute(&ctx100), -1.0);

        // Below from_y should clamp
        let ctx_low = super::super::context::SinglePointContext::new(0, -10, 0);
        assert_eq!(g.compute(&ctx_low), 1.0);

        // Above to_y should clamp
        let ctx_high = super::super::context::SinglePointContext::new(0, 150, 0);
        assert_eq!(g.compute(&ctx_high), -1.0);
    }

    #[test]
    fn test_squeeze_formula() {
        // Verify squeeze matches Java: clamp(d, -1, 1) THEN x/2 - x³/24
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);

        // Test with value in range [-1, 1]
        let input_half = Arc::new(Constant::new(0.5));
        let squeeze_half = Mapped::new(MappedType::Squeeze, input_half);
        let expected_half = 0.5 / 2.0 - 0.5_f64.powi(3) / 24.0; // 0.25 - 0.00520833... ≈ 0.2448
        assert!((squeeze_half.compute(&ctx) - expected_half).abs() < 0.0001);

        // Test with value outside range - should clamp to 1.0 first
        let input_large = Arc::new(Constant::new(2.0));
        let squeeze_large = Mapped::new(MappedType::Squeeze, input_large);
        let expected_clamped = 1.0 / 2.0 - 1.0_f64.powi(3) / 24.0; // 0.5 - 0.0416... ≈ 0.4583
        assert!((squeeze_large.compute(&ctx) - expected_clamped).abs() < 0.0001);

        // Test with negative value outside range - should clamp to -1.0 first
        let input_neg = Arc::new(Constant::new(-5.0));
        let squeeze_neg = Mapped::new(MappedType::Squeeze, input_neg);
        let expected_neg = -1.0 / 2.0 - (-1.0_f64).powi(3) / 24.0; // -0.5 + 0.0416... ≈ -0.4583
        assert!((squeeze_neg.compute(&ctx) - expected_neg).abs() < 0.0001);
    }

    #[test]
    fn test_two_arg_add() {
        let a = Arc::new(Constant::new(3.0));
        let b = Arc::new(Constant::new(2.0));
        let add = TwoArg::new(TwoArgType::Add, a, b);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(add.compute(&ctx), 5.0);
    }

    #[test]
    fn test_two_arg_mul() {
        let a = Arc::new(Constant::new(3.0));
        let b = Arc::new(Constant::new(2.0));
        let mul = TwoArg::new(TwoArgType::Mul, a, b);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(mul.compute(&ctx), 6.0);
    }

    #[test]
    fn test_clamp() {
        let input = Arc::new(Constant::new(5.0));
        let clamp = Clamp::new(input, 0.0, 3.0);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(clamp.compute(&ctx), 3.0);

        let input2 = Arc::new(Constant::new(-5.0));
        let clamp2 = Clamp::new(input2, 0.0, 3.0);
        assert_eq!(clamp2.compute(&ctx), 0.0);
    }

    #[test]
    fn test_range_choice() {
        let input = Arc::new(Constant::new(0.5));
        let in_range = Arc::new(Constant::new(100.0));
        let out_range = Arc::new(Constant::new(-100.0));

        let choice = RangeChoice::new(input, 0.0, 1.0, in_range, out_range);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(choice.compute(&ctx), 100.0);

        // Test out of range
        let input2 = Arc::new(Constant::new(1.5));
        let in_range2 = Arc::new(Constant::new(100.0));
        let out_range2 = Arc::new(Constant::new(-100.0));
        let choice2 = RangeChoice::new(input2, 0.0, 1.0, in_range2, out_range2);
        assert_eq!(choice2.compute(&ctx), -100.0);
    }

    #[test]
    fn test_lerp3() {
        // Test center interpolation (all t = 0.5)
        let corners = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let center = lerp3(
            0.5, 0.5, 0.5, corners[0], corners[1], corners[2], corners[3], corners[4], corners[5],
            corners[6], corners[7],
        );
        assert!((center - 3.5).abs() < 0.001);
    }

    #[test]
    fn test_mapped_abs() {
        let input = Arc::new(Constant::new(-5.0));
        let mapped = Mapped::new(MappedType::Abs, input);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(mapped.compute(&ctx), 5.0);
    }

    #[test]
    fn test_mapped_square() {
        let input = Arc::new(Constant::new(3.0));
        let mapped = Mapped::new(MappedType::Square, input);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(mapped.compute(&ctx), 9.0);
    }

    #[test]
    fn test_mapped_half_negative() {
        let pos_input = Arc::new(Constant::new(4.0));
        let pos_mapped = Mapped::new(MappedType::HalfNegative, pos_input);
        let ctx = super::super::context::SinglePointContext::new(0, 0, 0);
        assert_eq!(pos_mapped.compute(&ctx), 4.0);

        let neg_input = Arc::new(Constant::new(-4.0));
        let neg_mapped = Mapped::new(MappedType::HalfNegative, neg_input);
        assert_eq!(neg_mapped.compute(&ctx), -2.0);
    }
}
