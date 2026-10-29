//! The command syntax every player is sent, and the index the parser uses.
//!
//! Both come from `unastar_data::commands::vanilla()`, the captured BDS 1.26.51
//! `AvailableCommandsPacket`. The packet is rebuilt from that data once and is
//! byte-identical to the capture (see the tests); BDS sends the same packet to
//! members and operators and the client hides what the player's command
//! permission does not allow.

use jolyne::valentine::AvailableCommandsPacket;
use jolyne::valentine::types::{
    AvailableCommandsPacketCommandDatajson, AvailableCommandsPacketPayloadChainedSubcommandData,
    AvailableCommandsPacketPayloadChainedSubcommandRelationship,
    AvailableCommandsPacketPayloadConstrainedValueData, AvailableCommandsPacketPayloadEnumData,
    AvailableCommandsPacketPayloadOverloadData, AvailableCommandsPacketPayloadParamData,
    AvailableCommandsPacketPayloadSoftEnumData,
};
use std::collections::HashMap;
use std::sync::Arc;
use unastar_data::commands::{
    CommandDefinition, CommandEnum, CommandsArtifact, ParamKind, SYMBOL_CHAINED, SYMBOL_ENUM,
    SYMBOL_POSTFIX, SYMBOL_SOFT_ENUM, SYMBOL_VALID,
};

/// Frozen command syntax: the cached join packet plus lookup tables.
pub struct CommandSchema {
    syntax: &'static CommandsArtifact,
    packet: Arc<AvailableCommandsPacket>,
    /// Lowercase command names and aliases → index in `syntax.commands`.
    by_name: HashMap<String, usize>,
    /// Enum name → lowercase value → canonical value.
    enums: HashMap<&'static str, HashMap<String, &'static str>>,
}

impl std::fmt::Debug for CommandSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSchema")
            .field("commands", &self.syntax.commands.len())
            .finish_non_exhaustive()
    }
}

impl CommandSchema {
    /// The vanilla BDS 1.26.51 syntax.
    pub fn vanilla() -> Self {
        Self::new(unastar_data::commands::vanilla())
    }

    pub fn new(syntax: &'static CommandsArtifact) -> Self {
        let mut by_name = HashMap::new();
        for (index, command) in syntax.commands.iter().enumerate() {
            by_name.insert(command.name.to_ascii_lowercase(), index);
            if let Some(aliases) = &command.aliases {
                let aliases = syntax.enum_named(aliases).expect("validated alias enum");
                for alias in &aliases.values {
                    by_name.entry(alias.to_ascii_lowercase()).or_insert(index);
                }
            }
        }
        let enums = syntax
            .enums
            .iter()
            .map(|e| (e.name.as_str(), lowercase_index(e)))
            .collect();
        Self {
            syntax,
            packet: Arc::new(build_packet(syntax)),
            by_name,
            enums,
        }
    }

    pub fn syntax(&self) -> &'static CommandsArtifact {
        self.syntax
    }

    /// The join packet; clone the `Arc` contents per send.
    pub fn packet(&self) -> &AvailableCommandsPacket {
        &self.packet
    }

    /// Looks up a command by name or alias, case-insensitively.
    pub fn command(&self, name: &str) -> Option<&'static CommandDefinition> {
        self.by_name
            .get(&name.to_ascii_lowercase())
            .map(|&index| &self.syntax.commands[index])
    }

    /// Matches `value` against an enum case-insensitively, returning BDS's spelling.
    pub fn enum_value(&self, enum_name: &str, value: &str) -> Option<&'static str> {
        self.enums
            .get(enum_name)?
            .get(&value.to_ascii_lowercase())
            .copied()
    }
}

fn lowercase_index(e: &'static CommandEnum) -> HashMap<String, &'static str> {
    e.values
        .iter()
        .map(|value| (value.to_ascii_lowercase(), value.as_str()))
        .collect()
}

fn index_of<'a>(names: impl IntoIterator<Item = &'a String>) -> HashMap<&'a str, u32> {
    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index as u32))
        .collect()
}

