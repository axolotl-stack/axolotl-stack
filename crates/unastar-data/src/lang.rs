//! English (`en_US`) resource-pack strings, read from the pinned bedrock-samples
//! submodule at build time (nothing derived from it is committed, D13).
//!
//! The server needs them where BDS resolves text itself: command feedback on
//! the server console, and the item/entity names BDS puts into command
//! messages (`commands.kill.successful` lists "Sheep, Chicken").

use std::collections::HashMap;
use std::sync::LazyLock;

/// A parsed `.lang` file.
#[derive(Debug, Default)]
pub struct Lang {
    entries: HashMap<&'static str, &'static str>,
}

static EN_US: LazyLock<Lang> = LazyLock::new(|| {
    Lang::parse(include_str!(
        "../data/vanilla_bp/resource_pack/texts/en_US.lang"
    ))
});

/// The vanilla `en_US.lang`, parsed once.
pub fn en_us() -> &'static Lang {
    &EN_US
}

impl Lang {
    /// `key=value` lines; `##` lines are comments and a value ends at a tab-led `#` comment.
    pub fn parse(text: &'static str) -> Self {
        let entries = text
            .lines()
            .filter(|line| !line.starts_with("##"))
            .filter_map(|line| {
                let (key, value) = line.split_once('=')?;
                let value = value.split_once("\t#").map_or(value, |(v, _)| v);
                Some((key.trim(), value.trim_end()))
            })
            .collect();
        Self { entries }
    }

    pub fn get(&self, key: &str) -> Option<&'static str> {
        self.entries.get(key).copied()
    }

    /// Formats `key` with printf-style parameters the way the client does
    /// (`%s`, `%d`, `%1$s`, `%2$d`, `%.2f`, `%%`). A parameter that starts with
    /// `%` is itself a key and is translated first. Unknown keys are returned
    /// verbatim, which is also what the client shows for them.
    pub fn format(&self, key: &str, params: &[String]) -> String {
        let Some(template) = self.get(key) else {
            return key.to_owned();
        };
        let params: Vec<String> = params
            .iter()
            .map(|param| match param.strip_prefix('%') {
                Some(nested) if self.get(nested).is_some() => self.format(nested, &[]),
                _ => param.clone(),
            })
            .collect();
        let mut out = String::with_capacity(template.len());
        let mut next = 0usize;
        let mut rest = template;
        while let Some(index) = rest.find('%') {
            out.push_str(&rest[..index]);
            rest = &rest[index + 1..];
            if let Some(after) = rest.strip_prefix('%') {
                out.push('%');
                rest = after;
                continue;
            }
            // Optional `n$` position.
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            let (position, spec) = if digits > 0 && rest[digits..].starts_with('$') {
                (rest[..digits].parse::<usize>().ok(), &rest[digits + 1..])
            } else {
                (None, rest)
            };
            // Optional `.precision`, then the conversion character.
            let precision_len = spec.strip_prefix('.').map_or(0, |digits| {
                1 + digits.bytes().take_while(u8::is_ascii_digit).count()
            });
            let Some(conversion) = spec[precision_len..].chars().next() else {
                out.push('%');
                break;
            };
            if !matches!(conversion, 's' | 'd' | 'f' | 'i') {
                out.push('%');
                continue;
            }
            let index = match position {
                Some(position) => position.saturating_sub(1),
                None => {
                    next += 1;
                    next - 1
                }
            };
            let value = params.get(index).map(String::as_str).unwrap_or_default();
            if conversion == 'f' && precision_len > 1 {
                let precision: usize = spec[1..precision_len].parse().unwrap_or(2);
                match value.parse::<f64>() {
                    Ok(number) => out.push_str(&format!("{number:.precision$}")),
                    Err(_) => out.push_str(value),
                }
            } else {
                out.push_str(value);
            }
            rest = &spec[precision_len + conversion.len_utf8()..];
        }
        out.push_str(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn formats_positional_sequential_and_nested_parameters() {
        let lang = en_us();
        assert_eq!(
            lang.format("commands.give.success", &params(&["Wheat", "64", "Steve"])),
            "Gave Wheat * 64 to Steve"
        );
        assert_eq!(
            lang.format(
                "commands.gamemode.success.self",
                &params(&["%createWorldScreen.gameMode.creative"])
            ),
            "Set own game mode to Creative"
        );
        assert_eq!(
            lang.format("commands.time.query.daytime", &params(&["1000"])),
            "Daytime is 1000"
        );
        assert_eq!(
            lang.format("commands.generic.syntax", &params(&["/give ", "x", ""])),
            "Syntax error: Unexpected \"x\": at \"/give >>x<<\""
        );
        assert_eq!(lang.format("no.such.key", &[]), "no.such.key");
    }
}
