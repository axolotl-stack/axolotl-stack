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
    rebind: Option<raw::SourceRebindOperation>,
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
    state.operation = Some(
        unastar_api::WorldTx::__from_borrowed(tx)
            .inspect_source(&archive(key))
            .unwrap(),
    );
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
            if state.mode >= 2 && !matches!(state.mode, 11 | 13 | 30 | 31 | 32 | 34) {
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
            if matches!(state.mode, 6 | 10 | 38 | 40 | 41 | 43 | 44 | 45) {
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
    fn on_player_damaged(_tx: &raw::WorldTx, player: &raw::Player, _damage: raw::DamageCommitted) {
        log("source:damaged");
        STATE.with_borrow(|state| {
            if state.mode == 40 && player.vitals().is_ok_and(|vitals| vitals.health > 0.0) {
                let actual = player.inventory_stack(raw::InventoryArea::Main, 0).unwrap();
                drop(player.consume_selected(&actual, 1).unwrap());
            }
        });
    }
    fn on_player_death(tx: &raw::WorldTx, _death: raw::DeathCommitted) {
        log("source:death");
        STATE.with_borrow_mut(|state| {
            if matches!(state.mode, 50 | 51) {
                log(&format!(
                    "terminal:death:{}",
                    _death.state.as_ref().unwrap().facts().unwrap().capture_id
                ));
                let tx = unastar_api::WorldTx::__from_borrowed(tx);
                let player = tx.player(state.original.as_ref().unwrap()).unwrap();
                let operation = player.checkpoint_terminal_source().unwrap();
                drop(operation.archive("terminal-origin").unwrap());
                if state.mode == 51 {
                    panic!("controlled terminal adoption failure");
                }
                state.operation = Some(operation);
                state.phase = 90;
                log("terminal:staged");
            }
        });
    }
    fn on_player_quit(tx: &raw::WorldTx, event: QuitEvent) -> QuitDisposition {
        STATE.with_borrow_mut(|state| {
            if state.mode >= 40 && !event.previously_admitted {
                return QuitDisposition::Remove;
            }
            assert!(event.previously_admitted);
            let player = tx.player(state.original.as_ref().unwrap()).unwrap();
            if matches!(state.mode, 50 | 51 | 52) {
                log("terminal:retained");
                return QuitDisposition::RetainEntity;
            }
            if state.mode == 11 {
                drop(player.archive("unheld").unwrap());
                log("source:ordinary-departure");
                return QuitDisposition::Remove;
            }
            if matches!(state.mode, 30 | 31 | 32 | 34) {
                let operation = player.checkpoint_source().unwrap();
                assert!(matches!(operation.status(), Ok(SourceActorStatus::Staged)));
                drop(operation.archive("origin").unwrap());
                if state.mode == 31 {
                    panic!("controlled provisional live failure");
                }
                if state.mode == 34 {
                    return QuitDisposition::Remove;
                }
                state.operation = Some(operation);
                state.phase = 70;
                log("source:live-staged");
                return QuitDisposition::RetainEntity;
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
    fn on_source_rebind_result(event: raw::SourceRebindResult) {
        assert!(!event.request_id.is_empty());
        assert_eq!(event.correlation, 901);
        match event.status {
            raw::SourceRebindStatus::Acknowledged(_) => log("rebind:owned-ack"),
            raw::SourceRebindStatus::Rebound(_) => log("rebind:owned-rebound"),
            raw::SourceRebindStatus::Terminal(_) => log("rebind:owned-terminal"),
            _ => (),
        }
    }
    fn on_tick(_tick: u64) {
        STATE.with_borrow(|state| {
            if !matches!(state.phase, 12 | 22 | 24 | 32 | 42 | 52 | 74)
                && let Some(world) = &state.world
            {
                drop(world.schedule(100).unwrap());
            }
        });
    }
    fn on_world_task(tx: &raw::WorldTx, _task: u64, _callback: u64) {
        STATE.with_borrow_mut(|state| match state.phase {
            0 if matches!(state.mode, 50 | 51)
                || (state.mode == 52 && state.original.is_none()) =>
            {
                ()
            }
            0 if matches!(state.mode, 53 | 54) => {
                state.operation = Some(
                    unastar_api::WorldTx::__from_borrowed(tx)
                        .inspect_source(&archive("terminal-origin"))
                        .unwrap(),
                );
                state.phase = 91;
            }
            0 if state.mode == 52 && state.original.is_some() => {
                let tx = unastar_api::WorldTx::__from_borrowed(tx);
                let player = tx.player(state.original.as_ref().unwrap()).unwrap();
                if player.vitals().is_ok_and(|vitals| vitals.health <= 0.0) {
                    assert!(player.checkpoint_terminal_source().is_err());
                    state.phase = 22;
                    log("terminal:no-witness");
                }
            }
            90 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Observed(observed) => {
                    assert_eq!(observed.phase, SourceActorPhase::Terminal);
                    assert!(observed.live_applied);
                    drop(
                        state
                            .operation
                            .as_ref()
                            .unwrap()
                            .archive("terminal-owned")
                            .unwrap(),
                    );
                    state.phase = 22;
                    log("terminal:owned");
                }
                other => panic!("terminal adoption: {other:?}"),
            },
            91 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Observed(observed) if state.mode == 53 => {
                    assert_eq!(observed.phase, SourceActorPhase::Terminal);
                    assert!(!observed.live_applied);
                    assert!(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .source_player(state.operation.as_ref().unwrap())
                            .unwrap()
                            .is_none()
                    );
                    state.phase = 22;
                    log("terminal:cold-owned");
                }
                SourceActorStatus::Unowned if state.mode == 54 => {
                    state.phase = 22;
                    log("terminal:cold-unowned");
                }
                other => panic!("terminal readback: {other:?}"),
            },
            0 if matches!(state.mode, 42 | 46) => {
                let intent = unastar_api::storage::read("rebind-intent")
                    .unwrap()
                    .unwrap();
                let id = String::from_utf8(intent.bytes).unwrap();
                state.rebind = Some(
                    unastar_api::WorldTx::__from_borrowed(tx)
                        .inspect_source_rebind(&archive("sealed"), &id)
                        .unwrap(),
                );
                state.phase = 83;
            }
            83 => match state.rebind.as_ref().unwrap().status().unwrap() {
                raw::SourceRebindStatus::Queued | raw::SourceRebindStatus::Staged => (),
                raw::SourceRebindStatus::Acknowledged(commit) => {
                    assert!(!commit.live_applied);
                    assert!(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .rebound_player(state.rebind.as_ref().unwrap())
                            .unwrap()
                            .is_none()
                    );
                    assert!(matches!(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .publish_source_rebind(state.rebind.as_ref().unwrap()),
                        Err(raw::StateError::SourceGone)
                    ));
                    state.operation = Some(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .inspect_source(&archive("sealed"))
                            .unwrap(),
                    );
                    state.phase = 84;
                }
                raw::SourceRebindStatus::Retired(commit) if state.mode == 46 => {
                    assert!(!commit.live_applied);
                    assert!(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .rebound_player(state.rebind.as_ref().unwrap())
                            .unwrap()
                            .is_none()
                    );
                    assert!(matches!(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .publish_source_rebind(state.rebind.as_ref().unwrap()),
                        Err(raw::StateError::SourceGone)
                    ));
                    state.operation = Some(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .inspect_source(&archive("rebind-retired"))
                            .unwrap(),
                    );
                    state.phase = 88;
                }
                raw::SourceRebindStatus::Unowned => {
                    state.phase = 22;
                    log("rebind:cold-unowned");
                }
                other => panic!("rebind readback: {other:?}"),
            },
            84 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Observed(observed) => {
                    assert_eq!(observed.phase, SourceActorPhase::Transferred);
                    state.operation = Some(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .recreate_source(state.operation.as_ref().unwrap())
                            .unwrap(),
                    );
                    state.phase = 85;
                }
                other => panic!("closed source readback: {other:?}"),
            },
            85 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Unowned => {
                    state.phase = 22;
                    log("rebind:cold-ack");
                }
                other => panic!("closed source recreation: {other:?}"),
            },
            88 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Observed(observed) => {
                    assert_eq!(observed.phase, SourceActorPhase::Retired);
                    assert!(!observed.live_applied);
                    assert!(
                        state
                            .operation
                            .as_ref()
                            .unwrap()
                            .player(tx)
                            .unwrap()
                            .is_none()
                    );
                    state.phase = 22;
                    log("rebind:cold-retired");
                }
                other => panic!("retired source readback: {other:?}"),
            },
            87 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Observed(observed) => {
                    assert_eq!(observed.phase, SourceActorPhase::Retired);
                    assert!(observed.live_applied);
                    drop(
                        state
                            .operation
                            .as_ref()
                            .unwrap()
                            .archive("rebind-retired")
                            .unwrap(),
                    );
                    state.phase = 22;
                    log("rebind:retired");
                }
                other => panic!("rebind retirement: {other:?}"),
            },
            80 => match state.rebind.as_ref().unwrap().status().unwrap() {
                raw::SourceRebindStatus::Queued | raw::SourceRebindStatus::Staged => (),
                raw::SourceRebindStatus::Acknowledged(commit) => {
                    assert!(!commit.live_applied);
                    unastar_api::storage::compare_exchange(
                        "rebind-accepted",
                        None,
                        &commit.receipt.revision.to_le_bytes(),
                    )
                    .unwrap();
                    if state.mode == 45 {
                        state.operation = Some(
                            unastar_api::WorldTx::__from_borrowed(tx)
                                .retire_source_rebind(state.rebind.as_ref().unwrap())
                                .unwrap(),
                        );
                        state.phase = 87;
                        return;
                    }
                    if state.mode == 44 {
                        panic!("controlled accepted rebind failure");
                    }
                    unastar_api::WorldTx::__from_borrowed(tx)
                        .publish_source_rebind(state.rebind.as_ref().unwrap())
                        .unwrap();
                    state.phase = 81;
                    log("rebind:publish-staged");
                }
                raw::SourceRebindStatus::Terminal(_) => {
                    state.phase = 22;
                    log("rebind:terminal");
                }
                other => panic!("rebind acknowledgement: {other:?}"),
            },
            81 => match state.rebind.as_ref().unwrap().status().unwrap() {
                raw::SourceRebindStatus::Acknowledged(_) => (),
                raw::SourceRebindStatus::Rebound(commit) => {
                    assert!(commit.live_applied);
                    let player = unastar_api::WorldTx::__from_borrowed(tx)
                        .rebound_player(state.rebind.as_ref().unwrap())
                        .unwrap()
                        .unwrap();
                    assert_eq!(player.identity().unwrap().connection_id, Some(2));
                    state.phase = 22;
                    log("rebind:published");
                }
                raw::SourceRebindStatus::Terminal(_) => {
                    state.phase = 22;
                    log("rebind:terminal");
                }
                raw::SourceRebindStatus::Failed(failure) => {
                    assert!(!failure.receipt.request_hash.is_empty());
                    state.phase = 22;
                    log("rebind:failed-publication");
                }
                other => panic!("rebind publication: {other:?}"),
            },
            0 if state.mode == 38 => {
                let desired = archive("origin")
                    .state_image()
                    .unwrap()
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
                assert!(matches!(
                    state.admission.as_ref().unwrap().submit_state(
                        tx,
                        "unowned-provisional",
                        &expected,
                        &desired,
                        None
                    ),
                    Err(raw::StateError::Conflict)
                ));
                state.phase = 22;
                log("source:provisional-install-rejected");
            }
            0 if state.mode == 12 => {
                assert!(matches!(
                    archive("unheld").source(tx),
                    Err(raw::StateError::InvalidImage)
                ));
                state.phase = 22;
                log("source:unheld-rejected");
            }
            0 if state.mode >= 2 && !matches!(state.mode, 11 | 13 | 30 | 31 | 32 | 34) => {
                inspect(tx, state)
            }
            70 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => {
                    let player = tx.player(state.original.as_ref().unwrap()).unwrap();
                    assert!(matches!(
                        player.checkpoint_source(),
                        Err(raw::StateError::Busy)
                    ));
                    if let Ok(image) = player.state_image() {
                        assert!(matches!(
                            player.fence_state(&image),
                            Err(raw::StateError::Busy)
                        ));
                    }
                }
                SourceActorStatus::Observed(observation) => {
                    assert!(matches!(
                        observation.phase,
                        SourceActorPhase::Active | SourceActorPhase::Terminal
                    ));
                    assert!(observation.live_applied);
                    if observation.phase == SourceActorPhase::Active {
                        assert_eq!(observation.physical, SourceActorPhysical::Present);
                        log("source:live-ack");
                    } else {
                        log("source:live-terminal");
                    }
                    drop(
                        state
                            .operation
                            .as_ref()
                            .unwrap()
                            .archive(if state.mode == 32 {
                                "live-first"
                            } else {
                                "sealed"
                            })
                            .unwrap(),
                    );
                    state.phase = 72;
                }
                other => panic!("live ownership: {other:?}"),
            },
            72 if state.mode == 32 => {
                let handle = state
                    .operation
                    .as_ref()
                    .unwrap()
                    .player(tx)
                    .unwrap()
                    .unwrap();
                let player = tx.player(&handle).unwrap();
                state.operation = Some(player.checkpoint_source().unwrap());
                state.phase = 73;
            }
            73 => match state.operation.as_ref().unwrap().status().unwrap() {
                SourceActorStatus::Staged | SourceActorStatus::Queued => (),
                SourceActorStatus::Observed(observation) => {
                    assert_eq!(observation.phase, SourceActorPhase::Active);
                    drop(state.operation.as_ref().unwrap().archive("sealed").unwrap());
                    state.phase = 74;
                    log("source:live-latest");
                }
                other => panic!("live second checkpoint: {other:?}"),
            },
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
                    if matches!(state.mode, 40 | 41 | 43 | 44 | 45) {
                        assert_eq!(observation.phase, SourceActorPhase::Active);
                        state.original = unastar_api::WorldTx::__from_borrowed(tx)
                            .source_player(state.operation.as_ref().unwrap())
                            .unwrap();
                        state.rebind = Some(
                            state
                                .admission
                                .as_ref()
                                .unwrap()
                                .rebind_source(tx, state.operation.as_ref().unwrap(), 901)
                                .unwrap(),
                        );
                        let id = state.rebind.as_ref().unwrap().request_id().unwrap();
                        unastar_api::storage::compare_exchange(
                            "rebind-intent",
                            None,
                            id.as_bytes(),
                        )
                        .unwrap();
                        if state.mode == 41 {
                            panic!("controlled provisional rebind failure");
                        }
                        state.phase = 80;
                        log("rebind:staged");
                        return;
                    }
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
                        state.operation = Some(
                            unastar_api::WorldTx::__from_borrowed(tx)
                                .recreate_source(state.operation.as_ref().unwrap())
                                .unwrap(),
                        );
                        state.phase = 21;
                    } else if matches!(state.mode, 4 | 10) {
                        state.source =
                            Some(state.operation.as_ref().unwrap().source_fence(tx).unwrap());
                        state.operation = Some(
                            unastar_api::WorldTx::__from_borrowed(tx)
                                .activate_source(state.operation.as_ref().unwrap())
                                .unwrap(),
                        );
                        state.phase = if state.mode == 10 { 50 } else { 23 };
                    } else if state.mode == 5 {
                        state.operation = Some(
                            unastar_api::WorldTx::__from_borrowed(tx)
                                .retire_source(state.operation.as_ref().unwrap())
                                .unwrap(),
                        );
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
                    state.operation = Some(
                        unastar_api::WorldTx::__from_borrowed(tx)
                            .retire_source(state.operation.as_ref().unwrap())
                            .unwrap(),
                    );
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
