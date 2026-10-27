//! Bedrock filters: parsing every accepted spelling, one canonical form, and
//! evaluation against a caller-provided context.
//!
//! # Accepted spellings (parser fidelity)
//!
//! | Concern | Accepted | Canonical |
//! |---|---|---|
//! | Group | `all_of` / `all` / `AND`, `any_of` / `any` / `OR`, `none_of` / `NOT`; a bare array is `all_of`; a group value may be one filter instead of an array; `{}` is an empty `all_of` | `{"all_of": [...]}`, `{"any_of": [...]}`, `{"none_of": [...]}` |
//! | Subject | `self`, `other`, `parent`, `player`, `target`, `block`, `damager`, `baby`, `holder`, `item`; compact integers `0` (self) and `1` (other); omitted = `self` | the name |
//! | Operator | `==` / `equals` / `=`, `!=` / `not` / `<>`, `<`, `<=`, `>`, `>=`; compact integers `0` (equals) and `1` (not equals); omitted = equals | `==`, `!=`, `<`, `<=`, `>`, `>=` |
//! | Value | per test kind ([`ValueKind`]); omitted = `true` for boolean tests, the documented default for slot tests; compact integer difficulties `0..=3` | typed JSON value |
//!
//! The integer subjects and operators are the compact form found in 27 vanilla
//! entity files (bedrock-samples 1.26.50.4 and the BDS 1.26.51 packs); only the
//! integers vanilla uses are accepted. An unknown test name is kept and logged
//! as a [`Diagnostic`](crate::Diagnostic) and evaluates to `false`.
//!
//! # Evaluation
//!
//! Filters are pure: the caller implements [`FilterContext::fact`] for the
//! tests it supports, returning `None` when the subject is absent from the
//! event context (BDS: such a test is false, whatever the operator). Boolean
//! tests compare the fact with the value, numeric tests compare numbers with
//! the operator, and matching tests (`is_family`, `has_component`, ...) ask the
//! caller whether the subject matches the value and apply `==`/`!=`.

use crate::{Diagnostics, ParseError};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::sync::LazyLock;

include!(concat!(env!("OUT_DIR"), "/filter_subject.rs"));

impl Subject {
    fn parse(value: &Value, path: &str) -> Result<Self, ParseError> {
        match value {
            Value::String(s) => Self::from_name(s)
                .ok_or_else(|| ParseError::new(path, format!("unknown subject {s:?}"))),
            Value::Number(n) => n
                .as_i64()
                .and_then(Self::from_id)
                .ok_or_else(|| ParseError::new(path, format!("unknown compact subject {n}"))),
            other => Err(ParseError::new(
                path,
                format!("subject must be a string, got {other}"),
            )),
        }
    }
}

/// Comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operator {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Operator {
    pub fn canonical(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }

    pub fn from_spelling(s: &str) -> Option<Self> {
        Some(match s {
            "==" | "equals" | "=" => Self::Eq,
            "!=" | "not" | "<>" => Self::Ne,
            "<" => Self::Lt,
            "<=" => Self::Le,
            ">" => Self::Gt,
            ">=" => Self::Ge,
            _ => return None,
        })
    }

    fn parse(value: &Value, path: &str) -> Result<Self, ParseError> {
        match value {
            Value::String(s) => Self::from_spelling(s)
                .ok_or_else(|| ParseError::new(path, format!("unknown operator {s:?}"))),
            Value::Number(n) => match n.as_u64() {
                Some(0) => Ok(Self::Eq),
                Some(1) => Ok(Self::Ne),
                _ => Err(ParseError::new(
                    path,
                    format!(
                        "compact operator {n} is not one vanilla uses (0 = equals, 1 = not equals)"
                    ),
                )),
            },
            other => Err(ParseError::new(
                path,
                format!("operator must be a string, got {other}"),
            )),
        }
    }

    /// Apply to an ordering of `fact` relative to `value`.
    fn holds(self, ordering: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering::*;
        match self {
            Self::Eq => ordering == Equal,
            Self::Ne => ordering != Equal,
            Self::Lt => ordering == Less,
            Self::Le => ordering != Greater,
            Self::Gt => ordering == Greater,
            Self::Ge => ordering != Less,
        }
    }
}

