//! Typed intermediate representation of the BDS worldgen documents.
//!
//! Every parser here is strict: unknown keys, wrong types and dangling block
//! names are build errors that name the source document, so a schema change in
//! a future BDS release fails the build instead of silently generating
//! different worlds. Block descriptors are resolved to canonical runtime ids
//! (see [`crate::blocks`]); Molang strings are parsed and folded
//! (see [`crate::molang`]).

use crate::blocks::{BlockPalette, IdSet, parse_descriptor, parse_predicate};
use crate::error::{Error, Result};
use crate::molang::{self, Expr};
use crate::packs::Document;
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------
// Strict object access
// ---------------------------------------------------------------------------

/// A JSON object whose keys must all be consumed (see [`Obj::finish`]).
pub struct Obj<'a> {
    pub origin: &'a str,
    map: &'a Map<String, Value>,
    used: RefCell<BTreeSet<&'a str>>,
}

impl<'a> Obj<'a> {
    pub fn new(origin: &'a str, value: &'a Value) -> Result<Self> {
        let map = value
            .as_object()
            .ok_or_else(|| Error::data(origin, format!("expected an object, got {value}")))?;
        Ok(Self {
            origin,
            map,
            used: RefCell::default(),
        })
    }

    pub fn opt(&self, key: &'a str) -> Option<&'a Value> {
        let value = self.map.get(key)?;
        self.used.borrow_mut().insert(key);
        Some(value)
    }

    pub fn req(&self, key: &'a str) -> Result<&'a Value> {
        self.opt(key)
            .ok_or_else(|| Error::data(self.origin, format!("missing key {key:?}")))
    }

    /// Mark keys as known without reading them (documentation-only fields).
    pub fn ignore(&self, keys: &[&'a str]) {
        self.used.borrow_mut().extend(keys.iter().copied());
    }

    /// Error on any key that was never read.
    pub fn finish(self) -> Result<()> {
        let used = self.used.into_inner();
        match self.map.keys().find(|k| !used.contains(k.as_str())) {
            Some(extra) => Err(Error::data(self.origin, format!("unknown key {extra:?}"))),
            None => Ok(()),
        }
    }

    pub fn f32(&self, key: &'a str) -> Result<f32> {
        num(self.origin, key, self.req(key)?)
    }

    pub fn f32_or(&self, key: &'a str, default: f32) -> Result<f32> {
        self.opt(key).map_or(Ok(default), |v| num(self.origin, key, v))
    }

    pub fn i32(&self, key: &'a str) -> Result<i32> {
        int(self.origin, key, self.req(key)?)
    }

    pub fn i32_or(&self, key: &'a str, default: i32) -> Result<i32> {
        self.opt(key).map_or(Ok(default), |v| int(self.origin, key, v))
    }

    pub fn bool_or(&self, key: &'a str, default: bool) -> Result<bool> {
        match self.opt(key) {
            None => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(other) => Err(Error::data(self.origin, format!("{key} is not a bool: {other}"))),
        }
    }

    pub fn str(&self, key: &'a str) -> Result<&'a str> {
        self.req(key)?
            .as_str()
            .ok_or_else(|| Error::data(self.origin, format!("{key} is not a string")))
    }

    pub fn array(&self, key: &'a str) -> Result<&'a [Value]> {
        self.req(key)?
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| Error::data(self.origin, format!("{key} is not an array")))
    }
}

pub fn num(origin: &str, key: &str, v: &Value) -> Result<f32> {
    v.as_f64()
        .map(|f| f as f32)
        .ok_or_else(|| Error::data(origin, format!("{key} is not a number: {v}")))
}

pub fn int(origin: &str, key: &str, v: &Value) -> Result<i32> {
    match v.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() <= i32::MAX as f64 => Ok(f as i32),
        _ => Err(Error::data(origin, format!("{key} is not an integer: {v}"))),
    }
}

/// Parse a number-or-Molang value.
pub fn expr(origin: &str, v: &Value) -> Result<Expr> {
    match v {
        Value::Number(n) => Ok(Expr::Num(n.as_f64().expect("json number") as f32)),
        Value::Bool(b) => Ok(Expr::Num(if *b { 1.0 } else { 0.0 })),
        Value::String(s) => molang::parse(origin, s),
        other => Err(Error::data(origin, format!("expected a number or Molang, got {other}"))),
    }
}

/// `{"numerator": n, "denominator": d}` or a probability in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Chance {
    Fraction { numerator: i32, denominator: i32 },
    Probability(f32),
}

pub fn chance(origin: &str, v: &Value) -> Result<Chance> {
    match v {
        Value::Number(_) => Ok(Chance::Probability(num(origin, "chance", v)?)),
        Value::Object(_) => {
            let o = Obj::new(origin, v)?;
            let c = Chance::Fraction {
                numerator: o.i32("numerator")?,
                denominator: o.i32("denominator")?,
            };
            o.finish()?;
            Ok(c)
        }
        other => Err(Error::data(origin, format!("not a chance: {other}"))),
    }
}

/// `{"range_min": a, "range_max": b}` or a constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntRange {
    pub min: i32,
    pub max: i32,
}

pub fn int_range(origin: &str, v: &Value) -> Result<IntRange> {
    if v.is_number() {
        let n = int(origin, "range", v)?;
        return Ok(IntRange { min: n, max: n });
    }
    let o = Obj::new(origin, v)?;
    let r = match (o.opt("range_min"), o.opt("range_max"), o.opt("min"), o.opt("max")) {
        (Some(a), Some(b), None, None) | (None, None, Some(a), Some(b)) => IntRange {
            min: int(origin, "range_min", a)?,
            max: int(origin, "range_max", b)?,
        },
        _ => return Err(Error::data(origin, format!("not an int range: {v}"))),
    };
    o.finish()?;
    Ok(r)
}

/// Resolve a block descriptor to its runtime id.
pub fn block(palette: &BlockPalette, origin: &str, v: &Value) -> Result<u32> {
    palette.resolve(origin, &parse_descriptor(origin, v)?)
}

/// Resolve a single predicate or a list of predicates to the ids they match.
pub fn block_set(palette: &BlockPalette, origin: &str, v: &Value) -> Result<IdSet> {
    let items: &[Value] = match v {
        Value::Array(items) => items,
        single => std::slice::from_ref(single),
    };
    let mut set = IdSet::default();
    for item in items {
        set = set.union(&palette.matching(origin, &parse_predicate(origin, item)?)?);
    }
    Ok(set)
}

// ---------------------------------------------------------------------------
// Feature rules
// ---------------------------------------------------------------------------

/// BDS placement passes in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pass {
    Pregeneration,
    First,
    BeforeUnderground,
    Underground,
    AfterUnderground,
    BeforeSurface,
    Surface,
    AfterSurface,
    Final,
}

