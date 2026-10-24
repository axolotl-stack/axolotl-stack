//! Normalised JSON for decoded Valentine packets.
//!
//! Generated Valentine types derive `Debug` but not `Serialize`, and hand
//! written per-packet normalisers would have to follow every regeneration.
//! Instead this module parses the derived `{:?}` form, which is fully
//! determined by the type structure, into `serde_json::Value`:
//!
//! | Debug form | JSON |
//! |---|---|
//! | `Name { a: x, b: y }` (struct, struct variant) | `{"a": x, "b": y}` (the type name is static and dropped) |
//! | `Some(x)` / `None` | `x` / `null` |
//! | `Nbt(b"..")`, `NbtCompoundBody(b"..")` | the decoded network little-endian NBT tree |
//! | `Name(x)` (newtype, enum tuple variant) | `{"Name": x}` |
//! | `Name(x, y)` | `{"Name": [x, y]}` |
//! | `Name` (unit variant) | `"Name"` |
//! | `[..]`, `(..)`, `{a, b}` (set) | array |
//! | `{k: v}` (map) | object, keys as strings |
//! | `b".."` (raw bytes) | lowercase hex string |
//! | `A \| B` (bitflags) | `"A \| B"` |
//! | numbers, `true`/`false`, strings, chars | JSON scalars; `NaN`/`inf` as strings |
//! | other bare tokens (UUIDs, `0x0`) | string |
//!
//! NBT compounds are emitted with sorted keys; tag types are not kept (the
//! raw `.bin` next to every JSON file is the exact form).

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;

/// Converts the `{:?}` rendering of a value into normalised JSON.
pub fn to_json(debug: &str) -> Result<Value> {
    let mut parser = Parser {
        input: debug.as_bytes(),
        pos: 0,
    };
    let value = parser.value()?;
    parser.skip_ws();
    ensure!(
        parser.pos == parser.input.len(),
        "trailing input at byte {}",
        parser.pos
    );
    Ok(value)
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<()> {
        self.skip_ws();
        ensure!(
            self.peek() == Some(byte),
            "expected '{}' at byte {}",
            byte as char,
            self.pos
        );
        self.pos += 1;
        Ok(())
    }

    /// Consumes `byte` if it is next (after whitespace).
    fn eat(&mut self, byte: u8) -> bool {
        self.skip_ws();
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Value> {
        self.skip_ws();
        let value = match self.peek().context("unexpected end of input")? {
            b'"' => Value::String(self.string(b'"')?),
            b'\'' => Value::String(self.string(b'\'')?),
            b'[' => {
                self.pos += 1;
                Value::Array(self.sequence(b']')?)
            }
            b'(' => {
                self.pos += 1;
                Value::Array(self.sequence(b')')?)
            }
            b'{' => {
                self.pos += 1;
                self.map_or_set()?
            }
            b'b' if self.input.get(self.pos + 1) == Some(&b'"') => {
                self.pos += 1;
                Value::String(hex::encode(self.byte_string()?))
            }
            _ => self.token_value()?,
        };
        // Bitflags render their set flags as `A | B`.
        if self.eat(b'|') {
            let mut text = scalar_text(&value)?;
            loop {
                text.push_str(" | ");
                text.push_str(&scalar_text(&self.token_value()?)?);
                if !self.eat(b'|') {
                    break;
                }
            }
            return Ok(Value::String(text));
        }
        Ok(value)
    }

    fn sequence(&mut self, close: u8) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        if self.eat(close) {
            return Ok(items);
        }
        loop {
            items.push(self.value()?);
            if self.eat(close) {
                return Ok(items);
            }
            self.expect(b',')?;
            // Pretty Debug and some impls leave a trailing comma.
            if self.eat(close) {
                return Ok(items);
            }
        }
    }

    fn map_or_set(&mut self) -> Result<Value> {
        if self.eat(b'}') {
            return Ok(Value::Object(Map::new()));
        }
        let first = self.value()?;
        if self.eat(b':') {
            let mut map = Map::new();
            let value = self.value()?;
            map.insert(key_text(&first)?, value);
            loop {
                if self.eat(b'}') {
                    return Ok(Value::Object(map));
                }
                self.expect(b',')?;
                if self.eat(b'}') {
                    return Ok(Value::Object(map));
                }
                let key = self.value()?;
                self.expect(b':')?;
                let value = self.value()?;
                map.insert(key_text(&key)?, value);
            }
        }
        let mut items = vec![first];
        loop {
            if self.eat(b'}') {
                return Ok(Value::Array(items));
            }
            self.expect(b',')?;
            if self.eat(b'}') {
                return Ok(Value::Array(items));
            }
            items.push(self.value()?);
        }
    }

    fn token(&mut self) -> Result<&str> {
        self.skip_ws();
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || b"_-+.#".contains(&byte))
        {
            self.pos += 1;
        }
        ensure!(self.pos > start, "unexpected byte at {}", self.pos);
        Ok(std::str::from_utf8(&self.input[start..self.pos])?)
    }

    fn token_value(&mut self) -> Result<Value> {
        let token = self.token()?.to_owned();
        let is_ident = token
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        if is_ident {
            self.skip_ws();
            match self.peek() {
                Some(b'{') => {
                    self.pos += 1;
                    return self.struct_fields();
                }
                Some(b'(') => {
                    self.pos += 1;
                    let items = self.sequence(b')')?;
                    return tuple_value(&token, items);
                }
                _ => {}
            }
        }
        Ok(scalar(&token))
    }

    fn struct_fields(&mut self) -> Result<Value> {
        let mut map = Map::new();
        if self.eat(b'}') {
            return Ok(Value::Object(map));
        }
        loop {
            let name = self.token()?.to_owned();
            // `..` marks fields omitted by `finish_non_exhaustive`.
            if name == ".." {
                self.expect(b'}')?;
                return Ok(Value::Object(map));
            }
            self.expect(b':')?;
            let value = self.value()?;
            map.insert(name, value);
            if self.eat(b'}') {
                return Ok(Value::Object(map));
            }
            self.expect(b',')?;
            if self.eat(b'}') {
                return Ok(Value::Object(map));
            }
        }
    }

    fn string(&mut self, quote: u8) -> Result<String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let rest = &self.input[self.pos..];
            let end = rest
                .iter()
                .position(|&byte| byte == quote || byte == b'\\')
                .context("unterminated string")?;
            out.push_str(std::str::from_utf8(&rest[..end])?);
            self.pos += end;
            if self.input[self.pos] == quote {
                self.pos += 1;
                return Ok(out);
            }
            self.pos += 1;
            let escape = self.peek().context("unterminated escape")?;
            self.pos += 1;
            match escape {
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'0' => out.push('\0'),
                b'\\' => out.push('\\'),
                b'"' => out.push('"'),
                b'\'' => out.push('\''),
                b'u' => {
                    self.expect(b'{')?;
                    let start = self.pos;
                    while self.peek().is_some_and(|byte| byte != b'}') {
                        self.pos += 1;
                    }
                    let code = u32::from_str_radix(
                        std::str::from_utf8(&self.input[start..self.pos])?,
                        16,
                    )?;
                    self.pos += 1;
                    out.push(char::from_u32(code).context("invalid \\u escape")?);
                }
                other => bail!("unknown string escape \\{}", other as char),
            }
        }
    }

    fn byte_string(&mut self) -> Result<Vec<u8>> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let byte = self.peek().context("unterminated byte string")?;
            self.pos += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escape = self.peek().context("unterminated escape")?;
                    self.pos += 1;
                    out.push(match escape {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'0' => 0,
                        b'\\' => b'\\',
                        b'"' => b'"',
                        b'\'' => b'\'',
                        b'x' => {
                            let digits = self
                                .input
                                .get(self.pos..self.pos + 2)
                                .context("truncated \\x escape")?;
                            self.pos += 2;
                            u8::from_str_radix(std::str::from_utf8(digits)?, 16)?
                        }
                        other => bail!("unknown byte escape \\{}", other as char),
                    });
                }
                other => out.push(other),
            }
        }
    }
}