/// How a test's `value` is typed and compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// JSON boolean, default `true`; the fact is a boolean.
    Bool,
    /// JSON number (integers in vanilla data); compared numerically.
    Int,
    /// JSON number; compared numerically.
    Float,
    /// JSON string; the caller decides whether the subject matches it.
    Str,
    /// `peaceful`/`easy`/`normal`/`hard`, or the compact integers `0..=3`; matched by the caller.
    Difficulty,
}

/// Static description of a filter test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TestDef {
    pub name: &'static str,
    pub value: ValueKind,
    /// Value used when the JSON omits `value` (`None`: the value is required).
    pub default: Option<DefaultValue>,
}

/// A default `value`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DefaultValue {
    Bool(bool),
    Str(&'static str),
}

macro_rules! builtin_tests {
    ($($variant:ident = $name:literal : $kind:ident $(= $default:expr)?,)*) => {
        /// Every filter test implemented by unastar, named as in pack JSON.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum BuiltinTest {
            $($variant,)*
        }

        /// Definitions of [`BuiltinTest`] in discriminant order.
        pub static BUILTIN_TESTS: &[TestDef] = &[
            $(TestDef {
                name: $name,
                value: ValueKind::$kind,
                default: builtin_tests!(@default $kind $(, $default)?),
            },)*
        ];

        impl BuiltinTest {
            pub const ALL: &'static [BuiltinTest] = &[$(BuiltinTest::$variant,)*];
        }
    };
    (@default Bool) => { Some(DefaultValue::Bool(true)) };
    (@default $kind:ident) => { None };
    (@default $kind:ident, $default:expr) => { Some($default) };
}

