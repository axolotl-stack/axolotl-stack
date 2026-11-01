//! Actual scoped component coverage for immutable native state archives.

use std::cell::RefCell;
use unastar_api::{
    AccessError, ArchiveError, ArchivedPlayerState, GameMode, Handler, InventoryArea, ItemValue,
    LogLevel, PlayerContext, archives, server,
};

struct Archive;
unastar_api::export_handler!(Archive);
thread_local! { static RETAINED: RefCell<Option<ArchivedPlayerState>> = const { RefCell::new(None) }; }

/// Keep unexpected fixture failures as component traps without relying on WASI stdio.
fn check(value: bool) {
    if !value {
        core::arch::wasm32::unreachable();
    }
}
/// Unwrap only expected successful fixture operations.
fn ok<T, E>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(_) => core::arch::wasm32::unreachable(),
    }
}
/// Read full typed observations without a current player or world capability.
fn inspect(archive: &ArchivedPlayerState) {
    let facts = ok(archive.facts());
    check(facts.owner.authenticated_xuid == Some(12345));
    check(facts.owner.name == "Example");
    check(facts.owner.uuid == "00000000-0000-0000-0000-000000000001");
    check(facts.owner.connection_id == Some(7));
    check(facts.source_world.uuid == "00000000-0000-0000-0000-000000000009");
    check(facts.source_world.name == "sample-overworld" && facts.source_world.dimension == 0);
    check(facts.position.x == 12.5 && facts.position.y == 65.0 && facts.position.z == -7.5);
    check(facts.health == 7.0 && facts.max_health == 20.0 && facts.food == 14);
    check(facts.saturation == 3.5 && facts.exhaustion == 1.5);
    check(matches!(facts.base_game_mode, GameMode::Survival));
    check(matches!(facts.effective_game_mode, GameMode::Creative));
    check(!facts.dead);
    let image = ok(archive.inventory());
    check(ok(image.identity()).uuid == facts.owner.uuid);
    check(ok(image.dimension()) == 0 && ok(image.revision()) == 0 && ok(image.held_slot()) == 4);
    for (area, slot, count) in [
        (InventoryArea::Main, 4, 3),
        (InventoryArea::Armour, 0, 2),
        (InventoryArea::Offhand, 0, 1),
        (InventoryArea::Ui, 0, 5),
        (InventoryArea::Ui, 1, 6),
        (InventoryArea::EnderChest, 7, 7),
    ] {
        let stack = ok(image.slot(area, slot));
        check(ok(stack.count()) == count && ok(stack.identifier()) == "minecraft:diamond");
        check(ok(stack.name()).as_deref() == Some("Archive"));
        check(
            matches!(ok(stack.value(&["unknown-property".into()])),Some(ItemValue::Bytes(bytes)) if bytes == [3,1,4]),
        );
    }
    let derived = ok(image.with_held_slot(2));
    check(ok(derived.held_slot()) == 2 && ok(image.held_slot()) == 4);
}
impl Handler for Archive {
    fn on_load() {
        let mode = server::configuration();
        if mode == b"read" || mode == b"bad-reference" || mode == b"resource-limit" {
            let reference = ok(archives::lookup("first"));
            let Some(reference) = reference else {
                core::arch::wasm32::unreachable()
            };
            if mode == b"bad-reference" {
                let mut wrong = reference.clone();
                wrong.owner.uuid = "00000000-0000-0000-0000-000000000002".into();
                check(matches!(
                    archives::read(&wrong),
                    Err(ArchiveError::InvalidReference)
                ));
                server::log(LogLevel::Info, "archive:bad-reference-rejected");
                return;
            }
            let first = ok(archives::read(&reference));
            if mode == b"resource-limit" {
                check(matches!(
                    archives::read(&reference),
                    Err(ArchiveError::Access(AccessError::LimitExceeded))
                ));
                drop(first);
                let next = ok(archives::read(&reference));
                check(ok(next.facts()).owner.authenticated_xuid == Some(12345));
                server::log(LogLevel::Info, "archive:resource-limit-released");
                return;
            }
            inspect(&first);
            server::log(LogLevel::Info, "archive:reloaded");
        }
    }
    fn on_player_command(ctx: &mut PlayerContext<'_>, line: String, _request: Option<String>) {
        let mode = server::configuration();
        let line = line.trim_start_matches('/');
        if line == "archive-inspect" {
            RETAINED.with(|value| {
                let value = value.borrow();
                let Some(archive) = value.as_ref() else {
                    core::arch::wasm32::unreachable()
                };
                check(ok(ctx.player().identity()).uuid != ok(archive.facts()).owner.uuid);
                inspect(archive);
            });
            server::log(LogLevel::Info, "archive:retained");
            ctx.cancel();
            return;
        }
        let Some(key) = line.strip_prefix("archive-save ") else {
            return;
        };
        if mode == b"oversized" {
            check(matches!(
                ctx.player().inventory(),
                Err(AccessError::LimitExceeded)
            ));
        }
        let result = ctx.player().archive(key);
        if mode == b"oversized" {
            check(matches!(
                result,
                Err(ArchiveError::QuotaExceeded)
                    | Err(ArchiveError::Access(AccessError::LimitExceeded))
            ));
            server::log(LogLevel::Info, "archive:oversized-rejected");
            ctx.cancel();
            return;
        }
        if mode == b"denied" {
            check(matches!(
                result,
                Err(ArchiveError::Access(AccessError::PermissionDenied))
            ));
            server::log(LogLevel::Info, "archive:denied");
            ctx.cancel();
            return;
        }
        match result {
            Ok(reference) => {
                check(
                    reference.key == key
                        && reference.revision == 1
                        && reference.fingerprint.len() == 64,
                );
                if mode == b"trap" {
                    core::arch::wasm32::unreachable();
                }
                let archive = ok(archives::read(&reference));
                inspect(&archive);
                RETAINED.with(|value| *value.borrow_mut() = Some(archive));
                server::log(LogLevel::Info, "archive:captured");
            }
            Err(ArchiveError::Conflict) => server::log(LogLevel::Info, "archive:conflict"),
            Err(ArchiveError::QuotaExceeded) => server::log(LogLevel::Info, "archive:quota"),
            Err(_) => core::arch::wasm32::unreachable(),
        }
        ctx.cancel();
    }
}
