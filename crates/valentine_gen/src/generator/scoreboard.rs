//! Explicit SetScore wire correction, retained by every generator run.
//!
//! Its packet-level action controls every array row. The canonical JSON-shaped
//! union instead puts strings and a synthetic discriminator inside each row.
//! This boundary is specified by the public protocol/library formats linked in
//! the generator README and tested against independently written byte fixtures.

use crate::ir::{Packet, Primitive, Type};
use crate::parser::ParseResult;
use proc_macro2::TokenStream;
use std::collections::{HashMap, HashSet};

/// Match only the known malformed packet shape; upstream changes require review.
pub(super) fn applies(packet: &Packet, types: &HashMap<String, Type>) -> Result<bool, String> {
    if packet.id != 108 || packet.name != "SetScorePacket" {
        return Ok(false);
    }
    let [field] = packet.body.fields.as_slice() else {
        return Err(
            "SetScore canonical shape changed; review or remove its wire correction".into(),
        );
    };
    let Type::Array {
        count_type,
        inner_type,
    } = &field.type_def
    else {
        return Err("SetScore canonical array changed".into());
    };
    let Type::Union {
        control_type,
        variants,
    } = inner_type.as_ref()
    else {
        return Err("SetScore canonical row union changed".into());
    };
    let changed =
        || "SetScore canonical row shape changed; review or remove its wire correction".to_owned();
    if count_type.as_ref() != &Type::Primitive(Primitive::VarUInt)
        || *control_type != Primitive::VarUInt
        || variants.len() != 4
    {
        return Err(changed());
    }
    for (index, (variant, name)) in variants
        .iter()
        .zip([
            "RemoveScore",
            "ChangePlayerScore",
            "ChangeEntityScore",
            "ChangeFakePlayerScore",
        ])
        .enumerate()
    {
        let ty = match &variant.type_def {
            Type::Reference(name) => types.get(name),
            other => Some(other),
        };
        let Some(Type::Container(container)) = ty else {
            return Err(changed());
        };
        let string_action = container.fields.first().is_some_and(|field| {
            field.name == "Action" && matches!(field.type_def, Type::String { .. })
        });
        if variant.control_value != index as i64 || variant.name != name || !string_action {
            return Err(changed());
        }
    }
    Ok(true)
}

/// The owned packet, row and identity codecs, independent of malformed schema fields.
pub(super) fn owned_tokens() -> TokenStream {
    include_str!("scoreboard/owned.rs")
        .parse()
        .expect("valid SetScore owned template")
}
/// Borrowed row decoding shares the same action boundary and strict byte discriminants.
pub(super) fn borrowed_tokens() -> TokenStream {
    include_str!("scoreboard/borrowed.rs")
        .parse()
        .expect("valid SetScore borrowed template")
}

/// Old score-only helper types must not remain as misleading public wire codecs.
pub(super) fn legacy_types(parsed: &ParseResult) -> HashSet<String> {
    /// Walk named dependencies while keeping shared types used by other packets.
    fn visit(ty: &Type, types: &HashMap<String, Type>, out: &mut HashSet<String>) {
        match ty {
            Type::Reference(name) => {
                if out.insert(name.clone())
                    && let Some(ty) = types.get(name)
                {
                    visit(ty, types, out);
                }
            }
            Type::Container(container) => {
                for field in &container.fields {
                    visit(&field.type_def, types, out)
                }
            }
            Type::Array {
                count_type,
                inner_type,
            } => {
                visit(count_type, types, out);
                visit(inner_type, types, out);
            }
            Type::Union { variants, .. } => {
                for variant in variants {
                    visit(&variant.type_def, types, out)
                }
            }
            Type::Switch {
                fields, default, ..
            } => {
                for (_, ty) in fields {
                    visit(ty, types, out)
                }
                visit(default, types, out);
            }
            Type::Option(inner)
            | Type::FixedArray {
                inner_type: inner, ..
            } => visit(inner, types, out),
            Type::String { count_type, .. } => visit(count_type, types, out),
            Type::Encapsulated { length_type, inner } => {
                visit(length_type, types, out);
                visit(inner, types, out);
            }
            _ => {}
        }
    }
    if !parsed
        .packets
        .iter()
        .any(|packet| packet.id == 108 && packet.name == "SetScorePacket")
    {
        return HashSet::new();
    }
    let mut score = HashSet::new();
    let mut others = HashSet::new();
    for packet in &parsed.packets {
        visit(
            &Type::Container(packet.body.clone()),
            &parsed.types,
            if packet.id == 108 {
                &mut score
            } else {
                &mut others
            },
        );
    }
    score.retain(|name| !others.contains(name));
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::context::GlobalRegistry;
    use std::path::PathBuf;

    /// Parse the pinned schema instead of a synthetic copy of its malformed rows.
    fn parsed() -> ParseResult {
        crate::parser::protocolgen::parse(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("protocolgen/generated/1.26.51/manifest.json"),
        )
        .unwrap()
        .result
    }

    #[test]
    fn regeneration_retains_packet_action_and_borrowed_identity_boundaries() {
        let parsed = parsed();
        let packet = parsed
            .packets
            .iter()
            .find(|packet| packet.id == 108)
            .unwrap();
        assert!(applies(packet, &parsed.types).unwrap());
        let dir = std::env::temp_dir().join(format!(
            "valentine-score-regeneration-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        crate::generator::generate_protocol_module(
            "score_fixture",
            "",
            &parsed,
            &dir,
            &mut GlobalRegistry::new(),
        )
        .unwrap();
        let owned = std::fs::read_to_string(dir.join("proto.rs")).unwrap();
        let borrowed = std::fs::read_to_string(dir.join("borrowed.rs")).unwrap();
        assert!(owned.contains("pub action: SetScoreAction"));
        assert!(owned.contains("pub entries: Vec<ScoreEntry>"));
        assert!(owned.contains("pub identity: Option<ScoreIdentity>"));
        assert!(borrowed.contains("pub identity: Option<ScoreIdentityView>"));
        assert!(!owned.contains("pub score_info: Vec<SetScorePacketScoreInfoItem>"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_changed_upstream_score_shape_requires_review_instead_of_a_stale_override() {
        let mut parsed = parsed();
        let packet = parsed
            .packets
            .iter_mut()
            .find(|packet| packet.id == 108)
            .unwrap();
        packet.body.fields.push(crate::ir::Field {
            name: "Action".into(),
            type_def: Type::Primitive(Primitive::U8),
        });
        assert!(applies(packet, &parsed.types).is_err());
    }
}
