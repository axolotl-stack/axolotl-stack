//! Target selectors (`@a @p @r @e @s @initiator` and their arguments),
//! parsed once and resolved against the world.
//!
//! Arguments: `type`, `name`, `r`, `rm`, `x`/`y`/`z`, `dx`/`dy`/`dz`, `c`, `m`,
//! `l`/`lm`, `rx`/`rxm`, `ry`/`rym`, `tag`, `family`, `scores`, `hasitem`,
//! `haspermission`, `has_property`. Repeated `type`/`name`/`m`/`tag`/`family`
//! conditions must all hold (a negated one excludes).
//!
//! unastar has no entity tags, scoreboard or input-permission system yet, so
//! every entity has no tags and no scores and all input permissions enabled:
//! `tag=x` and any `scores` condition match nothing, `tag=!x` matches
//! everything, `haspermission={…=enabled}` always holds.

use super::parser::{Coordinate, Cursor, ErrorAt, IntRange, parse_range};
use bevy_ecs::prelude::*;
use glam::DVec3;

/// Which entities a selector starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKind {
    /// `@a`
    AllPlayers,
    /// `@p`
    NearestPlayer,
    /// `@r`
    RandomPlayer,
    /// `@e`
    AllEntities,
    /// `@s`
    Executor,
    /// `@initiator`: the player who talked to an NPC; never set by chat commands.
    Initiator,
}

/// `name`/`!name` condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub negated: bool,
    pub value: String,
}

/// One `hasitem` condition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HasItem {
    pub item: String,
    pub data: Option<i32>,
    pub quantity: Option<IntRange>,
    pub location: Option<String>,
    pub slot: Option<IntRange>,
}

/// A parsed selector.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selector {
    pub kind: Option<SelectorKind>,
    pub types: Vec<Condition>,
    pub names: Vec<Condition>,
    pub tags: Vec<Condition>,
    pub families: Vec<Condition>,
    /// Game modes (as wire game types: 0 survival, 1 creative, 2 adventure, 6 spectator).
    pub modes: Vec<(bool, i32)>,
    pub x: Option<Coordinate>,
    pub y: Option<Coordinate>,
    pub z: Option<Coordinate>,
    pub dx: Option<f64>,
    pub dy: Option<f64>,
    pub dz: Option<f64>,
    pub r: Option<f64>,
    pub rm: Option<f64>,
    pub count: Option<i32>,
    pub level_max: Option<i32>,
    pub level_min: Option<i32>,
    pub pitch_max: Option<f32>,
    pub pitch_min: Option<f32>,
    pub yaw_max: Option<f32>,
    pub yaw_min: Option<f32>,
    /// Objective name → range. unastar has no scoreboard, so any entry matches nothing.
    pub scores: Vec<(String, IntRange)>,
    pub has_item: Vec<HasItem>,
    /// Input permission name → required `enabled`.
    pub permissions: Vec<(String, bool)>,
    /// `has_property` is parsed but actor property conditions are not evaluated yet.
    pub has_property: bool,
}

impl Selector {
    pub fn kind(&self) -> SelectorKind {
        self.kind.unwrap_or(SelectorKind::AllEntities)
    }

    /// True when only players can match (`@a`, `@p`, `@r` without a non-player `type`).
    pub fn players_only(&self) -> bool {
        match self.kind() {
            SelectorKind::AllPlayers | SelectorKind::NearestPlayer => true,
            SelectorKind::RandomPlayer => self.types.is_empty(),
            _ => self
                .types
                .iter()
                .any(|t| !t.negated && normalize_type(&t.value) == "minecraft:player"),
        }
    }
}

/// `cow` → `minecraft:cow`.
pub fn normalize_type(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.contains(':') {
        lower
    } else {
        format!("minecraft:{lower}")
    }
}

