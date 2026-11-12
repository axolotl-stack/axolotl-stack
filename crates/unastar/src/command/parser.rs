//! One parser for every command, driven by the captured overloads.
//!
//! A command line is matched against each overload of its command in packet
//! order; the first overload that consumes the whole line wins. When none
//! does, the error points at the furthest position any overload reached, which
//! is the token BDS reports in `commands.generic.syntax`.
//!
//! Optional parameters only ever trail required ones in the captured syntax, so
//! matching is a single left-to-right pass per overload: a token that does not
//! fit the next parameter fails that overload (optional parameters are never
//! skipped in the middle).

use super::args::Args;
use super::schema::CommandSchema;
use super::selector::{Selector, parse_selector};
use unastar_data::commands::{BasicType, CommandDefinition, CommandParam, ParamKind};

mod json;
pub use json::JsonArg;

/// A position on the line the parser could not get past (byte offset).
pub type ErrorAt = usize;

/// A cursor over the command line (without the leading `/`).
#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    pub text: &'a str,
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }

    pub fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    pub fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    pub fn skip_whitespace(&mut self) {
        let trimmed = self.rest().trim_start();
        self.pos = self.text.len() - trimmed.len();
    }

    pub fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// Reads up to the next whitespace (or one of `stop`).
    pub fn word(&mut self, stop: &[char]) -> &'a str {
        let rest = self.rest();
        let end = rest
            .find(|c: char| c.is_whitespace() || stop.contains(&c))
            .unwrap_or(rest.len());
        self.pos += end;
        &rest[..end]
    }

    /// Reads a `"…"` string with `\"` and `\\` escapes; the cursor is on the quote.
    pub fn quoted(&mut self) -> Result<String, ErrorAt> {
        let start = self.pos;
        if !self.eat('"') {
            return Err(start);
        }
        let mut out = String::new();
        let mut chars = self.rest().char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    self.pos += i + 1;
                    return Ok(out);
                }
                '\\' => match chars.next() {
                    Some((_, escaped)) => out.push(escaped),
                    None => break,
                },
                c => out.push(c),
            }
        }
        Err(start)
    }

    /// Reads a balanced `{…}` / `[…]` group (quotes respected); the cursor is on the opener.
    pub fn balanced(&mut self) -> Result<&'a str, ErrorAt> {
        let start = self.pos;
        let rest = self.rest();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (i, c) in rest.char_indices() {
            if in_string {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, '\\') => escaped = true,
                    (false, '"') => in_string = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => in_string = true,
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    depth = depth.checked_sub(1).ok_or(start)?;
                    if depth == 0 {
                        self.pos += i + 1;
                        return Ok(&rest[..=i]);
                    }
                }
                _ => {}
            }
        }
        Err(start)
    }

    /// A word (or quoted string) that must end at whitespace or the end of the line.
    fn token(&mut self) -> Result<(usize, &'a str), ErrorAt> {
        let start = self.pos;
        let word = self.word(&[]);
        if word.is_empty() {
            Err(start)
        } else {
            Ok((start, word))
        }
    }
}

/// How a coordinate is written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coordinate {
    Absolute(f64),
    /// `~offset`: relative to the source position.
    Relative(f64),
    /// `^offset`: along the source's local axes (left, up, forward).
    Local(f64),
}

/// Three coordinates as written (`x y z`, `~ ~1 ~`, `^ ^ ^2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommandPosition(pub [Coordinate; 3]);

