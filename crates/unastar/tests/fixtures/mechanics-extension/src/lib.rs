//! Actual scoped component coverage for native infinite and retained mechanics.

use std::cell::RefCell;
use unastar_api::{
    AccessError, EffectApplication, EffectKind, FoodPolicy, Handler, InfiniteEffectApplication,
    LogLevel, PlayerContext, PlayerHandle, QuitDisposition, QuitEvent, WorldTx, commands, host,
};

struct Mechanics;
unastar_api::export_handler!(Mechanics);
thread_local! {
    static TARGET: RefCell<Option<PlayerHandle>> = const { RefCell::new(None) };
    static DESTINATION: RefCell<Option<PlayerHandle>> = const { RefCell::new(None) };
    static VITALS: RefCell<bool> = const { RefCell::new(false) };
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
/// Exercise native food processing without creating replacement guest-side timers.
fn no_food_processing() -> FoodPolicy {
    FoodPolicy {
        exhaustion: false,
        food_drain: false,
        natural_regeneration: false,
        starvation: false,
    }
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
            "probe heal" => {
                let before = ok(ctx.player().vitals());
                check(matches!(
                    ok(ctx.player().heal(1.25)),
                    unastar_api::MechanicsAcknowledgement::Staged
                ));
                let after = ok(ctx.player().vitals());
                check(before.health == after.health);
                log("heal-staged");
            }
            "probe heal-clamp" => {
                ok(ctx.player().heal(f32::MAX));
            }
            "probe heal-invalid" => {
                for amount in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                    check(matches!(
                        ctx.player().heal(amount),
                        Err(AccessError::InvalidArgument)
                    ));
                }
                log("invalid-heal-rejected");
            }
            "probe food-off" => {
                ok(ctx.player().set_food_policy(Some(no_food_processing())));
            }
            "probe food-on" => {
                ok(ctx.player().set_food_policy(None));
            }
            "probe food-no-drain" => {
                ok(ctx.player().set_food_policy(Some(FoodPolicy {
                    exhaustion: true,
                    food_drain: false,
                    natural_regeneration: false,
                    starvation: false,
                })));
            }
            "probe vitals-unavailable" => {
                check(matches!(
                    ctx.player().vitals(),
                    Err(AccessError::Unavailable)
                ));
                log("vitals-unavailable");
            }
            "probe vitals" => {
                let value = ok(ctx.player().vitals());
                log(&format!(
                    "vitals:{},{},{},{},{},{},{}",
                    value.health,
                    value.max_health,
                    value.absorption,
                    value.food_level,
                    value.saturation,
                    value.exhaustion,
                    value.food_policy_override.is_some()
                ));
            }
            "probe vitals-denied" => {
                check(matches!(
                    ctx.player().heal(1.25),
                    Err(AccessError::PermissionDenied)
                ));
                check(matches!(
                    ctx.player().set_food_policy(Some(no_food_processing())),
                    Err(AccessError::PermissionDenied)
                ));
                log("vitals-denied");
            }
            "probe vitals-trap" => {
                ok(ctx.player().heal(1.25));
                ok(ctx.player().set_food_policy(Some(no_food_processing())));
                check(false);
            }
            "probe remember-vitals" => {
                TARGET.with(|target| *target.borrow_mut() = Some(ok(ctx.player().handle())));
                VITALS.with(|value| *value.borrow_mut() = true);
            }
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
            if VITALS.with_borrow(|value| *value) {
                match player.heal(1.25) {
                    Ok(_) => {
                        check(availability == unastar_api::PlayerAvailability::Retained);
                        ok(player.set_food_policy(Some(no_food_processing())));
                        log("retained-vitals-staged");
                    }
                    Err(AccessError::Unavailable) => log("retained-vitals-unavailable"),
                    Err(_) => check(false),
                }
                return;
            }
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
