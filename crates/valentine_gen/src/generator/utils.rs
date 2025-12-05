// ----- Helper utilities for naming and type token resolution -----
use crate::generator::definitions::fingerprint_type;
use crate::ir::Type;

pub fn compute_fingerprint(name: &str, t: &Type) -> String {
    format!("{}::{}", name, fingerprint_type(t))
}

pub fn to_screaming_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_is_upper = false;
    let mut last_was_underscore = false;

    for (i, c) in s.chars().enumerate() {
        // 1. Handle Non-Alphanumeric characters (The Fix)
        // This catches spaces, '|', '.', etc.
        if !c.is_alphanumeric() {
            if !last_was_underscore {
                result.push('_');
                last_was_underscore = true;
            }
            prev_is_upper = false;
            continue;
        }

        // 2. Handle CamelCase boundaries (e.g. "HasX" -> "HAS_X")
        if c.is_uppercase() {
            if i > 0 && !prev_is_upper && !last_was_underscore {
                result.push('_');
            }
            result.push(c);
            prev_is_upper = true;
            last_was_underscore = false;
        } else {
            result.push(c.to_ascii_uppercase());
            prev_is_upper = false;
            last_was_underscore = false;
        }
    }

    // Clean up trailing/leading underscores or double underscores
    let cleaned = result.replace("__", "_");
    cleaned.trim_matches('_').to_string()
}

pub fn camel_case(s: &str) -> String {
    // Split on non-alphanumeric and case transitions; preserve common acronyms.
    fn split_segment(seg: &str) -> Vec<String> {
        if seg.is_empty() {
            return vec![];
        }
        let mut parts: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut chars = seg.chars().peekable();
        let mut prev = '\0';
        while let Some(ch) = chars.next() {
            let is_boundary = if cur.is_empty() {
                false
            } else if ch.is_ascii_digit() && !prev.is_ascii_digit()
                || !ch.is_ascii_digit() && prev.is_ascii_digit()
                || ch.is_ascii_uppercase() && prev.is_ascii_lowercase()
            {
                true
            } else {
                // Handle transitions like "XMLHttp" -> XML + Http (keep acronym run)
                // If prev and ch are uppercase but next is lowercase, split before current
                if prev.is_ascii_uppercase() && ch.is_ascii_uppercase() {
                    if let Some(nxt) = chars.peek() {
                        nxt.is_ascii_lowercase()
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if is_boundary {
                parts.push(cur.clone());
                cur.clear();
            }
            cur.push(ch);
            prev = ch;
        }
        if !cur.is_empty() {
            parts.push(cur);
        }
        parts
    }

    // 1) Split by non-alphanumeric
    let mut raw_segs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch)
        } else if !cur.is_empty() {
            raw_segs.push(cur.clone());
            cur.clear();
        }
    }
    if !cur.is_empty() {
        raw_segs.push(cur);
    }

    // 2) Split each segment by case/digit boundaries
    let mut tokens: Vec<String> = Vec::new();
    for seg in raw_segs {
        tokens.extend(split_segment(&seg));
    }

    // 3) Normalize tokens and build CamelCase
    let acronyms = ["id", "uri", "uuid", "nbt", "url", "rgb", "rgba"];
    let mut out = String::new();
    for tok in tokens {
        let lower = tok.to_ascii_lowercase();
        if acronyms.contains(&lower.as_str()) {
            out.push_str(&lower.to_ascii_uppercase());
        } else {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    if out
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
    {
        out
    } else {
        format!("T{}", out)
    }
}

pub fn safe_camel_ident(s: &str) -> String {
    let mut ident = camel_case(s);
    // Avoid leading digits just in case camel_case kept them
    if ident
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        ident = format!("V{}", ident);
    }
    ident
}

pub fn clean_type_name(s: &str) -> String {
    // Build a valid Rust type identifier using CamelCase and stripping invalid chars
    let base = camel_case(s);
    base.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub fn clean_field_name(s: &str, _container: &str) -> String {
    // Convert to snake_case-ish identifier safe for Rust fields
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            let c = ch.to_ascii_lowercase();
            out.push(c);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }
    if out.is_empty() {
        out.push_str("field");
    }
    // Trim leading/trailing underscores
    while out.starts_with('_') {
        out.remove(0);
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("field");
    }
    // Avoid starting with digit
    if out
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        out = format!("f_{}", out);
    }
    // Avoid Rust keywords by suffixing underscore
    match out.as_str() {
        "type" | "match" | "ref" | "box" | "self" | "super" | "crate" | "mod" | "fn" | "let"
        | "enum" | "struct" | "trait" | "impl" | "use" | "as" | "in" | "where" | "for" | "loop"
        | "while" | "if" | "else" | "continue" | "break" | "move" | "return" | "unsafe" | "pub"
        | "async" | "await" | "dyn" | "static" | "const" | "mut" | "extern" | "false" | "true" => {
            out.push('_');
            out
        }
        _ => out,
    }
}