impl CommandPosition {
    /// Resolves against a source position and rotation (`yaw`, `pitch` in degrees).
    ///
    /// For `position` parameters a whole-number absolute x or z (written
    /// without a decimal point) is centred on its block, as Bedrock does;
    /// `block_position` parameters are floored by the caller.
    pub fn resolve(&self, origin: glam::DVec3, yaw: f32, pitch: f32) -> glam::DVec3 {
        if let Coordinate::Local(_) = self.0[0] {
            let [left, up, forward] = self.0.map(|c| match c {
                Coordinate::Local(v) => v,
                _ => 0.0,
            });
            let (yaw, pitch) = ((yaw as f64).to_radians(), (pitch as f64).to_radians());
            let forward_v = glam::DVec3::new(
                -yaw.sin() * pitch.cos(),
                -pitch.sin(),
                yaw.cos() * pitch.cos(),
            );
            let up_v = glam::DVec3::new(
                -yaw.sin() * (-pitch).sin(),
                pitch.cos(),
                yaw.cos() * (-pitch).sin(),
            );
            let left_v = up_v.cross(forward_v);
            return origin + forward_v * forward + up_v * up + left_v * left;
        }
        let axis = |c: Coordinate, origin: f64| match c {
            Coordinate::Absolute(v) => v,
            Coordinate::Relative(v) => origin + v,
            Coordinate::Local(_) => origin,
        };
        glam::DVec3::new(
            axis(self.0[0], origin.x),
            axis(self.0[1], origin.y),
            axis(self.0[2], origin.z),
        )
    }

    /// Resolves and floors to a block position.
    pub fn resolve_block(&self, origin: glam::DVec3, yaw: f32, pitch: f32) -> glam::IVec3 {
        self.resolve(origin, yaw, pitch).floor().as_ivec3()
    }
}

/// A target: a selector or a player name.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Selector(Box<Selector>),
    Name(String),
}

/// A value of a block-state list (`["color"="red", "age"=3, "open_bit"=true]`).
#[derive(Debug, Clone, PartialEq)]
pub enum StateLiteral {
    String(String),
    Int(i32),
    Bool(bool),
}

/// An `x..y` range, either end open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntRange {
    pub min: Option<i32>,
    pub max: Option<i32>,
    pub negated: bool,
}

impl IntRange {
    pub fn contains(&self, value: i32) -> bool {
        let inside =
            self.min.is_none_or(|min| value >= min) && self.max.is_none_or(|max| value <= max);
        inside != self.negated
    }
}

/// One parsed argument.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    Int(i32),
    Float(f32),
    /// `value`: a float that may be relative (`~10`).
    Value {
        relative: bool,
        value: f32,
    },
    /// `None` is `*`.
    WildcardInt(Option<i32>),
    Target(Target),
    /// `None` is `*`.
    WildcardTarget(Option<Target>),
    String(String),
    BlockPosition(CommandPosition),
    Position(CommandPosition),
    /// The rest of the line, as typed.
    Message(String),
    RawText(String),
    Json(JsonArg),
    BlockStates(Vec<(String, StateLiteral)>),
    /// A nested command line (without `/`).
    Command(String),
    /// The enum's own spelling of the value.
    Enum(&'static str),
    SoftEnum(String),
    /// The number in front of a postfix (`5` of `5L`).
    Postfix(i32),
    Operator(String),
    CompareOperator(String),
    IntRange(IntRange),
    FilePath(String),
    /// A chained subcommand keyword and the rest of the line.
    Chained(String),
}

/// Why a line did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// No command has this name (or alias).
    Unknown(String),
    /// The line does not fit any overload.
    Syntax(SyntaxError),
    /// A JSON argument passed the syntax check but not the JSON reader
    /// (`commands.generic.json.invalid` with these errors).
    Json(String),
}

impl ParseError {
    /// BDS's answer to a line that does not parse.
    pub fn into_output(self) -> crate::command::CommandOutput {
        use crate::command::CommandOutput;
        match self {
            Self::Unknown(name) => CommandOutput::failure("commands.generic.unknown", vec![name]),
            Self::Syntax(error) => {
                CommandOutput::failure("commands.generic.syntax", error.params())
            }
            Self::Json(errors) => {
                CommandOutput::failure("commands.generic.json.invalid", vec![errors])
            }
        }
    }
}

/// How many characters before the offending token a syntax error shows.
const SYNTAX_CONTEXT: usize = 10;

/// BDS's `commands.generic.syntax` parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    /// Text before the offending token, including the leading `/`.
    pub before: String,
    pub token: String,
    pub after: String,
}

