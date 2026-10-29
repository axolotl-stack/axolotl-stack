//! `output/available_commands.json` from the BDS 1.26.51 `AvailableCommands` capture.
//!
//! Input is a capture session root (the shared, untracked cache written by
//! `tools/bds-sweep/capture.py`), holding `member/commands/member.bin` and
//! `operator/commands/operator.bin`. Each file must match the SHA-256 the
//! committed bds-extractor manifest records, and both must be identical (BDS
//! sends one command list to everyone). The packet is decoded with Valentine and
//! every index is replaced by the name it refers to (see `unastar_data::commands`).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::info;
use unastar_data::commands::{
    ARTIFACT, BasicType, CaptureSource, ChainedSubcommand, ChainedSubcommandValue,
    CommandDefinition, CommandEnum, CommandOverload, CommandParam, CommandPermission,
    CommandsArtifact, EnumConstraint, ParamKind, SYMBOL_CHAINED, SYMBOL_ENUM, SYMBOL_POSTFIX,
    SYMBOL_SOFT_ENUM, SYMBOL_VALID,
};
use valentine::bedrock::version::v1_26_51::{
    AvailableCommandsPacket, McpePacketArgs, McpePacketData,
};

const ROLES: [&str; 2] = ["member", "operator"];

fn extractor_manifest(role: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../bds-extractor/captures/1.26.51/{role}/manifest.json"
    ))
}

/// SHA-256 the committed bds-extractor manifest records for `commands/<role>.bin`.
fn recorded_sha256(role: &str) -> miette::Result<String> {
    let path = extractor_manifest(role);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| miette::miette!("read {}: {e}", path.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| miette::miette!("{}: {e}", path.display()))?;
    let wanted = format!("commands/{role}.bin");
    manifest["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["path"] == wanted.as_str())
        .and_then(|entry| entry["sha256"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| miette::miette!("{} records no {wanted}", path.display()))
}

/// Prefixes the capture's header+payload with its varint length, the frame
/// Valentine's packet decoder expects.
fn framed(bytes: &[u8]) -> bytes::Bytes {
    let mut out = Vec::with_capacity(bytes.len() + 5);
    let mut length = bytes.len() as u32;
    while length >= 0x80 {
        out.push(length as u8 | 0x80);
        length >>= 7;
    }
    out.push(length as u8);
    out.extend_from_slice(bytes);
    out.into()
}

pub fn write_commands(capture_root: &Path, output_dir: &Path) -> miette::Result<()> {
    let mut captures = Vec::new();
    let mut packet_bytes: Option<Vec<u8>> = None;
    for role in ROLES {
        let path = capture_root
            .join(role)
            .join("commands")
            .join(format!("{role}.bin"));
        let bytes =
            std::fs::read(&path).map_err(|e| miette::miette!("read {}: {e}", path.display()))?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let recorded = recorded_sha256(role)?;
        if sha256 != recorded {
            return Err(miette::miette!(
                "{} has SHA-256 {sha256}, the committed manifest records {recorded}",
                path.display()
            ));
        }
        if let Some(previous) = &packet_bytes
            && *previous != bytes
        {
            return Err(miette::miette!(
                "member and operator AvailableCommands differ; the artefact assumes one list"
            ));
        }
        packet_bytes = Some(bytes);
        captures.push(CaptureSource {
            permission: role.to_owned(),
            path: format!("commands/{role}.bin"),
            manifest: format!("crates/bds-extractor/captures/1.26.51/{role}/manifest.json"),
            sha256,
        });
    }
    let bytes = packet_bytes.expect("two roles");
    let (_, data) = McpePacketData::decode_inner_bytes(&mut framed(&bytes), McpePacketArgs)
        .map_err(|e| miette::miette!("decode AvailableCommands: {e:?}"))?;
    let McpePacketData::AvailableCommandsPacket(packet) = data else {
        return Err(miette::miette!("capture is not an AvailableCommandsPacket"));
    };
    let artifact = from_packet(&packet, captures)?;
    artifact.validate().map_err(|e| miette::miette!("{e}"))?;

    let mut json = serde_json::to_string_pretty(&artifact)
        .map_err(|e| miette::miette!("serialize commands: {e}"))?;
    json.push('\n');
    let path = output_dir.join(ARTIFACT);
    std::fs::write(&path, json).map_err(|e| miette::miette!("write {}: {e}", path.display()))?;
    info!(
        "Wrote {} ({} commands, {} enums, {} soft enums)",
        path.display(),
        artifact.commands.len(),
        artifact.enums.len(),
        artifact.soft_enums.len()
    );
    Ok(())
}

fn lookup<'a, T>(list: &'a [T], index: usize, what: &str) -> miette::Result<&'a T> {
    list.get(index)
        .ok_or_else(|| miette::miette!("{what} index {index} out of range ({})", list.len()))
}