// Names and value kinds follow Mojang's filter reference; every test used by
// vanilla entity, spawn-rule and trading files is here (a coverage test fails
// otherwise).
builtin_tests! {
    ActorHasItemWithEnchantmentInSlot = "actor_has_item_with_enchantment_in_slot": Str,
    ActorHealth = "actor_health": Int,
    AllSlotsEmpty = "all_slots_empty": Str = DefaultValue::Str("any"),
    AnySlotEmpty = "any_slot_empty": Str = DefaultValue::Str("any"),
    BoolProperty = "bool_property": Bool,
    ClockTime = "clock_time": Float,
    DistanceToNearestPlayer = "distance_to_nearest_player": Float,
    EnumProperty = "enum_property": Str,
    FloatProperty = "float_property": Float,
    HasAbility = "has_ability": Str,
    HasBiomeTag = "has_biome_tag": Str,
    HasComponent = "has_component": Str,
    HasContainerOpen = "has_container_open": Bool,
    HasDamage = "has_damage": Str,
    HasDamagedEquipment = "has_damaged_equipment": Str,
    HasEquipment = "has_equipment": Str,
    HasEquipmentTag = "has_equipment_tag": Str,
    HasMobEffect = "has_mob_effect": Str,
    HasNametag = "has_nametag": Bool,
    HasProperty = "has_property": Str,
    HasRangedWeapon = "has_ranged_weapon": Bool,
    HasSameEquipmentInSlotAs = "has_same_equipment_in_slot_as": Bool,
    HasSilkTouch = "has_silk_touch": Bool,
    HasTag = "has_tag": Str,
    HasTarget = "has_target": Bool,
    HasTradeSupply = "has_trade_supply": Bool,
    HomeDistance = "home_distance": Float,
    HourlyClockTime = "hourly_clock_time": Int,
    InBlock = "in_block": Str,
    InCaravan = "in_caravan": Bool,
    InClouds = "in_clouds": Bool,
    InContactWithWater = "in_contact_with_water": Bool,
    InLava = "in_lava": Bool,
    InNether = "in_nether": Bool,
    InOverworld = "in_overworld": Bool,
    InWater = "in_water": Bool,
    InWaterOrRain = "in_water_or_rain": Bool,
    InactivityTimer = "inactivity_timer": Int,
    IntProperty = "int_property": Int,
    IsAltitude = "is_altitude": Int,
    IsAvoidingMobs = "is_avoiding_mobs": Bool,
    IsBaby = "is_baby": Bool,
    IsBiome = "is_biome": Str,
    IsBlock = "is_block": Str,
    IsBoundToCreakingHeart = "is_bound_to_creaking_heart": Bool,
    IsBrightness = "is_brightness": Float,
    IsClimbing = "is_climbing": Bool,
    IsColor = "is_color": Str,
    IsControllingPassengerFamily = "is_controlling_passenger_family": Str,
    IsDaytime = "is_daytime": Bool,
    IsDifficulty = "is_difficulty": Difficulty,
    IsFamily = "is_family": Str,
    IsGameRule = "is_game_rule": Bool,
    IsHumid = "is_humid": Bool,
    IsImmobile = "is_immobile": Bool,
    IsInVillage = "is_in_village": Bool,
    IsLeashed = "is_leashed": Bool,
    IsLeashedTo = "is_leashed_to": Bool,
    IsMarkVariant = "is_mark_variant": Int,
    IsMissingHealth = "is_missing_health": Bool,
    IsMoving = "is_moving": Bool,
    IsNavigating = "is_navigating": Bool,
    IsOwner = "is_owner": Bool,
    IsPanicking = "is_panicking": Bool,
    IsPersistent = "is_persistent": Bool,
    IsRaider = "is_raider": Bool,
    IsRiding = "is_riding": Bool,
    IsRidingSelf = "is_riding_self": Bool,
    IsSitting = "is_sitting": Bool,
    IsSkinId = "is_skin_id": Int,
    IsSleeping = "is_sleeping": Bool,
    IsSneakHeld = "is_sneak_held": Bool,
    IsSneaking = "is_sneaking": Bool,
    IsSnowCovered = "is_snow_covered": Bool,
    IsSprinting = "is_sprinting": Bool,
    IsTamed = "is_tamed": Bool,
    IsTarget = "is_target": Bool,
    IsTemperatureType = "is_temperature_type": Str,
    IsTemperatureValue = "is_temperature_value": Float,
    IsUnderground = "is_underground": Bool,
    IsUnderwater = "is_underwater": Bool,
    IsVariant = "is_variant": Int,
    IsVehicleFamily = "is_vehicle_family": Str,
    IsVisible = "is_visible": Bool,
    IsWaterlogged = "is_waterlogged": Bool,
    LightLevel = "light_level": Int,
    MoonIntensity = "moon_intensity": Float,
    MoonPhase = "moon_phase": Int,
    OnFire = "on_fire": Bool,
    OnGround = "on_ground": Bool,
    OnHotBlock = "on_hot_block": Bool,
    OnLadder = "on_ladder": Bool,
    OwnerDistance = "owner_distance": Float,
    RandomChance = "random_chance": Int,
    RedstoneStrengthAtPosition = "redstone_strength_at_position": Int,
    RiderCount = "rider_count": Int,
    SurfaceMob = "surface_mob": Bool,
    TakingFireDamage = "taking_fire_damage": Bool,
    TargetDistance = "target_distance": Float,
    Trusts = "trusts": Bool,
    WasLastHurtBy = "was_last_hurt_by": Bool,
    Weather = "weather": Str,
    WeatherAtPosition = "weather_at_position": Str,
    YRotation = "y_rotation": Float,
}

impl BuiltinTest {
    pub fn def(self) -> &'static TestDef {
        &BUILTIN_TESTS[self as usize]
    }

    pub fn name(self) -> &'static str {
        self.def().name
    }
}