impl SyntaxError {
    fn at(line: &str, at: usize) -> Self {
        let rest = &line[at..];
        let end = token_len(rest);
        // Measured: BDS shows at most the last ten characters before the token
        // (`/scoreboard objectives add bad deathCount` → "s add bad ").
        let before = format!("/{}", &line[..at]);
        let skip = before.chars().count().saturating_sub(SYNTAX_CONTEXT);
        Self {
            before: before.chars().skip(skip).collect(),
            token: rest[..end].to_owned(),
            // Measured: and at most ten after it (`packstack behavior
            // verbose exclude-vanilla` → " verbose e").
            after: rest[end..].chars().take(SYNTAX_CONTEXT).collect(),
        }
    }

    /// `[before, token, after]`, the order `commands.generic.syntax` formats them in.
    pub fn params(&self) -> Vec<String> {
        vec![self.before.clone(), self.token.clone(), self.after.clone()]
    }
}

/// The length of the token `rest` starts with, as BDS shows it in a syntax
/// error. Measured (command capture and `tools/bds-sweep/command_probe.py`):
/// `/` and each of `{ } [ ] , : =` are tokens of their own (`loot
/// entities/wither_boss` is wrong at "/", `@e[typ=cow]` at "typ" followed by
/// "=cow]", `{} {}` at "{" followed by "}"); a `"` string runs to its closing
/// quote and a `'` to the end of the line; a word ends at whitespace, a quote
/// or one of those but `/` and `:` (`minecraft:nothing` is one token).
fn token_len(rest: &str) -> usize {
    match rest.chars().next() {
        Some('/' | '{' | '}' | '[' | ']' | ',' | ':' | '=') => 1,
        Some('"') => {
            let mut escaped = false;
            for (i, c) in rest.char_indices().skip(1) {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, '\\') => escaped = true,
                    (false, '"') => return i + 1,
                    _ => {}
                }
            }
            rest.len()
        }
        Some('\'') => rest.len(),
        _ => rest
            .find(|c: char| c.is_whitespace() || "{}[],=\"".contains(c))
            .unwrap_or(rest.len()),
    }
}

/// Splits off the command name (`line` has no leading `/`).
pub fn command_name(line: &str) -> &str {
    let line = line.trim_start();
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    &line[..end]
}

/// Parses a command line (without its leading `/`).
///
/// The whole line is checked, including the rest of an `/execute` chain and
/// the command after `run` (measured on BDS 1.26.51: `execute run bogus` and
/// a chain of 2000 subcommands ending in `bogus` both answer
/// `commands.generic.syntax` at `bogus`, before anything runs). The check
/// keeps its own stack, so a line of any length cannot overflow the thread's.
pub fn parse(schema: &CommandSchema, line: &str) -> Result<Args, ParseError> {
    let name = command_name(line);
    let definition = schema
        .command(name)
        .ok_or_else(|| ParseError::Unknown(name.to_owned()))?;
    let name_end = line.len() - line.trim_start().len() + name.len();
    parse_overloads(schema, definition, line, name_end)
}

/// Matches `line[start..]` against the overloads of `definition`, checking
/// the rest of the line a chained subcommand or nested command holds.
pub fn parse_overloads(
    schema: &CommandSchema,
    definition: &'static CommandDefinition,
    line: &str,
    start: usize,
) -> Result<Args, ParseError> {
    let syntax = |at: ErrorAt| ParseError::Syntax(SyntaxError::at(line, at.min(line.len())));
    let mut furthest = start;
    for overload in 0..definition.overloads.len() {
        let args = match match_overload(schema, definition, overload, line, start) {
            Ok((args, None)) => args,
            Ok((args, Some(tail))) => match check_tails(schema, line, tail) {
                Ok(()) => args,
                Err(at) => {
                    furthest = furthest.max(at);
                    continue;
                }
            },
            Err(at) => {
                furthest = furthest.max(at);
                continue;
            }
        };
        return match args.invalid_json() {
            Some(errors) => Err(ParseError::Json(errors.to_owned())),
            None => Ok(args),
        };
    }
    Err(syntax(furthest))
}

