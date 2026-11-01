//! Actual scoped component controls for complete native state handoff.

use std::cell::RefCell;
use unastar_api::{
    ActiveStatusEffect, AdmissionDecision, AdmissionResolution, AdmittedEvent, EffectKind,
    GameMode, InventoryArea, LogLevel, Plugin, QuitDisposition, QuitEvent, StateOperation,
    StateStatus, Vec3, host, raw, server,
};

#[derive(Default)]
struct State {
    mode: u8,
    phase: u8,
    operation: Option<StateOperation>,
    image: Option<raw::PlayerStateImage>,
    source: Option<raw::SourceFence>,
    player: Option<raw::PlayerHandle>,
    world: Option<raw::WorldHandle>,
    hold: Option<raw::AdmissionHold>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

/// Logs are observations from executed guest code rather than host-side test bookkeeping.
fn log(text: &str) {
    server::log(LogLevel::Info, text);
}

/// Detached edits preserve account identity, instance metadata and proof history.
fn desired(tx: &raw::WorldTx, image: &raw::PlayerStateImage) -> raw::PlayerStateImage {
    let inventory = image.inventory().unwrap();
    let diamond = tx.item("minecraft:diamond", 3).unwrap();
    let inventory = inventory
        .with_slot(InventoryArea::Main, 0, &diamond)
        .unwrap();
    let inventory = inventory
        .with_slot(InventoryArea::EnderChest, 2, &diamond)
        .unwrap();
    let image = image.with_inventory(&inventory).unwrap();
    let mut values = image.values().unwrap();
    values.health = 7.0;
    values.max_health = 30.0;
    values.absorption = 4.0;
    values.food = 9;
    values.saturation = 2.0;
    values.velocity = Vec3 {
        x: 0.25,
        y: 0.5,
        z: -0.25,
    };
    values.air_current = 17;
    values.on_fire_ticks = 40;
    values.burning_remaining = 30;
    values.burning_next = 6;
    values.base_mode = GameMode::Survival;
    values.mode_override = Some(GameMode::Creative);
    values.flight_override = Some(true);
    values.age_ticks = 77;
    values.cold_exposure = 0.3;
    values.effects = vec![ActiveStatusEffect {
        kind: EffectKind::Speed,
        amplifier: 1,
        duration_ticks: 93,
        ambient: true,
        show_particles: false,
        infinite: false,
    }];
    image.with_values(&values).unwrap()
}

struct Fixture;
impl Plugin for Fixture {
    fn on_load() {
        std::panic::set_hook(Box::new(|info| {
            server::log(LogLevel::Error, &info.to_string())
        }));
        STATE.with_borrow_mut(|state| {
            state.mode = host::configuration().first().copied().unwrap_or(0)
        });
    }

    fn on_player_admission(tx: &raw::WorldTx, player: &raw::Player) -> AdmissionDecision {
        STATE.with_borrow_mut(|state| match state.mode {
            5 => AdmissionDecision::Reject("Controlled rejection".into()),
            4 | 6 | 7 | 23 if !matches!(state.mode, 7 | 23) || state.phase > 0 => {
                state.hold = Some(player.defer_admission().unwrap());
                state.player = Some(player.to_handle().unwrap());
                state.world = Some(tx.world().unwrap());
                if state.mode != 6 {
                    drop(state.world.as_ref().unwrap().schedule(1).unwrap());
                }
                AdmissionDecision::Deferred
            }
            _ => AdmissionDecision::Allow,
        })
    }

