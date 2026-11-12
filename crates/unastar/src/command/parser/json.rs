//! The `json` parameter (`/tellraw`, `/titleraw`, `/give` components).
//!
//! Measured on BDS 1.26.51 (`tools/bds-sweep/command_probe.py`, player
//! `/tellraw` lines): BDS first checks the message token by token, and a
//! token that does not fit is a `commands.generic.syntax` error at that
//! token; a message that passes is then read as JSON, and what the reader
//! refuses answers `commands.generic.json.invalid [errors]`.
//!
//! Tokens: `{ } [ ] , :` stand alone; a `"` string runs to its closing quote
//! (unterminated: to the end of the line); a `'` runs to the end of the line;
//! anything else is a word up to whitespace, punctuation or a quote. The
//! check is looser than JSON in places and stricter in others:
//! - the message must be an object: after a first `[` every token is wrong
//!   (`[]` fails at `]`);
//! - an object key may be a string or any word but a number (`{bad:1}` and
//!   `{true:1}` pass, `{1}` fails at `1`);
//! - a value is a string, an object, an array, `true`, `false` or a number
//!   `-?digits` with an optional `.` and digits (`1.` passes; `.5`, `1x`,
//!   `1.5e3` and `null` fail);
//! - an array takes commas anywhere (`[1,]`, `[,]`, `[1,,2]` pass), but two
//!   values need one between them (`[1 2]` fails at `2`);
//! - a member's value may be left out before `}` (`{"a":}` passes), a member
//!   after `,` may not (`{"x":1,}` fails at `}`).
//!
//! What the reader then refuses answers its errors as `* Line 1, Column C\n
//! message\n` (columns from 1 within the message): a word key is `Missing
//! '}' or object member name` at the key; a left-out value is `Syntax error:
//! value, object or array expected.` at the token in its place; and every
//! object around the error adds `Missing '}' or object member name` at the
//! key of its member that holds it (`{"rawtext":[1,]}`: column 15, then
//! column 2).

use super::ErrorAt;
use serde_json::{Map, Number, Value};

/// A `json` parameter that passed the syntax check.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonArg {
    Valid(Value),
    /// Refused by the JSON reader: the text of
    /// `commands.generic.json.invalid`.
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// One of `{ } [ ] , :`.
    Punct(char),
    String,
    /// A token that never fits (`'…`, an unterminated string).
    Bad,
    Word,
    End,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

/// The token at `pos`, after whitespace.
fn token(line: &str, pos: usize) -> Token {
    let rest = &line[pos..];
    let start = pos + (rest.len() - rest.trim_start().len());
    let rest = &line[start..];
    let Some(first) = rest.chars().next() else {
        return Token {
            kind: Kind::End,
            start,
            end: start,
        };
    };
    let (kind, len) = match first {
        '{' | '}' | '[' | ']' | ',' | ':' => (Kind::Punct(first), 1),
        '"' => string_len(rest).map_or((Kind::Bad, rest.len()), |len| (Kind::String, len)),
        '\'' => (Kind::Bad, rest.len()),
        _ => (
            Kind::Word,
            rest.find(|c: char| c.is_whitespace() || "{}[],:\"".contains(c))
                .unwrap_or(rest.len()),
        ),
    };
    Token {
        kind,
        start,
        end: start + len,
    }
}