fn tuple_value(name: &str, mut items: Vec<Value>) -> Result<Value> {
    match (name, items.len()) {
        ("Some", 1) => Ok(items.pop().unwrap_or(Value::Null)),
        ("Nbt", 1) => nbt_json(&hex_bytes(&items[0])?),
        ("NbtCompoundBody", 1) => {
            let mut bytes = vec![0x0a, 0x00];
            bytes.extend(hex_bytes(&items[0])?);
            nbt_json(&bytes)
        }
        (_, 1) => Ok(Value::Object(Map::from_iter([(
            name.to_owned(),
            items.pop().unwrap_or(Value::Null),
        )]))),
        _ => Ok(Value::Object(Map::from_iter([(
            name.to_owned(),
            Value::Array(items),
        )]))),
    }
}

fn hex_bytes(value: &Value) -> Result<Vec<u8>> {
    let text = value.as_str().context("NBT payload is not a byte string")?;
    Ok(hex::decode(text)?)
}

fn scalar(token: &str) -> Value {
    match token {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "None" => return Value::Null,
        _ => {}
    }
    if let Ok(value) = token.parse::<i64>() {
        return Value::Number(value.into());
    }
    if let Ok(value) = token.parse::<u64>() {
        return Value::Number(value.into());
    }
    let numeric = token
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_digit() || byte == b'-');
    if numeric && let Ok(value) = token.parse::<f64>() {
        // NaN and infinities have no JSON number form.
        if let Some(number) = Number::from_f64(value) {
            return Value::Number(number);
        }
    }
    Value::String(token.to_owned())
}

