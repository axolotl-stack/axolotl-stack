//! Vanilla command syntax: the `AvailableCommandsPacket` BDS 1.26.51 sends at
//! the end of every join, as reviewable data.
//!
//! `unastar-data-gen --commands-only` decodes the captured packet and writes
//! `output/available_commands.json`. The artefact is lossless: every list keeps
//! BDS's order, and every packet index (enum value, enum, soft enum, postfix,
//! chained subcommand) is replaced by the name it points at, so the packet can
//! be rebuilt byte for byte (`unastar`'s `command::schema` does, and its tests
//! compare the result with the SHA-256 the capture manifests record).
//!
//! BDS sends the identical packet to members and operators; the client filters
//! commands by the command permission level in `UpdateAbilities`
//! (`thoughts/shared/research/bds-quirks.md`).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

/// The whole captured command syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandsArtifact {
    pub bds_version: String,
    pub protocol: u32,
    /// The capture files this artefact was generated from.
    pub captures: Vec<CaptureSource>,
    /// Packet `enum_values`, in packet order.
    pub enum_values: Vec<String>,
    /// Packet `chained_subcommand_values`, in packet order.
    pub chained_subcommand_values: Vec<String>,
    /// Packet `post_fixes` (`L` for levels, `t`/`s`/`d` for ticks/seconds/days).
    pub postfixes: Vec<String>,
    /// Packet `enum_data`; values are names from `enum_values`.
    pub enums: Vec<CommandEnum>,
    /// Packet `chained_subcommand_data`.
    pub chained_subcommands: Vec<ChainedSubcommand>,
    pub commands: Vec<CommandDefinition>,
    /// Packet `soft_enums` (values the server may update at runtime).
    pub soft_enums: Vec<CommandEnum>,
    /// Packet `constraints`.
    pub constraints: Vec<EnumConstraint>,
}

/// One capture file with its manifest digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSource {
    /// Permission of the joining player (`member` or `operator`).
    pub permission: String,
    /// Path inside the capture session (`commands/<permission>.bin`).
    pub path: String,
    /// Committed manifest that records the digest.
    pub manifest: String,
    /// SHA-256 of the packet header and payload.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnum {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainedSubcommand {
    pub name: String,
    pub values: Vec<ChainedSubcommandValue>,
}

/// A chained subcommand keyword (`as`, `at`, …) and the enum describing what follows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainedSubcommandValue {
    pub value: String,
    #[serde(rename = "enum")]
    pub enum_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDefinition {
    pub name: String,
    /// Translation key (or literal English text for a few BDS-only commands).
    pub description: String,
    pub flags: u16,
    pub permission: CommandPermission,
    /// Enum holding the command's name and aliases, if it has aliases.
    pub aliases: Option<String>,
    /// Names of `chained_subcommands` entries this command uses.
    pub chained_subcommands: Vec<String>,
    pub overloads: Vec<CommandOverload>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandOverload {
    pub chaining: bool,
    pub params: Vec<CommandParam>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandParam {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ParamKind,
    pub optional: bool,
    /// Packet parameter option bits, kept verbatim.
    pub options: u8,
}

/// An enum value that is only valid under some conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumConstraint {
    pub value: String,
    #[serde(rename = "enum")]
    pub enum_name: String,
    pub constraints: Vec<u8>,
}

/// Command permission level strings as BDS writes them in the packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandPermission {
    Any,
    GameDirectors,
    Admin,
    Host,
    Owner,
}

impl CommandPermission {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::GameDirectors => "gamedirectors",
            Self::Admin => "admin",
            Self::Host => "host",
            Self::Owner => "owner",
        }
    }

    pub fn from_packet(value: &str) -> Option<Self> {
        Some(match value {
            "any" => Self::Any,
            "gamedirectors" => Self::GameDirectors,
            "admin" => Self::Admin,
            "host" => Self::Host,
            "owner" => Self::Owner,
            _ => return None,
        })
    }
}