/// Length of the `"…"` string `text` starts with, quotes included.
fn string_len(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in text.char_indices().skip(1) {
        match (escaped, c) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// A number as BDS's check accepts it: `-?digits(.digits*)?`.
fn number(word: &str) -> Option<Number> {
    let digits = word.strip_prefix('-').unwrap_or(word);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if !digits.contains('.')
        && let Ok(integer) = word.parse::<i64>()
    {
        return Some(Number::from(integer));
    }
    Number::from_f64(word.trim_end_matches('.').parse().ok()?)
}

/// A string token's text (escapes resolved as JSON does).
fn string_text(token: &str) -> String {
    serde_json::from_str(token).unwrap_or_else(|_| token[1..token.len() - 1].to_owned())
}

/// An open container and where it stands.
enum Open {
    Object {
        map: Map<String, Value>,
        state: ObjectState,
        /// The current member's key and the key's column.
        key: (String, usize),
    },
    Array {
        items: Vec<Value>,
        /// The last token: `[`, a value or `,`.
        last: ArrayLast,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectState {
    /// After `{`: a key or `}`.
    Open,
    /// After a key: `:`.
    Key,
    /// After `:`: a value or `}`.
    Colon,
    /// After a value: `,` or `}`.
    Value,
    /// After `,`: a key.
    Comma,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrayLast {
    Open,
    Value,
    Comma,
}

/// What a token does to the message.
enum Step {
    /// The open container stays open.
    Stay,
    Push(Open),
    /// A value is complete (a scalar, or the container the token closed).
    Value(Value),
}

/// The first error the JSON reader would report.
struct ReadError {
    column: usize,
    message: &'static str,
    /// Key columns of the objects around it, innermost first.
    around: Vec<usize>,
}

const MISSING_MEMBER: &str = "Missing '}' or object member name";
const VALUE_EXPECTED: &str = "Syntax error: value, object or array expected.";

impl ReadError {
    /// `skip_top`: the innermost object reported the error itself.
    fn new(stack: &[Open], column: usize, message: &'static str, skip_top: bool) -> Self {
        let objects = stack.len() - usize::from(skip_top);
        let around = stack[..objects]
            .iter()
            .rev()
            .filter_map(|open| match open {
                Open::Object { key, .. } => Some(key.1),
                Open::Array { .. } => None,
            })
            .collect();
        Self {
            column,
            message,
            around,
        }
    }

    fn text(&self) -> String {
        let mut text = format!("* Line 1, Column {}\n  {}\n", self.column, self.message);
        for column in &self.around {
            text.push_str(&format!("* Line 1, Column {column}\n  {MISSING_MEMBER}\n"));
        }
        text
    }
}

/// Checks the message that starts at `start` and reads it. `Ok` is the end
/// of the message and what it reads as; `Err` the token the check stopped
/// at. Nesting is kept on the heap, so any depth is safe.
pub(super) fn parse(line: &str, start: usize) -> Result<(usize, JsonArg), ErrorAt> {
    let column = |pos: usize| pos - start + 1;
    let first = token(line, start);
    match first.kind {
        Kind::Punct('{') => {}
        // The message must be an object: whatever follows a `[` is wrong.
        Kind::Punct('[') => return Err(token(line, first.end).start),
        _ => return Err(first.start),
    }
    let mut stack = vec![object()];
    let mut error: Option<ReadError> = None;
    let mut pos = first.end;
    loop {
        let t = token(line, pos);
        let text = &line[t.start..t.end];
        pos = t.end;
        // The reader's first error, if here: its message, and whether the
        // innermost object reports it itself.
        let mut refused: Option<(&'static str, bool)> = None;
        let Some(top) = stack.last_mut() else {
            return Err(t.start);
        };
        let step = match top {
            Open::Object { map, state, key } => match (*state, t.kind) {
                (ObjectState::Open | ObjectState::Value, Kind::Punct('}')) => {
                    Step::Value(Value::Object(std::mem::take(map)))
                }
                (ObjectState::Open | ObjectState::Comma, Kind::String | Kind::Word) => {
                    if t.kind == Kind::Word && number(text).is_some() {
                        return Err(t.start);
                    }
                    *key = match t.kind {
                        Kind::String => (string_text(text), column(t.start)),
                        _ => (text.to_owned(), column(t.start)),
                    };
                    *state = ObjectState::Key;
                    if t.kind == Kind::Word {
                        refused = Some((MISSING_MEMBER, true));
                    }
                    Step::Stay
                }
                (ObjectState::Key, Kind::Punct(':')) => {
                    *state = ObjectState::Colon;
                    Step::Stay
                }
                (ObjectState::Colon, Kind::Punct('}')) => {
                    refused = Some((VALUE_EXPECTED, false));
                    Step::Value(Value::Object(std::mem::take(map)))
                }
                (ObjectState::Colon, _) => value(t, text).ok_or(t.start)?,
                (ObjectState::Value, Kind::Punct(',')) => {
                    *state = ObjectState::Comma;
                    Step::Stay
                }
                _ => return Err(t.start),
            },
            Open::Array { items, last } => match (t.kind, *last) {
                (Kind::Punct(']'), previous) => {
                    if previous == ArrayLast::Comma {
                        refused = Some((VALUE_EXPECTED, false));
                    }
                    Step::Value(Value::Array(std::mem::take(items)))
                }
                (Kind::Punct(','), previous) => {
                    *last = ArrayLast::Comma;
                    if previous != ArrayLast::Value {
                        refused = Some((VALUE_EXPECTED, false));
                    }
                    Step::Stay
                }
                (_, ArrayLast::Value) => return Err(t.start),
                _ => value(t, text).ok_or(t.start)?,
            },
        };
        if let Some((message, skip_top)) = refused
            && error.is_none()
        {
            error = Some(ReadError::new(&stack, column(t.start), message, skip_top));
        }
        let mut completed = match step {
            Step::Stay => continue,
            Step::Push(open) => {
                stack.push(open);
                continue;
            }
            Step::Value(value) => value,
        };
        // A closed container is a value of the one around it.
        if matches!(t.kind, Kind::Punct('}' | ']')) {
            stack.pop();
        }
        match stack.last_mut() {
            None => {
                let read = match error {
                    Some(error) => JsonArg::Invalid(error.text()),
                    None => JsonArg::Valid(completed),
                };
                return Ok((pos, read));
            }
            Some(Open::Object { map, state, key }) => {
                map.insert(std::mem::take(&mut key.0), std::mem::take(&mut completed));
                *state = ObjectState::Value;
            }
            Some(Open::Array { items, last }) => {
                items.push(completed);
                *last = ArrayLast::Value;
            }
        }
    }
}

fn object() -> Open {
    Open::Object {
        map: Map::new(),
        state: ObjectState::Open,
        key: (String::new(), 0),
    }
}

/// A value token: a scalar, or a container it opens.
fn value(t: Token, text: &str) -> Option<Step> {
    Some(match t.kind {
        Kind::Punct('{') => Step::Push(object()),
        Kind::Punct('[') => Step::Push(Open::Array {
            items: Vec::new(),
            last: ArrayLast::Open,
        }),
        Kind::String => Step::Value(Value::String(string_text(text))),
        Kind::Word if text == "true" => Step::Value(Value::Bool(true)),
        Kind::Word if text == "false" => Step::Value(Value::Bool(false)),
        Kind::Word => Step::Value(Value::Number(number(text)?)),
        _ => return None,
    })
}