/// [`parse_overloads`] for the rest of a chain the whole line was already
/// checked with: the rest it holds is not checked again.
pub fn parse_checked_overloads(
    schema: &CommandSchema,
    definition: &'static CommandDefinition,
    line: &str,
) -> Result<Args, ParseError> {
    let mut furthest = 0;
    for overload in 0..definition.overloads.len() {
        match match_overload(schema, definition, overload, line, 0) {
            Ok((args, _)) => return Ok(args),
            Err(at) => furthest = furthest.max(at),
        }
    }
    Err(ParseError::Syntax(SyntaxError::at(
        line,
        furthest.min(line.len()),
    )))
}

/// The rest of a line that a last parameter hands on: the next subcommand
/// of the same command, or a whole nested command line.
#[derive(Debug, Clone, Copy)]
enum Tail {
    Chained(&'static CommandDefinition, usize),
    Command(usize),
}

/// One command's overloads being tried at `start` by [`check_tails`].
struct Frame {
    definition: &'static CommandDefinition,
    start: usize,
    next: usize,
    furthest: usize,
}

/// Whether the rest `tail` hands on parses, without recursion: an overload
/// that ends in another tail is tried by pushing a frame. Any frame that
/// matches the line to its end makes the whole line valid (a tail is always
/// the last parameter); a frame whose overloads all fail hands its furthest
/// position to the frame below, which tries its next overload.
fn check_tails(schema: &CommandSchema, line: &str, tail: Tail) -> Result<(), ErrorAt> {
    // Tails already known to fail, by (command, start): a frame is tried once.
    let mut failed: std::collections::HashMap<(usize, usize), usize> = Default::default();
    let mut stack = vec![frame(schema, line, tail)?];
    let mut pending = None;
    loop {
        if let Some(tail) = pending.take() {
            match frame(schema, line, tail) {
                Ok(frame) => match failed.get(&key(&frame)) {
                    Some(&at) => fail(&mut stack, at),
                    None => stack.push(frame),
                },
                Err(at) => fail(&mut stack, at),
            }
        }
        // The bottom frame's failure returns, so a frame is always on top.
        let Some(top) = stack.last_mut() else {
            return Err(line.len());
        };
        if top.next == top.definition.overloads.len() {
            let (key, furthest) = (key(top), top.furthest);
            stack.pop();
            failed.insert(key, furthest);
            match stack.last_mut() {
                Some(below) => below.furthest = below.furthest.max(furthest),
                None => return Err(furthest),
            }
            continue;
        }
        let overload = top.next;
        top.next += 1;
        match match_overload(schema, top.definition, overload, line, top.start) {
            Ok((_, None)) => return Ok(()),
            Ok((_, Some(tail))) => pending = Some(tail),
            Err(at) => top.furthest = top.furthest.max(at),
        }
    }

    fn key(frame: &Frame) -> (usize, usize) {
        (std::ptr::from_ref(frame.definition) as usize, frame.start)
    }

    fn fail(stack: &mut [Frame], at: usize) {
        if let Some(top) = stack.last_mut() {
            top.furthest = top.furthest.max(at);
        }
    }
}

/// The frame that tries the overloads `tail` hands on to; a nested command
/// that does not exist fails at its name.
fn frame(schema: &CommandSchema, line: &str, tail: Tail) -> Result<Frame, ErrorAt> {
    let (definition, start) = match tail {
        Tail::Chained(definition, start) => (definition, start),
        Tail::Command(at) => {
            let at = at + usize::from(line[at..].starts_with('/'));
            let name = command_name(&line[at..]);
            (schema.command(name).ok_or(at)?, at + name.len())
        }
    };
    Ok(Frame {
        definition,
        start,
        next: 0,
        furthest: start,
    })
}

/// Matches `line[start..]` against one overload; a last parameter that hands
/// the rest of the line on returns it unchecked as a [`Tail`].
fn match_overload(
    schema: &CommandSchema,
    definition: &'static CommandDefinition,
    overload: usize,
    line: &str,
    start: usize,
) -> Result<(Args, Option<Tail>), ErrorAt> {
    let o = &definition.overloads[overload];
    let mut cursor = Cursor {
        text: line,
        pos: start,
    };
    let mut args = Args::new(definition, overload);
    for (index, param) in o.params.iter().enumerate() {
        let before = cursor.pos;
        cursor.skip_whitespace();
        if cursor.at_end() {
            // The line may stop here only if nothing after is required
            // (`/summon` has an optional position before required ones).
            if o.params[index..].iter().all(|p| p.optional) {
                break;
            }
            // A missing final `message` is not a syntax error on BDS: `/me`
            // and `/tell @s` reach the command, which answers
            // `commands.generic.invalidMessage`.
            if index + 1 == o.params.len() && param.kind == ParamKind::Basic(BasicType::Message) {
                args.insert(index, ArgValue::Message(String::new()));
                break;
            }
            return Err(cursor.pos);
        }
        // Parameters are separated by whitespace, except a block-state list
        // may follow its block name directly (`wool["color"="red"]`).
        if cursor.pos == before
            && before != start
            && !matches!(param.kind, ParamKind::Basic(BasicType::BlockStates))
        {
            return Err(cursor.pos);
        }
        // The rest of the line is the next subcommand of the same command
        // (`/execute as @s at @s run …`) or a nested command line (`run …`):
        // checked by the caller, so errors point into the whole line.
        let tail = match param.kind {
            ParamKind::Chained(_) => Some(Tail::Chained(definition, cursor.pos)),
            ParamKind::Basic(BasicType::Command) => Some(Tail::Command(cursor.pos)),
            _ => None,
        };
        if let Some(tail) = tail {
            let rest = cursor.rest().trim_end().to_owned();
            args.insert(
                index,
                match tail {
                    Tail::Chained(..) => ArgValue::Chained(rest),
                    Tail::Command(_) => ArgValue::Command(rest),
                },
            );
            return Ok((args, Some(tail)));
        }
        args.insert(index, parse_param(schema, param, &mut cursor)?);
    }
    cursor.skip_whitespace();
    if !cursor.at_end() {
        return Err(cursor.pos);
    }
    Ok((args, None))
}

fn parse_param(
    schema: &CommandSchema,
    param: &CommandParam,
    cursor: &mut Cursor<'_>,
) -> Result<ArgValue, ErrorAt> {
    let start = cursor.pos;
    match &param.kind {
        ParamKind::Basic(basic) => parse_basic(*basic, cursor),
        ParamKind::Enum(name) => {
            // Measured: an enum word ends at a bracket, brace, comma or `=`
            // (`/give @s stone{x}` is wrong at "{").
            let word = cursor.word(&['[', ']', '{', '}', ',', '=']);
            schema
                .enum_value(name, word)
                .map(ArgValue::Enum)
                .ok_or(start)
        }
        ParamKind::SoftEnum(_) => {
            let value = if cursor.peek() == Some('"') {
                cursor.quoted()?
            } else {
                unquoted_word(cursor)?
            };
            Ok(ArgValue::SoftEnum(value))
        }
        ParamKind::Postfix(postfix) => {
            let (_, word) = cursor.token()?;
            let lower = word.to_ascii_lowercase();
            let number = lower.strip_suffix(postfix.as_str()).ok_or(start)?;
            parse_int(number).map(ArgValue::Postfix).ok_or(start)
        }
        // A chained subcommand is a tail, handed on by `match_overload`.
        ParamKind::Chained(_) => Err(start),
    }
}

fn parse_int(text: &str) -> Option<i32> {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn parse_float(text: &str) -> Option<f32> {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    let valid = !digits.is_empty()
        && digits.bytes().any(|b| b.is_ascii_digit())
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && digits.bytes().filter(|&b| b == b'.').count() <= 1;
    if !valid {
        return None;
    }
    text.parse().ok().filter(|v: &f32| v.is_finite())
}

/// A string parameter: quoted, or one word.
fn string_value(cursor: &mut Cursor<'_>) -> Result<String, ErrorAt> {
    if cursor.peek() == Some('"') {
        cursor.quoted()
    } else {
        unquoted_word(cursor)
    }
}

/// An unquoted `string` or soft-enum word, which ends at `/` (measured: BDS
/// reads `loot entities/wither_boss` as the table `entities` and stops at
/// the `/`). File paths (`/function a/b`) keep their slashes.
fn unquoted_word(cursor: &mut Cursor<'_>) -> Result<String, ErrorAt> {
    let start = cursor.pos;
    let word = cursor.word(&['/']);
    if word.is_empty() {
        return Err(start);
    }
    Ok(word.to_owned())
}

/// The rest of the line (must not be empty).
fn rest_of_line(cursor: &mut Cursor<'_>) -> Result<String, ErrorAt> {
    let rest = cursor.rest().trim_end();
    if rest.is_empty() {
        return Err(cursor.pos);
    }
    cursor.pos = cursor.text.len();
    Ok(rest.to_owned())
}

fn parse_basic(basic: BasicType, cursor: &mut Cursor<'_>) -> Result<ArgValue, ErrorAt> {
    let start = cursor.pos;
    Ok(match basic {
        BasicType::Int => ArgValue::Int(parse_int(cursor.token()?.1).ok_or(start)?),
        BasicType::Float => ArgValue::Float(parse_float(cursor.token()?.1).ok_or(start)?),
        BasicType::Value => {
            let word = cursor.token()?.1;
            match word.strip_prefix('~') {
                Some("") => ArgValue::Value {
                    relative: true,
                    value: 0.0,
                },
                Some(offset) => ArgValue::Value {
                    relative: true,
                    value: parse_float(offset).ok_or(start)?,
                },
                None => ArgValue::Value {
                    relative: false,
                    value: parse_float(word).ok_or(start)?,
                },
            }
        }
        BasicType::WildcardInt => {
            let word = cursor.token()?.1;
            ArgValue::WildcardInt(if word == "*" {
                None
            } else {
                Some(parse_int(word).ok_or(start)?)
            })
        }
        BasicType::Operator => {
            let word = cursor.token()?.1;
            if !["=", "+=", "-=", "*=", "/=", "%=", "<", ">", "><"].contains(&word) {
                return Err(start);
            }
            ArgValue::Operator(word.to_owned())
        }
        BasicType::CompareOperator => {
            let word = cursor.token()?.1;
            if !["<", "<=", "=", ">=", ">"].contains(&word) {
                return Err(start);
            }
            ArgValue::CompareOperator(word.to_owned())
        }
        BasicType::Target => ArgValue::Target(parse_target(cursor)?),
        BasicType::WildcardTarget => {
            if cursor.rest().starts_with('*')
                && cursor.rest()[1..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
            {
                cursor.pos += 1;
                ArgValue::WildcardTarget(None)
            } else {
                ArgValue::WildcardTarget(Some(parse_target(cursor)?))
            }
        }
        BasicType::FilePath => ArgValue::FilePath(if cursor.peek() == Some('"') {
            cursor.quoted()?
        } else {
            cursor.token()?.1.to_owned()
        }),
        BasicType::IntegerRange => ArgValue::IntRange(parse_range(cursor.token()?.1).ok_or(start)?),
        BasicType::String => ArgValue::String(string_value(cursor)?),
        BasicType::BlockPosition => ArgValue::BlockPosition(parse_position(cursor)?),
        BasicType::Position => ArgValue::Position(parse_position(cursor)?),
        BasicType::Message => ArgValue::Message(rest_of_line(cursor)?),
        BasicType::RawText => ArgValue::RawText(rest_of_line(cursor)?),
        BasicType::Command => ArgValue::Command(rest_of_line(cursor)?),
        BasicType::Json => {
            let (end, read) = json::parse(cursor.text, cursor.pos)?;
            cursor.pos = end;
            ArgValue::Json(read)
        }
        BasicType::BlockStates => ArgValue::BlockStates(parse_block_states(cursor)?),
    })
}

/// `name`, `"quoted name"` or a selector.
pub fn parse_target(cursor: &mut Cursor<'_>) -> Result<Target, ErrorAt> {
    match cursor.peek() {
        Some('@') => Ok(Target::Selector(Box::new(parse_selector(cursor)?))),
        Some('"') => Ok(Target::Name(cursor.quoted()?)),
        _ => Ok(Target::Name(cursor.token()?.1.to_owned())),
    }
}

/// `5`, `1..5`, `..5`, `1..`, optionally prefixed with `!`.
pub fn parse_range(text: &str) -> Option<IntRange> {
    let (negated, text) = match text.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (min, max) = match text.split_once("..") {
        Some((min, max)) => (
            if min.is_empty() {
                None
            } else {
                Some(parse_int(min)?)
            },
            if max.is_empty() {
                None
            } else {
                Some(parse_int(max)?)
            },
        ),
        None => {
            let value = parse_int(text)?;
            (Some(value), Some(value))
        }
    };
    if min.is_none() && max.is_none() {
        return None;
    }
    Some(IntRange { min, max, negated })
}

/// Three coordinates; they may be glued (`~~1~`) when each starts with `~`/`^`.
fn parse_position(cursor: &mut Cursor<'_>) -> Result<CommandPosition, ErrorAt> {
    let mut coords = [Coordinate::Absolute(0.0); 3];
    for (axis, slot) in coords.iter_mut().enumerate() {
        if axis > 0 {
            let before = cursor.pos;
            cursor.skip_whitespace();
            if cursor.at_end() {
                return Err(cursor.pos);
            }
            if cursor.pos == before && !matches!(cursor.peek(), Some('~' | '^')) {
                return Err(cursor.pos);
            }
        }
        let start = cursor.pos;
        let prefix = cursor.peek();
        if matches!(prefix, Some('~' | '^')) {
            cursor.pos += 1;
        }
        let rest = cursor.rest();
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '~' || c == '^')
            .unwrap_or(rest.len());
        let number = &rest[..end];
        cursor.pos += end;
        let value = |number: &str| -> Result<f64, ErrorAt> {
            if number.is_empty() {
                return Ok(0.0);
            }
            parse_float(number).map(f64::from).ok_or(start)
        };
        *slot = match prefix {
            Some('~') => Coordinate::Relative(value(number)?),
            Some('^') => Coordinate::Local(value(number)?),
            _ => {
                if number.is_empty() {
                    return Err(start);
                }
                let v = value(number)?;
                // Bedrock centres whole-number x/z positions on their block.
                let centred = axis != 1 && !number.contains('.');
                Coordinate::Absolute(if centred { v + 0.5 } else { v })
            }
        };
    }
    let local = coords
        .iter()
        .filter(|c| matches!(c, Coordinate::Local(_)))
        .count();
    if local != 0 && local != 3 {
        return Err(cursor.pos);
    }
    Ok(CommandPosition(coords))
}

/// `["name"="value", "age"=3, "open"=true]`.
fn parse_block_states(cursor: &mut Cursor<'_>) -> Result<Vec<(String, StateLiteral)>, ErrorAt> {
    let start = cursor.pos;
    if cursor.peek() != Some('[') {
        return Err(start);
    }
    let group = cursor.balanced()?;
    let mut inner = Cursor::new(&group[1..group.len() - 1]);
    let mut states = Vec::new();
    loop {
        inner.skip_whitespace();
        if inner.at_end() {
            break;
        }
        let key = inner.quoted().map_err(|_| start)?;
        inner.skip_whitespace();
        if !inner.eat('=') && !inner.eat(':') {
            return Err(start);
        }
        inner.skip_whitespace();
        let value = if inner.peek() == Some('"') {
            StateLiteral::String(inner.quoted().map_err(|_| start)?)
        } else {
            let word = inner.word(&[',']);
            match word {
                "true" => StateLiteral::Bool(true),
                "false" => StateLiteral::Bool(false),
                _ => StateLiteral::Int(parse_int(word).ok_or(start)?),
            }
        };
        states.push((key, value));
        inner.skip_whitespace();
        if !inner.eat(',') {
            inner.skip_whitespace();
            if !inner.at_end() {
                return Err(start);
            }
            break;
        }
    }
    Ok(states)
}

#[cfg(test)]
mod tests;