/// Parses `@x` or `@x[args]`; the cursor is on the `@`.
pub fn parse_selector(cursor: &mut Cursor<'_>) -> Result<Selector, ErrorAt> {
    let start = cursor.pos;
    if !cursor.eat('@') {
        return Err(start);
    }
    let name = cursor.word(&['[']);
    let kind = match name {
        "a" => SelectorKind::AllPlayers,
        "p" => SelectorKind::NearestPlayer,
        "r" => SelectorKind::RandomPlayer,
        "e" => SelectorKind::AllEntities,
        "s" => SelectorKind::Executor,
        "initiator" => SelectorKind::Initiator,
        _ => return Err(start),
    };
    let mut selector = Selector {
        kind: Some(kind),
        ..Selector::default()
    };
    if cursor.peek() != Some('[') {
        return Ok(selector);
    }
    let group_start = cursor.pos;
    let group = cursor.balanced()?;
    let mut inner = Cursor::new(&group[1..group.len() - 1]);
    loop {
        inner.skip_whitespace();
        if inner.at_end() {
            break;
        }
        let key_at = group_start + 1 + inner.pos;
        let key = inner.word(&['=', ',']).to_ascii_lowercase();
        inner.skip_whitespace();
        if !inner.eat('=') {
            return Err(key_at);
        }
        inner.skip_whitespace();
        let value_at = group_start + 1 + inner.pos;
        let raw = read_value(&mut inner).map_err(|_| value_at)?;
        apply_argument(&mut selector, &key, &raw)
            .map_err(|bad_key| if bad_key { key_at } else { value_at })?;
        inner.skip_whitespace();
        if !inner.eat(',') {
            inner.skip_whitespace();
            if !inner.at_end() {
                return Err(group_start + 1 + inner.pos);
            }
            break;
        }
    }
    Ok(selector)
}

/// One argument value: a quoted string, a `{…}`/`[…]` group, or text up to `,`.
fn read_value(cursor: &mut Cursor<'_>) -> Result<String, ErrorAt> {
    let negated = cursor.eat('!');
    cursor.skip_whitespace();
    let body = match cursor.peek() {
        Some('"') => format!("\"{}\"", cursor.quoted()?),
        Some('{' | '[') => cursor.balanced()?.to_owned(),
        _ => {
            let rest = cursor.rest();
            let end = rest.find(',').unwrap_or(rest.len());
            cursor.pos += end;
            rest[..end].trim().to_owned()
        }
    };
    Ok(if negated { format!("!{body}") } else { body })
}

fn condition(raw: &str) -> Condition {
    let (negated, value) = match raw.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    Condition {
        negated,
        value: value.to_owned(),
    }
}

fn number<T: std::str::FromStr>(raw: &str) -> Result<T, bool> {
    raw.parse().map_err(|_| false)
}

fn coordinate(raw: &str) -> Result<Coordinate, bool> {
    match raw.strip_prefix('~') {
        Some("") => Ok(Coordinate::Relative(0.0)),
        Some(offset) => Ok(Coordinate::Relative(number(offset)?)),
        None => Ok(Coordinate::Absolute(number(raw)?)),
    }
}

fn game_mode(raw: &str) -> Result<i32, bool> {
    Ok(match raw.to_ascii_lowercase().as_str() {
        "0" | "s" | "survival" => 0,
        "1" | "c" | "creative" => 1,
        "2" | "a" | "adventure" => 2,
        "5" | "d" | "default" => 5,
        "6" | "spectator" => 6,
        _ => return Err(false),
    })
}

