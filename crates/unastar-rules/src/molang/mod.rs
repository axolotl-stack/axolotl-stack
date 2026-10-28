//! Molang: one lexer and parser for every consumer.
//!
//! The runtime evaluates the tree ([`eval`]); the Bedrock-style worldgen codegen
//! compiles the same tree to Rust ahead of time. Parsing follows Mojang's Molang
//! reference and the parser-fidelity rules of the entities research §4.7:
//!
//! - names are case-insensitive (`Math.Random` = `math.random`); identifiers
//!   are lower-cased at parse time, string literals are kept verbatim;
//! - `q.`, `v.`, `t.`, `c.` are aliases of `query.`, `variable.`, `temp.`,
//!   `context.` and resolve to the same node;
//! - numbers may carry an `f` suffix; strings use single quotes;
//! - a simple expression (no `;`) yields its value; a complex expression
//!   (statements separated by `;`) yields the value of its `return`, else 0;
//! - operators, loosest first: `=`, `??`, `?:` / binary `?`, `||`, `&&`,
//!   `==` `!=`, `<` `<=` `>` `>=`, `+` `-`, `*` `/`, unary `!` `-`, `->`.
//!
//! Resource-pack-only namespaces (`geometry.`, `material.`, `texture.`,
//! `array.`) and `for_each` are not accepted; an unknown `math.` function is a
//! parse error. Query names are not checked here: a [`QueryRegistry`] binds
//! them, so consumers can register their own queries (D5).

pub mod eval;

use crate::ParseError;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

/// Variable scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// `variable.` / `v.`: per-entity, persistent.
    Variable,
    /// `temp.` / `t.`: local to one evaluation.
    Temp,
    /// `context.` / `c.`: supplied by the caller (read-only).
    Context,
}

impl Scope {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Variable => "variable",
            Self::Temp => "temp",
            Self::Context => "context",
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
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

/// `math.*` functions with their argument counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathFn {
    Abs,
    Acos,
    Asin,
    Atan,
    Atan2,
    Ceil,
    Clamp,
    Cos,
    DieRoll,
    DieRollInteger,
    Exp,
    Floor,
    HermiteBlend,
    Lerp,
    LerpRotate,
    Ln,
    Max,
    Min,
    MinAngle,
    Mod,
    Pi,
    Pow,
    Random,
    RandomInteger,
    Round,
    Sign,
    Sin,
    Sqrt,
    Trunc,
}

impl MathFn {
    /// Look up `name` (without `math.`, lower case) → function and arity.
    pub fn lookup(name: &str) -> Option<(Self, usize)> {
        Some(match name {
            "abs" => (Self::Abs, 1),
            "acos" => (Self::Acos, 1),
            "asin" => (Self::Asin, 1),
            "atan" => (Self::Atan, 1),
            "atan2" => (Self::Atan2, 2),
            "ceil" => (Self::Ceil, 1),
            "clamp" => (Self::Clamp, 3),
            "cos" => (Self::Cos, 1),
            "die_roll" => (Self::DieRoll, 3),
            "die_roll_integer" => (Self::DieRollInteger, 3),
            "exp" => (Self::Exp, 1),
            "floor" => (Self::Floor, 1),
            "hermite_blend" => (Self::HermiteBlend, 1),
            "lerp" => (Self::Lerp, 3),
            "lerprotate" => (Self::LerpRotate, 3),
            "ln" => (Self::Ln, 1),
            "max" => (Self::Max, 2),
            "min" => (Self::Min, 2),
            "min_angle" => (Self::MinAngle, 1),
            "mod" => (Self::Mod, 2),
            "pi" => (Self::Pi, 0),
            "pow" => (Self::Pow, 2),
            "random" => (Self::Random, 2),
            "random_integer" => (Self::RandomInteger, 2),
            "round" => (Self::Round, 1),
            "sign" => (Self::Sign, 1),
            "sin" => (Self::Sin, 1),
            "sqrt" => (Self::Sqrt, 1),
            "trunc" => (Self::Trunc, 1),
            _ => return None,
        })
    }

