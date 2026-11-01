//! Actual scoped component coverage for native infinite and retained mechanics.

use std::cell::RefCell;
use unastar_api::{
    AccessError, EffectApplication, EffectKind, Handler, InfiniteEffectApplication, LogLevel,
    PlayerContext, PlayerHandle, QuitDisposition, QuitEvent, WorldTx, commands, host,
};

struct Mechanics;
unastar_api::export_handler!(Mechanics);
thread_local! {
    static TARGET: RefCell<Option<PlayerHandle>> = const { RefCell::new(None) };
    static DESTINATION: RefCell<Option<PlayerHandle>> = const { RefCell::new(None) };
}
/// Keep fixture failures as ordinary component traps without WASI stream dependencies.
fn check(value: bool) {
    if !value {
        core::arch::wasm32::unreachable()
    }
}
fn ok<T, E>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(_) => core::arch::wasm32::unreachable(),
    }
}
fn infinite(kind: EffectKind, amplifier: u8) -> InfiniteEffectApplication {
    InfiniteEffectApplication {
        kind,
        amplifier,
        ambient: true,
        show_particles: false,
    }
}
fn log(message: &str) {
    host::log(LogLevel::Info, message);
}

impl Handler for Mechanics {
    fn on_load() {
        ok(commands::register_root(&commands::Root {
            name: "probe".into(),
            description: "Inspect typed native mechanics".into(),
        }));
    }
    fn on_player_command(ctx: &mut PlayerContext<'_>, line: String, _: Option<String>) {
        ctx.cancel();
        match line.trim_start_matches('/') {
            "probe infinite" => {
                ok(ctx
                    .player()
                    .apply_infinite_effect(&infinite(EffectKind::Speed, 1), None));
                log("infinite-staged");
            }
            "probe strong" => {
                ok(ctx
                    .player()
                    .apply_infinite_effect(&infinite(EffectKind::Speed, 6), None));
            }
            "probe regeneration" => {
                ok(ctx
                    .player()
                    .apply_infinite_effect(&infinite(EffectKind::Regeneration, 5), None));
            }
            "probe finite" => {
                ok(ctx.player().apply_effect(
                    &EffectApplication {
                        kind: EffectKind::Speed,
                        amplifier: 1,
                        duration_ticks: 5,
                        ambient: false,
                        show_particles: true,
                    },
                    None,
                ));
            }
            "probe remove" => {
                ok(ctx.player().remove_effect(EffectKind::Speed));
            }
            "probe instant" => {
                for kind in [
                    EffectKind::InstantHealth,
                    EffectKind::InstantDamage,
                    EffectKind::Saturation,
                ] {
                    check(matches!(
                        ctx.player().apply_infinite_effect(&infinite(kind, 0), None),
                        Err(AccessError::InvalidArgument)
                    ));
                }
                log("instant-rejected");
            }
            "probe fall" => match ctx.player().mechanics() {
                Ok(state) => match state.fall_distance {
                    Some(value) => log(&format!("fall:{value}")),
                    None => log("fall:none"),
                },
                Err(AccessError::Unavailable) => log("fall:unavailable"),
                Err(_) => check(false),
            },
            "probe remember" => {
                TARGET.with(|target| *target.borrow_mut() = Some(ok(ctx.player().handle())));
            }
            "probe destination" => {
                DESTINATION.with(|target| *target.borrow_mut() = Some(ok(ctx.player().handle())));
            }
            "probe denied" => {
                check(matches!(
                    ctx.player()
                        .apply_infinite_effect(&infinite(EffectKind::Speed, 1), None),
                    Err(AccessError::PermissionDenied)
                ));
                log("denied");
            }
            "probe trap" => {
                ok(ctx
                    .player()
                    .apply_infinite_effect(&infinite(EffectKind::Speed, 1), None));
                check(false);
            }
            _ => check(false),
        }
    }
    fn on_player_quit(tx: &WorldTx<'_>, event: QuitEvent) -> QuitDisposition {
        let target =
            TARGET.with(|target| target.borrow().as_ref().map(|target| ok(target.identity())));
        if target.is_some_and(|target| {
            target.uuid == event.identity.uuid
                && target.connection_id == event.identity.connection_id
        }) {
            ok(ok(tx.world()).schedule(71));
            QuitDisposition::RetainEntity
        } else {
            QuitDisposition::Remove
        }
    }
    fn on_world_task(tx: &WorldTx<'_>, _: u64, callback: u64) {
        check(callback == 71);
        TARGET.with(|target| {
            let target = target.borrow();
            let handle = target.as_ref().unwrap();
            let player = match tx.player(handle) {
                Ok(player) => player,
                Err(_) => {
                    log("stale-retained");
                    return;
                }
            };
            let availability = ok(player.availability());
            match player.apply_infinite_effect(&infinite(EffectKind::Resistance, 2), None) {
                Ok(_) => {
                    check(availability == unastar_api::PlayerAvailability::Retained);
                    log("retained-staged");
                }
                Err(AccessError::Unavailable) => {
                    log("retained-unavailable");
                    return;
                }
                Err(_) => check(false),
            }
            ok(player.set_velocity(unastar_api::Vec3 {
                x: 0.25,
                y: 0.5,
                z: -0.25,
            }));
            ok(player.set_game_mode(unastar_api::GameMode::Adventure));
            ok(player.set_mode_override(Some(unastar_api::GameMode::Creative)));
            ok(player.set_flight_override(Some(true)));
            DESTINATION.with(|destination| {
                if let Some(handle) = destination.borrow().as_ref() {
                    let destination = ok(tx.player(handle));
                    ok(destination
                        .apply_infinite_effect(&infinite(EffectKind::Speed, 1), Some(&player)));
                    log("retained-source-staged");
                }
            });
        });
    }
}