// Generate unique names from a list of base names, preserving order.
// First occurrence keeps its name; duplicates get a _2, _3, ... suffix.
pub fn make_unique_names(bases: &[String]) -> Vec<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<String> = Vec::with_capacity(bases.len());
    for b in bases {
        let c = counts.entry(b.clone()).and_modify(|v| *v += 1).or_insert(1);
        if *c == 1 {
            out.push(b.clone());
        } else {
            out.push(format!("{}_{}", b, *c));
        }
    }
    out
}

pub fn get_group_name(type_name: &str) -> String {
    // Heuristic grouping based on type name
    let lower = type_name.to_lowercase();

    if lower.contains("login")
        || lower.contains("handshake")
        || lower.contains("disconnect")
        || lower.contains("encryption")
        || lower.contains("playstatus")
        || lower.contains("clienttoserver")
        || lower.contains("servertoclient")
        || lower.contains("setlocalplayer")
    {
        return "connection".to_string();
    }

    if lower.contains("resource") || lower.contains("texture") || lower.contains("skin") {
        return "resource".to_string();
    }

    if lower.contains("level")
        || lower.contains("chunk")
        || lower.contains("block")
        || lower.contains("biome")
        || lower.contains("structure")
        || lower.contains("map")
        || lower.contains("dimension")
        || lower.contains("tick")
        || lower.contains("piston")
    {
        return "world".to_string();
    }

    if lower.contains("entity")
        || lower.contains("player")
        || lower.contains("actor")
        || lower.contains("move")
        || lower.contains("animate")
        || lower.contains("attribute")
        || lower.contains("effect")
        || lower.contains("mob")
        || lower.contains("camera")
        || lower.contains("npc")
        || lower.contains("agent")
        || lower.contains("motion")
    {
        return "entity".to_string();
    }

    if lower.contains("item")
        || lower.contains("inventory")
        || lower.contains("window")
        || lower.contains("craft")
        || lower.contains("trade")
        || lower.contains("book")
        || lower.contains("enchant")
        || lower.contains("hotbar")
        || lower.contains("container")
    {
        return "inventory".to_string();
    }

    if lower.contains("score") || lower.contains("objective") || lower.contains("display") {
        return "score".to_string();
    }

    if lower.contains("command") || lower.contains("settings") || lower.contains("game") {
        // "Game" catches StartGame, GameRules
        return "game".to_string();
    }

    if lower.contains("text")
        || lower.contains("chat")
        || lower.contains("message")
        || lower.contains("title")
        || lower.contains("toast")
    {
        return "chat".to_string();
    }

    if lower.contains("packet") || lower.contains("network") || lower.contains("transfer") {
        // Catch-all for packets not matched above
        return "packet".to_string();
    }

    // Fallback for common types that aren't packets
    "common".to_string()
}

// Append a `_ctx` suffix for dependency/context parameter names
pub fn ctx_param_name(n: &str) -> String {
    format!("{}_ctx", n)
}