/// Rebuilds the packet: every name in the artefact maps back to its packet index.
fn build_packet(syntax: &CommandsArtifact) -> AvailableCommandsPacket {
    let values = index_of(&syntax.enum_values);
    let enums = index_of(syntax.enums.iter().map(|e| &e.name));
    let soft_enums = index_of(syntax.soft_enums.iter().map(|e| &e.name));
    let postfixes = index_of(&syntax.postfixes);
    let chained = index_of(syntax.chained_subcommands.iter().map(|c| &c.name));
    let chained_values = index_of(&syntax.chained_subcommand_values);
    let symbol = |kind: &ParamKind| -> u32 {
        match kind {
            ParamKind::Basic(basic) => SYMBOL_VALID | basic.symbol(),
            ParamKind::Enum(name) => SYMBOL_VALID | SYMBOL_ENUM | enums[name.as_str()],
            ParamKind::SoftEnum(name) => {
                SYMBOL_VALID | SYMBOL_SOFT_ENUM | soft_enums[name.as_str()]
            }
            ParamKind::Postfix(postfix) => SYMBOL_POSTFIX | postfixes[postfix.as_str()],
            ParamKind::Chained(name) => SYMBOL_VALID | SYMBOL_CHAINED | chained[name.as_str()],
        }
    };
    let strings = |list: &[String]| list.iter().map(|s| s.as_str().into()).collect::<Vec<_>>();

    AvailableCommandsPacket {
        enum_values: strings(&syntax.enum_values),
        chained_subcommand_values: strings(&syntax.chained_subcommand_values),
        post_fixes: strings(&syntax.postfixes),
        enum_data: syntax
            .enums
            .iter()
            .map(|e| AvailableCommandsPacketPayloadEnumData {
                name: e.name.as_str().into(),
                values: e.values.iter().map(|v| values[v.as_str()]).collect(),
            })
            .collect(),
        chained_subcommand_data: syntax
            .chained_subcommands
            .iter()
            .map(|c| AvailableCommandsPacketPayloadChainedSubcommandData {
                name: c.name.as_str().into(),
                sub_commandvalues: c
                    .values
                    .iter()
                    .map(
                        |v| AvailableCommandsPacketPayloadChainedSubcommandRelationship {
                            sub_command_first_value: chained_values[v.value.as_str()],
                            sub_command_second_value: enums[v.enum_name.as_str()],
                        },
                    )
                    .collect(),
            })
            .collect(),
        commands: syntax
            .commands
            .iter()
            .map(|c| AvailableCommandsPacketCommandDatajson {
                name: c.name.as_str().into(),
                description: c.description.as_str().into(),
                flags: c.flags,
                permission_level: c.permission.as_str().into(),
                alias_enum: c
                    .aliases
                    .as_ref()
                    .map_or(-1, |name| enums[name.as_str()] as i32),
                command_data_chained_subcommand_indexes: c
                    .chained_subcommands
                    .iter()
                    .map(|name| chained[name.as_str()])
                    .collect(),
                overloads: c
                    .overloads
                    .iter()
                    .map(|o| AvailableCommandsPacketPayloadOverloadData {
                        is_chaining: o.chaining,
                        parameter_data: o
                            .params
                            .iter()
                            .map(|p| AvailableCommandsPacketPayloadParamData {
                                name: p.name.as_str().into(),
                                parse_symbol: symbol(&p.kind),
                                is_optional: p.optional,
                                options: p.options,
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
        soft_enums: syntax
            .soft_enums
            .iter()
            .map(|e| AvailableCommandsPacketPayloadSoftEnumData {
                enum_name: e.name.as_str().into(),
                enum_options: strings(&e.values),
            })
            .collect(),
        constraints: syntax
            .constraints
            .iter()
            .map(|c| AvailableCommandsPacketPayloadConstrainedValueData {
                enum_value_symbol: values[c.value.as_str()],
                enum_symbol: enums[c.enum_name.as_str()],
                constraint_indices: c.constraints.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::bedrock::codec::BedrockCodec;
    use sha2::{Digest, Sha256};

    /// `AvailableCommandsPacket` id; the capture files are header + payload.
    const PACKET_ID: u8 = 76;

    fn packet_bytes(packet: &AvailableCommandsPacket) -> Vec<u8> {
        let mut bytes = vec![PACKET_ID];
        packet.encode(&mut bytes).expect("encode");
        bytes
    }

    /// The SHA-256 the committed bds-extractor manifest records for a capture.
    fn recorded_sha256(permission: &str) -> String {
        let path = format!(
            "{}/../bds-extractor/captures/1.26.51/{permission}/manifest.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let wanted = format!("commands/{permission}.bin");
        manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == wanted.as_str())
            .and_then(|entry| entry["sha256"].as_str())
            .unwrap()
            .to_owned()
    }

    #[test]
    fn join_packet_is_byte_identical_to_the_bds_capture_for_member_and_operator() {
        let schema = CommandSchema::vanilla();
        let bytes = packet_bytes(schema.packet());
        let ours = format!("{:x}", Sha256::digest(&bytes));
        for permission in ["member", "operator"] {
            assert_eq!(
                ours,
                recorded_sha256(permission),
                "{permission} AvailableCommands differs from the BDS capture"
            );
        }
        // With the shared capture cache present, also compare the bytes so a
        // regression reports where they first differ.
        let cache = std::path::Path::new(
            "C:/Users/jvigu/orca/bds-runtime/captures/1.26.51/20261025T172246Z/operator/commands/operator.bin",
        );
        if let Ok(capture) = std::fs::read(cache) {
            let first_difference = bytes.iter().zip(&capture).position(|(a, b)| a != b);
            assert_eq!(first_difference, None);
            assert_eq!(bytes.len(), capture.len());
        }
    }

    #[test]
    fn names_and_aliases_resolve_case_insensitively() {
        let schema = CommandSchema::vanilla();
        assert_eq!(schema.command("TP").unwrap().name, "teleport");
        assert_eq!(schema.command("msg").unwrap().name, "tell");
        assert_eq!(schema.command("w").unwrap().name, "tell");
        assert_eq!(schema.command("whitelist").unwrap().name, "allowlist");
        assert!(schema.command("help").is_none());
        assert_eq!(schema.enum_value("GameMode", "Creative"), Some("creative"));
        assert_eq!(
            schema.enum_value("Item", "minecraft:WHEAT"),
            Some("minecraft:wheat")
        );
        assert_eq!(schema.enum_value("GameMode", "hardcore"), None);
    }
}
