use unastar_api::{plugin, GameContext, Plugin, PluginAction, PluginEvent};

#[plugin]
#[derive(Default)]
pub struct MyPlugin;

impl Plugin for MyPlugin {
    fn on_tick(&mut self, events: Vec<PluginEvent>, ctx: &GameContext) -> Vec<PluginAction> {
        let mut actions = Vec::new();

        for event in events {
            match event {
                PluginEvent::PlayerChat { player, message } => {
                    // Check for commands
                    if message == "!whereami" {
                        // Query player info using the handle
                        if let Some(info) = ctx.player(player) {
                            let msg = format!(
                                "Hello {}! You are at ({:.1}, {:.1}, {:.1})",
                                info.name, info.position.x, info.position.y, info.position.z
                            );

                            // Send message back using UUID
                            actions.push(PluginAction::SendMessage {
                                player_id: info.uuid,
                                message: msg,
                            });
                        }
                    } else if message == "!spawn" {
                        if let Some(info) = ctx.player(player) {
                            if let Some(spawn) = ctx.world_spawn() {
                                actions.push(PluginAction::Teleport {
                                    player_id: info.uuid,
                                    position: spawn,
                                });
                            }
                        }
                    } else if message == "!bench_rust" {
                        let start = std::time::Instant::now();
                        for _ in 0..100 {
                            let _ = ctx.world_spawn();
                        }
                        let duration = start.elapsed();
                        actions.push(PluginAction::Log {
                            level: unastar_api::LogLevel::Info,
                            message: format!(
                                "Rust Bench: 10 world_get_spawn calls took {:?}",
                                duration
                            ),
                        });
                    }
                }
                PluginEvent::BlockBreak {
                    player: _,
                    position: (x, y, z),
                    block_id: _,
                } => {
                    // Check what block is below
                    let below_id = ctx.get_block(x, y - 1, z);
                    if below_id.0 != 0 {
                        // Found something below (demonstration query)
                    }
                }
                _ => {}
            }
        }
        actions
    }
}
