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

impl Func {
    fn lookup(name: &str) -> Option<(Func, usize)> {
        Some(match name {
            "query.heightmap" | "q.heightmap" => (Func::Heightmap, 2),
            "query.above_top_solid" | "q.above_top_solid" => (Func::AboveTopSolid, 2),
            "query.noise" | "q.noise" => (Func::Noise, 2),
            "query.has_biome_tag" | "q.has_biome_tag" => (Func::HasBiomeTag, 1),
            "math.random" => (Func::Random, 2),
            "math.random_integer" => (Func::RandomInteger, 2),
            "math.max" => (Func::Max, 2),
            "math.min" => (Func::Min, 2),
            "math.ceil" => (Func::Ceil, 1),
            "math.floor" => (Func::Floor, 1),
            "math.abs" => (Func::Abs, 1),
            _ => return None,
        })
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
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

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f32),
    Str(String),
    Ident(String),
    Op(&'static str),
}

fn lex(src: &str) -> Result<Vec<Token>, String> {
    const OPS: [&str; 19] = [
        "&&", "||", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "(", ")", ",", "?", ":", "!", ";",
    ];
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let text = &src[start..i];
            // Molang allows an `f` suffix on literals.
            if i < bytes.len() && bytes[i] == b'f' {
                i += 1;
            }
            out.push(Token::Num(text.parse().map_err(|_| format!("bad number {text:?}"))?));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.') {
                i += 1;
            }
            out.push(Token::Ident(src[start..i].to_ascii_lowercase()));
        } else if c == '\'' {
            let end = src[i + 1..].find('\'').ok_or("unterminated string")? + i + 1;
            out.push(Token::Str(src[i + 1..end].to_owned()));
            i = end + 1;
        } else if let Some(op) = OPS.iter().find(|op| src[i..].starts_with(**op)) {
            out.push(Token::Op(op));
            i += op.len();
        } else {
            return Err(format!("unexpected character {c:?}"));
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek_op(&self, op: &str) -> bool {
        matches!(self.tokens.get(self.pos), Some(Token::Op(o)) if *o == op)
    }

    fn eat_op(&mut self, op: &str) -> bool {
        let hit = self.peek_op(op);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn expect_op(&mut self, op: &str) -> Result<(), String> {
        if self.eat_op(op) {
            Ok(())
        } else {
            Err(format!("expected `{op}` at token {}", self.pos))
        }
    }

    fn ternary(&mut self) -> Result<Expr, String> {
        let cond = self.binary(0)?;
        if self.eat_op("?") {
            let a = self.ternary()?;
            self.expect_op(":")?;
            let b = self.ternary()?;
            return Ok(Expr::Ternary(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn binary(&mut self, min_level: u8) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let Some(Token::Op(op)) = self.tokens.get(self.pos) else {
                break;
            };
            let (level, bin) = match *op {
                "||" => (1, BinOp::Or),
                "&&" => (2, BinOp::And),
                "==" => (3, BinOp::Eq),
                "!=" => (3, BinOp::Ne),
                "<" => (4, BinOp::Lt),
                "<=" => (4, BinOp::Le),
                ">" => (4, BinOp::Gt),
                ">=" => (4, BinOp::Ge),
                "+" => (5, BinOp::Add),
                "-" => (5, BinOp::Sub),
                "*" => (6, BinOp::Mul),
                "/" => (6, BinOp::Div),
                _ => break,
            };
            if level < min_level.max(1) {
                break;
            }
            self.pos += 1;
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Bin(bin, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_op("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat_op("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self.tokens.get(self.pos).cloned().ok_or("unexpected end of expression")?;
        self.pos += 1;
        match token {
            Token::Num(n) => Ok(Expr::Num(n)),
            Token::Str(s) => Ok(Expr::Str(s)),
            Token::Op("(") => {
                let e = self.ternary()?;
                self.expect_op(")")?;
                Ok(e)
            }
            Token::Ident(name) => {
                if self.eat_op("(") {
                    let (func, arity) = Func::lookup(&name).ok_or_else(|| format!("unsupported function {name}"))?;
                    let mut args = Vec::new();
                    if !self.eat_op(")") {
                        loop {
                            args.push(self.ternary()?);
                            if self.eat_op(")") {
                                break;
                            }
                            self.expect_op(",")?;
                        }
                    }
                    if args.len() != arity {
                        return Err(format!("{name} takes {arity} arguments, got {}", args.len()));
                    }
                    return Ok(Expr::Call(func, args));
                }
                let var = match name.as_str() {
                    "variable.worldx" | "v.worldx" => Var::WorldX,
                    "variable.worldy" | "v.worldy" => Var::WorldY,
                    "variable.worldz" | "v.worldz" => Var::WorldZ,
                    "variable.originx" | "v.originx" => Var::OriginX,
                    "variable.originy" | "v.originy" => Var::OriginY,
                    "variable.originz" | "v.originz" => Var::OriginZ,
                    "variable.is_legacy" | "v.is_legacy" => Var::IsLegacy,
                    "variable.sea_level" | "v.sea_level" => Var::SeaLevel,
                    "true" => return Ok(Expr::Num(1.0)),
                    "false" => return Ok(Expr::Num(0.0)),
                    _ => return Err(format!("unsupported identifier {name}")),
                };
                Ok(Expr::Var(var))
            }
            Token::Op(op) => Err(format!("unexpected `{op}`")),
        }
    }
}

/// Parse and constant-fold a Molang expression.
pub fn parse(origin: &str, src: &str) -> Result<Expr> {
    let err = |reason: String| Error::data(origin, format!("Molang {src:?}: {reason}"));
    let tokens = lex(src).map_err(err)?;
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.ternary().map_err(err)?;
    // A single trailing `;` terminates a simple expression.
    parser.eat_op(";");
    if parser.pos != parser.tokens.len() {
        return Err(err(format!("trailing input at token {}", parser.pos)));
    }
    Ok(fold(expr))
}

fn truthy(v: f32) -> bool {
    v != 0.0
}

fn bool_f(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
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
            let nums: Option<Vec<f32>> = args.iter().map(|a| if let Num(v) = a { Some(*v) } else { None }).collect();
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
        Expr::Str(s) => return Err(Error::data(origin, format!("string {s:?} used as a number"))),
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
            let (c, a, b) = (emit_bool(origin, c, env)?, emit(origin, a, env)?, emit(origin, b, env)?);
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
        Expr::Bin(op @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne), a, b) => {
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
            let (c, a, b) = (emit_bool(origin, c, env)?, emit_bool(origin, a, env)?, emit_bool(origin, b, env)?);
            quote!((if #c { #a } else { #b }))
        }
        Expr::Call(Func::HasBiomeTag, args) => match &args[0] {
            Expr::Str(tag) => env.has_biome_tag(tag)?,
            other => return Err(Error::data(origin, format!("has_biome_tag needs a string, got {other:?}"))),
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
            syn::parse2::<syn::Expr>(tokens.clone()).unwrap_or_else(|e| panic!("{src}: {e}\n{tokens}"));
        }
    }

    #[test]
    fn is_legacy_folds_to_zero() {
        assert_eq!(parse("t", "variable.is_legacy ? 0.0 : 100.0").unwrap(), Expr::Num(100.0));
        assert_eq!(parse("t", "variable.is_legacy == 0.0 ? 1 : 0").unwrap(), Expr::Num(1.0));
        let e = parse("t", "((query.noise(1, 2) >= -0.8) && variable.is_legacy == 0.0) ? 7 : 0").unwrap();
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
        assert_eq!(parse("t", "math.max(2, 3) - math.ceil(0.5)").unwrap(), Expr::Num(2.0));
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
        for bad in ["query.block_state('x')", "variable.foo", "1 +", "math.max(1)", "a.b = 3"] {
            assert!(parse("doc.json", bad).is_err(), "{bad}");
        }
    }
}
