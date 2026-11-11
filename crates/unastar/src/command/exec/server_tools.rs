//! `/reloadconfig`, `/reloadpacketlimitconfig`, `/wsserver` and `/script`:
//! server tools run from the console (owner and admin levels; an operator
//! gets `commands.generic.error.permissions`).
//!
//! Measured on BDS 1.26.51's console (`tools/bds-sweep/command_probe.py`,
//! `testdata/bds-command-probe.json`), a dedicated server without script
//! packs:
//!
//! - `reloadconfig` prints `No CDN config file found at: cdn_config.json for
//!   dedicated server` and then `commands.reloadconfig.success [config]`;
//! - `reloadpacketlimitconfig` answers `Packet limit config updated`;
//! - `wsserver <uri>` (and `wsserver out`) prints nothing;
//! - `script debugger listen|connect` answer that inbound and outbound
//!   connections are disabled, `debugger close` that the debugger closed,
//!   `profiler start` that it started and `profiler stop` (as a failure) that
//!   it stopped without saving a profile; `diagnostics startcapture` that the
//!   capture started.
//!
//! unastar has none of these systems (pack configuration, packet limit
//! files, a websocket client, a script runtime), so they answer as BDS does
//! where that answer holds without them. `script diagnostics stopcapture`,
//! which on BDS writes a capture file and names it, answers the
//! not-implemented failure.

use crate::command::prelude::*;

overloads! {
    pub enum ReloadConfigArgs for "reloadconfig" {
        0 => Reload {},
    }
}

/// The directory BDS reloads pack configurations from.
const CONFIG_DIRECTORY: &str = "config";

pub fn run_reloadconfig(ctx: &mut CommandContext<'_>, _args: ReloadConfigArgs) -> CommandResult {
    ctx.console_note(
        false,
        "No CDN config file found at: cdn_config.json for dedicated server",
    );
    success!("commands.reloadconfig.success", CONFIG_DIRECTORY)
}

overloads! {
    pub enum ReloadPacketLimitConfigArgs for "reloadpacketlimitconfig" {
        0 => Reload {},
    }
}

pub fn run_reloadpacketlimitconfig(
    _ctx: &mut CommandContext<'_>,
    _args: ReloadPacketLimitConfigArgs,
) -> CommandResult {
    success!("Packet limit config updated")
}

overloads! {
    pub enum WsServerArgs for "wsserver" {
        0 => Connect { server_uri: RawText = "serverUri" },
    }
}

/// Measured: no answer. [INFERENCE] Counted as one success.
pub fn run_wsserver(_ctx: &mut CommandContext<'_>, _args: WsServerArgs) -> CommandResult {
    Ok(CommandOutput::default().with_count(1))
}

overloads! {
    pub enum ScriptArgs for "script" {
        0 => DebuggerListen { mode: EnumValue, action: EnumValue, port: i32 },
        1 => DebuggerConnect {
            mode: EnumValue,
            action: EnumValue,
            host: Option<Text>,
            port: Option<i32>,
        },
        2 => DebuggerClose { mode: EnumValue, action: EnumValue },
        3 => ProfilerStart { mode: EnumValue, action: EnumValue },
        4 => ProfilerStop { mode: EnumValue, action: EnumValue },
        5 => DiagnosticsStart { mode: EnumValue, action: EnumValue },
        6 => DiagnosticsStop { mode: EnumValue, action: EnumValue },
    }
}

pub fn run_script(_ctx: &mut CommandContext<'_>, args: ScriptArgs) -> CommandResult {
    match args {
        ScriptArgs::DebuggerListen { .. } => failure!("script.error.debuggerListenDisabled"),
        ScriptArgs::DebuggerConnect { .. } => failure!("script.error.debuggerConnectDisabled"),
        ScriptArgs::DebuggerClose { .. } => success!("script.success.debuggerClosed"),
        ScriptArgs::ProfilerStart { .. } => success!("script.success.profilerStarted"),
        // [INFERENCE] Also without a profile started (measured after start).
        ScriptArgs::ProfilerStop { .. } => failure!("script.error.profilerStopped"),
        ScriptArgs::DiagnosticsStart { .. } => {
            success!("script.success.diagnosticsCaptureStarted")
        }
        ScriptArgs::DiagnosticsStop { .. } => Err(CommandOutput::not_implemented("script")),
    }
}

#[cfg(test)]
mod tests {
    use crate::command::exec::evidence::console_records;
    use crate::command::exec::probe::{Probe, captured_console};
    use crate::command::{CommandSource, execute};

    #[test]
    fn server_tools_answer_like_bds_on_the_console() {
        let mut probe = Probe::new();
        for command in [
            "reloadconfig",
            "reloadpacketlimitconfig",
            "wsserver ws://127.0.0.1:1",
            "wsserver out",
            "script debugger listen 19399",
            "script debugger close",
            "script debugger connect",
            "script profiler start",
            "script profiler stop",
            "script diagnostics startcapture",
        ] {
            let world = probe.h.world();
            let output = execute(world, CommandSource::server(), command);
            let mut ours = probe.notes();
            ours.extend(console_records(&output));
            assert_eq!(ours, captured_console(command), "{command}");
        }
    }
}
