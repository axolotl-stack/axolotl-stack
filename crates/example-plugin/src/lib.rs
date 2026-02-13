use unastar_api::{BlockPos, LogLevel, Player, Plugin, Vec3, host};

struct ExamplePlugin;

impl Plugin for ExamplePlugin {
    fn on_load() {
        host::log(LogLevel::Info, "Example plugin loaded!");
    }

    fn on_tick(tick_id: u64) {
        if tick_id % 100 == 0 {
            let count = host::get_player_count();
            host::log(
                LogLevel::Info,
                &format!("Tick {}: {} players online", tick_id, count),
            );
        }
    }

    fn on_player_join(player: &Player, username: String) {
        host::log(LogLevel::Info, &format!("Player {} joined!", username));
        player.send_message(&format!("Welcome, {}!", username));
    }

    fn on_player_quit(player: &Player) {
        let name = player.get_name();
        host::log(LogLevel::Info, &format!("Player {} quit", name));
    }

    fn on_player_chat(player: &Player, message: String) -> bool {
        if message == "!hello" {
            player.send_message("Hello from Component Model plugin!");
            return true;
        }

        if message == "!up" {
            let pos = player.get_position();
            player.teleport(Vec3 {
                x: pos.x,
                y: pos.y + 5.0,
                z: pos.z,
            });
            player.send_message("Whoosh!");
            return true;
        }

        if message == "!spawn" {
            let spawn = host::get_spawn();
            player.teleport(spawn);
            player.send_message("Teleported to spawn!");
            return true;
        }

        if message == "!players" {
            let players = host::list_players();
            player.send_message(&format!("Online: {}", players.join(", ")));
            return true;
        }

        if message.starts_with("!cancel") {
            return false;
        }

        true
    }

    fn on_block_break(player: &Player, pos: BlockPos, _block_id: u32) -> bool {
        if pos.x == 0 && pos.z == 0 && pos.y < 20 {
            player.send_message("Cannot break blocks at spawn!");
            return false;
        }
        true
    }
}

unastar_api::export_plugin!(ExamplePlugin);