    /// `true` for functions that draw random numbers.
    pub fn is_random(self) -> bool {
        matches!(
            self,
            Self::Random | Self::RandomInteger | Self::DieRoll | Self::DieRollInteger
        )
    }
}

/// Identifier of a query bound by a [`QueryRegistry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryId(pub u16);

/// Query availability is expression-context dependent in BDS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueryContext {
    Runtime,
    PropertyDefault,
}

/// A query reference: the normalised name (without `query.`) and, once bound,
/// its id.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub name: Arc<str>,
    pub id: Option<QueryId>,
}

/// Parsed Molang.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f32),
    Str(Arc<str>),
    /// `this`: the value being computed / the current entity, per context.
    This,
    Var(Scope, Arc<str>),
    /// `query.name` or `query.name(args)`.
    Query(Query, Vec<Expr>),
    Math(MathFn, Vec<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `a ? b : c`
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `a ? b` (0 when `a` is false)
    Conditional(Box<Expr>, Box<Expr>),
    /// `a ?? b`
    Coalesce(Box<Expr>, Box<Expr>),
    /// `a -> b`: evaluate `b` on the entity `a` refers to.
    Arrow(Box<Expr>, Box<Expr>),
    /// `variable.x = value` / `temp.x = value`
    Assign(Scope, Arc<str>, Box<Expr>),
    /// `{ statements }`
    Block(Vec<Expr>),
    Return(Box<Expr>),
    /// `loop(count, { body })`
    Loop(Box<Expr>, Box<Expr>),
    Break,
    Continue,
    /// A complex expression: statements separated by `;`.
    Program(Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f32),
    Str(Arc<str>),
    Ident(String),
    Op(&'static str),
}

const OPS: [&str; 24] = [
    "->", "??", "&&", "||", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "(", ")", "{",
    "}", ",", "?", ":", "!", ";", "=",
];

fn lex(src: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let text = &src[start..i];
            if i < bytes.len() && (bytes[i] == b'f' || bytes[i] == b'F') {
                i += 1;
            }
            out.push(Token::Num(
                text.parse().map_err(|_| format!("bad number {text:?}"))?,
            ));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            out.push(Token::Ident(src[start..i].to_ascii_lowercase()));
        } else if c == '\'' {
            let end = src[i + 1..].find('\'').ok_or("unterminated string")? + i + 1;
            out.push(Token::Str(Arc::from(&src[i + 1..end])));
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

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Statements up to `end` (end of input or `}`), separated by `;`.
    /// Returns the statements and whether any `;` was seen.
    fn statements(&mut self, until_brace: bool) -> Result<(Vec<Expr>, bool), String> {
        let mut stmts = Vec::new();
        let mut complex = false;
        loop {
            while self.eat_op(";") {
                complex = true;
            }
            if self.at_end() || (until_brace && self.peek_op("}")) {
                break;
            }
            stmts.push(self.expr()?);
            if self.eat_op(";") {
                complex = true;
                continue;
            }
            if self.at_end() || (until_brace && self.peek_op("}")) {
                break;
            }
            return Err(format!("expected `;` at token {}", self.pos));
        }
        Ok((stmts, complex))
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let lhs = self.coalesce()?;
        if self.eat_op("=") {
            let Expr::Var(scope, name) = lhs else {
                return Err("only variable.* and temp.* can be assigned".into());
            };
            if scope == Scope::Context {
                return Err("context.* is read-only".into());
            }
            return Ok(Expr::Assign(scope, name, Box::new(self.expr()?)));
        }
        Ok(lhs)
    }

    fn coalesce(&mut self) -> Result<Expr, String> {
        let lhs = self.conditional()?;
        if self.eat_op("??") {
            return Ok(Expr::Coalesce(Box::new(lhs), Box::new(self.coalesce()?)));
        }
        Ok(lhs)
    }

    fn conditional(&mut self) -> Result<Expr, String> {
        let cond = self.binary(1)?;
        if self.eat_op("?") {
            let a = self.conditional()?;
            if self.eat_op(":") {
                let b = self.conditional()?;
                return Ok(Expr::Ternary(Box::new(cond), Box::new(a), Box::new(b)));
            }
            return Ok(Expr::Conditional(Box::new(cond), Box::new(a)));
        }
        Ok(cond)
    }

    fn binary(&mut self, min_level: u8) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        while let Some(Token::Op(op)) = self.tokens.get(self.pos) {
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
            if level < min_level {
                break;
            }
            self.pos += 1;
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Binary(bin, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_op("-") {
            return Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.unary()?)));
        }
        if self.eat_op("!") {
            return Ok(Expr::Unary(UnaryOp::Not, Box::new(self.unary()?)));
        }
        self.arrow()
    }

    fn arrow(&mut self) -> Result<Expr, String> {
        let lhs = self.primary()?;
        if self.eat_op("->") {
            return Ok(Expr::Arrow(Box::new(lhs), Box::new(self.arrow()?)));
        }
        Ok(lhs)
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.eat_op("(") && !self.eat_op(")") {
            loop {
                args.push(self.expr()?);
                if self.eat_op(")") {
                    break;
                }
                self.expect_op(",")?;
            }
        }
        Ok(args)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self
            .tokens
            .get(self.pos)
            .cloned()
            .ok_or("unexpected end of expression")?;
        self.pos += 1;
        match token {
            Token::Num(n) => Ok(Expr::Num(n)),
            Token::Str(s) => Ok(Expr::Str(s)),
            Token::Op("(") => {
                let e = self.expr()?;
                self.expect_op(")")?;
                Ok(e)
            }
            Token::Op("{") => {
                let (stmts, _) = self.statements(true)?;
                self.expect_op("}")?;
                Ok(Expr::Block(stmts))
            }
            Token::Ident(name) => self.identifier(&name),
            Token::Op(op) => Err(format!("unexpected `{op}`")),
        }
    }

    fn identifier(&mut self, name: &str) -> Result<Expr, String> {
        match name {
            "true" => return Ok(Expr::Num(1.0)),
            "false" => return Ok(Expr::Num(0.0)),
            "this" => return Ok(Expr::This),
            "break" => return Ok(Expr::Break),
            "continue" => return Ok(Expr::Continue),
            "return" => return Ok(Expr::Return(Box::new(self.expr()?))),
            "loop" => {
                self.expect_op("(")?;
                let count = self.expr()?;
                self.expect_op(",")?;
                let body = self.expr()?;
                self.expect_op(")")?;
                return Ok(Expr::Loop(Box::new(count), Box::new(body)));
            }
            _ => {}
        }
        let (namespace, rest) = name
            .split_once('.')
            .ok_or_else(|| format!("unknown identifier {name}"))?;
        if rest.is_empty() || rest.contains('.') {
            return Err(format!("malformed name {name}"));
        }
        let scope = match namespace {
            "query" | "q" => {
                let args = self.args()?;
                return Ok(Expr::Query(
                    Query {
                        name: Arc::from(rest),
                        id: None,
                    },
                    args,
                ));
            }
            "math" => {
                let (func, arity) =
                    MathFn::lookup(rest).ok_or_else(|| format!("unknown function math.{rest}"))?;
                let args = self.args()?;
                if args.len() != arity {
                    return Err(format!(
                        "math.{rest} takes {arity} arguments, got {}",
                        args.len()
                    ));
                }
                return Ok(Expr::Math(func, args));
            }
            "variable" | "v" => Scope::Variable,
            "temp" | "t" => Scope::Temp,
            "context" | "c" => Scope::Context,
            other => return Err(format!("unsupported namespace {other}.")),
        };
        Ok(Expr::Var(scope, Arc::from(rest)))
    }
}

