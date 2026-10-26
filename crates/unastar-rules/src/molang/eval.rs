//! Runtime Molang evaluator.
//!
//! Values are `f32` numbers (Molang is single precision; booleans are 1/0 and
//! truthiness is `!= 0`), strings, and entity references produced by context
//! variables such as `context.other`. Queries must be bound
//! ([`Expr::bind`]) before evaluation; the caller answers them through
//! [`MolangContext`], so no name lookup happens per evaluation. `math.sin` and
//! `math.cos` take degrees, as in Mojang's reference.

use super::{BinOp, Expr, MathFn, QueryId, Scope, UnaryOp};
use std::sync::Arc;

/// A Molang value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f32),
    Str(Arc<str>),
    /// An entity handle owned by the caller (`context.other`, `this`).
    Actor(u64),
}

impl Value {
    pub fn num(&self) -> f32 {
        match self {
            Self::Num(n) => *n,
            Self::Str(_) | Self::Actor(_) => 0.0,
        }
    }

    pub fn truthy(&self) -> bool {
        self.num() != 0.0
    }

    fn from_bool(b: bool) -> Self {
        Self::Num(if b { 1.0 } else { 0.0 })
    }
}

/// What the evaluator needs from its caller.
pub trait MolangContext {
    /// Answer a bound query with evaluated arguments; `None` when the query
    /// has no value in this context (evaluates to 0).
    fn query(&mut self, id: QueryId, args: &[Value]) -> Option<Value>;
    /// A `variable.*` value of the current entity.
    fn variable(&self, name: &str) -> Option<Value>;
    /// Store a `variable.*` value on the current entity.
    fn set_variable(&mut self, name: &str, value: Value);
    /// A `context.*` value supplied by the caller.
    fn context(&self, name: &str) -> Option<Value>;
    /// `this`.
    fn this(&self) -> Value;
    /// Switch the current entity for `a -> b`; `false` when the handle is stale
    /// (the arrow then yields 0).
    fn enter(&mut self, actor: u64) -> bool;
    /// Leave the entity entered last.
    fn leave(&mut self);
    /// A uniform random number in `[0, 1)`.
    fn random(&mut self) -> f32;
}

/// Evaluation error (unbound query, loop limit).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    #[error("query.{0} was not bound")]
    Unbound(Arc<str>),
    #[error("loop count {0} exceeds the limit of 1024")]
    LoopLimit(i64),
}

/// Mojang's documented upper bound for `loop` iterations.
const LOOP_LIMIT: i64 = 1024;

/// Non-local control flow, unwound through expressions (`cond ? break`).
enum Ctl {
    Err(EvalError),
    Return(Value),
    Break,
    Continue,
}

impl From<EvalError> for Ctl {
    fn from(e: EvalError) -> Self {
        Self::Err(e)
    }
}

struct Frame {
    temps: Vec<(Arc<str>, Value)>,
}

/// Evaluate `expr`.
pub fn evaluate(expr: &Expr, ctx: &mut dyn MolangContext) -> Result<Value, EvalError> {
    let mut frame = Frame { temps: Vec::new() };
    match value(expr, ctx, &mut frame) {
        Ok(v) => Ok(match expr {
            // A complex expression without `return` yields 0.
            Expr::Program(_) => Value::Num(0.0),
            _ => v,
        }),
        Err(Ctl::Return(v)) => Ok(v),
        Err(Ctl::Break | Ctl::Continue) => Ok(Value::Num(0.0)),
        Err(Ctl::Err(e)) => Err(e),
    }
}

/// Evaluate and coerce to a number.
pub fn evaluate_num(expr: &Expr, ctx: &mut dyn MolangContext) -> Result<f32, EvalError> {
    evaluate(expr, ctx).map(|v| v.num())
}