impl Pass {
    pub const ALL: [Pass; 9] = [
        Pass::Pregeneration,
        Pass::First,
        Pass::BeforeUnderground,
        Pass::Underground,
        Pass::AfterUnderground,
        Pass::BeforeSurface,
        Pass::Surface,
        Pass::AfterSurface,
        Pass::Final,
    ];

    fn parse(origin: &str, s: &str) -> Result<Self> {
        Ok(match s {
            "pregeneration_pass" => Pass::Pregeneration,
            "first_pass" => Pass::First,
            "before_underground_pass" => Pass::BeforeUnderground,
            "underground_pass" => Pass::Underground,
            "after_underground_pass" => Pass::AfterUnderground,
            "before_surface_pass" => Pass::BeforeSurface,
            "surface_pass" => Pass::Surface,
            "after_surface_pass" => Pass::AfterSurface,
            "final_pass" => Pass::Final,
            other => return Err(Error::data(origin, format!("unknown placement_pass {other:?}"))),
        })
    }

    /// Rust variant name used by the emitted code.
    pub fn variant(self) -> &'static str {
        match self {
            Pass::Pregeneration => "Pregeneration",
            Pass::First => "First",
            Pass::BeforeUnderground => "BeforeUnderground",
            Pass::Underground => "Underground",
            Pass::AfterUnderground => "AfterUnderground",
            Pass::BeforeSurface => "BeforeSurface",
            Pass::Surface => "Surface",
            Pass::AfterSurface => "AfterSurface",
            Pass::Final => "Final",
        }
    }
}

/// A `minecraft:biome_filter` over biome tags (BDS 1.26.51 rules only use
/// `has_biome_tag`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiomeFilter {
    All(Vec<BiomeFilter>),
    Any(Vec<BiomeFilter>),
    HasTag { tag: String, negate: bool },
}