    fn on_player_jump(tx: &raw::WorldTx, player: &raw::Player) {
        STATE.with_borrow_mut(|state| {
            if state.mode == 23 && state.phase == 0 {
                let _reference = player.archive("held-source").unwrap();
                state.phase = 1;
                log("historical");
                return;
            }
            if state.mode == 18 {
                assert!(matches!(
                    player.state_image(),
                    Err(unastar_api::AccessError::PermissionDenied)
                ));
                log("denied");
                return;
            }
            if matches!(state.mode, 12..=14) {
                let reference = unastar_api::archives::lookup("held-source")
                    .unwrap()
                    .unwrap();
                let archive = unastar_api::archives::read(&reference).unwrap();
                if state.mode == 14 {
                    let image = archive.state_image().unwrap();
                    let candidate = desired(tx, &image);
                    let expected = player.state_image().unwrap();
                    let operation = player
                        .submit_state("restore", &expected, &candidate, None)
                        .unwrap();
                    let StateStatus::Committed(commit) = operation.status().unwrap() else {
                        panic!("canonical receipt replay")
                    };
                    assert!(commit.already_committed);
                    assert!(!commit.live_applied);
                    log("replayed");
                    return;
                }
                let source = archive.source_fence(tx).unwrap();
                if state.mode == 13 {
                    assert!(source.is_none());
                    log("source-absent");
                } else {
                    assert!(matches!(
                        source.unwrap().status().unwrap(),
                        raw::SourceFenceStatus::Consumed
                    ));
                    log("source-reacquired");
                }
                return;
            }
            if matches!(state.mode, 7 | 22) && state.phase == 0 {
                let image = player.state_image().unwrap();
                let source = player.fence_state(&image).unwrap();
                assert!(matches!(
                    source.status().unwrap(),
                    raw::SourceFenceStatus::Staged
                ));
                let _reference = source.archive("held-source").unwrap();
                if state.mode == 22 {
                    panic!("controlled failure after provisional archive");
                }
                state.source = Some(source);
                state.image = Some(image);
                state.phase = 1;
                log("source-staged");
                return;
            }
            if matches!(state.mode, 8 | 9) && state.phase == 0 {
                state.image = Some(player.state_image().unwrap());
                state.phase = 1;
                log("captured");
                return;
            }
            let expected = state
                .image
                .take()
                .unwrap_or_else(|| player.state_image().unwrap());
            if state.mode == 9 {
                let fresh = player.state_image().unwrap();
                assert!(matches!(
                    player.submit_state("restore", &fresh, &expected, None),
                    Err(raw::StateError::WrongAccount)
                ));
                log("wrong-account");
                return;
            }
            if state.mode == 3 {
                let mut values = expected.values().unwrap();
                values.velocity.x = f64::NAN;
                assert!(expected.with_values(&values).is_err());
                values.velocity.x = 0.0;
                values.effects = vec![
                    values.effects.first().cloned().unwrap_or(
                        ActiveStatusEffect {
                            kind: EffectKind::Speed,
                            amplifier: 0,
                            duration_ticks: 20,
                            ambient: false,
                            show_particles: true,
                            infinite: false,
                        }
                    );
                    37
                ];
                assert!(expected.with_values(&values).is_err());
                log("invalid-values");
                return;
            }
            let candidate = desired(tx, &expected);
            if matches!(state.mode, 17 | 19) {
                let error = player
                    .submit_state(
                        if state.mode == 19 { "" } else { "restore" },
                        &expected,
                        &candidate,
                        None,
                    )
                    .err()
                    .unwrap();
                assert!(match state.mode {
                    17 => matches!(
                        error,
                        raw::StateError::Access(unastar_api::AccessError::LimitExceeded)
                    ),
                    _ => matches!(error, raw::StateError::InvalidIdentifier),
                });
                log(if state.mode == 17 {
                    "limited"
                } else {
                    "invalid-request"
                });
                return;
            }
            let operation = player
                .submit_state("restore", &expected, &candidate, None)
                .unwrap();
            assert!(matches!(operation.status().unwrap(), StateStatus::Staged));
            state.phase = 2;
            if state.mode == 1 {
                drop(operation);
                panic!("controlled callback failure");
            }
            if state.mode == 2 {
                drop(operation);
                log("dropped");
            } else {
                state.operation = Some(operation);
            }
        });
    }

    fn on_tick(_tick: u64) {
        STATE.with_borrow_mut(|state| {
            if let Some(source) = &state.source {
                match source.status().unwrap() {
                    raw::SourceFenceStatus::Held if state.phase == 1 => log("source-held"),
                    raw::SourceFenceStatus::Consumed => log("source-consumed"),
                    _ => (),
                }
            }
            let Some(operation) = &state.operation else {
                return;
            };
            match operation.status().unwrap() {
                StateStatus::Queued => log("queued"),
                StateStatus::Committed(commit) => {
                    assert!(!commit.already_committed);
                    assert_eq!(commit.live_applied, state.mode != 11);
                    log(if commit.live_applied {
                        "committed"
                    } else {
                        "committed-unapplied"
                    });
                    state.operation.take();
                    state.source.take();
                    state.image.take();
                    if matches!(state.mode, 4 | 7 | 23) {
                        drop(state.world.as_ref().unwrap().schedule(2).unwrap());
                    }
                }
                StateStatus::Rejected(raw::StateError::Conflict) => {
                    log("rejected:Conflict");
                    if state.mode == 23 {
                        drop(state.world.as_ref().unwrap().schedule(1).unwrap());
                    }
                }
                StateStatus::Rejected(error) => log(&format!("rejected:{error:?}")),
                StateStatus::Uncertain => log("uncertain"),
                _ => (),
            }
        });
    }

    fn on_world_task(tx: &raw::WorldTx, _task: u64, callback: u64) {
        STATE.with_borrow_mut(|state| {
            if callback == 2 {
                state
                    .hold
                    .take()
                    .unwrap()
                    .resolve(tx, &AdmissionResolution::Allow)
                    .unwrap();
                state.player.take();
                state.world.take();
                log("resolved");
                return;
            }
            let player = tx.player(state.player.as_ref().unwrap()).unwrap();
            let expected = player.state_image().unwrap();
            let candidate = if matches!(state.mode, 7 | 23) {
                if state.mode == 7 {
                    let source = state.source.as_ref().unwrap();
                    assert!(matches!(
                        source.status().unwrap(),
                        raw::SourceFenceStatus::Held
                    ));
                }
                let reference = unastar_api::archives::lookup("held-source")
                    .unwrap()
                    .unwrap();
                let archived = unastar_api::archives::read(&reference).unwrap();
                if state.mode == 23 {
                    assert!(archived.source_fence(tx).unwrap().is_none());
                }
                let image = archived.state_image().unwrap();
                desired(tx, &image)
            } else {
                desired(tx, &expected)
            };
            state.operation = Some(
                state
                    .hold
                    .as_ref()
                    .unwrap()
                    .submit_state(tx, "restore", &expected, &candidate, state.source.as_ref())
                    .unwrap(),
            );
            log("initial-staged");
        });
    }

    fn on_player_admitted(_event: AdmittedEvent) {
        log("admitted");
    }
    fn on_player_quit(_tx: &raw::WorldTx, _event: QuitEvent) -> QuitDisposition {
        STATE.with_borrow(|state| {
            if state.mode == 7 || state.mode == 21 {
                QuitDisposition::RetainEntity
            } else {
                QuitDisposition::Remove
            }
        })
    }
}
unastar_api::export_plugin!(Fixture);