fn scalar_text(value: &Value) -> Result<String> {
    Ok(match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        other => bail!("unexpected bitflag operand {other}"),
    })
}

fn key_text(value: &Value) -> Result<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string()),
        other => Ok(serde_json::to_string(other)?),
    }
}

/// Decodes one network little-endian NBT tag (named root) into JSON.
pub fn nbt_json(bytes: &[u8]) -> Result<Value> {
    let mut reader = bytes;
    let tag = zuri_nbt::NBTTag::read(&mut reader, zuri_nbt::encoding::NetworkLittleEndian)
        .map_err(|error| anyhow::anyhow!("invalid network NBT: {error:?}"))?;
    ensure!(reader.is_empty(), "trailing bytes after NBT");
    Ok(tag_json(&tag))
}

fn tag_json(tag: &zuri_nbt::NBTTag) -> Value {
    use zuri_nbt::NBTTag;
    match tag {
        NBTTag::Byte(value) => value.0.into(),
        NBTTag::Short(value) => value.0.into(),
        NBTTag::Int(value) => value.0.into(),
        NBTTag::Long(value) => value.0.into(),
        NBTTag::Float(value) => float_json(f64::from(value.0)),
        NBTTag::Double(value) => float_json(value.0),
        NBTTag::String(value) => value.0.clone().into(),
        NBTTag::Compound(value) => {
            let sorted: BTreeMap<_, _> = value.0.iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key.clone(), tag_json(value)))
                    .collect(),
            )
        }
        NBTTag::List(value) => Value::Array(value.0.iter().map(tag_json).collect()),
        NBTTag::ByteArray(value) => value.0.clone().into(),
        NBTTag::IntArray(value) => value.0.clone().into(),
        NBTTag::LongArray(value) => value.0.clone().into(),
    }
}

fn float_json(value: f64) -> Value {
    Number::from_f64(value).map_or_else(|| Value::String(value.to_string()), Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug)]
    #[allow(dead_code)]
    struct Inner {
        name: String,
        ratio: f32,
        tiny: f32,
        missing: Option<u8>,
        present: Option<i64>,
    }

    #[derive(Debug)]
    #[allow(dead_code)]
    enum Kind {
        Unit,
        Wrapped(Inner),
        Pair(i32, i32),
    }

    #[test]
    fn derived_debug_of_structs_enums_and_options_becomes_json() {
        let value = (
            Kind::Unit,
            Kind::Wrapped(Inner {
                name: "quote \" and \\ and \n and é".into(),
                ratio: -1.5,
                tiny: 1e-7,
                missing: None,
                present: Some(-9_000_000_000),
            }),
            Kind::Pair(1, -2),
            vec![f32::NAN, f32::INFINITY],
            uuid::Uuid::nil(),
        );
        assert_eq!(
            to_json(&format!("{value:?}")).unwrap(),
            json!([
                "Unit",
                {"Wrapped": {
                    "name": "quote \" and \\ and \n and é",
                    "ratio": -1.5,
                    "tiny": 1e-7,
                    "missing": null,
                    "present": -9_000_000_000i64,
                }},
                {"Pair": [1, -2]},
                ["NaN", "inf"],
                "00000000-0000-0000-0000-000000000000",
            ])
        );
    }

    #[test]
    fn maps_sets_bitflags_and_raw_bytes() {
        let map = BTreeMap::from([("a", 1), ("b", 2)]);
        let set = std::collections::BTreeSet::from([3, 4]);
        let bytes = bytes::Bytes::from_static(b"\x00\x7f\"\\az");
        assert_eq!(
            to_json(&format!("{map:?}")).unwrap(),
            json!({"a": 1, "b": 2})
        );
        assert_eq!(to_json(&format!("{set:?}")).unwrap(), json!([3, 4]));
        assert_eq!(
            to_json(&format!("{bytes:?}")).unwrap(),
            json!("007f225c617a")
        );
        assert_eq!(
            to_json("Abilities(BUILD | MINE | 0x80)").unwrap(),
            json!({"Abilities": "BUILD | MINE | 0x80"})
        );
    }

    #[test]
    fn nbt_payloads_are_decoded_with_sorted_keys() {
        // Network little-endian compound "" { "b": byte 1, "a": string "x" }.
        let nbt = bytes::Bytes::from_static(&[
            0x0a, 0x00, 0x01, 0x01, b'b', 0x01, 0x08, 0x01, b'a', 0x01, b'x', 0x00,
        ]);
        let wrapped = valentine::bedrock::codec::Nbt(nbt);
        let value = to_json(&format!("{wrapped:?}")).unwrap();
        assert_eq!(value, json!({"a": "x", "b": 1}));
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"a":"x","b":1}"#
        );
    }

    #[test]
    fn malformed_debug_is_rejected() {
        assert!(to_json("Foo { a: 1").is_err());
        assert!(to_json("[1, 2] trailing").is_err());
        assert!(to_json("\"unterminated").is_err());
    }
}