/// Parse a Molang string.
pub fn parse(src: &str) -> Result<Expr, String> {
    let tokens = lex(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    let (mut stmts, complex) = parser.statements(false)?;
    if !parser.at_end() {
        return Err(format!("trailing input at token {}", parser.pos));
    }
    Ok(match (complex, stmts.len()) {
        (_, 0) => Expr::Num(0.0),
        (false, 1) => stmts.pop().expect("one statement"),
        _ => Expr::Program(stmts),
    })
}

/// Parse with a [`ParseError`] that names the location.
pub fn parse_at(path: &str, src: &str) -> Result<Expr, ParseError> {
    parse(src).map_err(|reason| ParseError::new(path, format!("Molang {src:?}: {reason}")))
}

impl Expr {
    /// Visit this node and every child.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        f(self);
        match self {
            Self::Query(_, args) | Self::Math(_, args) => args.iter().for_each(|a| a.walk(f)),
            Self::Unary(_, e) | Self::Assign(_, _, e) | Self::Return(e) => e.walk(f),
            Self::Binary(_, a, b)
            | Self::Conditional(a, b)
            | Self::Coalesce(a, b)
            | Self::Arrow(a, b)
            | Self::Loop(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Self::Ternary(c, a, b) => {
                c.walk(f);
                a.walk(f);
                b.walk(f);
            }
            Self::Block(stmts) | Self::Program(stmts) => stmts.iter().for_each(|s| s.walk(f)),
            Self::Num(_)
            | Self::Str(_)
            | Self::This
            | Self::Var(..)
            | Self::Break
            | Self::Continue => {}
        }
    }

    fn walk_mut(
        &mut self,
        f: &mut impl FnMut(&mut Expr) -> Result<(), String>,
    ) -> Result<(), String> {
        f(self)?;
        match self {
            Self::Query(_, args) | Self::Math(_, args) => {
                args.iter_mut().try_for_each(|a| a.walk_mut(f))
            }
            Self::Unary(_, e) | Self::Assign(_, _, e) | Self::Return(e) => e.walk_mut(f),
            Self::Binary(_, a, b)
            | Self::Conditional(a, b)
            | Self::Coalesce(a, b)
            | Self::Arrow(a, b)
            | Self::Loop(a, b) => {
                a.walk_mut(f)?;
                b.walk_mut(f)
            }
            Self::Ternary(c, a, b) => {
                c.walk_mut(f)?;
                a.walk_mut(f)?;
                b.walk_mut(f)
            }
            Self::Block(stmts) | Self::Program(stmts) => {
                stmts.iter_mut().try_for_each(|s| s.walk_mut(f))
            }
            Self::Num(_)
            | Self::Str(_)
            | Self::This
            | Self::Var(..)
            | Self::Break
            | Self::Continue => Ok(()),
        }
    }

    /// Names of every query used.
    pub fn query_names(&self) -> Vec<Arc<str>> {
        let mut out = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Query(q, _) = e {
                out.push(q.name.clone());
            }
        });
        out
    }

    /// Bind queries before evaluation, rejecting names unavailable in this
    /// context. `had_component_group` is property-default-only; runtime actor
    /// queries are unavailable while those defaults are being determined.
    pub fn bind(&mut self, registry: &QueryRegistry, context: QueryContext) -> Result<(), String> {
        self.walk_mut(&mut |e| {
            if let Expr::Query(q, _) = e {
                let id = registry
                    .lookup(&q.name)
                    .ok_or_else(|| format!("unknown query query.{}", q.name))?;
                let property_only = matches!(
                    BuiltinQuery::from_id(id),
                    Some(BuiltinQuery::HadComponentGroup)
                );
                if (context == QueryContext::PropertyDefault) != property_only {
                    return Err(format!("query.{} is unavailable in {context:?}", q.name));
                }
                q.id = Some(id);
            }
            Ok(())
        })
    }

    /// `true` when evaluating draws random numbers.
    pub fn uses_random(&self) -> bool {
        let mut hit = false;
        self.walk(&mut |e| hit |= matches!(e, Expr::Math(f, _) if f.is_random()));
        hit
    }
}