/// Identifier of a registered test: built-ins first, then tests registered
/// by addons/plugins before the registry freezes (D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TestId(pub u16);

impl TestId {
    pub fn builtin(self) -> Option<BuiltinTest> {
        BuiltinTest::ALL.get(self.0 as usize).copied()
    }
}

impl From<BuiltinTest> for TestId {
    fn from(test: BuiltinTest) -> Self {
        Self(test as u16)
    }
}

/// Names → test definitions. Built-ins are always present.
#[derive(Debug, Clone)]
pub struct FilterRegistry {
    defs: Vec<TestDef>,
    by_name: HashMap<&'static str, TestId>,
}

impl Default for FilterRegistry {
    fn default() -> Self {
        let defs = BUILTIN_TESTS.to_vec();
        let by_name = defs
            .iter()
            .enumerate()
            .map(|(i, d)| (d.name, TestId(i as u16)))
            .collect();
        Self { defs, by_name }
    }
}

impl FilterRegistry {
    /// The shared registry with the built-in tests only.
    pub fn vanilla() -> &'static FilterRegistry {
        static VANILLA: LazyLock<FilterRegistry> = LazyLock::new(FilterRegistry::default);
        &VANILLA
    }

    /// Register an extra test (addon/plugin) before parsing data that uses it.
    /// Fails if the name is taken.
    pub fn register(&mut self, def: TestDef) -> Result<TestId, String> {
        if self.by_name.contains_key(def.name) {
            return Err(format!("filter test {} is already registered", def.name));
        }
        let id = TestId(self.defs.len() as u16);
        self.by_name.insert(def.name, id);
        self.defs.push(def);
        Ok(id)
    }

    pub fn lookup(&self, name: &str) -> Option<TestId> {
        self.by_name.get(name).copied()
    }

    pub fn def(&self, id: TestId) -> &TestDef {
        &self.defs[id.0 as usize]
    }
}

/// A parsed filter.
#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    Test(FilterTest),
    AllOf(Vec<Filter>),
    AnyOf(Vec<Filter>),
    NoneOf(Vec<Filter>),
}

/// One test with its subject, operator, domain and typed value.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterTest {
    /// Name as written (kept for unknown tests and for the canonical form).
    pub name: String,
    /// `None` for a test no registry knows (evaluates to `false`).
    pub id: Option<TestId>,
    pub subject: Subject,
    pub operator: Operator,
    /// `domain` as written (a string in vanilla data except one slot index).
    pub domain: Option<Value>,
    pub value: FilterValue,
}

/// A typed test value.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    Bool(bool),
    Number(Number),
    Str(String),
    /// Value of an unknown test, kept verbatim (absent = `null`).
    Raw(Value),
}

impl FilterValue {
    fn to_json(&self) -> Value {
        match self {
            Self::Bool(b) => Value::Bool(*b),
            Self::Number(n) => Value::Number(n.clone()),
            Self::Str(s) => Value::String(s.clone()),
            Self::Raw(v) => v.clone(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => n.as_f64(),
            _ => None,
        }
    }
}

const DIFFICULTIES: [&str; 4] = ["peaceful", "easy", "normal", "hard"];

const GROUP_KEYS: [(&str, GroupKind); 7] = [
    ("all_of", GroupKind::All),
    ("all", GroupKind::All),
    ("AND", GroupKind::All),
    ("any_of", GroupKind::Any),
    ("any", GroupKind::Any),
    ("OR", GroupKind::Any),
    ("none_of", GroupKind::None),
];
const NOT_KEY: &str = "NOT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    All,
    Any,
    None,
}

impl Filter {
    /// `true` for the filter that always passes (`{}` / `[]`).
    pub fn always() -> Self {
        Self::AllOf(Vec::new())
    }

