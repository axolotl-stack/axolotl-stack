//! Shared helpers for Bedrock JSON inputs.

/// Remove `//` comments from Bedrock JSON while preserving URL-like strings.
pub fn remove_json_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_string = false;
    let mut escape_next = false;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }

        match c {
            '\\' if in_string => {
                result.push(c);
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
                result.push(c);
            }
            '/' if !in_string && chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        result.push('\n');
                        break;
                    }
                }
            }
            _ => result.push(c),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_are_removed_outside_strings_only() {
        let json = r#"{"url":"https://example.test/path"} // comment"#;
        let stripped = remove_json_comments(json);

        assert!(stripped.contains("https://example.test/path"));
        assert!(!stripped.contains(" comment"));
    }
}