impl BiomeFilter {
    pub fn parse(origin: &str, v: &Value) -> Result<Self> {
        match v {
            Value::Array(items) => Ok(BiomeFilter::All(
                items.iter().map(|i| Self::parse(origin, i)).collect::<Result<_>>()?,
            )),
            Value::Object(map) if map.contains_key("all_of") || map.contains_key("any_of") => {
                if map.len() != 1 {
                    return Err(Error::data(origin, format!("mixed filter group {v}")));
                }
                let (key, inner) = map.iter().next().expect("one key");
                let items = match inner {
                    Value::Array(items) => items.iter().map(|i| Self::parse(origin, i)).collect::<Result<_>>()?,
                    single => vec![Self::parse(origin, single)?],
                };
                Ok(if key == "all_of" {
                    BiomeFilter::All(items)
                } else {
                    BiomeFilter::Any(items)
                })
            }
            Value::Object(_) => {
                let o = Obj::new(origin, v)?;
                let test = o.str("test")?;
                if test != "has_biome_tag" {
                    return Err(Error::data(origin, format!("unsupported biome filter test {test:?}")));
                }
                let negate = match o.opt("operator").and_then(Value::as_str).unwrap_or("==") {
                    "==" | "equals" => false,
                    "!=" | "not" | "<>" => true,
                    other => return Err(Error::data(origin, format!("unsupported filter operator {other:?}"))),
                };
                if let Some(subject) = o.opt("subject").and_then(Value::as_str)
                    && subject != "self"
                {
                    return Err(Error::data(origin, format!("unsupported filter subject {subject:?}")));
                }
                let tag = o.str("value")?.to_owned();
                o.finish()?;
                Ok(BiomeFilter::HasTag { tag, negate })
            }
            other => Err(Error::data(origin, format!("not a biome filter: {other}"))),
        }
    }

    pub fn matches(&self, tags: &BTreeSet<String>) -> bool {
        match self {
            BiomeFilter::All(items) => items.iter().all(|f| f.matches(tags)),
            BiomeFilter::Any(items) => items.iter().any(|f| f.matches(tags)),
            BiomeFilter::HasTag { tag, negate } => tags.contains(tag) != *negate,
        }
    }
}

/// Coordinate axes, for `coordinate_eval_order`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// Random distribution of a scattered coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistKind {
    Uniform,
    Gaussian,
    InverseGaussian,
    Triangle,
    FixedGrid,
    JitteredGrid,
}

/// One scattered coordinate.
#[derive(Debug, Clone, PartialEq)]
pub enum Coord {
    /// A Molang expression: evaluates to an absolute world coordinate.
    Expr(Expr),
    /// A constant: an offset from the input position.
    Offset(i32),
    /// A random offset from the input position.
    Dist {
        kind: DistKind,
        extent: [Expr; 2],
        step_size: i32,
        grid_offset: i32,
    },
}

fn coord(origin: &str, v: &Value) -> Result<Coord> {
    match v {
        Value::Number(_) => Ok(Coord::Offset(int(origin, "coordinate", v)?)),
        Value::String(s) => Ok(match molang::parse(origin, s)? {
            Expr::Num(n) if n.fract() == 0.0 => Coord::Offset(n as i32),
            e => Coord::Expr(e),
        }),
        Value::Object(_) => {
            let o = Obj::new(origin, v)?;
            let kind = match o.opt("distribution").and_then(Value::as_str).unwrap_or("uniform") {
                "uniform" => DistKind::Uniform,
                "gaussian" => DistKind::Gaussian,
                "inverse_gaussian" => DistKind::InverseGaussian,
                "triangle" => DistKind::Triangle,
                "fixed_grid" => DistKind::FixedGrid,
                "jittered_grid" => DistKind::JitteredGrid,
                other => return Err(Error::data(origin, format!("unknown distribution {other:?}"))),
            };
            let extent = o.array("extent")?;
            let [a, b] = extent else {
                return Err(Error::data(origin, "extent needs two values"));
            };
            let c = Coord::Dist {
                kind,
                extent: [expr(origin, a)?, expr(origin, b)?],
                step_size: o.i32_or("step_size", 1)?,
                grid_offset: o.i32_or("grid_offset", 0)?,
            };
            o.finish()?;
            Ok(c)
        }
        other => Err(Error::data(origin, format!("not a coordinate: {other}"))),
    }
}

/// Scatter parameters shared by feature rules and `minecraft:scatter_feature`.
///
/// Semantics (see `bds-quirks.md`): `scatter_chance` is rolled once for the
/// whole scatter; each iteration evaluates the coordinates in
/// `coordinate_eval_order`, `variable.worldx/y/z` holding the coordinates
/// computed so far.
#[derive(Debug, Clone, PartialEq)]
pub struct Scatter {
    pub iterations: Expr,
    pub chance: Option<ScatterChance>,
    /// x, y, z.
    pub coords: [Coord; 3],
    pub order: [Axis; 3],
    pub project_input_to_floor: bool,
}

/// `scatter_chance`: a fraction or a Molang percentage in `[0, 100]`.
#[derive(Debug, Clone, PartialEq)]
pub enum ScatterChance {
    Fraction { numerator: i32, denominator: i32 },
    Percent(Expr),
}

