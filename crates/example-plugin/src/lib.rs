//! Scoped Component Model example; build for `wasm32-wasip2`.

use unastar_api::{
    AdmissionDecision, AdmittedEvent, BlockPos, Handler, LogLevel, PlayerContext, commands, server,
};

struct ExamplePlugin;

#[unastar_api_macros::plugin]
impl Handler for ExamplePlugin {
    fn on_load() {
        server::log(LogLevel::Info, "Example component loaded");
        let _ = commands::register_root(&commands::Root {
            name: "greet".into(),
            description: "Send a greeting".into(),
        });
    }
    fn on_tick(tick_id: u64) {
        if tick_id.is_multiple_of(100)
            && let Ok(players) = server::players()
        {
            server::log(
                LogLevel::Info,
                &format!("Tick {tick_id}: {} player actors", players.len()),
            );
        }
    }
    fn on_player_admission(_ctx: &mut PlayerContext<'_>) -> AdmissionDecision {
        AdmissionDecision::Allow
    }
    fn on_player_admitted(event: AdmittedEvent) {
        server::log(
            LogLevel::Info,
            &format!("{} entered the world", event.observation.identity.name),
        );
    }
    fn on_player_command(ctx: &mut PlayerContext<'_>, line: String, _request_id: Option<String>) {
        if line.trim_start_matches('/').split_whitespace().next() == Some("greet") {
            let _ = ctx
                .player()
                .message("Hello from an advertised component command");
            ctx.cancel();
        }
    }
    fn on_player_chat(ctx: &mut PlayerContext<'_>, message: String) {
        if message == "!hello" {
            let _ = ctx
                .player()
                .message("Hello from a scoped component handler");
            ctx.cancel();
        } else if message == "!later" {
            if let Ok(handle) = ctx.player().handle() {
                let _ = handle.schedule(1);
            }
            ctx.cancel();
        } else if message.starts_with("!cancel") {
            ctx.cancel();
        }
    }
    fn on_player_task(ctx: &mut PlayerContext<'_>, _task_id: u64, callback_id: u64) {
        if callback_id == 1 {
            let _ = ctx
                .player()
                .message("Scheduled work resolved a fresh player scope");
        }
    }
    fn on_block_break(ctx: &mut PlayerContext<'_>, position: BlockPos, _block_id: u32) {
        if position.x == 0 && position.z == 0 && position.y < 20 {
            let _ = ctx.player().message("This column is protected");
            ctx.cancel();
        }
    }
}
