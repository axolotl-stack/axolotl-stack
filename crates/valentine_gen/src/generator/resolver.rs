use crate::generator::analysis::{find_redundant_fields, get_deps};
use crate::generator::context::Context;
use crate::generator::utils::{
    clean_field_name, clean_type_name, derive_field_names, safe_camel_ident,
};
use crate::ir::{Container, Type};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct ResolvedContainer {
    /// The original definition
    pub raw: Container,

    /// Arguments this container requires (Strongly Typed!)
    /// e.g. [("_enum_type", Type::Reference("PacketAvailableCommandsEnums"))]
    pub args: Vec<(String, Type)>,

    /// A lookup table for the type of every field/local variable in this container.
    /// Used by decode logic to know if "x" is an Enum, Int, or Bool.
    pub variable_types: HashMap<String, Type>,

    /// Specific metadata for Switches.
    /// Maps "field_name" -> "The Enum Type it switches on"
    pub switch_resolutions: HashMap<String, Type>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PacketSignature {
    pub fields: Vec<(String, String)>,
    pub args: Vec<(String, String)>,
}

impl ResolvedContainer {
    pub fn analyze(container: &Container, ctx: &Context) -> Self {
        let mut variable_types = HashMap::new();
        let mut switch_resolutions = HashMap::new();
        // Tracks variables whose type was upgraded due to switch discriminator inference.
        // Keyed by the dependency/arg name as produced by `get_deps` (already cleaned).
        let mut discriminator_upgrades: HashMap<String, Type> = HashMap::new();

        let unique_names = derive_field_names(container, &container.name);

        fn record_switch(
            switch_name: Option<&str>,
            compare_to: &str,
            fields: &[(String, Type)],
            ctx: &Context,
            variable_types: &mut HashMap<String, Type>,
            switch_resolutions: &mut HashMap<String, Type>,
            discriminator_upgrades: &mut HashMap<String, Type>,
        ) {
            let target_var = clean_path(compare_to);
            // 1) Try to infer from cases (provide discriminator hint)
            let inferred = infer_enum_from_cases(fields, ctx, Some(&target_var));

            // Consider common sanitized variants of the discriminator name.
            let mut target_keys = Vec::new();
            target_keys.push(target_var.clone());
            if let Some(stripped) = target_var.strip_prefix('_') {
                target_keys.push(stripped.to_string());
            } else {
                target_keys.push(format!("_{}", target_var));
            }

            // 2) Prefer the existing declared type of the discriminator if it is an enum/reference-to-enum
            let preferred = target_keys.iter().find_map(|k| {
                variable_types.get(k).and_then(|t| match t {
                    Type::Reference(r) => ctx.type_lookup.get(r).and_then(|inner| {
                        if matches!(inner, Type::Enum { .. }) {
                            Some(Type::Reference(r.clone()))
                        } else {
                            None
                        }
                    }),
                    Type::Enum { .. } => Some(t.clone()),
                    _ => None,
                })
            });

            let enum_type = preferred.or(inferred);

            if let Some(enum_type) = enum_type {
                if let Some(name) = switch_name {
                    switch_resolutions.insert(name.to_string(), enum_type.clone());
                }

                variable_types
                    .entry(target_var.clone())
                    .or_insert(enum_type.clone());

                // Back-propagate to dependency names (arg names).
                let mut upgrade_keys = target_keys.clone();
                if !upgrade_keys.contains(&target_var) {
                    upgrade_keys.push(target_var.clone());
                }
                let raw_base = compare_to
                    .trim_start_matches('/')
                    .trim_start_matches("../")
                    .split('.')
                    .next()
                    .unwrap_or(compare_to)
                    .to_string();
                upgrade_keys.push(clean_field_name(&raw_base, ""));

                for key in upgrade_keys {
                    discriminator_upgrades
                        .entry(key)
                        .or_insert(enum_type.clone());
                }
            }
        }

        // Recursive traversal to catch nested switches (arrays/options/containers).
        fn visit_type(
            switch_name: Option<&str>,
            ty: &Type,
            ctx: &Context,
            variable_types: &mut HashMap<String, Type>,
            switch_resolutions: &mut HashMap<String, Type>,
            discriminator_upgrades: &mut HashMap<String, Type>,
        ) {
            match ty {
                Type::Switch {
                    compare_to,
                    fields,
                    default,
                } => {
                    record_switch(
                        switch_name,
                        compare_to,
                        fields,
                        ctx,
                        variable_types,
                        switch_resolutions,
                        discriminator_upgrades,
                    );
                    for (_, t) in fields {
                        visit_type(
                            None,
                            t,
                            ctx,
                            variable_types,
                            switch_resolutions,
                            discriminator_upgrades,
                        );
                    }
                    if !matches!(
                        default.as_ref(),
                        Type::Primitive(crate::ir::Primitive::Void)
                    ) {
                        visit_type(
                            None,
                            default,
                            ctx,
                            variable_types,
                            switch_resolutions,
                            discriminator_upgrades,
                        );
                    }
                }
                Type::Array { inner_type, .. } => visit_type(
                    None,
                    inner_type,
                    ctx,
                    variable_types,
                    switch_resolutions,
                    discriminator_upgrades,
                ),
                Type::Option(inner) => visit_type(
                    None,
                    inner,
                    ctx,
                    variable_types,
                    switch_resolutions,
                    discriminator_upgrades,
                ),
                Type::Container(c) => {
                    let inner_names = derive_field_names(c, &c.name);
                    for (idx, f) in c.fields.iter().enumerate() {
                        let fname = inner_names
                            .get(idx)
                            .cloned()
                            .unwrap_or_else(|| clean_field_name(&f.name, &c.name));
                        visit_type(
                            Some(&fname),
                            &f.type_def,
                            ctx,
                            variable_types,
                            switch_resolutions,
                            discriminator_upgrades,
                        );
                    }
                }
                _ => {}
            }
        }

        // 1. Register all local fields and scan their types
        for (idx, field) in container.fields.iter().enumerate() {
            let clean_name = clean_field_name(&field.name, &container.name);
            let var_name = unique_names
                .get(idx)
                .cloned()
                .unwrap_or_else(|| clean_name.clone());

            variable_types.insert(var_name.clone(), field.type_def.clone());
            variable_types
                .entry(clean_name.clone())
                .or_insert(field.type_def.clone());

            // Scan this field (and nested types) for switches
            visit_type(
                Some(&var_name),
                &field.type_def,
                ctx,
                &mut variable_types,
                &mut switch_resolutions,
                &mut discriminator_upgrades,
            );
        }

        // 2. Calculate Dependencies (Arguments)
        // IMPORTANT: get_deps returns (Dependency, Type) where the Type may be a primitive
        // placeholder (e.g., VarInt). We must upgrade these to the strongest known type:
        //  - use discriminator_upgrades (from switch analysis)
        //  - fall back to variable_types (already upgraded via switches/args)
        //  - only if both are missing, keep the raw dep type
        // This ensures args (e.g., `type_`) become ScoreEntriesType instead of i32.
        let raw_deps = get_deps(&Type::Container(container.clone()), ctx);
        let mut args = Vec::new();

        // Helper to produce the set of lookup keys for a dep/arg name (raw, stripped, underscore)
        let make_keys = |name: &str| -> Vec<String> {
            let mut v = Vec::new();
            v.push(name.to_string());
            if let Some(stripped) = name.strip_prefix('_') {
                v.push(stripped.to_string());
            } else {
                v.push(format!("_{}", name));
            }
            v
        };

        // Helper to upgrade primitive discriminators to strong types.
        let upgrade_discriminator = |name: &str, ty: &mut Type| {
            if matches!(
                ty,
                Type::Primitive(
                    crate::ir::Primitive::VarInt
                        | crate::ir::Primitive::ZigZag32
                        | crate::ir::Primitive::VarLong
                )
            ) && name.contains("enum_type")
            {
                *ty = Type::Reference("enum_size_based_on_values_len".to_string());
            }

            if matches!(
                ty,
                Type::Primitive(
                    crate::ir::Primitive::VarInt
                        | crate::ir::Primitive::ZigZag32
                        | crate::ir::Primitive::VarLong
                )
            ) && name.contains("type")
            {
                let mut prefix = container.name.clone();
                for suf in ["EntriesItem", "Entries", "Entry", "Item", "Content"] {
                    if let Some(stripped) = prefix.strip_suffix(suf) {
                        prefix = stripped.to_string();
                        break;
                    }
                }
                let candidate = format!("{}Type", prefix);
                // Find the best enum name: prefer prefix match, then suffix match.
                let mut best_name: Option<String> = None;
                let mut best_score: usize = 0;
                for (ename, ety) in &ctx.type_lookup {
                    if !matches!(ety, Type::Enum { .. }) {
                        continue;
                    }
                    let base = ename.trim_end_matches("Type");
                    let mut score = 0;
                    if !base.is_empty() && container.name.starts_with(base) {
                        score = base.len() * 2; // weight prefix higher
                    } else if ename.ends_with(&candidate) {
                        score = candidate.len();
                    }
                    if score > best_score {
                        best_score = score;
                        best_name = Some(ename.clone());
                    }
                }
                if best_score > 0 {
                    let chosen = best_name.unwrap_or(candidate);
                    let clean = crate::generator::utils::clean_type_name(&chosen);
                    *ty = Type::Reference(clean);
                }
            }

            if matches!(
                ty,
                Type::Primitive(
                    crate::ir::Primitive::VarInt
                        | crate::ir::Primitive::ZigZag32
                        | crate::ir::Primitive::VarLong
                )
            ) && (name == "network_ids" || name == "_network_ids")
            {
                *ty = Type::Primitive(crate::ir::Primitive::Bool);
            }
        };

        for (dep, default_type) in raw_deps {
            let name = dep.name().to_string();

            // The golden lookup:
            // Did our switch analysis above upgrade this argument's type?
            let keys = make_keys(&name);
            let mut final_type = keys
                .iter()
                .find_map(|k| discriminator_upgrades.get(k).cloned())
                .or_else(|| keys.iter().find_map(|k| variable_types.get(k).cloned()))
                .unwrap_or(default_type);

            upgrade_discriminator(&name, &mut final_type);

            // Final safety net: if still primitive and looks like a discriminator, try best enum by prefix.
            if matches!(
                final_type,
                Type::Primitive(
                    crate::ir::Primitive::VarInt
                        | crate::ir::Primitive::ZigZag32
                        | crate::ir::Primitive::VarLong
                )
            ) && name.contains("type")
            {
                let mut best_name: Option<String> = None;
                let mut best_score: usize = 0;
                for (ename, ety) in &ctx.type_lookup {
                    if !matches!(ety, Type::Enum { .. }) {
                        continue;
                    }
                    let base = ename.trim_end_matches("Type");
                    let mut score = 0;
                    if !base.is_empty() && container.name.starts_with(base) {
                        score = base.len() * 2;
                    } else if name.starts_with(base) {
                        score = base.len();
                    }
                    if score > best_score {
                        best_score = score;
                        best_name = Some(ename.clone());
                    }
                }
                if let Some(b) = best_name {
                    final_type = Type::Reference(crate::generator::utils::clean_type_name(&b));
                }
            }

            args.push((name.clone(), final_type.clone()));

            // Keep variable_types consistent with upgraded args
            variable_types.entry(name).or_insert(final_type);
        }

        // Sort args for deterministic output
        args.sort_by(|a, b| a.0.cmp(&b.0));

        // Make argument types available to downstream resolution
        for (name, ty) in &args {
            variable_types.entry(name.clone()).or_insert(ty.clone());
        }

        Self {
            raw: container.clone(),
            args,
            variable_types,
            switch_resolutions,
        }
    }
}

fn normalize_case_name(name: &str) -> String {
    // Basic normalization: camel + trim trailing 's' to tolerate pluralized labels.
    let camel = safe_camel_ident(name);
    if camel.ends_with('s') && camel.len() > 1 {
        camel[..camel.len() - 1].to_string()
    } else {
        camel
    }
}

fn infer_enum_from_cases(
    cases: &[(String, Type)],
    ctx: &Context,
    discriminator_hint: Option<&str>,
) -> Option<Type> {
    let case_names: Vec<String> = cases
        .iter()
        .map(|(n, _)| normalize_case_name(n))
        .filter(|n| !n.is_empty() && n != "_" && !n.eq_ignore_ascii_case("default"))
        .collect();

    // Collect numeric cases separately for the "numeric-only" scenario.
    let numeric_cases: Vec<i64> = cases
        .iter()
        .filter_map(|(n, _)| n.parse::<i64>().ok())
        .collect();

    if case_names.is_empty() {
        // If we only have numeric cases, attempt a numeric-to-enum match.
        if numeric_cases.is_empty() {
            return None;
        }
    }

    // Special-case the well-known "_enum_type" discriminator used by AvailableCommands.
    // It always has exactly Byte/Short/Int variants and maps to the native helper type
    // "enum_size_based_on_values_len".
    {
        let set: HashSet<String> = case_names.iter().cloned().collect();
        let expected: HashSet<String> = ["Byte", "Short", "Int"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if set == expected {
            return Some(Type::Reference("enum_size_based_on_values_len".to_string()));
        }
        // Allow ordering differences as long as they are a subset/superset (defensive)
        if set.is_superset(&expected) || expected.is_superset(&set) {
            return Some(Type::Reference("enum_size_based_on_values_len".to_string()));
        }
    }

    // Track best numeric match with a simple score (name hint priority).
    let mut best_numeric: Option<(i32, String)> = None;

    for (type_name, ty) in &ctx.type_lookup {
        if let Type::Enum { variants, .. } = ty {
            let variant_set: HashSet<String> = variants
                .iter()
                .map(|(n, _)| normalize_case_name(n))
                .collect();
            let total = case_names.len();
            let matched = case_names
                .iter()
                .filter(|c| variant_set.contains(*c))
                .count();

            // Heuristic: allow matches when all observed string cases are covered.
            // For single-case switches (e.g., only "Remove"), allow a single match.
            if total > 0 && matched == total && (total == 1 || matched >= 2) {
                return Some(Type::Reference(type_name.clone()));
            }

            // Near-complete matches: tolerate one missing match when we have at least 2 cases.
            if total > 1 && matched >= total.saturating_sub(1) && matched >= 2 {
                return Some(Type::Reference(type_name.clone()));
            }

            // Numeric matching fallback when numeric cases are present.
            if !numeric_cases.is_empty() {
                let variant_nums: HashSet<i64> = variants.iter().map(|(_, v)| *v).collect();
                let all_present = numeric_cases.iter().all(|v| variant_nums.contains(v));
                if all_present {
                    let hint_score = discriminator_hint
                        .and_then(|h| {
                            let h_lower = h.to_lowercase();
                            let tn_lower = type_name.to_lowercase();
                            if tn_lower.contains(&h_lower) {
                                Some(1)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    // Prefer higher hint_score; tie-break by first seen.
                    if best_numeric
                        .as_ref()
                        .map(|(s, _)| hint_score > *s)
                        .unwrap_or(true)
                    {
                        best_numeric = Some((hint_score, type_name.clone()));
                    }
                }
            }
        }
    }

    if let Some((_, best)) = best_numeric {
        return Some(Type::Reference(best));
    }

    None
}

fn clean_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    let trimmed = trimmed.replace("../", "");
    let base = trimmed.split('.').next().unwrap_or(&trimmed);
    clean_field_name(base, "")
}

pub fn canonical_type_signature(ty: &Type, ctx: &Context) -> String {
    canonical_type_signature_inner(ty, ctx, &mut HashSet::new())
}

fn canonical_type_signature_inner(ty: &Type, ctx: &Context, seen: &mut HashSet<String>) -> String {
    match ty {
        Type::Primitive(p) => format!("P:{:?}", p),
        Type::String {
            count_type,
            encoding,
        } => format!(
            "PS:{:?}:{}",
            encoding,
            canonical_type_signature_inner(count_type, ctx, seen)
        ),
        Type::Encapsulated { length_type, inner } => format!(
            "ENC:{}:{}",
            canonical_type_signature_inner(length_type, ctx, seen),
            canonical_type_signature_inner(inner, ctx, seen)
        ),
        Type::Reference(r) => {
            let clean = clean_type_name(r);
            if seen.contains(&clean) {
                return format!("R:{}:<<cycle>>", clean);
            }
            let mut sig = format!("R:{}", clean);
            if let Some(resolved) = ctx.type_lookup.get(r) {
                seen.insert(clean.clone());
                let inner = canonical_type_signature_inner(resolved, ctx, seen);
                seen.remove(&clean);
                sig.push_str(&format!(":{}", inner));
            }
            sig
        }
        Type::Container(c) => {
            let redundant = find_redundant_fields(c);
            let names = derive_field_names(c, &c.name);
            let mut parts = Vec::new();
            for (idx, field) in c.fields.iter().enumerate() {
                if redundant.contains(&field.name) {
                    continue;
                }
                let fname = names
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| clean_field_name(&field.name, &c.name));
                parts.push(format!(
                    "{}:{}",
                    fname,
                    canonical_type_signature_inner(&field.type_def, ctx, seen)
                ));
            }
            format!("C{{{}}}", parts.join(","))
        }
        Type::Array {
            count_type,
            inner_type,
        } => format!(
            "A:[{}]->{}",
            canonical_type_signature_inner(count_type, ctx, seen),
            canonical_type_signature_inner(inner_type, ctx, seen)
        ),
        Type::Option(inner) => format!("O:{}", canonical_type_signature_inner(inner, ctx, seen)),
        Type::Switch {
            compare_to,
            fields,
            default,
        } => {
            let mut cases: Vec<String> = fields
                .iter()
                .map(|(name, ty)| {
                    format!(
                        "{}=>{}",
                        name,
                        canonical_type_signature_inner(ty, ctx, seen)
                    )
                })
                .collect();
            cases.sort();
            format!(
                "S:cmp:{}:[{}]|D:{}",
                compare_to,
                cases.join(","),
                canonical_type_signature_inner(default, ctx, seen)
            )
        }
        Type::Enum {
            underlying,
            variants,
        } => {
            let mut ordered = variants.clone();
            ordered.sort_by(|a, b| a.0.cmp(&b.0));
            let parts: Vec<String> = ordered
                .iter()
                .map(|(n, v)| format!("{}={}", n, v))
                .collect();
            format!("E:{:?}:[{}]", underlying, parts.join(","))
        }
        Type::Bitfield {
            storage_type,
            flags,
            ..
        } => {
            let mut ordered = flags.clone();
            ordered.sort_by(|a, b| a.0.cmp(&b.0));
            let parts: Vec<String> = ordered
                .iter()
                .map(|(n, v)| format!("{}={}", n, v))
                .collect();
            format!("B:{:?}:[{}]", storage_type, parts.join(","))
        }
        Type::Packed { backing, fields } => {
            let mut ordered = fields.clone();
            ordered.sort_by(|a, b| a.name.cmp(&b.name));
            let parts: Vec<String> = ordered
                .iter()
                .map(|f| format!("{}:{}:{}", f.name, f.shift, f.mask))
                .collect();
            format!("PK:{:?}:[{}]", backing, parts.join(","))
        }
    }
}

pub fn compute_packet_signature(
    struct_name: &str,
    container: &Container,
    ctx: &Context,
) -> PacketSignature {
    let redundant = find_redundant_fields(container);
    let unique_names = derive_field_names(container, struct_name);

    let mut fields = Vec::new();
    for (idx, field) in container.fields.iter().enumerate() {
        if redundant.contains(&field.name) {
            continue;
        }
        let fname = unique_names
            .get(idx)
            .cloned()
            .unwrap_or_else(|| clean_field_name(&field.name, struct_name));
        let ftype = canonical_type_signature(&field.type_def, ctx);
        fields.push((fname, ftype));
    }

    let resolved = ResolvedContainer::analyze(container, ctx);
    let mut args_vec = Vec::new();
    for (name, ty) in &resolved.args {
        args_vec.push((name.clone(), canonical_type_signature(ty, ctx)));
    }
    args_vec.sort_by(|a, b| a.0.cmp(&b.0));

    PacketSignature {
        fields,
        args: args_vec,
    }
}