impl Scatter {
    /// The no-op distribution of a rule without `distribution`: one placement
    /// at the chunk origin.
    pub fn once() -> Self {
        Scatter {
            iterations: Expr::Num(1.0),
            chance: None,
            coords: [Coord::Offset(0), Coord::Offset(0), Coord::Offset(0)],
            order: [Axis::X, Axis::Z, Axis::Y],
            project_input_to_floor: false,
        }
    }

    /// Parse scatter keys from `o` (rule `distribution` object or scatter feature body).
    pub fn parse_keys(o: &Obj<'_>) -> Result<Self> {
        let origin = o.origin;
        let iterations = expr(origin, o.req("iterations")?)?;
        let chance = match o.opt("scatter_chance") {
            None => None,
            Some(v @ Value::Object(_)) => match chance(origin, v)? {
                Chance::Fraction { numerator, denominator } => Some(ScatterChance::Fraction { numerator, denominator }),
                Chance::Probability(p) => Some(ScatterChance::Percent(Expr::Num(p * 100.0))),
            },
            Some(v) => Some(ScatterChance::Percent(expr(origin, v)?)),
        };
        let axis = |key| o.opt(key).map_or(Ok(Coord::Offset(0)), |v| coord(origin, v));
        let coords = [axis("x")?, axis("y")?, axis("z")?];
        let order_str = o.opt("coordinate_eval_order").and_then(Value::as_str).unwrap_or("xzy");
        let mut order = [Axis::X; 3];
        let mut seen = BTreeSet::new();
        if order_str.len() != 3 {
            return Err(Error::data(origin, format!("bad coordinate_eval_order {order_str:?}")));
        }
        for (slot, c) in order.iter_mut().zip(order_str.chars()) {
            *slot = match c {
                'x' => Axis::X,
                'y' => Axis::Y,
                'z' => Axis::Z,
                _ => return Err(Error::data(origin, format!("bad coordinate_eval_order {order_str:?}"))),
            };
            if !seen.insert(c) {
                return Err(Error::data(origin, format!("bad coordinate_eval_order {order_str:?}")));
            }
        }
        Ok(Scatter {
            iterations,
            chance,
            coords,
            order,
            project_input_to_floor: o.bool_or("project_input_to_floor", false)?,
        })
    }
}

/// A resolved `minecraft:feature_rules` document.
#[derive(Debug, Clone)]
pub struct FeatureRule {
    pub identifier: String,
    pub places_feature: String,
    pub pass: Pass,
    /// `None` when the rule has no biome filter (it then applies everywhere).
    pub biome_filter: Option<BiomeFilter>,
    pub scatter: Scatter,
    pub source: String,
}

impl FeatureRule {
    pub fn parse(doc: &Document) -> Result<Self> {
        let origin = doc.source.as_str();
        if doc.kind != "minecraft:feature_rules" {
            return Err(Error::data(origin, format!("expected minecraft:feature_rules, got {}", doc.kind)));
        }
        let o = Obj::new(origin, &doc.body)?;
        let desc = Obj::new(origin, o.req("description")?)?;
        let places_feature = desc.str("places_feature")?.to_owned();
        desc.ignore(&["identifier"]);
        desc.finish()?;
        let cond = Obj::new(origin, o.req("conditions")?)?;
        let pass = Pass::parse(origin, cond.str("placement_pass")?)?;
        let biome_filter = cond
            .opt("minecraft:biome_filter")
            .map(|f| BiomeFilter::parse(origin, f))
            .transpose()?;
        cond.finish()?;
        let scatter = match o.opt("distribution") {
            None => Scatter::once(),
            Some(d) => {
                let d = Obj::new(origin, d)?;
                let s = Scatter::parse_keys(&d)?;
                d.finish()?;
                s
            }
        };
        o.finish()?;
        Ok(FeatureRule {
            identifier: doc.identifier.clone(),
            places_feature,
            pass,
            biome_filter,
            scatter,
            source: doc.source.clone(),
        })
    }

    /// `true` when the rule can never place anything (constant zero iterations
    /// or zero scatter chance).
    pub fn is_disabled(&self) -> bool {
        let zero_iterations = self.scatter.iterations.as_const().is_some_and(|n| n <= 0.0);
        let zero_chance = match &self.scatter.chance {
            Some(ScatterChance::Fraction { numerator, .. }) => *numerator <= 0,
            Some(ScatterChance::Percent(e)) => e.as_const().is_some_and(|p| p <= 0.0),
            None => false,
        };
        zero_iterations || zero_chance
    }
}

