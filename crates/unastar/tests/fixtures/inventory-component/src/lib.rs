//! A real component exercises owned operations and advanced raw scope rejection.

use std::cell::RefCell;
use unastar_api::{
    AccessError, AdmissionDecision, AdmissionResolution, AdmittedEvent, InventoryArea,
    InventoryError, InventoryImage, InventoryOperation, InventoryStatus, LogLevel, Plugin,
    QuitDisposition, QuitEvent, Vec3, WorldHandle, archives, host, raw, server,
};

#[derive(Default)]
struct State {
    mode: u8,
    phase: u8,
    operation: Option<InventoryOperation>,
    candidate: Option<InventoryImage>,
    expected: Vec<u8>,
    hold: Option<raw::AdmissionHold>,
    player: Option<raw::PlayerHandle>,
    live: Option<raw::Player>,
    world: Option<WorldHandle>,
    queued: bool,
    replay: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Emit only explicit guest observations through the ordinary bounded host interface.
fn log(message: &str) {
    server::log(LogLevel::Info, message);
}

/// Build a candidate without altering its original expected image or live slots.
fn candidate(tx: &raw::WorldTx, player: &raw::Player) -> InventoryImage {
    let original = player.inventory().unwrap();
    let item = tx.item("minecraft:diamond", 3).unwrap();
    original.with_slot(InventoryArea::Main, 0, &item).unwrap()
}

/// Capture the exact candidate proof before submitting the scoped request.
fn submit(state: &mut State, tx: &raw::WorldTx, player: &raw::Player) {
    let image = candidate(tx, player);
    state.expected = image.request_hash().unwrap();
    assert_eq!(state.expected.len(), 32);
    let operation = player.submit_inventory("grant", &image).unwrap();
    if state.replay {
        assert!(matches!(
            operation.status().unwrap(),
            InventoryStatus::Committed(_)
        ));
    } else {
        assert!(matches!(
            operation.status().unwrap(),
            InventoryStatus::Staged
        ));
    }
    if state.mode == 3 {
        let duplicate = player.submit_inventory("grant", &image).unwrap();
        assert!(matches!(
            duplicate.status().unwrap(),
            InventoryStatus::Staged
        ));
        drop(duplicate);
    }
    drop(image);
    if state.mode == 1 {
        drop(operation);
        panic!("controlled failure after staging and dropping all guest handles");
    }
    if state.mode == 2 {
        drop(operation);
        log("dropped");
    } else {
        state.operation = Some(operation);
    }
    state.phase = 1;
    log("staged");
}

struct Fixture;
impl Plugin for Fixture {
    fn on_load() {
        std::panic::set_hook(Box::new(|info| {
            server::log(LogLevel::Error, &info.to_string());
        }));
        STATE.with_borrow_mut(|state| {
            state.mode = host::configuration().first().copied().unwrap_or(0);
            state.replay = state.mode == 18;
        });
    }