/// `Err(true)` marks an unknown key, `Err(false)` a bad value.
fn apply_argument(selector: &mut Selector, key: &str, raw: &str) -> Result<(), bool> {
    match key {
        "type" => selector.types.push(condition(raw)),
        "name" => selector.names.push(condition(raw)),
        "tag" => selector.tags.push(condition(raw)),
        "family" => selector.families.push(condition(raw)),
        "m" => {
            let c = condition(raw);
            selector.modes.push((c.negated, game_mode(&c.value)?));
        }
        "x" => selector.x = Some(coordinate(raw)?),
        "y" => selector.y = Some(coordinate(raw)?),
        "z" => selector.z = Some(coordinate(raw)?),
        "dx" => selector.dx = Some(number(raw)?),
        "dy" => selector.dy = Some(number(raw)?),
        "dz" => selector.dz = Some(number(raw)?),
        "r" => selector.r = Some(number(raw)?),
        "rm" => selector.rm = Some(number(raw)?),
        "c" => selector.count = Some(number(raw)?),
        "l" => selector.level_max = Some(number(raw)?),
        "lm" => selector.level_min = Some(number(raw)?),
        "rx" => selector.pitch_max = Some(number(raw)?),
        "rxm" => selector.pitch_min = Some(number(raw)?),
        "ry" => selector.yaw_max = Some(number(raw)?),
        "rym" => selector.yaw_min = Some(number(raw)?),
        "scores" => {
            for (objective, range) in object_entries(raw)? {
                selector
                    .scores
                    .push((objective, parse_range(&range).ok_or(false)?));
            }
        }
        "hasitem" => {
            let groups: Vec<&str> = if raw.starts_with('[') {
                split_groups(&raw[1..raw.len() - 1])
            } else {
                vec![raw]
            };
            for group in groups {
                let mut item = HasItem::default();
                for (key, value) in object_entries(group)? {
                    match key.as_str() {
                        "item" => item.item = normalize_type(&value),
                        "data" => item.data = Some(number(&value)?),
                        "quantity" => item.quantity = Some(parse_range(&value).ok_or(false)?),
                        "location" => item.location = Some(value),
                        "slot" => item.slot = Some(parse_range(&value).ok_or(false)?),
                        _ => return Err(false),
                    }
                }
                if item.item.is_empty() {
                    return Err(false);
                }
                selector.has_item.push(item);
            }
        }
        "haspermission" => {
            for (name, state) in object_entries(raw)? {
                let enabled = match state.as_str() {
                    "enabled" => true,
                    "disabled" => false,
                    _ => return Err(false),
                };
                selector.permissions.push((name, enabled));
            }
        }
        "has_property" => {
            object_entries(raw)?;
            selector.has_property = true;
        }
        _ => return Err(true),
    }
    Ok(())
}

/// `{a=1, b=!2..3}` → `[("a","1"), ("b","!2..3")]`.
fn object_entries(raw: &str) -> Result<Vec<(String, String)>, bool> {
    let inner = raw
        .strip_prefix('{')
        .and_then(|r| r.strip_suffix('}'))
        .ok_or(false)?;
    split_groups(inner)
        .into_iter()
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (key, value) = entry.split_once('=').ok_or(false)?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            Ok((key.trim().trim_matches('"').to_owned(), value.to_owned()))
        })
        .collect()
}

/// Splits on top-level commas.
fn split_groups(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '{' | '[' if !in_string => depth += 1,
            '}' | ']' if !in_string => depth -= 1,
            ',' if !in_string && depth == 0 => {
                parts.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !text[start..].trim().is_empty() {
        parts.push(text[start..].trim());
    }
    parts
}

/// What a selector is resolved against: one row per selectable entity.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub entity: Entity,
    pub is_player: bool,
    /// Namespaced type (`minecraft:player`, `minecraft:cow`, `minecraft:item`).
    pub type_id: String,
    /// Player name or name tag (empty when unnamed).
    pub name: String,
    pub feet: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Wire game type for players.
    pub game_mode: Option<i32>,
    pub level: i32,
    /// Dead players stay selectable by `@a` only.
    pub alive: bool,
    pub families: Vec<String>,
    /// Items carried (players): identifier, aux, count, location, slot.
    pub items: Vec<CarriedItem>,
}

#[derive(Debug, Clone)]
pub struct CarriedItem {
    pub identifier: String,
    pub aux: i32,
    pub count: i32,
    pub location: &'static str,
    pub slot: i32,
}

/// Why a selector could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorError {
    /// `commands.generic.invalidPlayerType`: `type` on a player-only selector.
    InvalidPlayerType,
    /// `commands.generic.radiusNegative`
    RadiusNegative,
    /// `commands.generic.radiusError`
    RadiusError,
    /// `commands.generic.levelError`
    LevelError,
    /// A selector argument whose game system unastar does not have yet.
    Unsupported(&'static str),
}