// ---------------------------------------------------------------------------
// Features (generic envelope; type-specific parsing lives in the emitter)
// ---------------------------------------------------------------------------

/// Keys whose string values name other features.
const FEATURE_REF_KEYS: [&str; 6] = [
    "places_feature",
    "feature_to_snap",
    "feature_to_place",
    "vegetation_feature",
    "log_decoration_feature",
    "decoration_feature",
];

/// A feature document with the features it references.
#[derive(Debug, Clone)]
pub struct Feature {
    pub identifier: String,
    /// Schema key, e.g. `minecraft:ore_feature`.
    pub kind: String,
    pub body: Value,
    pub source: String,
    /// Identifiers of every feature this one can place, in document order.
    pub refs: Vec<String>,
}

impl Feature {
    pub fn parse(doc: &Document) -> Result<Self> {
        let mut refs = Vec::new();
        collect_feature_refs(&doc.body, &mut refs);
        Ok(Feature {
            identifier: doc.identifier.clone(),
            kind: doc.kind.clone(),
            body: doc.body.clone(),
            source: doc.source.clone(),
            refs,
        })
    }
}

fn collect_feature_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                if FEATURE_REF_KEYS.contains(&k.as_str()) {
                    if let Some(s) = v.as_str() {
                        out.push(s.to_owned());
                    }
                } else if k == "features" {
                    // aggregate/sequence: ["id", ...]; weighted_random: [["id", w], ...]
                    for item in v.as_array().into_iter().flatten() {
                        match item {
                            Value::String(s) => out.push(s.clone()),
                            Value::Array(pair) => {
                                if let Some(s) = pair.first().and_then(Value::as_str) {
                                    out.push(s.to_owned());
                                }
                            }
                            other => collect_feature_refs(other, out),
                        }
                    }
                } else {
                    collect_feature_refs(v, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_feature_refs(i, out)),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Biomes
// ---------------------------------------------------------------------------

/// Materials shared by the overworld-family surface builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Materials {
    pub top: u32,
    pub mid: u32,
    pub sea_floor: u32,
    pub foundation: u32,
    pub sea: u32,
    pub sea_floor_depth: i32,
}

/// `minecraft:surface_builder` / `minecraft:subsurface_builder`.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceBuilder {
    Overworld(Materials),
    FrozenOcean(Materials),
    Swamp {
        materials: Materials,
        max_puddle_depth_below_sea_level: i32,
    },
    Mesa {
        materials: Materials,
        clay: u32,
        hard_clay: u32,
        bryce_pillars: bool,
        has_forest: bool,
    },
    Capped {
        floor: Vec<u32>,
        ceiling: Vec<u32>,
        sea: u32,
        foundation: u32,
        beach: Option<u32>,
    },
    TheEnd,
    NoiseGradient {
        noise_name: String,
        first_octave: i32,
        amplitudes: Vec<f32>,
        non_replaceable: IdSet,
        /// `(min, max, block)` noise ranges.
        specifiers: Vec<(f32, f32, u32)>,
    },
}

/// One `surface_material_adjustments` entry (unset materials keep the builder's).
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialAdjustment {
    pub top: Option<u32>,
    pub mid: Option<u32>,
    pub sea_floor: Option<u32>,
    pub foundation: Option<u32>,
    pub sea: Option<u32>,
    pub noise_frequency_scale: f32,
    pub noise_range: [f32; 2],
    /// Inclusive y range as Molang (`variable.sea_level` is the only variable used).
    pub height_range: Option<[Expr; 2]>,
}

/// `minecraft:mountain_parameters`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MountainParameters {
    /// `(material, north, south, west, east)` slopes that get the steep material.
    pub steep_material: Option<(u32, [bool; 4])>,
    pub top_slide_enabled: Option<bool>,
}

/// A resolved biome document.
#[derive(Debug, Clone)]
pub struct Biome {
    pub identifier: String,
    /// Network/numeric biome id (protocolgen `biome_id_map.json`).
    pub id: u16,
    pub tags: BTreeSet<String>,
    pub temperature: f32,
    pub downfall: f32,
    pub snow_accumulation: Option<[f32; 2]>,
    pub surface: Option<SurfaceBuilder>,
    pub subsurface: Option<SurfaceBuilder>,
    pub adjustments: Vec<MaterialAdjustment>,
    pub mountain: Option<MountainParameters>,
    pub source: String,
}

