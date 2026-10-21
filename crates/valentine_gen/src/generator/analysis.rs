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
        Type::Union { variants, .. } => {
            for variant in variants {
                collect_deps_recursive(&variant.type_def, ctx, visited, deps);
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
        Type::Bitset { .. } => {}
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
            Primitive::ByteArray => false,
            _ => false,
        },
        Type::Array { .. } => false,
        Type::FixedArray { .. } => false,
        Type::Bitset { .. } => false,
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
        Type::Union { .. } => true,
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

/// Conservative wire-size lower bounds. Unknown or cyclic references contribute
/// zero; unlike Rust's size_of, these bounds also preserve zero-byte schema values.
pub fn minimum_encoded_size(ty: &Type, ctx: &Context) -> usize {
    fn visit(ty: &Type, ctx: &Context, active: &mut HashSet<String>) -> usize {
        match ty {
            Type::Primitive(p) => match p {
                Primitive::Void | Primitive::ByteArray => 0,
                Primitive::U16 | Primitive::U16LE | Primitive::I16 | Primitive::I16LE => 2,
                Primitive::U32
                | Primitive::U32LE
                | Primitive::I32
                | Primitive::I32LE
                | Primitive::F32
                | Primitive::F32LE => 4,
                Primitive::U64
                | Primitive::U64LE
                | Primitive::I64
                | Primitive::I64LE
                | Primitive::F64
                | Primitive::F64LE => 8,
                Primitive::Uuid => 16,
                _ => 1,
            },
            Type::Reference(name) => {
                if !active.insert(name.clone()) {
                    return 0;
                }
                let size = ctx
                    .type_lookup
                    .get(name)
                    .map_or(0, |ty| visit(ty, ctx, active));
                active.remove(name);
                size
            }
            Type::Container(container) => container.fields.iter().fold(0usize, |size, field| {
                size.saturating_add(visit(&field.type_def, ctx, active))
            }),
            Type::String { count_type, .. } | Type::Array { count_type, .. } => {
                visit(count_type, ctx, active)
            }
            Type::Encapsulated { length_type, inner } => {
                visit(length_type, ctx, active).saturating_add(visit(inner, ctx, active))
            }
            Type::FixedArray { size, inner_type } => {
                size.saturating_mul(visit(inner_type, ctx, active))
            }
            Type::Bitset { .. } | Type::Option(_) => 1,
            Type::Switch {
                fields, default, ..
            } => {
                let fallback = visit(default, ctx, active);
                fields
                    .iter()
                    .map(|(_, ty)| visit(ty, ctx, active))
                    .fold(fallback, usize::min)
            }
            Type::Union {
                control_type,
                variants,
            } => visit(&Type::Primitive(control_type.clone()), ctx, active).saturating_add(
                variants
                    .iter()
                    .map(|v| visit(&v.type_def, ctx, active))
                    .min()
                    .unwrap_or(0),
            ),
            Type::Enum { underlying, .. } => {
                visit(&Type::Primitive(underlying.clone()), ctx, active)
            }
            Type::Bitfield { storage_type, .. } => {
                visit(&Type::Primitive(storage_type.clone()), ctx, active)
            }
            Type::Packed { backing, .. } => visit(&Type::Primitive(backing.clone()), ctx, active),
        }
    }
    visit(ty, ctx, &mut HashSet::new())
}

/// Finds cycles through named schema references, not mere syntactic nesting.
pub fn is_recursive_type(name: &str, ty: &Type, ctx: &Context) -> bool {
    fn visit(name: &str, ty: &Type, ctx: &Context, active: &mut HashSet<String>) -> bool {
        match ty {
            Type::Reference(target) => {
                if crate::generator::utils::clean_type_name(target) == name {
                    return true;
                }
                if !active.insert(target.clone()) {
                    return false;
                }
                let recursive = ctx
                    .type_lookup
                    .get(target)
                    .is_some_and(|ty| visit(name, ty, ctx, active));
                active.remove(target);
                recursive
            }
            Type::Container(c) => c
                .fields
                .iter()
                .any(|f| visit(name, &f.type_def, ctx, active)),
            Type::Array { inner_type, .. } | Type::FixedArray { inner_type, .. } => {
                visit(name, inner_type, ctx, active)
            }
            Type::Option(inner) | Type::Encapsulated { inner, .. } => {
                visit(name, inner, ctx, active)
            }
            Type::Switch {
                fields, default, ..
            } => {
                fields.iter().any(|(_, ty)| visit(name, ty, ctx, active))
                    || visit(name, default, ctx, active)
            }
            Type::Union { variants, .. } => variants
                .iter()
                .any(|v| visit(name, &v.type_def, ctx, active)),
            _ => false,
        }
    }
    visit(name, ty, ctx, &mut HashSet::new())
}