impl SelectorError {
    pub fn key(&self) -> String {
        match self {
            Self::InvalidPlayerType => "commands.generic.invalidPlayerType".into(),
            Self::RadiusNegative => "commands.generic.radiusNegative".into(),
            Self::RadiusError => "commands.generic.radiusError".into(),
            Self::LevelError => "commands.generic.levelError".into(),
            Self::Unsupported(what) => {
                format!("The selector argument {what} is not implemented in unastar yet")
            }
        }
    }
}

impl Selector {
    /// Filters and orders `candidates` the way BDS does. `origin` is the
    /// source's feet position, `source` the executing entity, if any.
    pub fn resolve(
        &self,
        candidates: &[Candidate],
        origin: DVec3,
        source: Option<Entity>,
        random: &mut impl FnMut(usize) -> usize,
    ) -> Result<Vec<Entity>, SelectorError> {
        let kind = self.kind();
        if matches!(kind, SelectorKind::AllPlayers | SelectorKind::NearestPlayer)
            && self
                .types
                .iter()
                .any(|t| t.negated || normalize_type(&t.value) != "minecraft:player")
        {
            return Err(SelectorError::InvalidPlayerType);
        }
        if self.r.is_some_and(|r| r < 0.0) || self.rm.is_some_and(|r| r < 0.0) {
            return Err(SelectorError::RadiusNegative);
        }
        if let (Some(r), Some(rm)) = (self.r, self.rm)
            && rm > r
        {
            return Err(SelectorError::RadiusError);
        }
        if let (Some(l), Some(lm)) = (self.level_max, self.level_min)
            && lm > l
        {
            return Err(SelectorError::LevelError);
        }
        if self.has_property {
            return Err(SelectorError::Unsupported("has_property"));
        }
        let axis = |c: Option<Coordinate>, base: f64| match c {
            Some(Coordinate::Absolute(v)) => v,
            Some(Coordinate::Relative(v)) => base + v,
            Some(Coordinate::Local(_)) | None => base,
        };
        let origin = DVec3::new(
            axis(self.x, origin.x),
            axis(self.y, origin.y),
            axis(self.z, origin.z),
        );

        let mut matched: Vec<&Candidate> = candidates
            .iter()
            // `@a` includes dead players; every other selector skips the dead.
            .filter(|c| match kind {
                SelectorKind::Executor => Some(c.entity) == source,
                SelectorKind::Initiator => false,
                SelectorKind::AllPlayers => c.is_player,
                SelectorKind::NearestPlayer => c.is_player && c.alive,
                SelectorKind::RandomPlayer => (!self.types.is_empty() || c.is_player) && c.alive,
                SelectorKind::AllEntities => c.alive,
            })
            .filter(|c| self.matches(c, origin))
            .collect();

        let distance = |c: &&Candidate| c.feet.distance_squared(origin);
        let count = self.count;
        match kind {
            SelectorKind::NearestPlayer => {
                matched.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
                let c = count.unwrap_or(1);
                if c < 0 {
                    matched.reverse();
                }
                matched.truncate(c.unsigned_abs() as usize);
            }
            SelectorKind::RandomPlayer => {
                // Fisher-Yates, then take `c` (default 1).
                for i in (1..matched.len()).rev() {
                    matched.swap(i, random(i + 1));
                }
                matched.truncate(count.unwrap_or(1).unsigned_abs() as usize);
            }
            _ => {
                if let Some(c) = count {
                    matched.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
                    if c < 0 {
                        matched.reverse();
                    }
                    matched.truncate(c.unsigned_abs() as usize);
                }
            }
        }
        Ok(matched.into_iter().map(|c| c.entity).collect())
    }