fn materials(palette: &BlockPalette, o: &Obj<'_>) -> Result<Materials> {
    let origin = o.origin;
    Ok(Materials {
        top: block(palette, origin, o.req("top_material")?)?,
        mid: block(palette, origin, o.req("mid_material")?)?,
        sea_floor: block(palette, origin, o.req("sea_floor_material")?)?,
        foundation: block(palette, origin, o.req("foundation_material")?)?,
        sea: block(palette, origin, o.req("sea_material")?)?,
        sea_floor_depth: o.i32("sea_floor_depth")?,
    })
}

fn surface_builder(palette: &BlockPalette, origin: &str, v: &Value) -> Result<SurfaceBuilder> {
    let outer = Obj::new(origin, v)?;
    let o = Obj::new(origin, outer.req("builder")?)?;
    outer.finish()?;
    let ty = o.str("type")?;
    let blocks = |key| -> Result<Vec<u32>> {
        o.array(key)?.iter().map(|b| block(palette, origin, b)).collect()
    };
    let builder = match ty {
        "minecraft:overworld" => SurfaceBuilder::Overworld(materials(palette, &o)?),
        "minecraft:frozen_ocean" => SurfaceBuilder::FrozenOcean(materials(palette, &o)?),
        "minecraft:swamp" => SurfaceBuilder::Swamp {
            materials: materials(palette, &o)?,
            max_puddle_depth_below_sea_level: o.i32("max_puddle_depth_below_sea_level")?,
        },
        "minecraft:mesa" => SurfaceBuilder::Mesa {
            materials: materials(palette, &o)?,
            clay: block(palette, origin, o.req("clay_material")?)?,
            hard_clay: block(palette, origin, o.req("hard_clay_material")?)?,
            bryce_pillars: o.bool_or("bryce_pillars", false)?,
            has_forest: o.bool_or("has_forest", false)?,
        },
        "minecraft:capped" => SurfaceBuilder::Capped {
            floor: blocks("floor_materials")?,
            ceiling: blocks("ceiling_materials")?,
            sea: block(palette, origin, o.req("sea_material")?)?,
            foundation: block(palette, origin, o.req("foundation_material")?)?,
            beach: o.opt("beach_material").map(|b| block(palette, origin, b)).transpose()?,
        },
        "minecraft:the_end" => SurfaceBuilder::TheEnd,
        "minecraft:noise_gradient" => {
            let noise = Obj::new(origin, o.req("noise_descriptor")?)?;
            let noise_name = noise.str("name")?.to_owned();
            let first_octave = noise.i32("first_octave")?;
            let amplitudes = noise
                .array("amplitudes")?
                .iter()
                .map(|a| num(origin, "amplitudes", a))
                .collect::<Result<_>>()?;
            noise.finish()?;
            let mut specifiers = Vec::new();
            for spec in o.array("noise_block_specifiers")? {
                let s = Obj::new(origin, spec)?;
                let range = Obj::new(origin, s.req("range")?)?;
                specifiers.push((range.f32("min")?, range.f32("max")?, block(palette, origin, s.req("block")?)?));
                range.finish()?;
                s.finish()?;
            }
            SurfaceBuilder::NoiseGradient {
                noise_name,
                first_octave,
                amplitudes,
                non_replaceable: block_set(palette, origin, o.req("non_replaceable_blocks")?)?,
                specifiers,
            }
        }
        other => return Err(Error::data(origin, format!("unknown surface builder type {other:?}"))),
    };
    o.finish()?;
    Ok(builder)
}