    fn on_player_jump(tx: &raw::WorldTx, player: &raw::Player) {
        STATE.with_borrow_mut(|state| {
            match state.mode {
                26 => {
                    let reference = player.archive("historical").unwrap();
                    let archived = archives::read(&reference).unwrap();
                    let image = archived.inventory().unwrap();
                    let item = tx.item("minecraft:diamond", 3).unwrap();
                    let candidate = image.with_slot(InventoryArea::Main, 0, &item).unwrap();
                    assert!(matches!(
                        player.submit_inventory("grant", &candidate),
                        Err(InventoryError::Access(AccessError::InvalidArgument))
                    ));
                    assert!(player.inventory_receipt("grant").unwrap().is_none());
                    log("archive-is-observation");
                    return;
                }
                24 if state.phase == 2 => {
                    assert!(matches!(
                        state.operation.as_ref().unwrap().status().unwrap(),
                        InventoryStatus::Committed(_)
                    ));
                    let image = candidate(tx, player);
                    let replay = player.submit_inventory("grant", &image).unwrap();
                    let InventoryStatus::Committed(commit) = replay.status().unwrap() else {
                        panic!("canonical reconnect receipt");
                    };
                    assert_eq!(commit.receipt.request_hash, state.expected);
                    assert!(commit.already_committed);
                    assert!(!commit.live_applied);
                    log("reconnect-receipt");
                    return;
                }
                25 => {
                    match state.phase {
                        0 => {
                            state.candidate = Some(candidate(tx, player));
                            state.phase = 1;
                        }
                        1 => {
                            state.operation = Some(
                                player
                                    .submit_inventory("grant", state.candidate.as_ref().unwrap())
                                    .unwrap(),
                            );
                            state.candidate.take();
                            state.phase = 2;
                        }
                        _ => {
                            assert!(matches!(
                                state.operation.as_ref().unwrap().status().unwrap(),
                                InventoryStatus::Rejected(InventoryError::Conflict)
                            ));
                            submit(state, tx, player);
                            state.phase = 3;
                            log("retry-staged");
                        }
                    }
                    return;
                }
                19 | 21 | 23 => {
                    if state.phase == 0 {
                        state.candidate = Some(candidate(tx, player));
                        state.phase = 1;
                        log("captured");
                    } else if matches!(state.mode, 21 | 23) {
                        let error = player
                            .submit_inventory("grant", state.candidate.as_ref().unwrap())
                            .err()
                            .unwrap();
                        assert!(match state.mode {
                            21 => matches!(error, InventoryError::Access(AccessError::StalePlayer)),
                            _ => matches!(error, InventoryError::Access(AccessError::WrongWorld)),
                        });
                        state.candidate.take();
                        log(if state.mode == 21 {
                            "wrong-player"
                        } else {
                            "wrong-world"
                        });
                    } else {
                        state.operation = Some(
                            player
                                .submit_inventory("grant", state.candidate.as_ref().unwrap())
                                .unwrap(),
                        );
                        state.candidate.take();
                    }
                    return;
                }
                22 => {
                    assert!(player.inventory_receipt("grant").unwrap().is_none());
                    log("isolated");
                    return;
                }
                8 => {
                    let image = candidate(tx, player);
                    let handle = player.to_handle().unwrap();
                    state.live = Some(tx.player(&handle).unwrap());
                    state.candidate = Some(image);
                    state.phase = 1;
                    return;
                }
                9 | 10 => {
                    let original = player.inventory().unwrap();
                    let item = tx.item("minecraft:diamond", 3).unwrap();
                    let image = original.with_slot(InventoryArea::Main, 0, &item).unwrap();
                    let error = player.submit_inventory("grant", &image).err().unwrap();
                    assert!(matches!(
                        error,
                        InventoryError::Access(AccessError::LimitExceeded)
                    ));
                    log("limited");
                    return;
                }
                11 => {
                    let image = candidate(tx, player);
                    assert!(matches!(
                        player.submit_inventory("grant", &image),
                        Err(InventoryError::Access(AccessError::PermissionDenied))
                    ));
                    log("denied");
                    return;
                }
                12 => {
                    let image = candidate(tx, player);
                    assert!(matches!(
                        player.submit_inventory("", &image),
                        Err(InventoryError::InvalidIdentifier)
                    ));
                    log("invalid");
                    return;
                }
                _ => (),
            }
            if state.phase == 0 {
                submit(state, tx, player);
            } else {
                let receipt = player.inventory_receipt("grant").unwrap().unwrap();
                assert_eq!(receipt.revision, 1);
                assert_eq!(receipt.request_hash, state.expected);
                log("receipt");
                if state.mode == 7 && state.phase == 2 {
                    state.replay = true;
                    submit(state, tx, player);
                }
            }
        });
    }

    fn on_player_move(
        tx: &raw::WorldTx,
        player: &raw::Player,
        _old: Vec3,
        destination: Vec3,
    ) -> Option<Vec3> {
        STATE.with_borrow_mut(|state| {
            if !matches!(state.mode, 27 | 28) {
                return Some(destination);
            }
            submit(state, tx, player);
            if state.mode == 27 {
                None
            } else {
                Some(Vec3 {
                    x: f64::NAN,
                    ..destination
                })
            }
        })
    }