macro_rules! builtin_queries {
    ($($variant:ident = $name:literal,)*) => {
        /// Queries with a runtime implementation in unastar (entities §4.4 and
        /// every query vanilla behaviour-pack data uses).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum BuiltinQuery {
            $($variant,)*
        }

        impl BuiltinQuery {
            pub const ALL: &'static [BuiltinQuery] = &[$(BuiltinQuery::$variant,)*];
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }
        }
    };
}

builtin_queries! {
    AllTags = "all_tags",
    AngerLevel = "anger_level",
    AnyTag = "any_tag",
    BlockState = "block_state",
    EquipmentCount = "equipment_count",
    HadComponentGroup = "had_component_group",
    HasAnyFamily = "has_any_family",
    HeadIsInWater = "head_is_in_water",
    IsBaby = "is_baby",
    IsOnGround = "is_on_ground",
    IsUsingItem = "is_using_item",
    LastHitByPlayer = "last_hit_by_player",
    MaxDurability = "max_durability",
    PlayerLevel = "player_level",
    Property = "property",
    RemainingDurability = "remaining_durability",
    Variant = "variant",
}

impl BuiltinQuery {
    pub fn id(self) -> QueryId {
        QueryId(self as u16)
    }

    pub fn from_id(id: QueryId) -> Option<Self> {
        Self::ALL.get(id.0 as usize).copied()
    }
}

