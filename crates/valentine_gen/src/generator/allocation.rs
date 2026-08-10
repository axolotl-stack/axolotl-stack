use crate::generator::context::Context;
use crate::ir::{Primitive, Type};
use std::collections::HashSet;

/// Returns a conservative lower bound for one encoded value. `None` means the
/// schema does not provide enough static information to prove a bound.
pub fn minimum_encoded_size(ty: &Type, ctx: &Context<'_>) -> Option<usize> {
    minimum_encoded_size_inner(ty, ctx, &mut HashSet::new())
}

fn minimum_encoded_size_inner(
    ty: &Type,
    ctx: &Context<'_>,
    visiting: &mut HashSet<String>,
) -> Option<usize> {
    match ty {
        Type::Primitive(primitive) => primitive_minimum(primitive),
        Type::String { count_type, .. }
        | Type::Encapsulated {
            length_type: count_type,
            ..
        }
        | Type::Array { count_type, .. } => minimum_encoded_size_inner(count_type, ctx, visiting),
        Type::Reference(name) => {
            if name == "LittleString" {
                return Some(4);
            }
            if !visiting.insert(name.clone()) {
                return None;
            }
            let minimum = ctx
                .type_lookup
                .get(name)
                .and_then(|resolved| minimum_encoded_size_inner(resolved, ctx, visiting));
            visiting.remove(name);
            minimum
        }
        Type::Container(container) => container.fields.iter().try_fold(0usize, |total, field| {
            total.checked_add(minimum_encoded_size_inner(&field.type_def, ctx, visiting)?)
        }),
        Type::FixedArray { size, inner_type } => {
            minimum_encoded_size_inner(inner_type, ctx, visiting)?.checked_mul(*size)
        }
        Type::Option(_) => Some(1),
        Type::Switch {
            fields, default, ..
        } => {
            let mut minimum = minimum_encoded_size_inner(default, ctx, visiting)?;
            for (_, field_type) in fields {
                minimum = minimum.min(minimum_encoded_size_inner(field_type, ctx, visiting)?);
            }
            Some(minimum)
        }
        Type::Union {
            control_type,
            variants,
        } => {
            let control = primitive_minimum(control_type)?;
            let payload = variants
                .iter()
                .map(|variant| minimum_encoded_size_inner(&variant.type_def, ctx, visiting))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .min()?;
            control.checked_add(payload)
        }
        Type::Enum { underlying, .. } => primitive_minimum(underlying),
        Type::Bitfield { storage_type, .. } => primitive_minimum(storage_type),
        Type::Packed { backing, .. } => primitive_minimum(backing),
    }
}

fn primitive_minimum(primitive: &Primitive) -> Option<usize> {
    match primitive {
        Primitive::Bool | Primitive::U8 | Primitive::I8 => Some(1),
        Primitive::U16 | Primitive::U16LE | Primitive::I16 | Primitive::I16LE => Some(2),
        Primitive::U32
        | Primitive::U32LE
        | Primitive::I32
        | Primitive::I32LE
        | Primitive::F32
        | Primitive::F32LE => Some(4),
        Primitive::U64
        | Primitive::U64LE
        | Primitive::I64
        | Primitive::I64LE
        | Primitive::F64
        | Primitive::F64LE => Some(8),
        Primitive::VarInt | Primitive::VarLong | Primitive::ZigZag32 | Primitive::ZigZag64 => {
            Some(1)
        }
        Primitive::Uuid => Some(16),
        Primitive::Void => Some(0),
        Primitive::ByteArray | Primitive::Nbt => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::context::{Context, GlobalRegistry};

    fn with_context(test: impl FnOnce(&mut Context<'_>)) {
        let mut registry = GlobalRegistry::new();
        let mut ctx = Context {
            definitions_by_group: Default::default(),
            emitted: Default::default(),
            in_progress: Default::default(),
            aliases_emitted: Default::default(),
            inline_cache: Default::default(),
            type_lookup: Default::default(),
            global_registry: &mut registry,
            current_crate_name: "test".into(),
            current_local_path: "test".into(),
            current_external_path: "test".into(),
            crate_dependencies: Default::default(),
            argful_types: Default::default(),
        };
        test(&mut ctx);
    }

    #[test]
    fn computes_positive_and_zero_minimums() {
        with_context(|ctx| {
            assert_eq!(
                minimum_encoded_size(&Type::Primitive(Primitive::U32LE), ctx),
                Some(4)
            );
            assert_eq!(
                minimum_encoded_size(&Type::Primitive(Primitive::Void), ctx),
                Some(0)
            );
            assert_eq!(
                minimum_encoded_size(&Type::Primitive(Primitive::Nbt), ctx),
                None
            );
            assert_eq!(
                minimum_encoded_size(
                    &Type::String {
                        count_type: Box::new(Type::Primitive(Primitive::VarInt)),
                        encoding: None,
                    },
                    ctx,
                ),
                Some(1)
            );
        });
    }

    #[test]
    fn resolves_references_and_detects_cycles() {
        with_context(|ctx| {
            ctx.type_lookup.insert(
                "Word".into(),
                Type::FixedArray {
                    size: 3,
                    inner_type: Box::new(Type::Primitive(Primitive::U16LE)),
                },
            );
            ctx.type_lookup
                .insert("Cycle".into(), Type::Reference("Cycle".into()));
            assert_eq!(
                minimum_encoded_size(&Type::Reference("Word".into()), ctx),
                Some(6)
            );
            assert_eq!(
                minimum_encoded_size(&Type::Reference("Cycle".into()), ctx),
                None
            );
        });
    }

    #[test]
    fn chooses_the_smallest_switch_branch() {
        with_context(|ctx| {
            let ty = Type::Switch {
                compare_to: "kind".into(),
                fields: vec![("one".into(), Type::Primitive(Primitive::U64LE))],
                default: Box::new(Type::Primitive(Primitive::U16LE)),
            };
            assert_eq!(minimum_encoded_size(&ty, ctx), Some(2));
        });
    }
}