    /// Parse any accepted spelling. `path` names the location in errors and
    /// diagnostics.
    pub fn parse(
        value: &Value,
        registry: &FilterRegistry,
        path: &str,
        diags: &mut Diagnostics,
    ) -> Result<Self, ParseError> {
        match value {
            Value::Array(items) => Ok(Self::AllOf(parse_list(items, registry, path, diags)?)),
            Value::Object(map) if map.contains_key("test") => {
                FilterTest::parse(map, registry, path, diags).map(Self::Test)
            }
            Value::Object(map) => {
                let mut groups = Vec::new();
                for (key, member) in map {
                    let member_path = format!("{path}/{key}");
                    let kind = if key == NOT_KEY {
                        GroupKind::None
                    } else if let Some((_, kind)) = GROUP_KEYS.iter().find(|(k, _)| k == key) {
                        *kind
                    } else {
                        diags.push(member_path, "unknown filter key ignored");
                        continue;
                    };
                    let items = match member {
                        Value::Array(items) => parse_list(items, registry, &member_path, diags)?,
                        Value::Object(_) => {
                            vec![Self::parse(member, registry, &member_path, diags)?]
                        }
                        other => {
                            return Err(ParseError::new(
                                member_path,
                                format!("filter group must be an array or object, got {other}"),
                            ));
                        }
                    };
                    groups.push(match kind {
                        GroupKind::All => Self::AllOf(items),
                        GroupKind::Any => Self::AnyOf(items),
                        GroupKind::None => Self::NoneOf(items),
                    });
                }
                Ok(match groups.len() {
                    0 => Self::always(),
                    1 => groups.pop().expect("one group"),
                    _ => Self::AllOf(groups),
                })
            }
            other => Err(ParseError::new(
                path,
                format!("filter must be an object or array, got {other}"),
            )),
        }
    }

    /// The canonical JSON form (readable spelling, every field explicit).
    pub fn to_json(&self) -> Value {
        let group = |key: &str, items: &[Filter]| {
            let mut map = Map::new();
            map.insert(
                key.to_owned(),
                Value::Array(items.iter().map(Filter::to_json).collect()),
            );
            Value::Object(map)
        };
        match self {
            Self::Test(test) => test.to_json(),
            Self::AllOf(items) => group("all_of", items),
            Self::AnyOf(items) => group("any_of", items),
            Self::NoneOf(items) => group("none_of", items),
        }
    }

    /// Visit every test.
    pub fn for_each_test<'a>(&'a self, f: &mut impl FnMut(&'a FilterTest)) {
        match self {
            Self::Test(test) => f(test),
            Self::AllOf(items) | Self::AnyOf(items) | Self::NoneOf(items) => {
                items.iter().for_each(|item| item.for_each_test(f))
            }
        }
    }

    /// Evaluate against `ctx`.
    pub fn evaluate(&self, ctx: &dyn FilterContext) -> bool {
        match self {
            Self::Test(test) => test.evaluate(ctx),
            Self::AllOf(items) => items.iter().all(|f| f.evaluate(ctx)),
            Self::AnyOf(items) => items.iter().any(|f| f.evaluate(ctx)),
            Self::NoneOf(items) => !items.iter().any(|f| f.evaluate(ctx)),
        }
    }
}

fn parse_list(
    items: &[Value],
    registry: &FilterRegistry,
    path: &str,
    diags: &mut Diagnostics,
) -> Result<Vec<Filter>, ParseError> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| Filter::parse(item, registry, &format!("{path}/{i}"), diags))
        .collect()
}

