//! Compiler for the Molang subset used by vanilla feature rules and features.
//!
//! BDS 1.26.51 worldgen documents use only:
//! `query.heightmap(x, z)`, `query.above_top_solid(x, z)`, `query.noise(x, z)`,
//! `query.has_biome_tag('tag')`, `variable.worldx/worldy/worldz`,
//! `variable.originx/originy/originz`, `variable.is_legacy`, `math.random`,
//! `math.max/min/ceil/floor/abs`, arithmetic, comparisons, `&&`, `||`, `!` and
//! the ternary operator. Anything else is a build error.
//!
//! Expressions are parsed into [`Expr`], constant-folded (`variable.is_legacy`
//! is `0` for every world unastar creates: it marks worlds of the old
//! "legacy" world type), and emitted as `f32` Rust expressions. Molang is
//! single-precision; booleans are `1.0`/`0.0` and truthiness is `!= 0.0`.
//! The surrounding emitter supplies the tokens for variables and queries
//! through [`Env`], so no Molang interpreter exists at runtime.

use crate::error::{Error, Result};
use proc_macro2::TokenStream;
use quote::quote;
use unastar_rules::molang::{self as syntax, BinOp, MathFn, Scope, UnaryOp};

/// A Molang variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    WorldX,
    WorldY,
    WorldZ,
    OriginX,
    OriginY,
    OriginZ,
    IsLegacy,
    /// `variable.sea_level`, used by nether material adjustments.
    SeaLevel,
}

/// A query or math function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Heightmap,
    AboveTopSolid,
    Noise,
    HasBiomeTag,
    Random,
    RandomInteger,
    Max,
    Min,
    Ceil,
    Floor,
    Abs,
}

/// Parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f32),
    Str(String),
    Var(Var),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

/// Lower the shared Molang syntax tree to the supported worldgen expression IR.
///
/// The lexer/parser lives only in `unastar-rules`; this phase checks the query
/// and variable bindings available to generated worldgen code. Unsupported
/// runtime constructs fail at build time rather than invoking an interpreter.
fn lower(expr: syntax::Expr) -> std::result::Result<Expr, String> {
    use syntax::Expr as S;
    let args = |xs: Vec<S>| {
        xs.into_iter()
            .map(lower)
            .collect::<std::result::Result<Vec<_>, _>>()
    };
    Ok(match expr {
        S::Num(n) => Expr::Num(n),
        S::Str(s) => Expr::Str(s.to_string()),
        S::Var(Scope::Variable, name) => Expr::Var(match name.as_ref() {
            "worldx" => Var::WorldX,
            "worldy" => Var::WorldY,
            "worldz" => Var::WorldZ,
            "originx" => Var::OriginX,
            "originy" => Var::OriginY,
            "originz" => Var::OriginZ,
            "is_legacy" => Var::IsLegacy,
            "sea_level" => Var::SeaLevel,
            _ => return Err(format!("unsupported variable {name}")),
        }),
        S::Unary(UnaryOp::Neg, x) => Expr::Neg(Box::new(lower(*x)?)),
        S::Unary(UnaryOp::Not, x) => Expr::Not(Box::new(lower(*x)?)),
        S::Binary(op, a, b) => Expr::Bin(op, Box::new(lower(*a)?), Box::new(lower(*b)?)),
        S::Ternary(c, a, b) => Expr::Ternary(
            Box::new(lower(*c)?),
            Box::new(lower(*a)?),
            Box::new(lower(*b)?),
        ),
        S::Query(q, xs) => {
            let (f, arity) = match q.name.as_ref() {
                "heightmap" => (Func::Heightmap, 2),
                "above_top_solid" => (Func::AboveTopSolid, 2),
                "noise" => (Func::Noise, 2),
                "has_biome_tag" => (Func::HasBiomeTag, 1),
                _ => return Err(format!("unsupported query {}", q.name)),
            };
            if xs.len() != arity {
                return Err(format!(
                    "query.{} takes {arity} arguments, got {}",
                    q.name,
                    xs.len()
                ));
            }
            Expr::Call(f, args(xs)?)
        }
        S::Math(f, xs) => Expr::Call(
            match f {
                MathFn::Random => Func::Random,
                MathFn::RandomInteger => Func::RandomInteger,
                MathFn::Max => Func::Max,
                MathFn::Min => Func::Min,
                MathFn::Ceil => Func::Ceil,
                MathFn::Floor => Func::Floor,
                MathFn::Abs => Func::Abs,
                _ => return Err(format!("unsupported worldgen math function {f:?}")),
            },
            args(xs)?,
        ),
        other => return Err(format!("unsupported worldgen expression {other:?}")),
    })
}

