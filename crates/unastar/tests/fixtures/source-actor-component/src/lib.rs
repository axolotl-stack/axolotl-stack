//! Executed component controls for canonical source ownership and real retained actors.

use std::cell::RefCell;
use unastar_api::{
    AdmissionDecision, AdmissionResolution, LogLevel, Plugin, QuitDisposition, QuitEvent,
    SourceActorOperation, SourceActorPhase, SourceActorPhysical, SourceActorStatus, StateOperation,
    StateStatus, raw, server,
};

#[derive(Default)]
struct State {
    mode: u8,
    phase: u8,
    world: Option<raw::WorldHandle>,
    original: Option<raw::PlayerHandle>,
    source: Option<raw::SourceFence>,
    operation: Option<SourceActorOperation>,
    target: Option<raw::PlayerHandle>,
    admission: Option<raw::AdmissionHold>,
    transfer: Option<StateOperation>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn log(message: &str) {
    server::log(LogLevel::Info, message);
}
fn archive(key: &str) -> raw::ArchivedPlayerState {
    let reference = unastar_api::archives::lookup(key)
        .unwrap()
        .expect("actual archive");
    unastar_api::archives::read(&reference).unwrap()
}
fn inspect(tx: &raw::WorldTx, state: &mut State) {
    let key = if state.mode == 5 {
        "refenced"
    } else if matches!(state.mode, 8 | 14 | 17) {
        "origin"
    } else {
        "sealed"
    };
    state.operation = Some(archive(key).source(tx).unwrap());
    state.phase = 20;
}

struct Fixture;
impl Plugin for Fixture {
    fn on_load() {
        std::panic::set_hook(Box::new(|info| {
            server::log(LogLevel::Error, &info.to_string())
        }));
        STATE.with_borrow_mut(|state| {
            state.mode = server::configuration().first().copied().unwrap_or(0);
            if state.mode >= 2 && !matches!(state.mode, 11 | 13) {
                state.world = server::worlds()
                    .unwrap()
                    .into_iter()
                    .find(|world| world.dimension() == Ok(0));
                drop(state.world.as_ref().unwrap().schedule(100).unwrap());
            }
        });
    }
    fn on_player_admission(_tx: &raw::WorldTx, player: &raw::Player) -> AdmissionDecision {
        STATE.with_borrow_mut(|state| {
            if matches!(state.mode, 6 | 10) {
                state.admission = Some(player.defer_admission().unwrap());
                state.target = Some(player.to_handle().unwrap());
                AdmissionDecision::Deferred
            } else {
                AdmissionDecision::Allow
            }
        })
    }
    fn on_player_jump(tx: &raw::WorldTx, player: &raw::Player) {
        STATE.with_borrow_mut(|state| {
            state.original = Some(player.to_handle().unwrap());
            state.world = Some(tx.world().unwrap());
            log("source:armed");
        });
    }
    fn on_player_damaged(_tx: &raw::WorldTx, _player: &raw::Player, _damage: raw::DamageCommitted) {
        log("source:damaged");
    }
    fn on_player_death(_tx: &raw::WorldTx, _death: raw::DeathCommitted) {
        log("source:death");
    }
    fn on_player_quit(tx: &raw::WorldTx, event: QuitEvent) -> QuitDisposition {
        STATE.with_borrow_mut(|state| {
            assert!(event.previously_admitted);
            let player = tx.player(state.original.as_ref().unwrap()).unwrap();
            if state.mode == 11 {
                drop(player.archive("unheld").unwrap());
                log("source:ordinary-departure");
                return QuitDisposition::Remove;
            }
            let image = player.state_image().unwrap();
            let source = player.fence_state(&image).unwrap();
            assert!(matches!(
                source.status(),
                Ok(raw::SourceFenceStatus::Staged)
            ));
            drop(source.archive("origin").unwrap());
            if state.mode == 1 {
                panic!("controlled provisional source failure");
            }
            state.source = Some(source);
            state.phase = 10;
            log("source:quit-retained");
            QuitDisposition::RetainEntity
        })
    }
    fn on_tick(_tick: u64) {
        STATE.with_borrow(|state| {
            if !matches!(state.phase, 12 | 22 | 24 | 32 | 42 | 52)
                && let Some(world) = &state.world
            {
                drop(world.schedule(100).unwrap());
            }
        });
    }
    fn on_world_task(tx: &raw::WorldTx, _task: u64, _callback: u64) {
        STATE.with_borrow_mut(|state| match state.phase {
            0 if state.mode == 12 => {
                assert!(matches!(
                    archive("unheld").source(tx),
                    Err(raw::StateError::InvalidImage)
                ));
                state.phase = 22;
                log("source:unheld-rejected");
            }
            0 if state.mode >= 2 && !matches!(state.mode, 11 | 13) => inspect(tx, state),
            10 => {
                let source = state.source.as_ref().unwrap();
                assert!(matches!(source.status(), Ok(raw::SourceFenceStatus::Held)));
                if state.mode == 13 {
                    drop(source.checkpoint(tx).unwrap());
                    drop(state.source.take());
                    state.phase = 12;
                    log("source:checkpoint-dropped");
                    return;
                }
                state.operation = Some(source.checkpoint(tx).unwrap());
                state.phase = 11;
            }
            11 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Frozen);
                    assert_eq!(observation.physical, SourceActorPhysical::Held);
                    assert!(observation.live_applied);
                    drop(state.operation.as_ref().unwrap().archive("sealed").unwrap());
                    state.phase = 12;
                    log("source:sealed");
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Uncertain => {
                    state.phase = 12;
                    log("source:uncertain");
                }
                other => panic!("source checkpoint: {other:?}"),
            },
            20 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert!(!observation.live_applied);
                    if state.mode == 2 {
                        state.phase = 22;
                        log("source:observed");
                        return;
                    }
                    if state.mode == 14 {
                        assert_eq!(observation.phase, SourceActorPhase::Frozen);
                        assert_eq!(observation.physical, SourceActorPhysical::Held);
                        state.source =
                            Some(state.operation.as_ref().unwrap().source_fence(tx).unwrap());
                        state.operation =
                            Some(state.source.as_ref().unwrap().checkpoint(tx).unwrap());
                        state.phase = 61;
                    } else if matches!(state.mode, 3 | 17) {
                        assert_eq!(observation.physical, SourceActorPhysical::Absent);
                        state.operation =
                            Some(state.operation.as_ref().unwrap().recreate(tx).unwrap());
                        state.phase = 21;
                    } else if matches!(state.mode, 4 | 10) {
                        state.source =
                            Some(state.operation.as_ref().unwrap().source_fence(tx).unwrap());
                        state.operation =
                            Some(state.operation.as_ref().unwrap().activate(tx).unwrap());
                        state.phase = if state.mode == 10 { 50 } else { 23 };
                    } else if state.mode == 5 {
                        state.operation =
                            Some(state.operation.as_ref().unwrap().retire(tx).unwrap());
                        state.phase = 31;
                    } else if state.mode == 6
                        && let Some(admission) = &state.admission
                    {
                        let operation = state.operation.as_ref().unwrap();
                        state.source = Some(operation.source_fence(tx).unwrap());
                        let image = operation.state_image().unwrap();
                        let desired = image
                            .at(
                                tx,
                                raw::Vec3 {
                                    x: 0.5,
                                    y: 16.0,
                                    z: 0.5,
                                },
                                (0.0, 0.0),
                            )
                            .unwrap();
                        let player = tx.player(state.target.as_ref().unwrap()).unwrap();
                        let expected = player.state_image().unwrap();
                        state.transfer = Some(
                            admission
                                .submit_state(
                                    tx,
                                    "source-handoff",
                                    &expected,
                                    &desired,
                                    state.source.as_ref(),
                                )
                                .unwrap(),
                        );
                        state.phase = 41;
                        log("source:handoff-staged");
                    }
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Unowned => {
                    state.phase = 22;
                    log("source:unowned");
                }
                other => panic!("source observation: {other:?}"),
            },
            21 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Frozen);
                    assert_eq!(observation.physical, SourceActorPhysical::Held);
                    assert!(observation.live_applied);
                    let player = state
                        .operation
                        .as_ref()
                        .unwrap()
                        .player(tx)
                        .unwrap()
                        .unwrap();
                    assert!(player.identity().unwrap().connection_id.is_none());
                    state.original = Some(player);
                    state.source =
                        Some(state.operation.as_ref().unwrap().source_fence(tx).unwrap());
                    assert!(matches!(
                        state.source.as_ref().unwrap().status(),
                        Ok(raw::SourceFenceStatus::Held)
                    ));
                    drop(
                        state
                            .operation
                            .as_ref()
                            .unwrap()
                            .archive("recreated")
                            .unwrap(),
                    );
                    state.phase = 22;
                    log("source:recreated");
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                other => panic!("source recreation: {other:?}"),
            },
            23 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Active);
                    assert_eq!(observation.physical, SourceActorPhysical::Present);
                    assert!(observation.live_applied);
                    assert!(matches!(
                        state.source.as_ref().unwrap().status(),
                        Ok(raw::SourceFenceStatus::Released)
                    ));
                    let handle = state
                        .operation
                        .as_ref()
                        .unwrap()
                        .player(tx)
                        .unwrap()
                        .unwrap();
                    let player = tx.player(&handle).unwrap();
                    let image = player.state_image().unwrap();
                    state.source = Some(player.fence_state(&image).unwrap());
                    state.phase = 25;
                    log("source:active");
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                other => panic!("source activation: {other:?}"),
            },
            25 => {
                state.operation = Some(state.source.as_ref().unwrap().checkpoint(tx).unwrap());
                state.phase = 26;
            }
            26 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Frozen);
                    assert_eq!(observation.physical, SourceActorPhysical::Held);
                    drop(
                        state
                            .operation
                            .as_ref()
                            .unwrap()
                            .archive("refenced")
                            .unwrap(),
                    );
                    state.phase = 24;
                    log("source:refenced");
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                other => panic!("source refencing: {other:?}"),
            },
            31 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Retired);
                    assert_eq!(observation.physical, SourceActorPhysical::Absent);
                    state.operation = Some(state.operation.as_ref().unwrap().retire(tx).unwrap());
                    state.phase = 33;
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                other => panic!("source retirement: {other:?}"),
            },
            33 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Retired);
                    assert_eq!(observation.physical, SourceActorPhysical::Absent);
                    assert!(!observation.live_applied);
                    state.phase = 32;
                    log("source:retired");
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                other => panic!("source retirement replay: {other:?}"),
            },
            61 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Frozen);
                    assert_eq!(observation.physical, SourceActorPhysical::Held);
                    assert!(matches!(
                        state.source.as_ref().unwrap().status(),
                        Ok(raw::SourceFenceStatus::Held)
                    ));
                    state.phase = 22;
                    log("source:adopted-replay");
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                other => panic!("adopted checkpoint replay: {other:?}"),
            },
            50 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Active);
                    assert_eq!(observation.physical, SourceActorPhysical::Present);
                    state.phase = 51;
                    log("source:active-live");
                }
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                other => panic!("live source activation: {other:?}"),
            },
            51 if state.admission.is_some() => {
                let image = state.operation.as_ref().unwrap().state_image().unwrap();
                let desired = image
                    .at(
                        tx,
                        raw::Vec3 {
                            x: 0.5,
                            y: 16.0,
                            z: 0.5,
                        },
                        (0.0, 0.0),
                    )
                    .unwrap();
                let player = tx.player(state.target.as_ref().unwrap()).unwrap();
                let expected = player.state_image().unwrap();
                match state.admission.as_ref().unwrap().submit_state(
                    tx,
                    "unfenced-transfer",
                    &expected,
                    &desired,
                    None,
                ) {
                    Ok(operation) => {
                        state.transfer = Some(operation);
                        state.phase = 53;
                    }
                    Err(_) => {
                        state.phase = 52;
                        log("source:unfenced-rejected");
                    }
                }
            }
            53 => match state.transfer.as_ref().unwrap().status().unwrap() {
                StateStatus::Rejected(_) => {
                    state.phase = 52;
                    log("source:unfenced-rejected");
                }
                StateStatus::Staged | StateStatus::Queued => (),
                other => panic!("unfenced live source must not commit: {other:?}"),
            },
            41 => match state.transfer.as_ref().unwrap().status().unwrap() {
                StateStatus::Committed(commit) => {
                    assert!(commit.live_applied);
                    assert!(!commit.already_committed);
                    assert!(matches!(
                        state.source.as_ref().unwrap().status(),
                        Ok(raw::SourceFenceStatus::Consumed)
                    ));
                    let player = tx.player(state.target.as_ref().unwrap()).unwrap();
                    let receipt = player.state_receipt("source-handoff").unwrap().unwrap();
                    assert_eq!(receipt.revision, commit.receipt.revision);
                    assert_eq!(receipt.request_hash, commit.receipt.request_hash);
                    state
                        .admission
                        .as_ref()
                        .unwrap()
                        .resolve(tx, &AdmissionResolution::Allow)
                        .unwrap();
                    state.phase = 42;
                    log("source:handed-off");
                }
                StateStatus::Staged | StateStatus::Queued => (),
                other => panic!("source handoff: {other:?}"),
            },
            _ => (),
        });
    }
}
unastar_api::export_plugin!(Fixture);