    fn matches(&self, c: &Candidate, origin: DVec3) -> bool {
        let all = |conditions: &[Condition], test: &dyn Fn(&str) -> bool| {
            conditions
                .iter()
                .all(|cond| test(&cond.value) != cond.negated)
        };
        if !all(&self.types, &|t| normalize_type(t) == c.type_id) {
            return false;
        }
        if !all(&self.names, &|n| n == c.name) {
            return false;
        }
        // No entity has tags: `tag=` (no tags) holds, `tag=x` does not.
        if !all(&self.tags, &|t| t.is_empty()) {
            return false;
        }
        if !all(&self.families, &|f| c.families.iter().any(|own| own == f)) {
            return false;
        }
        if !self.modes.is_empty() {
            let Some(mode) = c.game_mode else {
                return false;
            };
            if !self
                .modes
                .iter()
                .all(|(negated, m)| (mode == *m) != *negated)
            {
                return false;
            }
        }
        if !self.scores.is_empty() {
            return false;
        }
        if self.permissions.iter().any(|(_, enabled)| !enabled) {
            return false;
        }
        if self.dx.is_some() || self.dy.is_some() || self.dz.is_some() {
            let delta = DVec3::new(
                self.dx.unwrap_or(0.0),
                self.dy.unwrap_or(0.0),
                self.dz.unwrap_or(0.0),
            );
            let min = origin.min(origin + delta);
            let max = origin.max(origin + delta) + DVec3::ONE;
            if c.feet.cmplt(min).any() || c.feet.cmpge(max).any() {
                return false;
            }
        }
        let distance = c.feet.distance(origin);
        if self.r.is_some_and(|r| distance > r) || self.rm.is_some_and(|rm| distance < rm) {
            return false;
        }
        if self.level_max.is_some() || self.level_min.is_some() {
            if !c.is_player {
                return false;
            }
            if self.level_max.is_some_and(|l| c.level > l)
                || self.level_min.is_some_and(|l| c.level < l)
            {
                return false;
            }
        }
        if self.pitch_max.is_some_and(|v| c.pitch > v)
            || self.pitch_min.is_some_and(|v| c.pitch < v)
        {
            return false;
        }
        if self.yaw_max.is_some() || self.yaw_min.is_some() {
            let yaw = wrap_degrees(c.yaw);
            if self.yaw_max.is_some_and(|v| yaw > wrap_degrees(v))
                || self.yaw_min.is_some_and(|v| yaw < wrap_degrees(v))
            {
                return false;
            }
        }
        self.has_item.iter().all(|wanted| has_item(c, wanted))
    }
}

fn wrap_degrees(value: f32) -> f32 {
    let wrapped = (value + 180.0).rem_euclid(360.0) - 180.0;
    if wrapped == -180.0 { 180.0 } else { wrapped }
}

