use convert_case::{Case, Casing};

use crate::ir::{Container, Type};

fn sanitize_identifier(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

pub fn camel_case(s: &str) -> String {
    s.to_case(Case::Pascal)
}

pub fn to_screaming_snake_case(s: &str) -> String {
    s.to_case(Case::UpperSnake)
}

pub fn safe_camel_ident(s: &str) -> String {
    // 1. Handle Global Variables (e.g. "/ShieldItemID" -> "ShieldItemID")
    let s = if s.starts_with('/') { &s[1..] } else { s };

    // 2. Sanitize (Remove junk)
    let sanitized = sanitize_identifier(s);
    let res = camel_case(&sanitized);

    // 3. Handle Numeric Start (e.g. "1_18" -> "T118")
    if res.chars().next().map_or(false, |c| c.is_numeric()) {
        format!("T{}", res)
    } else if res.is_empty() {
        "Unknown".to_string()
    } else {
        res
    }
}

pub fn clean_field_name(name: &str, _context: &str) -> String {
    let sanitized = sanitize_identifier(name);
    let s = sanitized.to_case(Case::Snake);

    // Handle empty result (e.g. input was just "||")
    let s = if s.is_empty() {
        "unknown_field".to_string()
    } else {
        s
    };

    // Handle Numeric Start
    let s = if s.chars().next().map_or(false, |c| c.is_numeric()) {
        format!("_{}", s)
    } else {
        s
    };

    match s.as_str() {
        "type" => "type_".to_string(),
        "box" => "box_".to_string(),
        "use" => "use_".to_string(),
        "ref" => "ref_".to_string(),
        "self" => "self_".to_string(),
        "enum" => "enum_".to_string(),
        "struct" => "struct_".to_string(),
        "match" => "match_".to_string(),
        "impl" => "impl_".to_string(),
        "trait" => "trait_".to_string(),
        "fn" => "fn_".to_string(),
        "pub" => "pub_".to_string(),
        "where" => "where_".to_string(),
        "return" => "return_".to_string(),
        "crate" => "crate_".to_string(),
        "super" => "super_".to_string(),
        "loop" => "loop_".to_string(),
        "while" => "while_".to_string(),
        "for" => "for_".to_string(),
        "in" => "in_".to_string(),
        "if" => "if_".to_string(),
        "else" => "else_".to_string(),
        "break" => "break_".to_string(),
        "continue" => "continue_".to_string(),
        "const" => "const_".to_string(),
        "static" => "static_".to_string(),
        "extern" => "extern_".to_string(),
        "unsafe" => "unsafe_".to_string(),
        "move" => "move_".to_string(),
        "mut" => "mut_".to_string(),
        "abstract" => "abstract_".to_string(),
        "async" => "async_".to_string(),
        "await" => "await_".to_string(),
        "dyn" => "dyn_".to_string(),
        "virtual" => "virtual_".to_string(),
        _ => s,
    }
}

pub fn clean_type_name(name: &str) -> String {
    let sanitized = sanitize_identifier(name);
    let s = sanitized.to_case(Case::Pascal);

    if s.chars().next().map_or(false, |c| c.is_numeric()) {
        format!("T{}", s)
    } else if s.is_empty() {
        "UnknownType".to_string()
    } else {
        s
    }
}

pub fn compute_fingerprint(
    name: &str,
    t: &crate::ir::Type,
    ctx: &crate::generator::context::Context,
) -> String {
    use crate::generator::definitions::fingerprint_type;
    // Include the name AND the module path (version) to differentiate identically structured types across versions
    format!(
        "{}::{}::{}",
        ctx.current_module_path,
        name,
        fingerprint_type(t)
    )
}

pub fn make_unique_names(base_names: &[String]) -> Vec<String> {
    let mut counts = std::collections::HashMap::new();
    let mut result = Vec::new();

    for name in base_names {
        let count = counts.entry(name.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            result.push(name.clone());
        } else {
            result.push(format!("{}_{}", name, count));
        }
    }
    result
}

fn first_token_from_pascal(name: &str) -> String {
    let snake = name.to_case(Case::Snake);
    snake
        .split('_')
        .find(|s| !s.is_empty())
        .unwrap_or("misc")
        .to_string()
}

/// Returns a hierarchical group path like "packets/chat" or "types/entities"
pub fn get_group_name(struct_name: &str) -> String {
    if struct_name.starts_with("Packet") {
        let base = struct_name.trim_start_matches("Packet");
        let token = first_token_from_pascal(base);
        return format!("packets/{}", token);
    }
    let token = first_token_from_pascal(struct_name);
    format!("types/{}", token)
}

pub fn derive_field_names(container: &Container, struct_name: &str) -> Vec<String> {
    let mut content_count = 0;

    let base_names: Vec<String> = container
        .fields
        .iter()
        .map(|f| {
            let original = clean_field_name(&f.name, struct_name);

            // HEURISTIC: If the name looks like "action_id" but it's a switch *on* action_id,
            // it is likely the anonymous content field.
            // OR if the parser explicitly named it "anon" or "content".

            let is_switch_content = if let Type::Switch { compare_to, .. } = &f.type_def {
                // If the field name matches the compare_to variable, it's likely a collision
                // caused by the parser naming the switch after its target.
                let target = clean_field_name(&compare_to.replace("../", ""), "");
                original == target || original == "content" || original == "anon"
            } else {
                original == "content" || original == "anon"
            };

            if is_switch_content {
                content_count += 1;
                if content_count == 1 {
                    return "content".to_string();
                }
                if content_count == 2 {
                    return "extra".to_string();
                }
            }
            original
        })
        .collect();

    make_unique_names(&base_names)
}