fn value(expr: &Expr, ctx: &mut dyn MolangContext, frame: &mut Frame) -> Result<Value, Ctl> {
    Ok(match expr {
        Expr::Num(n) => Value::Num(*n),
        Expr::Str(s) => Value::Str(s.clone()),
        Expr::This => ctx.this(),
        Expr::Var(scope, name) => lookup(*scope, name, ctx, frame).unwrap_or(Value::Num(0.0)),
        Expr::Query(q, args) => {
            let id =
                q.id.ok_or_else(|| Ctl::Err(EvalError::Unbound(q.name.clone())))?;
            let args = args
                .iter()
                .map(|a| value(a, ctx, frame))
                .collect::<Result<Vec<_>, _>>()?;
            ctx.query(id, &args).unwrap_or(Value::Num(0.0))
        }
        Expr::Math(f, args) => {
            let mut nums = [0.0f32; 3];
            for (slot, arg) in nums.iter_mut().zip(args) {
                *slot = value(arg, ctx, frame)?.num();
            }
            Value::Num(math(*f, nums, ctx))
        }
        Expr::Unary(op, e) => {
            let v = value(e, ctx, frame)?;
            match op {
                UnaryOp::Neg => Value::Num(-v.num()),
                UnaryOp::Not => Value::from_bool(!v.truthy()),
            }
        }
        Expr::Binary(BinOp::And, a, b) => {
            Value::from_bool(value(a, ctx, frame)?.truthy() && value(b, ctx, frame)?.truthy())
        }
        Expr::Binary(BinOp::Or, a, b) => {
            Value::from_bool(value(a, ctx, frame)?.truthy() || value(b, ctx, frame)?.truthy())
        }
        Expr::Binary(op, a, b) => {
            let (a, b) = (value(a, ctx, frame)?, value(b, ctx, frame)?);
            binary(*op, &a, &b)
        }
        Expr::Ternary(c, a, b) => {
            if value(c, ctx, frame)?.truthy() {
                value(a, ctx, frame)?
            } else {
                value(b, ctx, frame)?
            }
        }
        Expr::Conditional(c, a) => {
            if value(c, ctx, frame)?.truthy() {
                value(a, ctx, frame)?
            } else {
                Value::Num(0.0)
            }
        }
        Expr::Coalesce(a, b) => match a.as_ref() {
            Expr::Var(scope, name) => match lookup(*scope, name, ctx, frame) {
                Some(v) => v,
                None => value(b, ctx, frame)?,
            },
            other => value(other, ctx, frame)?,
        },
        Expr::Arrow(target, body) => match value(target, ctx, frame)? {
            Value::Actor(actor) if ctx.enter(actor) => {
                let result = value(body, ctx, frame);
                ctx.leave();
                result?
            }
            _ => Value::Num(0.0),
        },
        Expr::Assign(scope, name, e) => {
            let v = value(e, ctx, frame)?;
            match scope {
                Scope::Variable => ctx.set_variable(name, v.clone()),
                Scope::Temp => match frame.temps.iter_mut().find(|(n, _)| n == name) {
                    Some(slot) => slot.1 = v.clone(),
                    None => frame.temps.push((name.clone(), v.clone())),
                },
                Scope::Context => unreachable!("the parser rejects assignments to context.*"),
            }
            v
        }
        Expr::Block(stmts) | Expr::Program(stmts) => {
            for stmt in stmts {
                value(stmt, ctx, frame)?;
            }
            Value::Num(0.0)
        }
        Expr::Return(e) => return Err(Ctl::Return(value(e, ctx, frame)?)),
        Expr::Break => return Err(Ctl::Break),
        Expr::Continue => return Err(Ctl::Continue),
        Expr::Loop(count, body) => {
            let n = value(count, ctx, frame)?.num() as i64;
            if n > LOOP_LIMIT {
                return Err(Ctl::Err(EvalError::LoopLimit(n)));
            }
            for _ in 0..n.max(0) {
                match value(body, ctx, frame) {
                    Ok(_) | Err(Ctl::Continue) => {}
                    Err(Ctl::Break) => break,
                    Err(other) => return Err(other),
                }
            }
            Value::Num(0.0)
        }
    })
}

fn lookup(scope: Scope, name: &str, ctx: &mut dyn MolangContext, frame: &Frame) -> Option<Value> {
    match scope {
        Scope::Variable => ctx.variable(name),
        Scope::Temp => frame
            .temps
            .iter()
            .find(|(n, _)| &**n == name)
            .map(|(_, v)| v.clone()),
        Scope::Context => ctx.context(name),
    }
}