/// Packet parse-symbol flag: the symbol is a valid parameter type.
pub const SYMBOL_VALID: u32 = 0x10_0000;
/// Packet parse-symbol flag: the low bits index `enum_data`.
pub const SYMBOL_ENUM: u32 = 0x20_0000;
/// Packet parse-symbol flag: the low bits index `post_fixes`.
pub const SYMBOL_POSTFIX: u32 = 0x100_0000;
/// Packet parse-symbol flag: the low bits index `soft_enums`.
pub const SYMBOL_SOFT_ENUM: u32 = 0x400_0000;
/// Packet parse-symbol flag: the low bits index `chained_subcommand_data`.
pub const SYMBOL_CHAINED: u32 = 0x800_0000;

/// A parameter's type, from its packet parse symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParamKind {
    Basic(BasicType),
    Enum(String),
    SoftEnum(String),
    /// Number with a postfix (`10L`); the postfix text, not its index.
    Postfix(String),
    Chained(String),
}

macro_rules! basic_types {
    ($($variant:ident = $symbol:literal, $name:literal;)*) => {
        /// Built-in parameter types and their 1.26.51 parse symbols (low 16 bits).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum BasicType { $($variant),* }

        impl BasicType {
            pub const ALL: &[BasicType] = &[$(BasicType::$variant),*];

            pub const fn symbol(self) -> u32 {
                match self { $(Self::$variant => $symbol),* }
            }

            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),* }
            }

            pub fn from_symbol(symbol: u32) -> Option<Self> {
                match symbol { $($symbol => Some(Self::$variant),)* _ => None }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)* _ => None }
            }
        }
    };
}

basic_types! {
    Int = 1, "int";
    Float = 3, "float";
    Value = 4, "value";
    WildcardInt = 5, "wildcard_int";
    Operator = 6, "operator";
    CompareOperator = 7, "compare_operator";
    Target = 8, "target";
    WildcardTarget = 10, "wildcard_target";
    FilePath = 17, "file_path";
    IntegerRange = 23, "integer_range";
    String = 56, "string";
    BlockPosition = 64, "block_position";
    Position = 65, "position";
    Message = 68, "message";
    RawText = 70, "raw_text";
    Json = 74, "json";
    BlockStates = 84, "block_states";
    Command = 90, "command";
}

impl fmt::Display for ParamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Basic(basic) => f.write_str(basic.name()),
            Self::Enum(name) => write!(f, "enum:{name}"),
            Self::SoftEnum(name) => write!(f, "soft_enum:{name}"),
            Self::Postfix(postfix) => write!(f, "postfix:{postfix}"),
            Self::Chained(name) => write!(f, "chained:{name}"),
        }
    }
}

impl std::str::FromStr for ParamKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let kind = match value.split_once(':') {
            Some(("enum", name)) => Self::Enum(name.to_owned()),
            Some(("soft_enum", name)) => Self::SoftEnum(name.to_owned()),
            Some(("postfix", postfix)) => Self::Postfix(postfix.to_owned()),
            Some(("chained", name)) => Self::Chained(name.to_owned()),
            Some(_) => return Err(format!("unknown parameter type `{value}`")),
            None => Self::Basic(
                BasicType::from_name(value)
                    .ok_or_else(|| format!("unknown parameter type `{value}`"))?,
            ),
        };
        Ok(kind)
    }
}

