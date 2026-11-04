//! A generic view of decoded packets for field-by-field comparison.
//!
//! The generated protocol types implement `Debug` (not serde), so a decoded
//! packet is turned into a [`Node`] tree by parsing its `{:?}` output: structs
//! keep their field names, enum variants their names, lists their order.
//! Masks address nodes by path (`actor_data.data[id=5].payload`), and the
//! comparison walks two trees in parallel.

use std::fmt::Write;

use anyhow::{Result, bail, ensure};

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// `Name { field: value, .. }`
    Struct(String, Vec<(String, Node)>),
    /// `Name(value, ..)`, or an anonymous tuple when the name is empty.
    Tuple(String, Vec<Node>),
    /// `[value, ..]`
    List(Vec<Node>),
    /// `{key: value, ..}`
    Map(Vec<(Node, Node)>),
    /// Numbers, strings, unit variants, bit flags, byte strings.
    Atom(String),
}

#[derive(Debug, PartialEq)]
enum Token {
    Open(char),
    Close(char),
    Comma,
    Colon,
    /// An identifier, number or other bare word.
    Word(String),
    /// A quoted string, byte string or char literal, quotes included.
    Quoted(String),
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '{' | '(' | '[' => tokens.push(Token::Open(c)),
            '}' | ')' | ']' => tokens.push(Token::Close(c)),
            ',' => tokens.push(Token::Comma),
            ':' => tokens.push(Token::Colon),
            '"' | '\'' => {
                let end = quoted_end(text, start, c)?;
                tokens.push(Token::Quoted(text[start..end].to_owned()));
                while chars.peek().is_some_and(|&(i, _)| i < end) {
                    chars.next();
                }
            }
            'b' if chars.peek().is_some_and(|&(_, next)| next == '"') => {
                let (quote, _) = chars.next().expect("peeked");
                let end = quoted_end(text, quote, '"')?;
                tokens.push(Token::Quoted(text[start..end].to_owned()));
                while chars.peek().is_some_and(|&(i, _)| i < end) {
                    chars.next();
                }
            }
            _ => {
                let mut end = start + c.len_utf8();
                while let Some(&(i, next)) = chars.peek() {
                    if next.is_whitespace() || "{}()[],:\"".contains(next) {
                        break;
                    }
                    end = i + next.len_utf8();
                    chars.next();
                }
                tokens.push(Token::Word(text[start..end].to_owned()));
            }
        }
    }
    Ok(tokens)
}

/// The byte index just past the literal starting with `quote` at `start`.
fn quoted_end(text: &str, start: usize, quote: char) -> Result<usize> {
    let mut escaped = false;
    for (i, c) in text[start + 1..].char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Ok(start + 1 + i + 1);
        }
    }
    bail!("unterminated literal at byte {start}")
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<&Token> {
        self.at += 1;
        self.tokens.get(self.at - 1)
    }

    /// Values separated by commas up to `close` (trailing comma allowed).
    fn items(&mut self, close: char) -> Result<Vec<Node>> {
        let mut items = Vec::new();
        loop {
            if matches!(self.peek(), Some(Token::Close(c)) if *c == close) {
                self.at += 1;
                return Ok(items);
            }
            items.push(self.value()?);
            match self.next() {
                Some(Token::Comma) => {}
                Some(Token::Close(c)) if *c == close => return Ok(items),
                other => bail!("expected ',' or {close:?}, found {other:?}"),
            }
        }
    }

    fn value(&mut self) -> Result<Node> {
        match self.next() {
            Some(Token::Open('[')) => Ok(Node::List(self.items(']')?)),
            Some(Token::Open('(')) => Ok(Node::Tuple(String::new(), self.items(')')?)),
            Some(Token::Open('{')) => self.map(),
            Some(Token::Quoted(text)) => Ok(Node::Atom(text.clone())),
            Some(Token::Word(word)) => {
                let mut name = word.clone();
                match self.peek() {
                    Some(Token::Open('{')) => {
                        self.at += 1;
                        self.fields(name)
                    }
                    Some(Token::Open('(')) => {
                        self.at += 1;
                        Ok(Node::Tuple(name, self.items(')')?))
                    }
                    _ => {
                        // Bit flags print as `A | B`: keep the words together.
                        while let Some(Token::Word(word)) = self.peek() {
                            name.push(' ');
                            name.push_str(word);
                            self.at += 1;
                        }
                        Ok(Node::Atom(name))
                    }
                }
            }
            other => bail!("unexpected {other:?}"),
        }
    }

    fn fields(&mut self, name: String) -> Result<Node> {
        let mut fields = Vec::new();
        loop {
            match self.next() {
                Some(Token::Close('}')) => return Ok(Node::Struct(name, fields)),
                Some(Token::Word(field)) => {
                    let field = field.clone();
                    match self.next() {
                        Some(Token::Colon) => {}
                        other => bail!("expected ':' after field {field}, found {other:?}"),
                    }
                    fields.push((field, self.value()?));
                    match self.next() {
                        Some(Token::Comma) => {}
                        Some(Token::Close('}')) => return Ok(Node::Struct(name, fields)),
                        other => bail!("expected ',' or '}}', found {other:?}"),
                    }
                }
                None => bail!("unterminated struct {name}"),
                other => bail!("unexpected {other:?} in struct {name}"),
            }
        }
    }

    fn map(&mut self) -> Result<Node> {
        let mut entries = Vec::new();
        loop {
            if matches!(self.peek(), Some(Token::Close('}'))) {
                self.at += 1;
                return Ok(Node::Map(entries));
            }
            let key = self.value()?;
            match self.next() {
                Some(Token::Colon) => {}
                other => bail!("expected ':' in map, found {other:?}"),
            }
            entries.push((key, self.value()?));
            match self.next() {
                Some(Token::Comma) => {}
                Some(Token::Close('}')) => return Ok(Node::Map(entries)),
                other => bail!("expected ',' or '}}' in map, found {other:?}"),
            }
        }
    }
}

