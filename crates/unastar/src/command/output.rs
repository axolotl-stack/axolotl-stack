//! Command feedback in BDS's `CommandOutputPacket` shape.
//!
//! Messages carry resource-pack translation keys and parameters, so the client
//! renders the same text BDS produces. The server console renders them with
//! the vanilla `en_US.lang` instead.

use jolyne::valentine::types::{
    CommandOriginDatajson, CommandOutputMessagejson, CommandOutputjson, EnumsTextPacketType,
    TextPacketBody, TextPacketPayloadMessageOnly,
};
use jolyne::valentine::{CommandOutputPacket, TextPacket};

/// One feedback line: a translation key (or literal text) with parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMessage {
    pub key: String,
    pub params: Vec<String>,
    pub success: bool,
}

/// The result of one command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutput {
    /// BDS's success count: how many targets/actions succeeded.
    pub success_count: u32,
    pub messages: Vec<CommandMessage>,
}

/// What an executor returns. `Err` is a failure answer; `?` on the context's
/// helpers (`ctx.entities(..)?`) returns BDS's failure message early.
pub type CommandResult = Result<CommandOutput, CommandOutput>;

/// `Ok` with one successful message (success count 1):
/// `success!("commands.give.success", item, count, names)`.
macro_rules! success {
    ($key:expr $(, $param:expr)* $(,)?) => {
        Ok($crate::command::CommandOutput::success($key, vec![$($param.to_string()),*]))
    };
}
pub(crate) use success;

/// `Err` with one failure message: `return failure!("commands.generic.noTargetMatch")`.
macro_rules! failure {
    ($key:expr $(, $param:expr)* $(,)?) => {
        Err($crate::command::CommandOutput::failure($key, vec![$($param.to_string()),*]))
    };
}
pub(crate) use failure;

impl CommandOutput {
    /// A single successful message with success count 1.
    pub fn success(key: impl Into<String>, params: Vec<String>) -> Self {
        let mut output = Self::default();
        output.add_success(key, params);
        output
    }

    /// A single failure message with success count 0.
    pub fn failure(key: impl Into<String>, params: Vec<String>) -> Self {
        let mut output = Self::default();
        output.add_failure(key, params);
        output
    }

    /// Adds a successful message and counts it.
    pub fn add_success(&mut self, key: impl Into<String>, params: Vec<String>) {
        self.success_count += 1;
        self.messages.push(CommandMessage {
            key: key.into(),
            params,
            success: true,
        });
    }

    /// Adds a successful message without changing the success count.
    pub fn add_message(&mut self, key: impl Into<String>, params: Vec<String>) {
        self.messages.push(CommandMessage {
            key: key.into(),
            params,
            success: true,
        });
    }

    pub fn add_failure(&mut self, key: impl Into<String>, params: Vec<String>) {
        self.messages.push(CommandMessage {
            key: key.into(),
            params,
            success: false,
        });
    }

    /// Answer for a command BDS offers whose game system unastar does not have yet.
    /// The text is literal (not a key), so the client shows it as written.
    pub fn not_implemented(command: &str) -> Self {
        Self::failure(
            format!("/{command} is not implemented in unastar yet"),
            vec![],
        )
    }

    /// `commands.generic.noTargetMatch`.
    pub fn no_targets() -> Self {
        Self::failure("commands.generic.noTargetMatch", vec![])
    }

    /// Sets the success count (BDS counts affected targets: `/kill` of nine
    /// mobs reports 9 with one message).
    pub fn with_count(mut self, success_count: usize) -> Self {
        self.success_count = success_count as u32;
        self
    }

    pub fn is_success(&self) -> bool {
        self.success_count > 0
    }

    /// The packet sent back to the requesting player; the origin echoes the request's.
    pub fn to_packet(&self, origin: CommandOriginDatajson) -> CommandOutputPacket {
        CommandOutputPacket {
            origin_data: origin,
            output: CommandOutputjson {
                output_type: "alloutput".into(),
                success_count: self.success_count,
                output_messages: self
                    .messages
                    .iter()
                    .map(|m| CommandOutputMessagejson {
                        message_id: m.key.as_str().into(),
                        successful: m.success,
                        parameters: m.params.iter().map(|p| p.as_str().into()).collect(),
                    })
                    .collect(),
                data_set: None,
            },
        }
    }

    /// English text for the server console, one line per message.
    pub fn to_text(&self) -> Vec<String> {
        let lang = unastar_data::lang::en_us();
        self.messages
            .iter()
            .map(|m| lang.format(&m.key, &m.params))
            .collect()
    }
}

/// The `Textobject` message BDS sends a command's target alongside the
/// command output (`gameMode.changed`, `commands.give.successRecipient`,
/// `commands.tp.successVictim`): a rawtext translation with text parameters.
pub fn translation_text(key: &str, params: &[String]) -> TextPacket {
    let with: Vec<serde_json::Value> = params
        .iter()
        .map(|text| serde_json::json!({ "text": text }))
        .collect();
    let rawtext = serde_json::json!({
        "rawtext": [{ "translate": key, "with": { "rawtext": with } }]
    });
    TextPacket {
        localize: false,
        body: TextPacketBody::MessageOnly(TextPacketPayloadMessageOnly {
            message_type: EnumsTextPacketType::Textobject,
            message: format!("{rawtext}\n").into(),
        }),
        senders_xuid: "".into(),
        platform_id: "".into(),
        filtered_message: None,
    }
}
