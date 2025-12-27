use crate::generator::context::Context;
use crate::generator::utils::clean_field_name;
use crate::ir::{Container, Primitive, Type};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dependency {
    LocalField(String),
    Global(String),
}

impl Dependency {
    pub fn name(&self) -> &str {
        match self {
            Dependency::LocalField(n) => n,
            Dependency::Global(n) => n,
        }
    }
}

pub type DepMap = HashSet<(Dependency, Type)>;

pub fn get_deps(t: &Type, ctx: &Context) -> DepMap {
    let mut deps = DepMap::new();
    let mut visited = HashSet::new();
    collect_deps_recursive(t, ctx, &mut visited, &mut deps);
    deps
}

fn collect_deps_recursive(
    t: &Type,
    ctx: &Context,
    visited: &mut HashSet<String>,
    deps: &mut DepMap,
) {
    // Helper to clean dependency names consistent with codec.rs
    let clean_dep_name = |p: &str| -> String {
        let inner = if let Some(stripped) = p.strip_prefix('/') {
            stripped
        } else {
            p
        };
        clean_field_name(inner.replace("../", "").as_str(), "")
    };

    match t {
        Type::Container(c) => {
            for field in &c.fields {
                let mut child_deps = DepMap::new();
                collect_deps_recursive(&field.type_def, ctx, visited, &mut child_deps);

                for (dep, dep_type) in child_deps {
                    match &dep {
                        Dependency::LocalField(name) => {
                            let is_satisfied_locally = c.fields.iter().any(|f| {
                                let clean = crate::generator::utils::clean_field_name(&f.name, "");
                                clean == *name || f.name == *name
                            });

                            if !is_satisfied_locally {
                                deps.insert((dep.clone(), dep_type));
                            }
                        }
                        Dependency::Global(_) => {
                            deps.insert((dep.clone(), dep_type));
                        }
                    }
                }
            }
        }
        Type::Switch {
            compare_to,
            fields,
            default,
        } => {
            let dependency_type = Type::Primitive(Primitive::VarInt);
            let parts: Vec<&str> = compare_to
                .split(|c| c == '|' || c == '&' || c == ' ' || c == '(' || c == ')')
                .filter(|s| !s.is_empty() && *s != "||" && *s != "&&")
                .collect();

            for part in parts {
                // Handle property access like "flags.has_rot_x" -> depends on "flags"
                // Or "../flags.has_rot_x" -> depends on "flags" (after cleanup)

                let clean_part_str = part.replace("../", "");
                let base_name_str = clean_part_str.split('.').next().unwrap_or(&clean_part_str);

                if part.starts_with('/') {
                    let name = clean_dep_name(base_name_str);
                    deps.insert((Dependency::Global(name), dependency_type.clone()));
                } else {
                    let name = clean_dep_name(base_name_str);
                    deps.insert((Dependency::LocalField(name), dependency_type.clone()));
                }
            }

            // 2. Analyze Keys for Global Dependencies (Existing logic)
            for (k, t) in fields {
                if k.starts_with('/') {
                    let name = clean_dep_name(k);
                    deps.insert((Dependency::Global(name), t.clone()));
                }
            }

            // 3. Recurse into Values (Existing logic)
            for (_, type_def) in fields {
                collect_deps_recursive(type_def, ctx, visited, deps);
            }

            // 4. Recurse into Default (Existing logic)
            if !matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                collect_deps_recursive(default, ctx, visited, deps);
            }
        }
        Type::Reference(r) => {
            if !visited.contains(r) {
                visited.insert(r.clone());
                if let Some(resolved) = ctx.type_lookup.get(r) {
                    collect_deps_recursive(resolved, ctx, visited, deps);
                }
                visited.remove(r);
            }
        }
        Type::Array {
            inner_type,
            count_type,
        } => {
            collect_deps_recursive(inner_type, ctx, visited, deps);
            // Also analyze count_type if it happens to be complex
            if let Type::Container(_) = count_type.as_ref() {
                let mut tmp = DepMap::new();
                collect_deps_recursive(count_type, ctx, visited, &mut tmp);
                deps.extend(tmp);
            }
        }
        Type::FixedArray { inner_type, .. } => {
            collect_deps_recursive(inner_type, ctx, visited, deps);
        }
        Type::String { count_type, .. } => {
            if let Type::Container(_) = count_type.as_ref() {
                let mut tmp = DepMap::new();
                collect_deps_recursive(count_type, ctx, visited, &mut tmp);
                deps.extend(tmp);
            }
        }
        Type::Encapsulated { length_type, inner } => {
            collect_deps_recursive(inner, ctx, visited, deps);
            if let Type::Container(_) = length_type.as_ref() {
                let mut tmp = DepMap::new();
                collect_deps_recursive(length_type, ctx, visited, &mut tmp);
                deps.extend(tmp);
            }
        }
        Type::Option(inner) => {
            collect_deps_recursive(inner, ctx, visited, deps);
        }
        _ => {}
    }
}
pub fn should_box_variant(t: &Type, ctx: &Context, depth: usize) -> bool {
    const MAX_DEPTH: usize = 8;
    if depth > MAX_DEPTH {
        return true;
    }
    match t {
        Type::Primitive(p) => match p {
            Primitive::McString | Primitive::ByteArray => false,
            _ => false,
        },
        Type::Array { .. } => false,
        Type::FixedArray { .. } => false,
        Type::String { .. } => false,
        Type::Encapsulated { .. } => false,
        Type::Reference(r) => {
            if let Some(inner) = ctx.type_lookup.get(r) {
                should_box_variant(inner, ctx, depth + 1)
            } else {
                true
            }
        }
        Type::Container(c) => {
            if c.fields.is_empty() {
                return false;
            }
            if c.fields.len() > 3 {
                return true;
            }
            for f in &c.fields {
                if should_box_variant(&f.type_def, ctx, depth + 1) {
                    return true;
                }
            }
            false
        }
        Type::Option(inner) => should_box_variant(inner, ctx, depth + 1),
        Type::Switch { .. } => true,
        _ => false,
    }
}

pub fn find_redundant_fields(container: &Container) -> HashSet<String> {
    let mut redundant = HashSet::new();
    for field in &container.fields {
        if let Type::Switch {
            compare_to,
            fields,
            default,
        } = &field.type_def
        {
            // Check if keys are boolean
            let keys_are_bool = fields.iter().all(|(k, _)| {
                let k_lower = k.to_lowercase();
                k_lower == "true" || k_lower == "false" || k_lower == "1" || k_lower == "0"
            });

            if keys_are_bool {
                // CRITICAL FIX: For "inverse option" switches (all field cases are void,
                // but default has data), the discriminator contains MEANINGFUL data, not
                // just a boolean flag. For example, ItemLegacy's network_id is 0 for air
                // but contains the actual item runtime ID otherwise. Don't mark as redundant.
                let is_inverse_option = fields
                    .iter()
                    .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)))
                    && !matches!(default.as_ref(), Type::Primitive(Primitive::Void));

                if is_inverse_option {
                    // The discriminator IS the data, NOT a derivable boolean flag
                    continue;
                }

                // For normal bool-keyed switches, the discriminator can be derived from the switch outcome:
                // - If one branch is Void and one has Data -> Option<T>, discriminator = is_some()
                // - If both branches have Data -> Discriminated enum, discriminator = matches!(variant)
                // Either way, the discriminator field is redundant and can be removed from the struct.
                let target = compare_to.replace("../", "");
                let clean_target = crate::generator::utils::clean_field_name(&target, "");
                redundant.insert(clean_target);
            }
        }
    }
    redundant
}