impl FilterTest {
    fn parse(
        map: &Map<String, Value>,
        registry: &FilterRegistry,
        path: &str,
        diags: &mut Diagnostics,
    ) -> Result<Self, ParseError> {
        let name = match &map["test"] {
            Value::String(s) => s.clone(),
            other => {
                return Err(ParseError::new(
                    format!("{path}/test"),
                    format!("test must be a string, got {other}"),
                ));
            }
        };
        for key in map.keys() {
            if !matches!(
                key.as_str(),
                "test" | "subject" | "operator" | "domain" | "value"
            ) {
                diags.push(
                    format!("{path}/{key}"),
                    format!("unknown key in {name} test ignored"),
                );
            }
        }
        let subject = match map.get("subject") {
            Some(v) => Subject::parse(v, &format!("{path}/subject"))?,
            None => Subject::Self_,
        };
        let operator = match map.get("operator") {
            Some(v) => Operator::parse(v, &format!("{path}/operator"))?,
            None => Operator::Eq,
        };
        let domain = map.get("domain").cloned();
        let id = registry.lookup(&name);
        let value_path = format!("{path}/value");
        let raw = map.get("value");
        let value = match id {
            None => {
                diags.push(
                    path,
                    format!("unknown filter test {name:?} (evaluates to false)"),
                );
                FilterValue::Raw(raw.cloned().unwrap_or(Value::Null))
            }
            Some(id) => parse_value(registry.def(id), raw, &value_path)?,
        };
        Ok(Self {
            name,
            id,
            subject,
            operator,
            domain,
            value,
        })
    }

    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("test".into(), Value::String(self.name.clone()));
        map.insert("subject".into(), Value::String(self.subject.name().into()));
        map.insert(
            "operator".into(),
            Value::String(self.operator.canonical().into()),
        );
        if let Some(domain) = &self.domain {
            map.insert("domain".into(), domain.clone());
        }
        map.insert("value".into(), self.value.to_json());
        Value::Object(map)
    }

    /// The built-in test, if this is one.
    pub fn builtin(&self) -> Option<BuiltinTest> {
        self.id.and_then(TestId::builtin)
    }

    /// `domain` as a string (`None` when absent or not a string).
    pub fn domain_str(&self) -> Option<&str> {
        self.domain.as_ref().and_then(Value::as_str)
    }

    fn evaluate(&self, ctx: &dyn FilterContext) -> bool {
        if self.id.is_none() {
            return false;
        }
        let Some(fact) = ctx.fact(self.subject, self) else {
            return false;
        };
        match (fact, &self.value) {
            (Fact::Bool(fact), FilterValue::Bool(want)) => self.operator.holds(fact.cmp(want)),
            (Fact::Number(fact), FilterValue::Number(want)) => want
                .as_f64()
                .and_then(|want| fact.partial_cmp(&want))
                .is_some_and(|ord| self.operator.holds(ord)),
            (Fact::Matches(matched), _) => match self.operator {
                Operator::Eq => matched,
                Operator::Ne => !matched,
                _ => false,
            },
            _ => false,
        }
    }
}

fn parse_value(def: &TestDef, raw: Option<&Value>, path: &str) -> Result<FilterValue, ParseError> {
    let Some(raw) = raw else {
        return match def.default {
            Some(DefaultValue::Bool(b)) => Ok(FilterValue::Bool(b)),
            Some(DefaultValue::Str(s)) => Ok(FilterValue::Str(s.to_owned())),
            None => Err(ParseError::new(
                path,
                format!("{} requires a value", def.name),
            )),
        };
    };
    let mismatch = || {
        ParseError::new(
            path,
            format!("{} expects a {:?} value, got {raw}", def.name, def.value),
        )
    };
    match def.value {
        ValueKind::Bool => raw.as_bool().map(FilterValue::Bool).ok_or_else(mismatch),
        ValueKind::Int | ValueKind::Float => match raw {
            Value::Number(n) => Ok(FilterValue::Number(n.clone())),
            _ => Err(mismatch()),
        },
        ValueKind::Str => raw
            .as_str()
            .map(|s| FilterValue::Str(s.to_owned()))
            .ok_or_else(mismatch),
        ValueKind::Difficulty => match raw {
            Value::String(s) if DIFFICULTIES.contains(&s.as_str()) => {
                Ok(FilterValue::Str(s.clone()))
            }
            Value::Number(n) => n
                .as_u64()
                .and_then(|i| DIFFICULTIES.get(i as usize))
                .map(|s| FilterValue::Str((*s).to_owned()))
                .ok_or_else(mismatch),
            _ => Err(mismatch()),
        },
    }
}

