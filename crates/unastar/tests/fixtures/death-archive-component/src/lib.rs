//! Complete death captures stay owned after native drops and source removal.

use std::cell::RefCell;
use unastar_api::{
    AccessError, ArchiveError, DamageCause, DeathCommitted, DeathState, InventoryArea, LogLevel,
    Plugin, archives, host, raw, server,
};

#[derive(Default)]
struct State {
    mode: u8,
    death: Option<DeathState>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}

/// Only executed guest observations become test witnesses.
fn log(text: &str) {
    server::log(LogLevel::Info, text);
}

/// Read genuine complete values, including item metadata that projected fields cannot reconstruct.
fn check_image(image: &raw::PlayerStateImage) {
    let values = image.values().unwrap();
    assert!(values.dead);
    assert_eq!(values.health, 0.0);
    assert_eq!(values.food, 9);
    assert_eq!(values.experience, 8);
    assert_eq!(values.experience_progress, 0.25);
    assert_eq!(values.air_current, 17);
    assert_eq!(values.age_ticks, 101);
    assert_eq!(values.on_fire_ticks, 40);
    assert_eq!(values.effects.len(), 1);
    assert_eq!(values.effects[0].duration_ticks, 93);
    assert_eq!(values.base_mode, unastar_api::GameMode::Survival);
    assert_eq!(values.mode_override, Some(unastar_api::GameMode::Creative));
    assert_eq!(values.flight_override, Some(true));
    assert_eq!(values.cold_exposure, 0.3);
    let inventory = image.inventory().unwrap();
    assert_eq!(
        inventory
            .slot(InventoryArea::Main, 0)
            .unwrap()
            .count()
            .unwrap(),
        7
    );
    let sword = inventory.slot(InventoryArea::Main, 5).unwrap();
    assert_eq!(sword.name().unwrap().as_deref(), Some("Observed"));
    assert_eq!(sword.lore().unwrap(), ["Original line"]);
    assert_eq!(sword.enchantments().unwrap()[0].level, 3);
    assert_eq!(
        inventory
            .slot(InventoryArea::Ui, 2)
            .unwrap()
            .count()
            .unwrap(),
        4
    );
    assert_eq!(
        inventory
            .slot(InventoryArea::EnderChest, 4)
            .unwrap()
            .count()
            .unwrap(),
        23
    );
}

/// An immutable archive is independently readable and never follows a replacement actor.
fn check_archive() {
    let reference = archives::lookup("committed-death").unwrap().unwrap();
    assert_eq!(reference.owner.authenticated_xuid, Some(42));
    assert_eq!(reference.owner.connection_id, Some(42));
    let archive = archives::read(&reference).unwrap();
    let facts = archive.death_facts().unwrap().unwrap();
    assert_eq!(facts.capture_id.len(), 36);
    assert_eq!(
        facts.phase,
        unastar_api::DeathCapturePhase::CommittedBeforeDrops
    );
    assert_eq!(facts.cause, DamageCause::Override);
    let occurrence = archive.death_occurrence().unwrap().unwrap();
    assert_eq!(occurrence.id, facts.capture_id);
    assert_eq!(occurrence.cause, facts.cause);
    assert_eq!(occurrence.keep_inventory, facts.keep_inventory);
    assert!(
        occurrence
            .unix_time_millis
            .is_none_or(|time| time == 1_710_000_000_123)
    );
    let raw = archive.facts().unwrap();
    assert!(raw.dead);
    assert_eq!(raw.experience, 8);
    check_image(&archive.state_image().unwrap());
    log("reopened");
}

/// Installation ACK is durable even if a later guest instruction traps.
fn archive(death: &DeathState) {
    let capture_id = death.facts().unwrap().capture_id;
    let reference = death.archive("committed-death").unwrap();
    assert_eq!(reference.revision, 1);
    assert_eq!(
        archives::read(&reference)
            .unwrap()
            .death_facts()
            .unwrap()
            .unwrap()
            .capture_id,
        capture_id
    );
    assert!(matches!(
        death.archive("committed-death"),
        Err(ArchiveError::Conflict)
    ));
    check_archive();
    log("archived");
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
        if STATE.with_borrow(|state| state.mode == 10) {
            check_archive();
        }
        if STATE.with_borrow(|state| state.mode == 13) {
            assert!(archives::lookup("committed-death").unwrap().is_none());
            log("isolated");
        }
    }
    fn on_player_death(_tx: &raw::WorldTx, event: DeathCommitted) {
        STATE.with_borrow_mut(|state| {
            assert_eq!(
                event.identity.as_ref().unwrap().authenticated_xuid,
                Some(42)
            );
            match state.mode {
                3 | 6 => {
                    assert!(matches!(event.state, Err(AccessError::LimitExceeded)));
                    log("limited");
                    return;
                }
                4 => {
                    assert!(matches!(event.state, Err(AccessError::PermissionDenied)));
                    log("denied");
                    return;
                }
                5 | 7 => {
                    assert!(matches!(event.state, Err(AccessError::Unavailable)));
                    log("unavailable");
                    return;
                }
                14 => {
                    assert!(matches!(event.state, Err(AccessError::InvalidArgument)));
                    log("malformed");
                    return;
                }
                _ => (),
            }
            let death = event.state.unwrap();
            let facts = death.facts().unwrap();
            let occurrence = death.death_occurrence().unwrap().unwrap();
            assert_eq!(occurrence.id, facts.capture_id);
            assert_eq!(occurrence.cause, event.cause);
            assert_eq!(
                occurrence.unix_time_millis,
                if state.mode == 17 {
                    None
                } else {
                    Some(1_710_000_000_123)
                }
            );
            let runtime = occurrence.runtime.as_ref().unwrap();
            assert_eq!(runtime.lineage.len(), 36);
            assert_eq!(runtime.world_lifetime.len(), 36);
            assert!(occurrence.post_drop.is_none());
            assert_eq!(facts.cause, event.cause);
            assert_eq!(facts.keep_inventory, event.keep_inventory);
            assert_eq!(facts.fall_distance, event.fall_distance);
            let image = death.state_image().unwrap();
            check_image(&image);
            // Detached life edits cannot rewrite the original death or its eventual archive.
            let mut values = image.values().unwrap();
            values.dead = false;
            values.health = 20.0;
            let changed = image.with_values(&values).unwrap();
            assert!(!changed.values().unwrap().dead);
            assert!(death.state_image().unwrap().values().unwrap().dead);
            drop(changed);
            drop(image);
            if state.mode == 16 {
                let capture_id = death.facts().unwrap().capture_id;
                assert!(matches!(
                    death.archive("committed-death"),
                    Err(ArchiveError::Uncertain)
                ));
                let reference = archives::lookup("committed-death").unwrap().unwrap();
                assert_eq!(
                    archives::read(&reference)
                        .unwrap()
                        .death_facts()
                        .unwrap()
                        .unwrap()
                        .capture_id,
                    capture_id
                );
                log("reconciled");
                return;
            }
            if state.mode == 12 {
                assert!(matches!(
                    death.archive("committed-death"),
                    Err(ArchiveError::QuotaExceeded)
                ));
                log("quota");
                return;
            }
            if state.mode == 1 {
                state.death = Some(death);
                log("retained");
                return;
            }
            archive(&death);
            if state.mode == 2 {
                panic!("controlled failure after immutable archive acknowledgement");
            }
        });
    }
    fn on_tick(_tick: u64) {
        STATE.with_borrow_mut(|state| {
            if let Some(death) = state.death.take() {
                archive(&death);
                log("archived-after-removal");
            }
        });
    }
}
unastar_api::export_plugin!(Fixture);