impl Biome {
    pub fn parse(doc: &Document, palette: &BlockPalette, ids: &BTreeMap<String, u16>) -> Result<Self> {
        let origin = doc.source.as_str();
        let o = Obj::new(origin, &doc.body)?;
        o.ignore(&["description"]);
        let c = Obj::new(origin, o.req("components")?)?;
        o.finish()?;
        let tags = Obj::new(origin, c.req("minecraft:tags")?)?;
        let tag_set = tags
            .array("tags")?
            .iter()
            .map(|t| t.as_str().map(str::to_owned).ok_or_else(|| Error::data(origin, "tag is not a string")))
            .collect::<Result<BTreeSet<_>>>()?;
        tags.finish()?;
        let climate = Obj::new(origin, c.req("minecraft:climate")?)?;
        let temperature = climate.f32("temperature")?;
        let downfall = climate.f32("downfall")?;
        let snow_accumulation = match climate.opt("snow_accumulation") {
            None => None,
            Some(v) => match v.as_array().map(Vec::as_slice) {
                Some([a, b]) => Some([num(origin, "snow_accumulation", a)?, num(origin, "snow_accumulation", b)?]),
                _ => return Err(Error::data(origin, "snow_accumulation needs two numbers")),
            },
        };
        climate.finish()?;
        let surface = c
            .opt("minecraft:surface_builder")
            .map(|v| surface_builder(palette, origin, v))
            .transpose()?;
        let subsurface = c
            .opt("minecraft:subsurface_builder")
            .map(|v| surface_builder(palette, origin, v))
            .transpose()?;
        let mut adjustments = Vec::new();
        if let Some(adj) = c.opt("minecraft:surface_material_adjustments") {
            let adj = Obj::new(origin, adj)?;
            for a in adj.array("adjustments")? {
                adjustments.push(material_adjustment(palette, origin, a)?);
            }
            adj.finish()?;
        }
        let mountain = c
            .opt("minecraft:mountain_parameters")
            .map(|m| mountain_parameters(palette, origin, m))
            .transpose()?;
        // Components that do not affect block generation in the overworld pipeline
        // (climate → biome placement is the Java parameter table, see
        // thoughts/shared/research/2026-10-24-data-driven-world.md §2.4), plus
        // client tints and spawning knobs consumed by other systems.
        c.ignore(&[
            "minecraft:overworld_height",
            "minecraft:overworld_generation_rules",
            "minecraft:multinoise_generation_rules",
            "minecraft:village_type",
            "minecraft:map_tints",
            "minecraft:creature_spawn_probability",
            "minecraft:partially_frozen",
            "minecraft:humidity",
        ]);
        c.finish()?;
        let id = *ids
            .get(&doc.identifier)
            .ok_or_else(|| Error::data(origin, format!("{} has no numeric id", doc.identifier)))?;
        Ok(Biome {
            identifier: doc.identifier.clone(),
            id,
            tags: tag_set,
            temperature,
            downfall,
            snow_accumulation,
            surface,
            subsurface,
            adjustments,
            mountain,
            source: doc.source.clone(),
        })
    }
}

fn material_adjustment(palette: &BlockPalette, origin: &str, v: &Value) -> Result<MaterialAdjustment> {
    let o = Obj::new(origin, v)?;
    let m = Obj::new(origin, o.req("materials")?)?;
    let get = |key| m.opt(key).map(|b| block(palette, origin, b)).transpose();
    let (top, mid, sea_floor, foundation, sea) = (
        get("top_material")?,
        get("mid_material")?,
        get("sea_floor_material")?,
        get("foundation_material")?,
        get("sea_material")?,
    );
    m.finish()?;
    let range = o.array("noise_range")?;
    let [lo, hi] = range else {
        return Err(Error::data(origin, "noise_range needs two numbers"));
    };
    let height_range = match o.opt("height_range") {
        None => None,
        Some(Value::Array(r)) if r.len() == 2 => Some([expr(origin, &r[0])?, expr(origin, &r[1])?]),
        Some(other) => return Err(Error::data(origin, format!("bad height_range {other}"))),
    };
    let adj = MaterialAdjustment {
        top,
        mid,
        sea_floor,
        foundation,
        sea,
        // Mojang's biome documentation gives 0.0625 as the default scale.
        noise_frequency_scale: o.f32_or("noise_frequency_scale", 0.0625)?,
        noise_range: [num(origin, "noise_range", lo)?, num(origin, "noise_range", hi)?],
        height_range,
    };
    o.finish()?;
    Ok(adj)
}