impl Serialize for ParamKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ParamKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl CommandsArtifact {
    /// Checks that every name the artefact uses resolves and that names are unique,
    /// which is what makes the packet indices reconstructible.
    pub fn validate(&self) -> Result<(), String> {
        unique("enum value", self.enum_values.iter())?;
        unique("enum", self.enums.iter().map(|e| &e.name))?;
        unique("soft enum", self.soft_enums.iter().map(|e| &e.name))?;
        unique("postfix", self.postfixes.iter())?;
        unique(
            "chained subcommand",
            self.chained_subcommands.iter().map(|c| &c.name),
        )?;
        unique("command", self.commands.iter().map(|c| &c.name))?;
        let values: HashSet<&str> = self.enum_values.iter().map(String::as_str).collect();
        let enums: HashSet<&str> = self.enums.iter().map(|e| e.name.as_str()).collect();
        let soft: HashSet<&str> = self.soft_enums.iter().map(|e| e.name.as_str()).collect();
        let chained: HashSet<&str> = self
            .chained_subcommands
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        for e in &self.enums {
            for value in &e.values {
                if !values.contains(value.as_str()) {
                    return Err(format!("enum {} uses unknown value {value}", e.name));
                }
            }
        }
        for c in &self.chained_subcommands {
            for value in &c.values {
                if !self.chained_subcommand_values.contains(&value.value)
                    || !enums.contains(value.enum_name.as_str())
                {
                    return Err(format!("chained subcommand {} is unresolved", c.name));
                }
            }
        }
        for constraint in &self.constraints {
            if !values.contains(constraint.value.as_str())
                || !enums.contains(constraint.enum_name.as_str())
            {
                return Err(format!(
                    "constraint {}/{} is unresolved",
                    constraint.enum_name, constraint.value
                ));
            }
        }
        for command in &self.commands {
            if let Some(aliases) = &command.aliases
                && !enums.contains(aliases.as_str())
            {
                return Err(format!(
                    "{} uses unknown alias enum {aliases}",
                    command.name
                ));
            }
            for name in &command.chained_subcommands {
                if !chained.contains(name.as_str()) {
                    return Err(format!(
                        "{} uses unknown chained subcommand {name}",
                        command.name
                    ));
                }
            }
            for param in command.overloads.iter().flat_map(|o| &o.params) {
                let known = match &param.kind {
                    ParamKind::Basic(_) => true,
                    ParamKind::Enum(name) => enums.contains(name.as_str()),
                    ParamKind::SoftEnum(name) => soft.contains(name.as_str()),
                    ParamKind::Postfix(postfix) => self.postfixes.contains(postfix),
                    ParamKind::Chained(name) => chained.contains(name.as_str()),
                };
                if !known {
                    return Err(format!(
                        "{} parameter {} has unresolved type {}",
                        command.name, param.name, param.kind
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn command(&self, name: &str) -> Option<&CommandDefinition> {
        self.commands.iter().find(|c| c.name == name)
    }

    pub fn enum_named(&self, name: &str) -> Option<&CommandEnum> {
        self.enums.iter().find(|e| e.name == name)
    }

    pub fn soft_enum_named(&self, name: &str) -> Option<&CommandEnum> {
        self.soft_enums.iter().find(|e| e.name == name)
    }
}

fn unique<'a>(what: &str, names: impl Iterator<Item = &'a String>) -> Result<(), String> {
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(format!("duplicate {what} {name}"));
        }
    }
    Ok(())
}

/// Path of the committed artefact, relative to this crate.
pub const ARTIFACT: &str = "available_commands.json";

static VANILLA: LazyLock<CommandsArtifact> = LazyLock::new(|| {
    let artifact: CommandsArtifact =
        serde_json::from_str(include_str!("../output/available_commands.json"))
            .expect("captured command syntax");
    assert_eq!(artifact.bds_version, "1.26.51");
    assert_eq!(artifact.protocol, 2193);
    artifact
        .validate()
        .unwrap_or_else(|error| panic!("captured command syntax: {error}"));
    artifact
});

/// The captured BDS 1.26.51 command syntax, decoded once.
pub fn vanilla() -> &'static CommandsArtifact {
    &VANILLA
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_types_round_trip_through_their_names() {
        for basic in BasicType::ALL {
            let kind = ParamKind::Basic(*basic);
            assert_eq!(kind.to_string().parse::<ParamKind>(), Ok(kind));
            assert_eq!(BasicType::from_symbol(basic.symbol()), Some(*basic));
        }
        assert_eq!(
            "postfix:L".parse::<ParamKind>(),
            Ok(ParamKind::Postfix("L".into()))
        );
        assert!("blob".parse::<ParamKind>().is_err());
    }

    #[test]
    fn captured_syntax_decodes_and_resolves() {
        let syntax = vanilla();
        assert_eq!(syntax.commands.len(), 82);
        let teleport = syntax.command("teleport").expect("teleport");
        assert_eq!(teleport.permission, CommandPermission::GameDirectors);
        let aliases = syntax
            .enum_named(teleport.aliases.as_deref().expect("tp alias"))
            .expect("alias enum");
        assert_eq!(aliases.values, ["teleport", "tp"]);
        let xp = syntax.command("xp").expect("xp");
        assert_eq!(
            xp.overloads[1].params[0].kind,
            ParamKind::Postfix("l".into())
        );
    }
}
