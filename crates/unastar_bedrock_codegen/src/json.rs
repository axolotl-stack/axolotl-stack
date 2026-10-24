//! JSON-with-comments decoding for BDS pack documents.
//!
//! BDS pack JSON (loose files and `.brarchive` payloads) may contain `//` line
//! comments and `/* */` block comments, which `serde_json` rejects. Comments
//! are removed outside string literals; byte offsets of the remaining text are
//! not preserved, line numbers are (newlines are kept).

use crate::error::{Error, Result};
use serde_json::Value;

/// Remove `//` and `/* */` comments outside string literals.
pub fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut segment_start = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            match b {
                b'\\' => i += 2,
                b'"' => {
                    in_string = false;
                    i += 1;
                }
                _ => i += 1,
            }
            continue;
        }
        match (b, bytes.get(i + 1)) {
            (b'"', _) => {
                in_string = true;
                i += 1;
            }
            (b'/', Some(b'/')) => {
                out.push_str(&text[segment_start..i]);
                i = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                segment_start = i;
            }
            (b'/', Some(b'*')) => {
                out.push_str(&text[segment_start..i]);
                let end = text[i + 2..].find("*/").map_or(bytes.len(), |n| i + 2 + n + 2);
                // Keep line structure so serde_json error positions stay useful.
                out.extend(text[i..end].chars().filter(|&c| c == '\n'));
                i = end;
                segment_start = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&text[segment_start.min(bytes.len())..]);
    out
}

/// Parse a JSON-with-comments document; `origin` names it in errors.
pub fn parse(origin: &str, bytes: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::data(origin, format!("not UTF-8: {e}")))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    serde_json::from_str(&strip_comments(text)).map_err(|source| Error::Json {
        origin: origin.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_line_and_block_comments_outside_strings() {
        let doc = "{\n // lead\n \"a\": \"x // not a comment\", /* block\n */ \"b\": \"/* kept */\" // tail\n}";
        let v = parse("t", doc.as_bytes()).unwrap();
        assert_eq!(v["a"], "x // not a comment");
        assert_eq!(v["b"], "/* kept */");
    }

    #[test]
    fn escaped_quotes_do_not_end_strings() {
        let v = parse("t", br#"{"a": "q\" // still string", "b": 1} // c"#).unwrap();
        assert_eq!(v["a"], "q\" // still string");
        assert_eq!(v["b"], 1);
    }

    #[test]
    fn errors_name_the_origin() {
        let err = parse("pack/x.json", b"{ nope }").unwrap_err().to_string();
        assert!(err.starts_with("pack/x.json"), "{err}");
    }

    #[test]
    fn tolerates_byte_order_mark() {
        assert_eq!(parse("t", "\u{feff}{\"a\":2}".as_bytes()).unwrap()["a"], 2);
    }
}