/// A fact about a subject, computed by the caller for one test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fact {
    /// Boolean tests ([`ValueKind::Bool`]).
    Bool(bool),
    /// Numeric tests ([`ValueKind::Int`], [`ValueKind::Float`]).
    Number(f64),
    /// Matching tests ([`ValueKind::Str`], [`ValueKind::Difficulty`]): does the
    /// subject match `test.value` (and `test.domain`)?
    Matches(bool),
}

/// Supplies facts for filter evaluation (implemented by the entity, item,
/// block and world layers).
pub trait FilterContext {
    /// The fact `test` asks about `subject`; `None` when the subject is absent
    /// from the context or the caller does not implement the test (the test
    /// is then false).
    fn fact(&self, subject: Subject, test: &FilterTest) -> Option<Fact>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> (Filter, Vec<crate::Diagnostic>) {
        let mut diags = Diagnostics::default();
        let f = Filter::parse(&v, FilterRegistry::vanilla(), "f", &mut diags).unwrap();
        (f, diags.items)
    }

    fn canonical(v: Value) -> Value {
        parse(v).0.to_json()
    }

    #[test]
    fn builtin_table_matches_enum_order() {
        assert_eq!(BUILTIN_TESTS.len(), BuiltinTest::ALL.len());
        for (i, t) in BuiltinTest::ALL.iter().enumerate() {
            assert_eq!(*t as usize, i);
        }
        assert_eq!(BuiltinTest::IsFamily.name(), "is_family");
        let names: std::collections::HashSet<_> = BUILTIN_TESTS.iter().map(|d| d.name).collect();
        assert_eq!(names.len(), BUILTIN_TESTS.len(), "duplicate test name");
    }

    #[test]
    fn compact_and_readable_spellings_normalise_identically() {
        let readable = json!({"all_of": [{"any_of": [
            {"test": "is_family", "subject": "other", "operator": "==", "value": "player"},
            {"test": "is_family", "subject": "other", "operator": "!=", "value": "creeper"}
        ]}]});
        let compact = json!({"AND": [{"OR": [
            {"test": "is_family", "subject": 1, "operator": 0, "value": "player"},
            {"test": "is_family", "subject": 1, "operator": 1, "value": "creeper"}
        ]}]});
        assert_eq!(canonical(readable.clone()), canonical(compact));
        assert_eq!(canonical(readable.clone()), canonical(canonical(readable)));
    }

    #[test]
    fn group_and_operator_aliases() {
        for (key, out) in [
            ("all", "all_of"),
            ("AND", "all_of"),
            ("any", "any_of"),
            ("OR", "any_of"),
            ("NOT", "none_of"),
            ("none_of", "none_of"),
        ] {
            let v = canonical(json!({key: [{"test": "on_fire"}]}));
            assert!(v.get(out).is_some(), "{key} -> {v}");
        }
        for (op, out) in [
            ("==", "=="),
            ("equals", "=="),
            ("=", "=="),
            ("!=", "!="),
            ("not", "!="),
            ("<>", "!="),
            ("<", "<"),
            ("<=", "<="),
            (">", ">"),
            (">=", ">="),
        ] {
            let v = canonical(json!({"test": "actor_health", "operator": op, "value": 4}));
            assert_eq!(v["operator"], out, "{op}");
        }
    }