impl Node {
    /// Parses `Debug` output (compact or pretty).
    pub fn parse(text: &str) -> Result<Node> {
        let mut parser = Parser {
            tokens: tokenize(text)?,
            at: 0,
        };
        let node = parser.value()?;
        ensure!(
            parser.at == parser.tokens.len(),
            "trailing input after value: {:?}",
            parser.peek()
        );
        Ok(node)
    }

    /// The `Debug` tree of a value.
    pub fn of(value: &impl std::fmt::Debug) -> Result<Node> {
        Node::parse(&format!("{value:?}"))
    }

    pub fn field(&self, name: &str) -> Option<&Node> {
        match self {
            Node::Struct(_, fields) => fields.iter().find(|(f, _)| f == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A one-line rendering (compact Debug style).
    pub fn inline(&self) -> String {
        let mut out = String::new();
        self.write_inline(&mut out);
        out
    }

    fn write_inline(&self, out: &mut String) {
        match self {
            Node::Atom(atom) => out.push_str(atom),
            Node::Struct(name, fields) => {
                let _ = write!(out, "{name} {{ ");
                for (i, (field, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "{field}: ");
                    value.write_inline(out);
                }
                out.push_str(" }");
            }
            Node::Tuple(name, items) => {
                out.push_str(name);
                out.push('(');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write_inline(out);
                }
                out.push(')');
            }
            Node::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write_inline(out);
                }
                out.push(']');
            }
            Node::Map(entries) => {
                out.push('{');
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    key.write_inline(out);
                    out.push_str(": ");
                    value.write_inline(out);
                }
                out.push('}');
            }
        }
    }

    /// One line per leaf as `path = value`, the form reports show side by
    /// side. Short subtrees stay on one line.
    pub fn leaves(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.collect_leaves(String::new(), &mut out);
        out
    }

    fn collect_leaves(&self, path: String, out: &mut Vec<(String, String)>) {
        let inline = self.inline();
        if inline.len() <= 48 || matches!(self, Node::Atom(_)) {
            out.push((path, inline));
            return;
        }
        match self {
            Node::Struct(_, fields) => {
                for (field, value) in fields {
                    value.collect_leaves(join(&path, field), out);
                }
            }
            Node::Tuple(name, items) => {
                for (i, item) in items.iter().enumerate() {
                    let segment = if items.len() == 1 {
                        name.clone()
                    } else {
                        format!("{name}.{i}")
                    };
                    item.collect_leaves(join(&path, &segment), out);
                }
            }
            Node::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    item.collect_leaves(format!("{path}[{i}]"), out);
                }
            }
            Node::Map(entries) => {
                for (key, value) in entries {
                    value.collect_leaves(format!("{path}{{{}}}", key.inline()), out);
                }
            }
            Node::Atom(_) => unreachable!("atoms are leaves"),
        }
    }
}

/// `parent.child`, or `child` at the root.
pub fn join(path: &str, segment: &str) -> String {
    if path.is_empty() {
        segment.to_owned()
    } else {
        format!("{path}.{segment}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    #[allow(dead_code)]
    struct Inner {
        id: u32,
        payload: Payload,
    }
    #[derive(Debug)]
    #[allow(dead_code)]
    enum Payload {
        Long(i64),
        Unit,
    }
    #[derive(Debug)]
    #[allow(dead_code)]
    struct Packet {
        name: String,
        data: Vec<Inner>,
        bytes: bytes::Bytes,
        pos: (f32, f32),
        maybe: Option<u8>,
        map: std::collections::BTreeMap<String, i32>,
    }

    #[test]
    fn debug_output_round_trips_into_a_tree() {
        let packet = Packet {
            name: "a \"quoted\", {tricky} name".into(),
            data: vec![
                Inner {
                    id: 5,
                    payload: Payload::Long(-8589934591),
                },
                Inner {
                    id: 0,
                    payload: Payload::Unit,
                },
            ],
            bytes: bytes::Bytes::from_static(b"\x00\"ab"),
            pos: (0.5, -1.5e-8),
            maybe: None,
            map: [("k".to_owned(), 1)].into(),
        };
        for text in [format!("{packet:?}"), format!("{packet:#?}")] {
            let node = Node::parse(&text).unwrap();
            assert_eq!(node.inline(), format!("{packet:?}"));
            let Some(Node::List(data)) = node.field("data") else {
                panic!("data is a list")
            };
            assert_eq!(
                data[0].field("payload"),
                Some(&Node::Tuple(
                    "Long".into(),
                    vec![Node::Atom("-8589934591".into())]
                ))
            );
        }
    }

    #[test]
    fn bit_flags_stay_one_atom() {
        let node = Node::parse("Flags(A | B)").unwrap();
        assert_eq!(
            node,
            Node::Tuple("Flags".into(), vec![Node::Atom("A | B".into())])
        );
    }
}
