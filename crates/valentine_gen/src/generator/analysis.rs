use crate::generator::context::Context;
use crate::ir::{Container, Primitive, Type};
use std::collections::HashSet;

/// Determines if a type should be Boxed when used as an enum variant field
/// to reduce the overall size of the enum.
pub fn should_box_variant(t: &Type, ctx: &Context, depth: usize) -> bool {
    const MAX_DEPTH: usize = 8;
    if depth > MAX_DEPTH {
        // Break potential recursion cycles or deep stacks by boxing
        return true;
    }

    match t {
        Type::Primitive(p) => match p {
            // Heap-backed types are already small stack-wise (pointers/metadata)
            Primitive::McString | Primitive::ByteArray => false,
            // Scalars are small
            Primitive::Bool
            | Primitive::U8
            | Primitive::I8
            | Primitive::U16
            | Primitive::I16
            | Primitive::U32
            | Primitive::I32
            | Primitive::F32
            | Primitive::U64
            | Primitive::I64
            | Primitive::F64
            | Primitive::VarInt
            | Primitive::VarLong
            | Primitive::ZigZag32
            | Primitive::ZigZag64
            | Primitive::Uuid
            | Primitive::Void => false,
        },
        Type::Array { .. } => false,
        Type::Reference(r) => {
            if let Some(inner) = ctx.type_lookup.get(r) {
                should_box_variant(inner, ctx, depth + 1)
            } else {
                true
            }
        }
        Type::Container(c) => {
            // Empty or small structs are fine.
            if c.fields.is_empty() {
                return false;
            }
            if c.fields.len() > 3 {
                return true;
            }

            // If any field is "large" (needs boxing), then this struct contains that large thing inline,
            // making the struct large.
            for f in &c.fields {
                if should_box_variant(&f.type_def, ctx, depth + 1) {
                    return true;
                }
            }
            false
        }
        Type::Option(inner) => should_box_variant(inner, ctx, depth + 1),

        Type::Switch { .. } => true,

        Type::Enum { .. } | Type::Bitfield { .. } => false,
    }
}

/// Identifies boolean fields that are redundant because they control a Switch
/// which has been optimized into an Option.
/// Returns a set of field names (raw JSON names) that should be hidden.
pub fn find_redundant_fields(container: &Container) -> HashSet<String> {
    let mut redundant = HashSet::new();
    for field in &container.fields {
        if let Type::Switch {
            compare_to,
            fields,
            default,
        } = &field.type_def
        {
            // Logic for Inverse Option optimization:
            // Cases are Void, Default is Data.
            let all_cases_void = fields
                .iter()
                .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));
            
            if all_cases_void && !matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                // This switch becomes Option<Default>. It depends on `compare_to`.
                // The `compare_to` field is the boolean flag we want to hide.
                // Handle simple relative paths like "../hidden_field" -> "hidden_field"
                let target = compare_to.replace("../", "");
                redundant.insert(target);
            }
        }
    }
    redundant
}