fn has_item(c: &Candidate, wanted: &HasItem) -> bool {
    let total: i32 = c
        .items
        .iter()
        .filter(|item| item.identifier == wanted.item)
        .filter(|item| wanted.data.is_none_or(|d| d == -1 || d == item.aux))
        .filter(|item| {
            wanted
                .location
                .as_deref()
                .is_none_or(|l| l == item.location)
        })
        .filter(|item| wanted.slot.is_none_or(|s| s.contains(item.slot)))
        .map(|item| item.count)
        .sum();
    match wanted.quantity {
        Some(range) => range.contains(total),
        None => total > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Selector, ErrorAt> {
        parse_selector(&mut Cursor::new(text))
    }

    fn candidate(index: u32, player: bool, type_id: &str, name: &str, feet: DVec3) -> Candidate {
        Candidate {
            entity: Entity::from_raw_u32(index).unwrap(),
            is_player: player,
            type_id: type_id.into(),
            name: name.into(),
            feet,
            yaw: 0.0,
            pitch: 0.0,
            game_mode: player.then_some(0),
            level: 0,
            alive: true,
            families: vec![],
            items: vec![],
        }
    }

    fn world() -> Vec<Candidate> {
        vec![
            candidate(1, true, "minecraft:player", "Alice", DVec3::ZERO),
            candidate(
                2,
                true,
                "minecraft:player",
                "Bob",
                DVec3::new(10.0, 0.0, 0.0),
            ),
            candidate(3, false, "minecraft:cow", "", DVec3::new(3.0, 0.0, 0.0)),
            candidate(
                4,
                false,
                "minecraft:pig",
                "Babe",
                DVec3::new(20.0, 0.0, 0.0),
            ),
        ]
    }

    /// Resolves against [`world`], returning the candidates' 1-based numbers.
    fn resolve(text: &str) -> Result<Vec<u32>, SelectorError> {
        let selector = parse(text).expect("parses");
        let candidates = world();
        let mut first = |_: usize| 0;
        let number =
            |entity: Entity| candidates.iter().position(|c| c.entity == entity).unwrap() as u32 + 1;
        selector
            .resolve(
                &candidates,
                DVec3::ZERO,
                Some(candidates[0].entity),
                &mut first,
            )
            .map(|entities| entities.into_iter().map(number).collect())
    }

    #[test]
    fn kinds_pick_their_base_sets() {
        assert_eq!(resolve("@a"), Ok(vec![1, 2]));
        assert_eq!(resolve("@e"), Ok(vec![1, 2, 3, 4]));
        assert_eq!(resolve("@s"), Ok(vec![1]));
        assert_eq!(resolve("@p"), Ok(vec![1]));
        assert_eq!(resolve("@initiator"), Ok(vec![]));
    }

    #[test]
    fn arguments_filter_and_order() {
        assert_eq!(resolve("@e[type=cow]"), Ok(vec![3]));
        assert_eq!(resolve("@e[type=!player]"), Ok(vec![3, 4]));
        assert_eq!(resolve("@e[type=!player,type=!cow]"), Ok(vec![4]));
        assert_eq!(resolve("@e[name=Babe]"), Ok(vec![4]));
        assert_eq!(resolve("@e[name=!Alice, r=5]"), Ok(vec![3]));
        assert_eq!(resolve("@e[rm=5]"), Ok(vec![2, 4]));
        assert_eq!(resolve("@e[c=2]"), Ok(vec![1, 3]));
        assert_eq!(resolve("@e[c=-1]"), Ok(vec![4]));
        assert_eq!(resolve("@a[x=10,y=0,z=0,c=1]"), Ok(vec![2]));
        assert_eq!(resolve("@e[x=2,y=-1,z=-1,dx=2,dy=2,dz=2]"), Ok(vec![3]));
        assert_eq!(resolve("@a[m=s]"), Ok(vec![1, 2]));
        assert_eq!(resolve("@a[m=creative]"), Ok(vec![]));
        assert_eq!(resolve("@e[tag=builder]"), Ok(vec![]));
        assert_eq!(resolve("@e[tag=!builder,type=pig]"), Ok(vec![4]));
        assert_eq!(resolve("@e[scores={kills=1..}]"), Ok(vec![]));
        assert_eq!(resolve("@r[type=cow]"), Ok(vec![3]));
        assert_eq!(resolve("@e[name=\"Babe\"]"), Ok(vec![4]));
    }

    #[test]
    fn invalid_combinations_report_bds_errors() {
        assert_eq!(
            resolve("@a[type=cow]"),
            Err(SelectorError::InvalidPlayerType)
        );
        assert_eq!(resolve("@e[r=-1]"), Err(SelectorError::RadiusNegative));
        assert_eq!(resolve("@e[r=1,rm=2]"), Err(SelectorError::RadiusError));
        assert_eq!(resolve("@e[l=1,lm=2]"), Err(SelectorError::LevelError));
    }

    #[test]
    fn malformed_selectors_fail_at_the_bad_part() {
        assert_eq!(parse("@x"), Err(0));
        assert_eq!(parse("@e[bogus=1]"), Err(3));
        assert_eq!(parse("@e[r=abc]"), Err(5));
        assert_eq!(parse("@e[type=cow"), Err(2));
        assert!(parse("@e[hasitem={item=diamond,quantity=1..}]").is_ok());
        assert!(parse("@e[hasitem=[{item=diamond},{item=apple,location=slot.hotbar}]]").is_ok());
        assert!(parse("@a[haspermission={camera=enabled}]").is_ok());
    }
}