    fn on_tick(_tick: u64) {
        STATE.with_borrow_mut(|state| {
            if state.mode == 8 && state.phase == 1 {
                assert!(matches!(
                    state
                        .live
                        .as_ref()
                        .unwrap()
                        .submit_inventory("grant", state.candidate.as_ref().unwrap()),
                    Err(InventoryError::Access(AccessError::ExpiredScope))
                ));
                state.live.take();
                state.candidate.take();
                state.phase = 2;
                log("expired");
                return;
            }
            let Some(operation) = state.operation.as_ref() else {
                return;
            };
            match operation.status().unwrap() {
                InventoryStatus::Queued if !state.queued => {
                    state.queued = true;
                    log("queued");
                }
                InventoryStatus::Committed(commit) => {
                    assert_eq!(commit.receipt.revision, 1);
                    assert_eq!(commit.receipt.request_hash, state.expected);
                    assert_eq!(commit.already_committed, state.replay);
                    assert_eq!(commit.live_applied, !state.replay && state.mode != 14);
                    if state.mode != 24 {
                        state.operation.take();
                    }
                    state.phase = 2;
                    if state.mode == 4 {
                        drop(state.world.as_ref().unwrap().schedule(92).unwrap());
                    }
                    log(if state.replay {
                        "replayed"
                    } else {
                        "committed"
                    });
                }
                InventoryStatus::Uncertain => {
                    log("uncertain");
                }
                InventoryStatus::Rejected(error) if matches!(state.mode, 6 | 16 | 19 | 25) => {
                    assert!(match state.mode {
                        6 => matches!(error, InventoryError::Access(AccessError::Cancelled)),
                        16 => matches!(error, InventoryError::NotApplied),
                        _ => matches!(error, InventoryError::Conflict),
                    });
                    log("rejected");
                }
                InventoryStatus::Staged | InventoryStatus::Queued => (),
                other => panic!("unexpected inventory progress: {other:?}"),
            }
        });
    }

    fn on_player_admission(tx: &raw::WorldTx, player: &raw::Player) -> AdmissionDecision {
        STATE.with_borrow_mut(|state| {
            if !matches!(state.mode, 4..=6) {
                return AdmissionDecision::Allow;
            }
            let hold = player.defer_admission().unwrap();
            let image = candidate(tx, player);
            state.expected = image.request_hash().unwrap();
            assert!(matches!(
                hold.submit_inventory(tx, "grant", &image),
                Err(InventoryError::Access(AccessError::Unavailable))
            ));
            state.player = Some(player.to_handle().unwrap());
            state.world = Some(tx.world().unwrap());
            state.candidate = Some(image);
            state.hold = Some(hold);
            drop(state.world.as_ref().unwrap().schedule(91).unwrap());
            AdmissionDecision::Deferred
        })
    }

    fn on_world_task(tx: &raw::WorldTx, _task: u64, callback: u64) {
        STATE.with_borrow_mut(|state| match callback {
            91 => {
                let player = tx.player(state.player.as_ref().unwrap()).unwrap();
                assert!(player.initial_state_ready().unwrap());
                let hold = state.hold.as_ref().unwrap();
                state.operation = Some(
                    hold.submit_inventory(tx, "grant", state.candidate.as_ref().unwrap())
                        .unwrap(),
                );
                state.candidate.take();
                if state.mode == 6 {
                    hold.resolve(
                        tx,
                        &AdmissionResolution::Reject("Controlled admission rejection".into()),
                    )
                    .unwrap();
                }
                log("initial-staged");
            }
            92 => {
                state
                    .hold
                    .take()
                    .unwrap()
                    .resolve(tx, &AdmissionResolution::Allow)
                    .unwrap();
                state.player.take();
                state.world.take();
                log("resolved");
            }
            _ => panic!("unexpected world task"),
        });
    }

    fn on_player_admitted(_event: AdmittedEvent) {
        log("admitted");
    }

    fn on_player_quit(_tx: &raw::WorldTx, _event: QuitEvent) -> QuitDisposition {
        STATE.with_borrow(|state| {
            if state.mode == 13 {
                QuitDisposition::RetainEntity
            } else {
                QuitDisposition::Remove
            }
        })
    }
}

unastar_api::export_plugin!(Fixture);