fn binary(op: BinOp, a: &Value, b: &Value) -> Value {
    if let (Value::Str(x), Value::Str(y)) = (a, b) {
        return match op {
            BinOp::Eq => Value::from_bool(x == y),
            BinOp::Ne => Value::from_bool(x != y),
            _ => Value::Num(0.0),
        };
    }
    if matches!((a, b), (Value::Str(_), _) | (_, Value::Str(_))) {
        // A string compared with a number never matches.
        return Value::from_bool(op == BinOp::Ne);
    }
    let (x, y) = (a.num(), b.num());
    Value::Num(match op {
        BinOp::Add => x + y,
        BinOp::Sub => x - y,
        BinOp::Mul => x * y,
        BinOp::Div => x / y,
        BinOp::Lt => (x < y) as u8 as f32,
        BinOp::Le => (x <= y) as u8 as f32,
        BinOp::Gt => (x > y) as u8 as f32,
        BinOp::Ge => (x >= y) as u8 as f32,
        BinOp::Eq => (x == y) as u8 as f32,
        BinOp::Ne => (x != y) as u8 as f32,
        BinOp::And | BinOp::Or => {
            unreachable!("short-circuit operators are evaluated by the caller")
        }
    })
}

fn random_range(ctx: &mut dyn MolangContext, low: f32, high: f32) -> f32 {
    low + ctx.random() * (high - low)
}

fn random_integer(ctx: &mut dyn MolangContext, low: f32, high: f32) -> f32 {
    let (low, high) = (low.round() as i64, high.round() as i64);
    if high <= low {
        return low as f32;
    }
    let span = (high - low + 1) as f32;
    (low + ((ctx.random() * span) as i64).min(high - low)) as f32
}

