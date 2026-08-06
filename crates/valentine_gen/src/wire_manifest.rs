//! Source-independent wire manifests emitted directly from Valentine's IR.
//!
//! Containers and named references are recursively expanded so the manifest
//! never depends on generated Rust syntax. Wire-shaping constructs remain
//! explicit structured nodes: their children are themselves fully expanded.

use crate::ir::{Container, Primitive, Type, UnionVariant};
use crate::parser::ParseResult;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WireManifest {
    pub schema_version: u32,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minecraft_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<i32>,
    pub packets: Vec<WirePacket>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WirePacket {
    pub id: u32,
    pub name: String,
    pub operations: Vec<WireOperation>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireOperation {
    Primitive {
        field: String,
        op: String,
    },
    String {
        field: String,
        prefix: String,
        encoding: Option<String>,
    },
    Encapsulated {
        field: String,
        prefix: String,
        operations: Vec<WireOperation>,
    },
    Array {
        field: String,
        prefix: String,
        element: Vec<WireOperation>,
    },
    FixedArray {
        field: String,
        length: usize,
        element: Vec<WireOperation>,
    },
    Option {
        field: String,
        /// Valentine options use a one-byte 0/1 presence marker. `Bool` is the
        /// shared oracle vocabulary for that wire operation.
        presence: String,
        value: Vec<WireOperation>,
    },
    Union {
        field: String,
        control: String,
        variants: Vec<WireVariant>,
    },
    Conditional {
        field: String,
        compare_to: String,
        cases: Vec<WireCase>,
        default: Vec<WireOperation>,
    },
    RecursiveReference {
        field: String,
        type_name: String,
    },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WireVariant {
    pub value: i64,
    pub name: String,
    pub operations: Vec<WireOperation>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WireCase {
    pub value: String,
    pub operations: Vec<WireOperation>,
}

pub fn build(
    parsed: &ParseResult,
    source: impl Into<String>,
    minecraft_version: Option<String>,
    protocol_version: Option<i32>,
) -> Result<WireManifest, String> {
    let mut packets = parsed
        .packets
        .iter()
        .map(|packet| {
            let mut expander = Expander {
                types: &parsed.types,
                reference_stack: HashSet::new(),
            };
            Ok(WirePacket {
                id: packet.id,
                name: packet.name.clone(),
                operations: expander.container(&packet.body, "")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    packets.sort_by_key(|packet| packet.id);
    Ok(WireManifest {
        schema_version: 1,
        source: source.into(),
        minecraft_version,
        protocol_version,
        packets,
    })
}

pub fn write(path: &Path, manifest: &WireManifest) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

struct Expander<'a> {
    types: &'a HashMap<String, Type>,
    reference_stack: HashSet<String>,
}

impl Expander<'_> {
    fn container(
        &mut self,
        container: &Container,
        parent: &str,
    ) -> Result<Vec<WireOperation>, String> {
        let mut operations = Vec::new();
        for field in &container.fields {
            let path = join_path(parent, &field.name);
            operations.extend(self.ty(&field.type_def, &path)?);
        }
        Ok(operations)
    }

    fn ty(&mut self, ty: &Type, field: &str) -> Result<Vec<WireOperation>, String> {
        match ty {
            Type::Primitive(Primitive::Void) => Ok(Vec::new()),
            Type::Primitive(Primitive::Uuid) => Ok(vec![WireOperation::FixedArray {
                field: field.to_string(),
                length: 16,
                element: vec![WireOperation::Primitive {
                    field: format!("{field}[]"),
                    op: "U8".to_string(),
                }],
            }]),
            Type::Primitive(primitive) => Ok(vec![WireOperation::Primitive {
                field: field.to_string(),
                op: primitive_name(primitive).to_string(),
            }]),
            Type::String {
                count_type,
                encoding,
            } => Ok(vec![WireOperation::String {
                field: field.to_string(),
                prefix: self.scalar(count_type)?,
                encoding: encoding.clone(),
            }]),
            Type::Encapsulated { length_type, inner } => Ok(vec![WireOperation::Encapsulated {
                field: field.to_string(),
                prefix: self.scalar(length_type)?,
                operations: self.ty(inner, field)?,
            }]),
            Type::Reference(name) => self.reference(name, field),
            Type::Container(container) => self.container(container, field),
            Type::Array {
                count_type,
                inner_type,
            } => Ok(vec![WireOperation::Array {
                field: field.to_string(),
                prefix: self.scalar(count_type)?,
                element: self.ty(inner_type, &format!("{field}[]"))?,
            }]),
            Type::FixedArray { size, inner_type } => Ok(vec![WireOperation::FixedArray {
                field: field.to_string(),
                length: *size,
                element: self.ty(inner_type, &format!("{field}[]"))?,
            }]),
            Type::Option(inner) => Ok(vec![WireOperation::Option {
                field: field.to_string(),
                presence: "Bool".to_string(),
                value: self.ty(inner, field)?,
            }]),
            Type::Switch {
                compare_to,
                fields,
                default,
            } => Ok(vec![WireOperation::Conditional {
                field: field.to_string(),
                compare_to: compare_to.clone(),
                cases: fields
                    .iter()
                    .map(|(value, case)| {
                        Ok(WireCase {
                            value: value.clone(),
                            operations: self.ty(case, field)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                default: self.ty(default, field)?,
            }]),
            Type::Union {
                control_type,
                variants,
            } => Ok(vec![WireOperation::Union {
                field: field.to_string(),
                control: primitive_name(control_type).to_string(),
                variants: self.union_variants(variants, field)?,
            }]),
            Type::Enum { underlying, .. } => Ok(vec![WireOperation::Primitive {
                field: field.to_string(),
                op: primitive_name(underlying).to_string(),
            }]),
            Type::Bitfield { storage_type, .. } => Ok(vec![WireOperation::Primitive {
                field: field.to_string(),
                op: primitive_name(storage_type).to_string(),
            }]),
            Type::Packed { backing, .. } => Ok(vec![WireOperation::Primitive {
                field: field.to_string(),
                op: primitive_name(backing).to_string(),
            }]),
        }
    }

    fn reference(&mut self, name: &str, field: &str) -> Result<Vec<WireOperation>, String> {
        if !self.reference_stack.insert(name.to_string()) {
            return Ok(vec![WireOperation::RecursiveReference {
                field: field.to_string(),
                type_name: name.to_string(),
            }]);
        }
        let resolved = self.types.get(name).ok_or_else(|| {
            format!("wire manifest cannot resolve named IR type {name} at field {field}")
        })?;
        let operations = self.ty(resolved, field);
        self.reference_stack.remove(name);
        operations
    }

    fn union_variants(
        &mut self,
        variants: &[UnionVariant],
        field: &str,
    ) -> Result<Vec<WireVariant>, String> {
        variants
            .iter()
            .map(|variant| {
                Ok(WireVariant {
                    value: variant.control_value,
                    name: variant.name.clone(),
                    operations: self.ty(&variant.type_def, field)?,
                })
            })
            .collect()
    }

    fn scalar(&mut self, ty: &Type) -> Result<String, String> {
        match ty {
            Type::Primitive(primitive) => Ok(primitive_name(primitive).to_string()),
            Type::Enum { underlying, .. }
            | Type::Bitfield {
                storage_type: underlying,
                ..
            }
            | Type::Packed {
                backing: underlying,
                ..
            } => Ok(primitive_name(underlying).to_string()),
            Type::Reference(name) => {
                if !self.reference_stack.insert(name.clone()) {
                    return Err(format!("cyclic scalar reference {name}"));
                }
                let resolved = self
                    .types
                    .get(name)
                    .ok_or_else(|| format!("wire manifest cannot resolve scalar type {name}"))?;
                let result = self.scalar(resolved);
                self.reference_stack.remove(name);
                result
            }
            other => Err(format!("wire length/control type is not scalar: {other:?}")),
        }
    }
}

fn join_path(parent: &str, field: &str) -> String {
    if parent.is_empty() {
        field.to_string()
    } else {
        format!("{parent}.{field}")
    }
}

fn primitive_name(primitive: &Primitive) -> &'static str {
    match primitive {
        Primitive::Bool => "Bool",
        Primitive::U8 => "U8",
        Primitive::I8 => "I8",
        Primitive::U16 => "U16BE",
        Primitive::U16LE => "U16LE",
        Primitive::I16 => "I16BE",
        Primitive::I16LE => "I16LE",
        Primitive::U32 => "U32BE",
        Primitive::U32LE => "U32LE",
        Primitive::I32 => "I32BE",
        Primitive::I32LE => "I32LE",
        Primitive::U64 => "U64BE",
        Primitive::U64LE => "U64LE",
        Primitive::I64 => "I64BE",
        Primitive::I64LE => "I64LE",
        Primitive::F32 => "F32BE",
        Primitive::F32LE => "F32LE",
        Primitive::F64 => "F64BE",
        Primitive::F64LE => "F64LE",
        Primitive::VarInt => "VarInt",
        Primitive::VarLong => "VarLong",
        Primitive::ZigZag32 => "ZigZag32",
        Primitive::ZigZag64 => "ZigZag64",
        Primitive::Uuid => "Uuid",
        Primitive::Void => "Void",
        Primitive::ByteArray => "RawBytes",
        Primitive::Nbt => "Nbt",
    }
}

#[cfg(test)]
mod tests {
    use super::{WireOperation, build};
    use crate::ir::{Container, Field, Packet, Primitive, Type, UnionVariant};
    use crate::parser::ParseResult;
    use std::collections::HashMap;

    #[test]
    fn recursively_flattens_references_and_preserves_wire_structure() {
        let parsed = ParseResult {
            packets: vec![Packet {
                id: 7,
                name: "Fixture".to_string(),
                body: Container {
                    name: "FixturePacket".to_string(),
                    fields: vec![
                        Field {
                            name: "position".to_string(),
                            type_def: Type::Reference("Vec2".to_string()),
                        },
                        Field {
                            name: "values".to_string(),
                            type_def: Type::Option(Box::new(Type::Array {
                                count_type: Box::new(Type::Primitive(Primitive::VarInt)),
                                inner_type: Box::new(Type::Reference("Mode".to_string())),
                            })),
                        },
                        Field {
                            name: "choice".to_string(),
                            type_def: Type::Union {
                                control_type: Primitive::VarInt,
                                variants: vec![UnionVariant {
                                    control_value: 3,
                                    name: "Number".to_string(),
                                    type_def: Type::Primitive(Primitive::U32LE),
                                }],
                            },
                        },
                    ],
                },
            }],
            types: HashMap::from([
                (
                    "Vec2".to_string(),
                    Type::Container(Container {
                        name: "Vec2".to_string(),
                        fields: vec![
                            Field {
                                name: "x".to_string(),
                                type_def: Type::Primitive(Primitive::F32LE),
                            },
                            Field {
                                name: "y".to_string(),
                                type_def: Type::Primitive(Primitive::F32LE),
                            },
                        ],
                    }),
                ),
                (
                    "Mode".to_string(),
                    Type::Enum {
                        underlying: Primitive::U8,
                        variants: vec![("A".to_string(), 0)],
                    },
                ),
            ]),
        };

        let manifest = build(&parsed, "fixture", None, None).expect("build manifest");
        assert_eq!(manifest.packets[0].operations.len(), 4);
        assert!(matches!(
            &manifest.packets[0].operations[0],
            WireOperation::Primitive { field, op } if field == "position.x" && op == "F32LE"
        ));
        assert!(matches!(
            &manifest.packets[0].operations[2],
            WireOperation::Option { presence, value, .. }
                if presence == "Bool" && matches!(value[0], WireOperation::Array { .. })
        ));
        assert!(matches!(
            &manifest.packets[0].operations[3],
            WireOperation::Union { control, variants, .. }
                if control == "VarInt" && variants[0].value == 3
        ));
    }

    #[test]
    fn marks_recursive_references_without_recursing_forever() {
        let parsed = ParseResult {
            packets: vec![Packet {
                id: 1,
                name: "Recursive".to_string(),
                body: Container {
                    name: "RecursivePacket".to_string(),
                    fields: vec![Field {
                        name: "root".to_string(),
                        type_def: Type::Reference("Node".to_string()),
                    }],
                },
            }],
            types: HashMap::from([(
                "Node".to_string(),
                Type::Container(Container {
                    name: "Node".to_string(),
                    fields: vec![Field {
                        name: "next".to_string(),
                        type_def: Type::Option(Box::new(Type::Reference("Node".to_string()))),
                    }],
                }),
            )]),
        };

        let manifest = build(&parsed, "fixture", None, None).expect("build manifest");
        let WireOperation::Option { value, .. } = &manifest.packets[0].operations[0] else {
            panic!("expected recursive option");
        };
        assert!(matches!(
            &value[0],
            WireOperation::RecursiveReference { type_name, .. } if type_name == "Node"
        ));
    }
}