/// Query names → ids. Built-ins first; extra queries registered before freeze.
#[derive(Debug, Clone)]
pub struct QueryRegistry {
    names: Vec<Arc<str>>,
    by_name: HashMap<Arc<str>, QueryId>,
}

impl Default for QueryRegistry {
    fn default() -> Self {
        let mut registry = Self {
            names: Vec::new(),
            by_name: HashMap::new(),
        };
        for q in BuiltinQuery::ALL {
            registry
                .register(q.name())
                .expect("built-in query names are unique");
        }
        registry
    }
}

impl QueryRegistry {
    /// The shared registry with the built-in queries only.
    pub fn vanilla() -> &'static QueryRegistry {
        static VANILLA: LazyLock<QueryRegistry> = LazyLock::new(QueryRegistry::default);
        &VANILLA
    }

    /// Register a query name (without `query.`, lower case).
    pub fn register(&mut self, name: &str) -> Result<QueryId, String> {
        let name: Arc<str> = Arc::from(name.to_ascii_lowercase());
        if self.by_name.contains_key(&name) {
            return Err(format!("query.{name} is already registered"));
        }
        let id = QueryId(self.names.len() as u16);
        self.names.push(name.clone());
        self.by_name.insert(name, id);
        Ok(id)
    }

    pub fn lookup(&self, name: &str) -> Option<QueryId> {
        self.by_name.get(name).copied()
    }

    pub fn name(&self, id: QueryId) -> &str {
        &self.names[id.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Molang string in the BDS 1.26.51 feature rules and features (as
    /// collected for the worldgen compiler) and the vanilla behaviour pack.
    pub(crate) const VANILLA: &[&str] = &[
        "query.heightmap(variable.worldx, variable.worldz)",
        "variable.is_legacy ? 0.0 : 100.0",
        "query.above_top_solid(variable.worldx, variable.worldz)",
        "(query.noise((variable.originx + 8) / 200.0, (variable.originz + 8) / 200.0) < -0.8) ? 5 : 10",
        "((query.noise((variable.originx + 8) / 200.0, (variable.originz + 8) / 200.0) >= -0.8) && variable.is_legacy == 0.0) ? 7 : 0",
        "math.random(-8, 8)",
        "variable.is_legacy == 0.0 ? 1 : 0",
        "math.max(query.above_top_solid(variable.worldx, variable.worldz), query.heightmap(variable.worldx, variable.worldz) - 4)",
        "math.ceil((query.noise(variable.originx / 80.0, variable.originz / 80.0) + 0.3) * 160.0)",
        "query.has_biome_tag('bee_habitat') ? (query.has_biome_tag('plains') || query.has_biome_tag('sunflower_plains') ? 5.0 : (query.has_biome_tag('flower_forest') ? 3.0 : (query.has_biome_tag('birch_forest') || query.has_biome_tag('forest') ? 0.035 : 0.0))) : 0.0",
        "query.last_hit_by_player ? (query.is_baby ? 12 : 5) + (Math.die_roll(query.equipment_count,1,3)) : 0",
        "Math.Min(query.player_level * 7, 100)",
        "query.last_hit_by_player ? 5: 0",
        "query.anger_level(this) >= 80",
        "2.0 - math.clamp(query.anger_level / 80 * 1.5, 0, 1.5)",
        "context.other->query.remaining_durability",
        "query.block_state('growth') == 1 && query.block_state('minecraft:cardinal_direction') == 'north'",
        "q.any_tag('minecraft:a', 'minecraft:b')",
        "variable.sea_level - 3",
        "!query.is_on_ground",
    ];

    #[test]
    fn every_vanilla_string_parses() {
        for src in VANILLA {
            parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        }
    }

    #[test]
    fn names_are_case_insensitive_and_aliases_resolve() {
        assert_eq!(
            parse("Math.Random(1, 3)").unwrap(),
            parse("math.random(1,3)").unwrap()
        );
        assert_eq!(parse("q.is_baby").unwrap(), parse("Query.IS_BABY").unwrap());
        assert_eq!(parse("v.x").unwrap(), parse("variable.x").unwrap());
        assert_eq!(parse("t.x").unwrap(), parse("temp.x").unwrap());
        assert_eq!(parse("c.other").unwrap(), parse("context.other").unwrap());
        // String literals keep their case.
        assert_eq!(parse("'Abc'").unwrap(), Expr::Str(Arc::from("Abc")));
    }

    #[test]
    fn precedence_and_associativity() {
        let e = parse("1 + 2 * 3").unwrap();
        assert!(matches!(e, Expr::Binary(BinOp::Add, _, _)));
        let e = parse("10 - 4 - 3").unwrap();
        let Expr::Binary(BinOp::Sub, lhs, _) = e else {
            panic!()
        };
        assert!(matches!(*lhs, Expr::Binary(BinOp::Sub, _, _)));
        let e = parse("a.b ?? 1").unwrap_err();
        assert!(e.contains("unsupported namespace"), "{e}");
        let e = parse("1 ? 2 : 0 ? 3 : 4").unwrap();
        let Expr::Ternary(_, _, else_) = e else {
            panic!()
        };
        assert!(matches!(*else_, Expr::Ternary(..)));
        assert!(matches!(parse("v.x ?? 2").unwrap(), Expr::Coalesce(..)));
        assert!(matches!(
            parse("q.is_baby ? 2").unwrap(),
            Expr::Conditional(..)
        ));
    }

    #[test]
    fn complex_expressions() {
        let e = parse("t.a = 2; t.b = t.a * 3; return t.b;").unwrap();
        let Expr::Program(stmts) = e else { panic!() };
        assert_eq!(stmts.len(), 3);
        assert!(matches!(
            parse("loop(3, {v.x = v.x + 1;});").unwrap(),
            Expr::Program(_)
        ));
        assert!(parse("c.other = 1").is_err());
        assert!(parse("1 = 2").is_err());
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "1 +",
            "math.max(1)",
            "math.nope(1)",
            "geometry.x",
            "foo",
            "'open",
            "1 2",
            "query.",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