fn math(f: MathFn, [a, b, c]: [f32; 3], ctx: &mut dyn MolangContext) -> f32 {
    match f {
        MathFn::Abs => a.abs(),
        MathFn::Acos => a.acos().to_degrees(),
        MathFn::Asin => a.asin().to_degrees(),
        MathFn::Atan => a.atan().to_degrees(),
        MathFn::Atan2 => a.atan2(b).to_degrees(),
        MathFn::Ceil => a.ceil(),
        MathFn::Clamp => a.max(b).min(c),
        MathFn::Cos => a.to_radians().cos(),
        MathFn::DieRoll => (0..a.max(0.0) as u32)
            .map(|_| random_range(ctx, b, c))
            .sum(),
        MathFn::DieRollInteger => (0..a.max(0.0) as u32)
            .map(|_| random_integer(ctx, b, c))
            .sum(),
        MathFn::Exp => a.exp(),
        MathFn::Floor => a.floor(),
        MathFn::HermiteBlend => 3.0 * a * a - 2.0 * a * a * a,
        MathFn::Lerp => a + (b - a) * c,
        MathFn::LerpRotate => {
            let diff = (b - a + 540.0).rem_euclid(360.0) - 180.0;
            a + diff * c
        }
        MathFn::Ln => a.ln(),
        MathFn::Max => a.max(b),
        MathFn::Min => a.min(b),
        MathFn::MinAngle => (a + 180.0).rem_euclid(360.0) - 180.0,
        MathFn::Mod => a % b,
        MathFn::Pi => std::f32::consts::PI,
        MathFn::Pow => a.powf(b),
        MathFn::Random => random_range(ctx, a, b),
        MathFn::RandomInteger => random_integer(ctx, a, b),
        MathFn::Round => a.round(),
        MathFn::Sign => {
            if a < 0.0 {
                -1.0
            } else {
                1.0
            }
        }
        MathFn::Sin => a.to_radians().sin(),
        MathFn::Sqrt => a.sqrt(),
        MathFn::Trunc => a.trunc(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::molang::{BuiltinQuery, QueryContext, QueryRegistry, parse};
    use std::collections::HashMap;

    struct Ctx {
        baby: bool,
        hit_by_player: bool,
        equipment: f32,
        variables: HashMap<String, Value>,
        saved_groups: Vec<&'static str>,
        rolls: Vec<f32>,
        other: Option<u64>,
        current: Vec<u64>,
        durability: HashMap<u64, f32>,
    }

    impl Default for Ctx {
        fn default() -> Self {
            Self {
                baby: false,
                hit_by_player: true,
                equipment: 2.0,
                variables: HashMap::new(),
                rolls: Vec::new(),
                saved_groups: Vec::new(),
                other: None,
                current: vec![0],
                durability: HashMap::new(),
            }
        }
    }

    impl MolangContext for Ctx {
        fn query(&mut self, id: QueryId, args: &[Value]) -> Option<Value> {
            Some(match BuiltinQuery::from_id(id)? {
                BuiltinQuery::IsBaby => Value::from_bool(self.baby),
                BuiltinQuery::LastHitByPlayer => Value::from_bool(self.hit_by_player),
                BuiltinQuery::EquipmentCount => Value::Num(self.equipment),
                BuiltinQuery::HadComponentGroup => {
                    let Some(Value::Str(name)) = args.first() else {
                        return None;
                    };
                    Value::from_bool(
                        self.saved_groups
                            .iter()
                            .any(|group| *group == name.as_ref()),
                    )
                }
                BuiltinQuery::HasAnyFamily => {
                    Value::from_bool(args.iter().any(|a| *a == Value::Str(Arc::from("zombie"))))
                }
                BuiltinQuery::RemainingDurability => {
                    Value::Num(*self.durability.get(self.current.last()?)?)
                }
                _ => return None,
            })
        }
        fn variable(&self, name: &str) -> Option<Value> {
            self.variables.get(name).cloned()
        }
        fn set_variable(&mut self, name: &str, value: Value) {
            self.variables.insert(name.to_owned(), value);
        }
        fn context(&self, name: &str) -> Option<Value> {
            (name == "other").then_some(Value::Actor(self.other?))
        }
        fn this(&self) -> Value {
            Value::Actor(*self.current.last().unwrap())
        }
        fn enter(&mut self, actor: u64) -> bool {
            self.current.push(actor);
            true
        }
        fn leave(&mut self) {
            self.current.pop();
        }
        fn random(&mut self) -> f32 {
            self.rolls.pop().unwrap_or(0.5)
        }
    }

    fn eval(src: &str, ctx: &mut Ctx) -> Value {
        let mut e = parse(src).unwrap();
        e.bind(QueryRegistry::vanilla(), QueryContext::Runtime)
            .unwrap();
        evaluate(&e, ctx).unwrap()
    }

    #[test]
    fn arithmetic_logic_and_strings() {
        let ctx = &mut Ctx::default();
        assert_eq!(eval("1 + 2 * 3", ctx), Value::Num(7.0));
        assert_eq!(eval("10 - 4 - 3", ctx), Value::Num(3.0));
        assert_eq!(eval("-2 - -3", ctx), Value::Num(1.0));
        assert_eq!(eval("1 < 2 && 3 > 4 || 1", ctx), Value::Num(1.0));
        assert_eq!(eval("!0", ctx), Value::Num(1.0));
        assert_eq!(eval("'a' == 'a'", ctx), Value::Num(1.0));
        assert_eq!(eval("'a' != 'b'", ctx), Value::Num(1.0));
        assert_eq!(eval("'a' == 1", ctx), Value::Num(0.0));
        assert_eq!(eval("0 ? 5", ctx), Value::Num(0.0));
        assert_eq!(eval("math.clamp(7, 0, 6)", ctx), Value::Num(6.0));
        assert_eq!(eval("math.sin(90)", ctx), Value::Num(1.0));
    }

    #[test]
    fn vanilla_experience_rewards() {
        let ctx = &mut Ctx::default();
        assert_eq!(
            eval("query.last_hit_by_player ? 5: 0", ctx),
            Value::Num(5.0)
        );
        // Two die rolls in [1, 3) with uniform draws 0.5 and 0.0: 2 + 1.
        ctx.rolls = vec![0.0, 0.5];
        assert_eq!(
            eval(
                "query.last_hit_by_player ? (query.is_baby ? 12 : 5) + (Math.die_roll(query.equipment_count,1,3)) : 0",
                ctx
            ),
            Value::Num(8.0)
        );
        ctx.hit_by_player = false;
        assert_eq!(
            eval("query.last_hit_by_player ? 5 : 0", ctx),
            Value::Num(0.0)
        );
        assert_eq!(
            eval("query.has_any_family('player', 'zombie') ? -90 : 0", ctx),
            Value::Num(-90.0)
        );
    }

    #[test]
    fn random_integer_is_inclusive() {
        let ctx = &mut Ctx::default();
        ctx.rolls = vec![0.999_999];
        assert_eq!(eval("math.random_integer(1, 3)", ctx), Value::Num(3.0));
        ctx.rolls = vec![0.0];
        assert_eq!(eval("math.random_integer(1, 3)", ctx), Value::Num(1.0));
    }

    #[test]
    fn variables_temps_programs_and_loops() {
        let ctx = &mut Ctx::default();
        assert_eq!(
            eval("t.a = 2; t.b = t.a * 3; return t.b;", ctx),
            Value::Num(6.0)
        );
        assert_eq!(
            eval("v.x = 1; loop(4, {v.x = v.x * 2;}); return v.x;", ctx),
            Value::Num(16.0)
        );
        assert_eq!(
            eval(
                "v.y = 0; loop(10, {v.y = v.y + 1; (v.y >= 3) ? break;}); return v.y;",
                ctx
            ),
            Value::Num(3.0)
        );
        assert_eq!(eval("v.missing ?? 7", ctx), Value::Num(7.0));
        assert_eq!(eval("v.x ?? 7", ctx), Value::Num(16.0));
        assert_eq!(
            eval("1; 2;", ctx),
            Value::Num(0.0),
            "complex expression without return"
        );
    }

    #[test]
    fn arrow_evaluates_on_the_target() {
        let ctx = &mut Ctx::default();
        ctx.durability.insert(9, 42.0);
        assert_eq!(
            eval("context.other->query.remaining_durability", ctx),
            Value::Num(0.0)
        );
        ctx.other = Some(9);
        assert_eq!(
            eval("context.other->query.remaining_durability", ctx),
            Value::Num(42.0)
        );
        assert_eq!(ctx.current, vec![0]);
    }

    #[test]
    fn unbound_query_is_an_error() {
        let e = parse("query.is_baby").unwrap();
        assert!(matches!(
            evaluate(&e, &mut Ctx::default()),
            Err(EvalError::Unbound(_))
        ));
    }

    #[test]
    fn query_context_separates_saved_property_defaults_from_live_actor_queries() {
        let mut default = parse("Q.Had_Component_Group('has_nectar') ? 1 : 0").unwrap();
        default
            .bind(QueryRegistry::vanilla(), QueryContext::PropertyDefault)
            .unwrap();
        let mut ctx = Ctx::default();
        assert_eq!(evaluate(&default, &mut ctx).unwrap(), Value::Num(0.0));
        ctx.saved_groups.push("has_nectar");
        assert_eq!(evaluate(&default, &mut ctx).unwrap(), Value::Num(1.0));
        assert!(
            default
                .bind(QueryRegistry::vanilla(), QueryContext::Runtime)
                .is_err()
        );

        ctx.baby = true;
        let mut actor = parse("query.is_baby").unwrap();
        actor
            .bind(QueryRegistry::vanilla(), QueryContext::Runtime)
            .unwrap();
        assert_eq!(evaluate(&actor, &mut ctx).unwrap(), Value::Num(1.0));
        assert!(
            actor
                .bind(QueryRegistry::vanilla(), QueryContext::PropertyDefault)
                .is_err()
        );
        let mut unknown = parse("query.made_up").unwrap();
        assert!(
            unknown
                .bind(QueryRegistry::vanilla(), QueryContext::Runtime)
                .is_err()
        );
    }
}
