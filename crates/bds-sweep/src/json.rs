//! The JSON layout of the movement harness's fixtures: the outer `depth`
//! levels of objects and arrays one entry a line, everything deeper as
//! compact JSON on its entry's line. With a trace's ticks as the last level
//! laid out, one tick is one line: the fixtures stay readable and diffable at
//! about a third of their pretty-printed size.

use std::io;

use serde::Serialize;
use serde_json::ser::{Formatter, Serializer};

/// `value` laid out to `depth` levels, with a final newline.
pub fn to_fixture_string(value: &impl Serialize, depth: usize) -> serde_json::Result<String> {
    let mut out = Vec::new();
    let mut serializer = Serializer::with_formatter(&mut out, Layout::new(depth));
    value.serialize(&mut serializer)?;
    out.push(b'\n');
    String::from_utf8(out).map_err(|e| serde_json::Error::io(io::Error::other(e)))
}

/// A formatter that lays out the outer levels and compacts the rest.
struct Layout {
    depth: usize,
    level: usize,
    has_entries: Vec<bool>,
}

impl Layout {
    fn new(depth: usize) -> Self {
        Self {
            depth,
            level: 0,
            has_entries: Vec::new(),
        }
    }

    fn laid_out(&self) -> bool {
        self.level <= self.depth
    }

    fn begin<W: ?Sized + io::Write>(&mut self, writer: &mut W, open: &[u8]) -> io::Result<()> {
        self.level += 1;
        self.has_entries.push(false);
        writer.write_all(open)
    }

    fn end<W: ?Sized + io::Write>(&mut self, writer: &mut W, close: &[u8]) -> io::Result<()> {
        let had_entries = self.has_entries.pop().unwrap_or(false);
        if self.laid_out() && had_entries {
            writer.write_all(b"\n")?;
            self.indent(writer, self.level - 1)?;
        }
        self.level -= 1;
        writer.write_all(close)
    }

    fn entry<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if let Some(last) = self.has_entries.last_mut() {
            *last = true;
        }
        if !first {
            writer.write_all(b",")?;
        }
        if self.laid_out() {
            writer.write_all(b"\n")?;
            self.indent(writer, self.level)?;
        }
        Ok(())
    }

    fn indent<W: ?Sized + io::Write>(&self, writer: &mut W, level: usize) -> io::Result<()> {
        for _ in 0..level {
            writer.write_all(b"  ")?;
        }
        Ok(())
    }
}

impl Formatter for Layout {
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.begin(writer, b"[")
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.end(writer, b"]")
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.entry(writer, first)
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.begin(writer, b"{")
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.end(writer, b"}")
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.entry(writer, first)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(if self.laid_out() { b": " } else { b":" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn outer_levels_are_laid_out_and_inner_ones_compact() {
        let value = json!({"a": {"ticks": [{"t": 1, "v": [1, 2]}, {"t": 2}]}, "b": [], "c": {}});
        let text = to_fixture_string(&value, 3).unwrap();
        assert_eq!(
            text,
            "{\n  \"a\": {\n    \"ticks\": [\n      {\"t\":1,\"v\":[1,2]},\n      {\"t\":2}\n    ]\n  },\n  \"b\": [],\n  \"c\": {}\n}\n"
        );
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), value);
    }
}