    #[test]
    fn omitted_fields_take_defaults() {
        assert_eq!(
            canonical(json!({"test": "in_contact_with_water"})),
            json!({"test": "in_contact_with_water", "subject": "self", "operator": "==", "value": true})
        );
        assert_eq!(
            canonical(json!({"test": "all_slots_empty"}))["value"],
            "any"
        );
        assert_eq!(canonical(json!([]))["all_of"], json!([]));
        assert_eq!(canonical(json!({}))["all_of"], json!([]));
        let mut diags = Diagnostics::default();
        let err = Filter::parse(
            &json!({"test": "is_family"}),
            FilterRegistry::vanilla(),
            "f",
            &mut diags,
        )
        .unwrap_err();
        assert!(err.message.contains("requires a value"), "{err}");
    }

    #[test]
    fn compact_difficulty_integer_is_named() {
        let v =
            canonical(json!({"test": "is_difficulty", "subject": 0, "operator": 1, "value": 0}));
        assert_eq!(
            v,
            json!({"test": "is_difficulty", "subject": "self", "operator": "!=", "value": "peaceful"})
        );
    }

    #[test]
    fn unknown_test_is_kept_logged_and_false() {
        let (f, diags) = parse(json!({"test": "made_up", "value": 3}));
        assert_eq!(diags.len(), 1);
        assert_eq!(f.to_json()["test"], "made_up");
        struct Yes;
        impl FilterContext for Yes {
            fn fact(&self, _: Subject, _: &FilterTest) -> Option<Fact> {
                Some(Fact::Matches(true))
            }
        }
        assert!(!f.evaluate(&Yes));
    }

    #[test]
    fn compact_subjects_normalise_using_canonical_enum_ids() {
        for (id, name) in [
            (2, "player"),
            (3, "target"),
            (4, "parent"),
            (5, "baby"),
            (6, "block"),
            (7, "damager"),
            (8, "holder"),
            (9, "item"),
        ] {
            let normal =
                canonical(json!({"test": "is_family", "subject": id, "value": "fidelity"}));
            assert_eq!(normal["subject"], name);
        }
    }

    struct Ctx {
        families: Vec<&'static str>,
        health: f64,
        other_present: bool,
    }
    impl FilterContext for Ctx {
        fn fact(&self, subject: Subject, test: &FilterTest) -> Option<Fact> {
            if subject == Subject::Other && !self.other_present {
                return None;
            }
            Some(match test.builtin()? {
                BuiltinTest::IsFamily => {
                    Fact::Matches(self.families.contains(&test.value.as_str()?))
                }
                BuiltinTest::ActorHealth => Fact::Number(self.health),
                BuiltinTest::OnFire => Fact::Bool(false),
                _ => return None,
            })
        }
    }

    #[test]
    fn evaluation_applies_groups_operators_and_missing_subjects() {
        let ctx = Ctx {
            families: vec!["zombie", "monster"],
            health: 8.0,
            other_present: false,
        };
        let eval = |v: Value| parse(v).0.evaluate(&ctx);
        assert!(eval(json!({"test": "is_family", "value": "zombie"})));
        assert!(eval(
            json!({"test": "is_family", "operator": "not", "value": "player"})
        ));
        assert!(!eval(
            json!({"test": "is_family", "operator": "<", "value": "zombie"})
        ));
        assert!(eval(
            json!({"test": "actor_health", "operator": ">", "value": 4})
        ));
        assert!(eval(
            json!({"test": "actor_health", "operator": "<=", "value": 8})
        ));
        assert!(!eval(json!({"test": "actor_health", "value": 4})));
        assert!(eval(json!({"test": "on_fire", "value": false})));
        assert!(!eval(json!({"test": "on_fire"})));
        // A missing subject fails the test whatever the operator.
        assert!(!eval(
            json!({"test": "is_family", "subject": "other", "operator": "!=", "value": "player"})
        ));
        assert!(eval(
            json!({"none_of": [{"test": "is_family", "subject": "other", "value": "player"}]})
        ));
        assert!(eval(
            json!({"any_of": [{"test": "on_fire"}, {"test": "is_family", "value": "monster"}]})
        ));
        assert!(eval(json!([])));
        assert!(!eval(json!({"any_of": []})));
    }
}