fn mountain_parameters(palette: &BlockPalette, origin: &str, v: &Value) -> Result<MountainParameters> {
    let o = Obj::new(origin, v)?;
    let mut out = MountainParameters::default();
    if let Some(steep) = o.opt("steep_material_adjustment") {
        let s = Obj::new(origin, steep)?;
        let material = block(palette, origin, s.req("material")?)?;
        let slopes = [
            s.bool_or("north_slopes", false)?,
            s.bool_or("south_slopes", false)?,
            s.bool_or("west_slopes", false)?,
            s.bool_or("east_slopes", false)?,
        ];
        s.finish()?;
        out.steep_material = Some((material, slopes));
    }
    if let Some(slide) = o.opt("top_slide") {
        let s = Obj::new(origin, slide)?;
        out.top_slide_enabled = Some(s.bool_or("enabled", true)?);
        s.finish()?;
    }
    o.finish()?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Spawn rules (data family only; the spawner is a later slice)
// ---------------------------------------------------------------------------

/// A resolved `minecraft:spawn_rules` document. Conditions are kept as their
/// component maps; the shared filter language (ENTITIES) will type them.
#[derive(Debug, Clone)]
pub struct SpawnRule {
    pub identifier: String,
    pub population_control: String,
    /// One entry per condition, each a `component name -> value` map.
    pub conditions: Vec<BTreeMap<String, Value>>,
    pub source: String,
}

impl SpawnRule {
    pub fn parse(doc: &Document) -> Result<Self> {
        let origin = doc.source.as_str();
        if doc.kind != "minecraft:spawn_rules" {
            return Err(Error::data(origin, format!("expected minecraft:spawn_rules, got {}", doc.kind)));
        }
        let o = Obj::new(origin, &doc.body)?;
        let desc = Obj::new(origin, o.req("description")?)?;
        let population_control = desc.str("population_control")?.to_owned();
        desc.ignore(&["identifier"]);
        desc.finish()?;
        let conditions = o
            .opt("conditions")
            .map(|c| {
                c.as_array()
                    .ok_or_else(|| Error::data(origin, "conditions is not an array"))?
                    .iter()
                    .map(|cond| {
                        cond.as_object()
                            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .ok_or_else(|| Error::data(origin, "condition is not an object"))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        o.finish()?;
        Ok(SpawnRule {
            identifier: doc.identifier.clone(),
            population_control,
            conditions,
            source: doc.source.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::parse_document;
    use serde_json::json;

    fn doc(v: Value) -> Document {
        parse_document("test.json", v.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn rule_parses_scatter_and_filter() {
        let rule = FeatureRule::parse(&doc(json!({
            "format_version": "1.13.0",
            "minecraft:feature_rules": {
                "description": {"identifier": "minecraft:r", "places_feature": "minecraft:f"},
                "conditions": {
                    "placement_pass": "surface_pass",
                    "minecraft:biome_filter": [
                        {"test": "has_biome_tag", "value": "ocean"},
                        {"test": "has_biome_tag", "operator": "not", "value": "deep"},
                        {"any_of": [{"test": "has_biome_tag", "value": "warm"}, {"test": "has_biome_tag", "value": "cold"}]}
                    ]
                },
                "distribution": {
                    "iterations": "variable.is_legacy ? 0 : 3",
                    "scatter_chance": {"numerator": 1, "denominator": 4},
                    "coordinate_eval_order": "zxy",
                    "x": {"distribution": "uniform", "extent": [0, 15]},
                    "y": "query.heightmap(variable.worldx, variable.worldz)",
                    "z": 7
                }
            }
        })))
        .unwrap();
        assert_eq!(rule.pass, Pass::Surface);
        assert_eq!(rule.scatter.iterations, Expr::Num(3.0));
        assert_eq!(rule.scatter.order, [Axis::Z, Axis::X, Axis::Y]);
        assert_eq!(rule.scatter.coords[2], Coord::Offset(7));
        assert!(matches!(rule.scatter.coords[1], Coord::Expr(_)));
        let filter = rule.biome_filter.unwrap();
        let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
        assert!(filter.matches(&tags(&["ocean", "warm"])));
        assert!(!filter.matches(&tags(&["ocean", "warm", "deep"])));
        assert!(!filter.matches(&tags(&["ocean"])));
    }

    #[test]
    fn unknown_keys_and_passes_are_rejected() {
        let base = |extra: Value| {
            json!({"minecraft:feature_rules": {
                "description": {"identifier": "minecraft:r", "places_feature": "minecraft:f"},
                "conditions": extra
            }})
        };
        let err = FeatureRule::parse(&doc(base(json!({"placement_pass": "surface_pass", "bogus": 1}))))
            .unwrap_err()
            .to_string();
        assert!(err.contains("bogus"), "{err}");
        assert!(FeatureRule::parse(&doc(base(json!({"placement_pass": "nope_pass"})))).is_err());
    }

    #[test]
    fn disabled_rules_are_detected() {
        let mut s = Scatter::once();
        let rule = |s: Scatter| FeatureRule {
            identifier: String::new(),
            places_feature: String::new(),
            pass: Pass::Surface,
            biome_filter: None,
            scatter: s,
            source: String::new(),
        };
        assert!(!rule(s.clone()).is_disabled());
        s.iterations = Expr::Num(0.0);
        assert!(rule(s.clone()).is_disabled());
        s.iterations = Expr::Num(2.0);
        s.chance = Some(ScatterChance::Percent(Expr::Num(0.0)));
        assert!(rule(s).is_disabled());
    }

    #[test]
    fn feature_refs_cover_every_reference_shape() {
        let f = Feature::parse(&doc(json!({"minecraft:weighted_random_feature": {
            "description": {"identifier": "minecraft:w"},
            "features": [["minecraft:a", 3], ["minecraft:b", 1]],
            "nested": {"places_feature": "minecraft:c", "trunk": {"log_decoration_feature": "minecraft:d"}}
        }})))
        .unwrap();
        assert_eq!(f.refs, ["minecraft:a", "minecraft:b", "minecraft:c", "minecraft:d"]);
    }
}