/// Parse with the shared frontend, bind the worldgen subset and constant-fold.
pub fn parse(origin: &str, src: &str) -> Result<Expr> {
    let err = |reason: String| Error::data(origin, format!("Molang {src:?}: {reason}"));
    let syntax = syntax::parse(src).map_err(err)?;
    Ok(fold(lower(syntax).map_err(err)?))
}

fn truthy(v: f32) -> bool {
    v != 0.0
}

fn bool_f(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

/// Replace `var` with a constant and fold again.
pub fn substitute(expr: &Expr, var: Var, value: f32) -> Expr {
    fn go(e: &Expr, var: Var, value: f32) -> Expr {
        match e {
            Expr::Var(v) if *v == var => Expr::Num(value),
            Expr::Neg(x) => Expr::Neg(Box::new(go(x, var, value))),
            Expr::Not(x) => Expr::Not(Box::new(go(x, var, value))),
            Expr::Bin(op, a, b) => Expr::Bin(
                *op,
                Box::new(go(a, var, value)),
                Box::new(go(b, var, value)),
            ),
            Expr::Ternary(c, a, b) => Expr::Ternary(
                Box::new(go(c, var, value)),
                Box::new(go(a, var, value)),
                Box::new(go(b, var, value)),
            ),
            Expr::Call(f, args) => Expr::Call(*f, args.iter().map(|a| go(a, var, value)).collect()),
            other => other.clone(),
        }
    }
    fold(go(expr, var, value))
}

/// Fold constants and `variable.is_legacy` (always `0`).
pub fn fold(expr: Expr) -> Expr {
    use Expr::*;
    match expr {
        Var(crate::molang::Var::IsLegacy) => Num(0.0),
        Neg(e) => match fold(*e) {
            Num(v) => Num(-v),
            e => Neg(Box::new(e)),
        },
        Not(e) => match fold(*e) {
            Num(v) => Num(bool_f(!truthy(v))),
            e => Not(Box::new(e)),
        },
        Ternary(c, a, b) => match fold(*c) {
            Num(v) if truthy(v) => fold(*a),
            Num(_) => fold(*b),
            c => Ternary(Box::new(c), Box::new(fold(*a)), Box::new(fold(*b))),
        },
        Bin(op, a, b) => {
            let (a, b) = (fold(*a), fold(*b));
            match (op, &a, &b) {
                (BinOp::And, Num(x), _) if !truthy(*x) => Num(0.0),
                (BinOp::Or, Num(x), _) if truthy(*x) => Num(1.0),
                (_, Num(x), Num(y)) => Num(apply(op, *x, *y)),
                _ => Bin(op, Box::new(a), Box::new(b)),
            }
        }
        Call(f, args) => {
            let args: Vec<Expr> = args.into_iter().map(fold).collect();
            let nums: Option<Vec<f32>> = args
                .iter()
                .map(|a| if let Num(v) = a { Some(*v) } else { None })
                .collect();
            match (f, nums.as_deref()) {
                (Func::Max, Some([a, b])) => Num(a.max(*b)),
                (Func::Min, Some([a, b])) => Num(a.min(*b)),
                (Func::Ceil, Some([a])) => Num(a.ceil()),
                (Func::Floor, Some([a])) => Num(a.floor()),
                (Func::Abs, Some([a])) => Num(a.abs()),
                _ => Call(f, args),
            }
        }
        e => e,
    }
}

fn apply(op: BinOp, x: f32, y: f32) -> f32 {
    match op {
        BinOp::Add => x + y,
        BinOp::Sub => x - y,
        BinOp::Mul => x * y,
        BinOp::Div => x / y,
        BinOp::Lt => bool_f(x < y),
        BinOp::Le => bool_f(x <= y),
        BinOp::Gt => bool_f(x > y),
        BinOp::Ge => bool_f(x >= y),
        BinOp::Eq => bool_f(x == y),
        BinOp::Ne => bool_f(x != y),
        BinOp::And => bool_f(truthy(x) && truthy(y)),
        BinOp::Or => bool_f(truthy(x) || truthy(y)),
    }
}

impl Expr {
    /// The constant value, if the expression folded to one.
    pub fn as_const(&self) -> Option<f32> {
        match self {
            Expr::Num(v) => Some(*v),
            _ => None,
        }
    }

    /// `true` when evaluating the expression consumes randomness.
    pub fn uses_random(&self) -> bool {
        match self {
            Expr::Call(Func::Random | Func::RandomInteger, _) => true,
            Expr::Call(_, args) => args.iter().any(Expr::uses_random),
            Expr::Neg(e) | Expr::Not(e) => e.uses_random(),
            Expr::Bin(_, a, b) => a.uses_random() || b.uses_random(),
            Expr::Ternary(c, a, b) => c.uses_random() || a.uses_random() || b.uses_random(),
            _ => false,
        }
    }

    /// `true` when the expression reads `var`.
    pub fn reads(&self, var: Var) -> bool {
        match self {
            Expr::Var(v) => *v == var,
            Expr::Call(_, args) => args.iter().any(|a| a.reads(var)),
            Expr::Neg(e) | Expr::Not(e) => e.reads(var),
            Expr::Bin(_, a, b) => a.reads(var) || b.reads(var),
            Expr::Ternary(c, a, b) => c.reads(var) || a.reads(var) || b.reads(var),
            _ => false,
        }
    }
}

/// Supplies the Rust tokens for the runtime parts of an expression.
pub trait Env {
    /// An `f32` expression for a variable.
    fn var(&self, var: Var) -> Result<TokenStream>;
    /// A `bool` expression for `query.has_biome_tag(tag)`.
    fn has_biome_tag(&self, tag: &str) -> Result<TokenStream>;
    /// The decoration context expression (`ctx`), used for queries.
    fn ctx(&self) -> TokenStream;
}

fn lit(v: f32) -> TokenStream {
    let v = proc_macro2::Literal::f32_suffixed(v);
    quote!(#v)
}

/// Emit an `f32` Rust expression.
pub fn emit(origin: &str, expr: &Expr, env: &dyn Env) -> Result<TokenStream> {
    let ctx = env.ctx();
    Ok(match expr {
        Expr::Num(v) => lit(*v),
        Expr::Str(s) => {
            return Err(Error::data(
                origin,
                format!("string {s:?} used as a number"),
            ));
        }
        Expr::Var(v) => env.var(*v)?,
        Expr::Neg(e) => {
            let e = emit(origin, e, env)?;
            quote!((-(#e)))
        }
        Expr::Not(e) => {
            let e = emit_bool(origin, e, env)?;
            quote!((if #e { 0.0f32 } else { 1.0f32 }))
        }
        Expr::Bin(op, a, b) => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                let (a, b) = (emit(origin, a, env)?, emit(origin, b, env)?);
                match op {
                    BinOp::Add => quote!((#a + #b)),
                    BinOp::Sub => quote!((#a - #b)),
                    BinOp::Mul => quote!((#a * #b)),
                    _ => quote!((#a / #b)),
                }
            }
            _ => {
                let b = emit_bool(origin, expr, env)?;
                quote!((if #b { 1.0f32 } else { 0.0f32 }))
            }
        },
        Expr::Ternary(c, a, b) => {
            let (c, a, b) = (
                emit_bool(origin, c, env)?,
                emit(origin, a, env)?,
                emit(origin, b, env)?,
            );
            quote!((if #c { #a } else { #b }))
        }
        Expr::Call(f, args) => {
            let num = |i: usize| emit(origin, &args[i], env);
            match f {
                Func::Heightmap => {
                    let (x, z) = (num(0)?, num(1)?);
                    quote!((#ctx.heightmap(#x.floor() as i32, #z.floor() as i32) as f32))
                }
                Func::AboveTopSolid => {
                    let (x, z) = (num(0)?, num(1)?);
                    quote!((#ctx.above_top_solid(#x.floor() as i32, #z.floor() as i32) as f32))
                }
                Func::Noise => {
                    let (x, z) = (num(0)?, num(1)?);
                    quote!(#ctx.noise(#x, #z))
                }
                Func::Random => {
                    let (a, b) = (num(0)?, num(1)?);
                    quote!(#ctx.rng.molang_random(#a, #b))
                }
                Func::RandomInteger => {
                    let (a, b) = (num(0)?, num(1)?);
                    quote!((#ctx.rng.molang_random_integer(#a, #b) as f32))
                }
                Func::Max => {
                    let (a, b) = (num(0)?, num(1)?);
                    quote!(#a.max(#b))
                }
                Func::Min => {
                    let (a, b) = (num(0)?, num(1)?);
                    quote!(#a.min(#b))
                }
                Func::Ceil => {
                    let a = num(0)?;
                    quote!(#a.ceil())
                }
                Func::Floor => {
                    let a = num(0)?;
                    quote!(#a.floor())
                }
                Func::Abs => {
                    let a = num(0)?;
                    quote!(#a.abs())
                }
                Func::HasBiomeTag => {
                    let b = emit_bool(origin, expr, env)?;
                    quote!((if #b { 1.0f32 } else { 0.0f32 }))
                }
            }
        }
    })
}

/// Emit a `bool` Rust expression (Molang truthiness).
pub fn emit_bool(origin: &str, expr: &Expr, env: &dyn Env) -> Result<TokenStream> {
    Ok(match expr {
        Expr::Num(v) => {
            if truthy(*v) {
                quote!(true)
            } else {
                quote!(false)
            }
        }
        Expr::Not(e) => {
            let e = emit_bool(origin, e, env)?;
            quote!((!#e))
        }
        Expr::Bin(op @ (BinOp::And | BinOp::Or), a, b) => {
            let (a, b) = (emit_bool(origin, a, env)?, emit_bool(origin, b, env)?);
            if *op == BinOp::And {
                quote!((#a && #b))
            } else {
                quote!((#a || #b))
            }
        }
        Expr::Bin(
            op @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne),
            a,
            b,
        ) => {
            let (a, b) = (emit(origin, a, env)?, emit(origin, b, env)?);
            match op {
                BinOp::Lt => quote!((#a < #b)),
                BinOp::Le => quote!((#a <= #b)),
                BinOp::Gt => quote!((#a > #b)),
                BinOp::Ge => quote!((#a >= #b)),
                BinOp::Eq => quote!((#a == #b)),
                _ => quote!((#a != #b)),
            }
        }
        Expr::Ternary(c, a, b) => {
            let (c, a, b) = (
                emit_bool(origin, c, env)?,
                emit_bool(origin, a, env)?,
                emit_bool(origin, b, env)?,
            );
            quote!((if #c { #a } else { #b }))
        }
        Expr::Call(Func::HasBiomeTag, args) => match &args[0] {
            Expr::Str(tag) => env.has_biome_tag(tag)?,
            other => {
                return Err(Error::data(
                    origin,
                    format!("has_biome_tag needs a string, got {other:?}"),
                ));
            }
        },
        other => {
            let v = emit(origin, other, env)?;
            quote!((#v != 0.0f32))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEnv;
    impl Env for TestEnv {
        fn var(&self, var: Var) -> Result<TokenStream> {
            Ok(match var {
                Var::WorldX => quote!(wx),
                Var::WorldZ => quote!(wz),
                Var::OriginX => quote!(ox),
                Var::OriginZ => quote!(oz),
                _ => quote!(0.0f32),
            })
        }
        fn has_biome_tag(&self, tag: &str) -> Result<TokenStream> {
            Ok(quote!(ctx.has_tag(#tag)))
        }
        fn ctx(&self) -> TokenStream {
            quote!(ctx)
        }
    }

    /// Every Molang string in the 1.26.51 feature rules and features.
    const VANILLA: &[&str] = &[
        "query.heightmap(variable.worldx, variable.worldz)",
        "variable.is_legacy ? 0.0 : 100.0",
        "query.above_top_solid(variable.worldx, variable.worldz)",
        "(query.noise((variable.originx + 8) / 200.0, (variable.originz + 8) / 200.0) < -0.8) ? 5 : 10",
        "((query.noise((variable.originx + 8) / 200.0, (variable.originz + 8) / 200.0) >= -0.8) && variable.is_legacy == 0.0) ? 7 : 0",
        "math.random(-8, 8)",
        "variable.is_legacy == 0.0 ? 1 : 0",
        "variable.is_legacy ? 0 : 1",
        "math.max(query.above_top_solid(variable.worldx, variable.worldz), query.heightmap(variable.worldx, variable.worldz) - 4)",
        "math.ceil((query.noise(variable.originx / 80.0, variable.originz / 80.0) + 0.3) * 160.0)",
        "(query.noise((variable.originx) / 200.0, (variable.originz) / 200.0) < -0.8) ? 5 : 10",
        "math.ceil(query.noise(variable.originx / 80.0, variable.originz / 80.0) * 80.0)",
        "(query.noise((variable.originx + 8) / 200.0, (variable.originz + 8) / 200.0) < -0.8) ? 4 : 1",
        "query.has_biome_tag('bee_habitat') ? (query.has_biome_tag('plains') || query.has_biome_tag('sunflower_plains') ? 5.0 : (query.has_biome_tag('flower_forest') ? 3.0 : (query.has_biome_tag('birch_forest') || query.has_biome_tag('forest') ? 0.035 : 0.0))) : 0.0",
    ];

    #[test]
    fn every_vanilla_expression_parses_and_emits_valid_rust() {
        for src in VANILLA {
            let expr = parse("t", src).unwrap();
            let tokens = emit("t", &expr, &TestEnv).unwrap();
            syn::parse2::<syn::Expr>(tokens.clone())
                .unwrap_or_else(|e| panic!("{src}: {e}\n{tokens}"));
        }
    }

    #[test]
    fn is_legacy_folds_to_zero() {
        assert_eq!(
            parse("t", "variable.is_legacy ? 0.0 : 100.0").unwrap(),
            Expr::Num(100.0)
        );
        assert_eq!(
            parse("t", "variable.is_legacy == 0.0 ? 1 : 0").unwrap(),
            Expr::Num(1.0)
        );
        let e = parse(
            "t",
            "((query.noise(1, 2) >= -0.8) && variable.is_legacy == 0.0) ? 7 : 0",
        )
        .unwrap();
        assert!(matches!(e, Expr::Ternary(..)));
        assert!(!e.reads(Var::IsLegacy));
    }

    #[test]
    fn precedence_matches_molang() {
        assert_eq!(parse("t", "1 + 2 * 3").unwrap(), Expr::Num(7.0));
        assert_eq!(parse("t", "(1 + 2) * 3").unwrap(), Expr::Num(9.0));
        assert_eq!(parse("t", "1 < 2 && 3 > 4 || 1").unwrap(), Expr::Num(1.0));
        assert_eq!(parse("t", "1 ? 2 : 0 ? 3 : 4").unwrap(), Expr::Num(2.0));
        assert_eq!(parse("t", "-2 - -3").unwrap(), Expr::Num(1.0));
        assert_eq!(
            parse("t", "math.max(2, 3) - math.ceil(0.5)").unwrap(),
            Expr::Num(2.0)
        );
        assert_eq!(parse("t", "10 - 4 - 3").unwrap(), Expr::Num(3.0));
    }

    #[test]
    fn random_is_detected_and_not_folded() {
        let e = parse("t", "math.random(-8, 8)").unwrap();
        assert!(e.uses_random());
        assert!(e.as_const().is_none());
    }

    #[test]
    fn unsupported_input_is_a_build_error() {
        for bad in [
            "query.block_state('x')",
            "variable.foo",
            "1 +",
            "math.max(1)",
            "a.b = 3",
        ] {
            assert!(parse("doc.json", bad).is_err(), "{bad}");
        }
    }
}