fn strings<S: ToString>(list: &[S]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

fn from_packet(
    packet: &AvailableCommandsPacket,
    captures: Vec<CaptureSource>,
) -> miette::Result<CommandsArtifact> {
    let enum_values = strings(&packet.enum_values);
    let chained_subcommand_values = strings(&packet.chained_subcommand_values);
    let postfixes = strings(&packet.post_fixes);

    let enums = packet
        .enum_data
        .iter()
        .map(|e| {
            Ok(CommandEnum {
                name: e.name.to_string(),
                values: e
                    .values
                    .iter()
                    .map(|&i| lookup(&enum_values, i as usize, "enum value").cloned())
                    .collect::<miette::Result<_>>()?,
            })
        })
        .collect::<miette::Result<Vec<_>>>()?;
    let enum_name = |index: usize| lookup(&enums, index, "enum").map(|e| e.name.clone());
    let soft_enums: Vec<CommandEnum> = packet
        .soft_enums
        .iter()
        .map(|e| CommandEnum {
            name: e.enum_name.to_string(),
            values: strings(&e.enum_options),
        })
        .collect();
    let chained_subcommands = packet
        .chained_subcommand_data
        .iter()
        .map(|c| {
            Ok(ChainedSubcommand {
                name: c.name.to_string(),
                values: c
                    .sub_commandvalues
                    .iter()
                    .map(|v| {
                        Ok(ChainedSubcommandValue {
                            value: lookup(
                                &chained_subcommand_values,
                                v.sub_command_first_value as usize,
                                "chained subcommand value",
                            )?
                            .clone(),
                            enum_name: enum_name(v.sub_command_second_value as usize)?,
                        })
                    })
                    .collect::<miette::Result<_>>()?,
            })
        })
        .collect::<miette::Result<Vec<_>>>()?;

    let param_kind = |symbol: u32| -> miette::Result<ParamKind> {
        let index = (symbol & 0xffff) as usize;
        let flags = symbol & !0xffff;
        Ok(match flags {
            f if f == SYMBOL_VALID => ParamKind::Basic(
                BasicType::from_symbol(symbol & 0xffff)
                    .ok_or_else(|| miette::miette!("unknown basic parse symbol {symbol:#x}"))?,
            ),
            f if f == SYMBOL_VALID | SYMBOL_ENUM => ParamKind::Enum(enum_name(index)?),
            f if f == SYMBOL_VALID | SYMBOL_SOFT_ENUM => {
                ParamKind::SoftEnum(lookup(&soft_enums, index, "soft enum")?.name.clone())
            }
            f if f == SYMBOL_POSTFIX => {
                ParamKind::Postfix(lookup(&postfixes, index, "postfix")?.clone())
            }
            f if f == SYMBOL_VALID | SYMBOL_CHAINED => ParamKind::Chained(
                lookup(&chained_subcommands, index, "chained subcommand")?
                    .name
                    .clone(),
            ),
            _ => return Err(miette::miette!("unknown parse symbol flags {symbol:#x}")),
        })
    };

    let commands = packet
        .commands
        .iter()
        .map(|c| {
            let permission =
                CommandPermission::from_packet(&c.permission_level).ok_or_else(|| {
                    miette::miette!("{}: unknown permission {}", c.name, c.permission_level)
                })?;
            Ok(CommandDefinition {
                name: c.name.to_string(),
                description: c.description.to_string(),
                flags: c.flags,
                permission,
                aliases: match c.alias_enum {
                    -1 => None,
                    i => Some(enum_name(
                        usize::try_from(i).map_err(|_| miette::miette!("alias enum {i}"))?,
                    )?),
                },
                chained_subcommands: c
                    .command_data_chained_subcommand_indexes
                    .iter()
                    .map(|&i| {
                        Ok(
                            lookup(&chained_subcommands, i as usize, "chained subcommand")?
                                .name
                                .clone(),
                        )
                    })
                    .collect::<miette::Result<_>>()?,
                overloads: c
                    .overloads
                    .iter()
                    .map(|o| {
                        Ok(CommandOverload {
                            chaining: o.is_chaining,
                            params: o
                                .parameter_data
                                .iter()
                                .map(|p| {
                                    Ok(CommandParam {
                                        name: p.name.to_string(),
                                        kind: param_kind(p.parse_symbol)?,
                                        optional: p.is_optional,
                                        options: p.options,
                                    })
                                })
                                .collect::<miette::Result<_>>()?,
                        })
                    })
                    .collect::<miette::Result<_>>()?,
            })
        })
        .collect::<miette::Result<Vec<_>>>()?;

    let constraints = packet
        .constraints
        .iter()
        .map(|c| {
            Ok(EnumConstraint {
                value: lookup(&enum_values, c.enum_value_symbol as usize, "enum value")?.clone(),
                enum_name: enum_name(c.enum_symbol as usize)?,
                constraints: c.constraint_indices.clone(),
            })
        })
        .collect::<miette::Result<Vec<_>>>()?;

    Ok(CommandsArtifact {
        bds_version: "1.26.51".into(),
        protocol: 2193,
        captures,
        enum_values,
        chained_subcommand_values,
        postfixes,
        enums,
        chained_subcommands,
        commands,
        soft_enums,
        constraints,
    })
}

const GAME_RULES_FIXTURE: &str = "member/join/00037-GameRulesChangedPacket.bin";

/// `output/game_rules.json` from the committed join `GameRulesChanged` fixture.
pub fn write_game_rules(output_dir: &Path) -> miette::Result<()> {
    use unastar_data::game_rules::{GameRuleDefault, GameRuleValue, GameRulesArtifact};
    use valentine::bedrock::version::v1_26_51::GameRuleRuleValue;

    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bds-extractor/captures/1.26.51");
    let path = root.join(GAME_RULES_FIXTURE);
    let bytes =
        std::fs::read(&path).map_err(|e| miette::miette!("read {}: {e}", path.display()))?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(extractor_manifest("member"))
            .map_err(|e| miette::miette!("read member manifest: {e}"))?,
    )
    .map_err(|e| miette::miette!("member manifest: {e}"))?;
    let wanted = GAME_RULES_FIXTURE.trim_start_matches("member/");
    let recorded = manifest["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["path"] == wanted)
        .and_then(|entry| entry["sha256"].as_str())
        .ok_or_else(|| miette::miette!("member manifest records no {wanted}"))?;
    if recorded != sha256 {
        return Err(miette::miette!(
            "{} has SHA-256 {sha256}, the manifest records {recorded}",
            path.display()
        ));
    }
    let (_, data) = McpePacketData::decode_inner_bytes(&mut framed(&bytes), McpePacketArgs)
        .map_err(|e| miette::miette!("decode GameRulesChanged: {e:?}"))?;
    let McpePacketData::GameRulesChangedPacket(packet) = data else {
        return Err(miette::miette!("fixture is not a GameRulesChangedPacket"));
    };
    let rules = packet
        .rule_data
        .rules_list
        .iter()
        .map(|rule| {
            let value = match rule.rule_value {
                GameRuleRuleValue::Bool(v) => GameRuleValue::Bool(v),
                GameRuleRuleValue::Int32(v) => GameRuleValue::Int(v),
                GameRuleRuleValue::Float(v) => GameRuleValue::Float(v),
                GameRuleRuleValue::Empty0 => {
                    return Err(miette::miette!("rule {} has no value", rule.rule_name));
                }
            };
            Ok(GameRuleDefault {
                name: rule.rule_name.to_string(),
                editable: rule.rule_can_be_modified,
                value,
            })
        })
        .collect::<miette::Result<Vec<_>>>()?;
    let artifact = GameRulesArtifact {
        bds_version: "1.26.51".into(),
        protocol: 2193,
        capture: format!("crates/bds-extractor/captures/1.26.51/{GAME_RULES_FIXTURE}"),
        capture_sha256: sha256,
        rules,
    };
    let mut json = serde_json::to_string_pretty(&artifact)
        .map_err(|e| miette::miette!("serialize game rules: {e}"))?;
    json.push('\n');
    let out = output_dir.join(unastar_data::game_rules::ARTIFACT);
    std::fs::write(&out, json).map_err(|e| miette::miette!("write {}: {e}", out.display()))?;
    info!("Wrote {} ({} rules)", out.display(), artifact.rules.len());
    Ok(())
}
