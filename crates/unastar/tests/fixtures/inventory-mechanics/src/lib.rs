//! Actual typed SDK calls for bounded live inventory changes.

use std::cell::RefCell;
use unastar_api::{
    AccessError, DamageCommitted, Handler, InventoryArea, InventoryMechanicsOperation,
    InventoryMechanicsStatus, ItemStack, LogLevel, PlayerContext, PlayerHandle, QuitDisposition,
    QuitEvent, WorldTx, commands, host,
};

struct Fixture;
unastar_api::export_handler!(Fixture);

#[derive(Default)]
struct State {
    operations: Vec<InventoryMechanicsOperation>,
    original: Option<ItemStack>,
    attacker: Option<PlayerHandle>,
    reverse_on_hit: bool,
}
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Assertions use a managed Wasm trap without requiring a WASI stream runtime.
fn check(value: bool) {
    if !value {
        core::arch::wasm32::unreachable();
    }
}

/// Surface component errors as an ordinary trap observed by the native harness.
fn ok<T, E>(value: Result<T, E>) -> T {
    match value {
        Ok(value) => value,
        Err(_) => core::arch::wasm32::unreachable(),
    }
}

/// Observe only actual owned operation progress through the public SDK.
fn remember(state: &mut State, operation: InventoryMechanicsOperation) {
    check(matches!(
        ok(operation.status()),
        InventoryMechanicsStatus::Staged
    ));
    state.operations.push(operation);
    host::log(LogLevel::Info, "staged");
}

impl Handler for Fixture {
    fn on_load() {
        ok(commands::register_root(&commands::Root {
            name: "mechanic".into(),
            description: "Exercise bounded inventory changes".into(),
        }));
    }

    fn on_player_command(ctx: &mut PlayerContext<'_>, line: String, _: Option<String>) {
        ctx.cancel();
        STATE.with_borrow_mut(|state| match line.trim_start_matches('/') {
            "mechanic reverse" | "mechanic drop" | "mechanic trap" => {
                let operation = ok(ctx.player().reorder_hotbar([8, 7, 6, 5, 4, 3, 2, 1, 0]));
                if line.ends_with("drop") {
                    drop(operation);
                } else {
                    remember(state, operation);
                }
                check(!line.ends_with("trap"));
            }
            "mechanic invalid" => {
                for order in [[0; 9], [0, 1, 2, 3, 4, 5, 6, 7, 9]] {
                    check(matches!(
                        ctx.player().reorder_hotbar(order),
                        Err(AccessError::InvalidArgument)
                    ));
                }
                host::log(LogLevel::Info, "invalid");
            }
            "mechanic denied" => {
                check(matches!(
                    ctx.player().reorder_hotbar([8, 7, 6, 5, 4, 3, 2, 1, 0]),
                    Err(AccessError::PermissionDenied)
                ));
                host::log(LogLevel::Info, "denied");
            }
            "mechanic unavailable" => {
                check(matches!(
                    ctx.player().reorder_hotbar([8, 7, 6, 5, 4, 3, 2, 1, 0]),
                    Err(AccessError::Unavailable)
                ));
                host::log(LogLevel::Info, "unavailable");
            }
            "mechanic consume" | "mechanic double" => {
                let expected = ok(ctx.player().inventory_stack(InventoryArea::Main, 0));
                remember(state, ok(ctx.player().consume_selected(&expected, 1)));
                if line.ends_with("double") {
                    remember(state, ok(ctx.player().consume_selected(&expected, 1)));
                }
            }
            "mechanic invalid-consume" => {
                let expected = ok(ctx.player().inventory_stack(InventoryArea::Main, 0));
                check(matches!(
                    ctx.player().consume_selected(&expected, 0),
                    Err(AccessError::InvalidArgument)
                ));
                check(matches!(
                    ctx.player().consume_selected(&expected, u8::MAX),
                    Err(AccessError::InvalidArgument)
                ));
                host::log(LogLevel::Info, "invalid-consume");
            }
            "mechanic capture" | "mechanic arm" | "mechanic arm-reverse" => {
                state.original = Some(ok(ctx.player().inventory_stack(InventoryArea::Main, 0)));
                if line.ends_with("arm") || line.ends_with("arm-reverse") {
                    state.attacker = Some(ok(ctx.player().handle()));
                    state.reverse_on_hit = line.ends_with("arm-reverse");
                }
            }
            "mechanic schedule" => {
                state.attacker = Some(ok(ctx.player().handle()));
                ok(ok(ctx.tx().world()).schedule(11));
            }
            "mechanic schedule-poll" => {
                ok(ok(ctx.tx().world()).schedule(12));
            }
            "mechanic consume-captured" => {
                match ctx
                    .player()
                    .consume_selected(state.original.as_ref().unwrap(), 1)
                {
                    Ok(operation) => remember(state, operation),
                    Err(AccessError::Unavailable) => host::log(LogLevel::Info, "stale"),
                    Err(_) => check(false),
                }
            }
            "mechanic repair" | "mechanic armour" | "mechanic offhand" => {
                let area = if line.ends_with("armour") {
                    InventoryArea::Armour
                } else if line.ends_with("offhand") {
                    InventoryArea::Offhand
                } else {
                    InventoryArea::Main
                };
                let expected = ok(ctx.player().inventory_stack(area, 0));
                remember(state, ok(ctx.player().repair_item(area, 0, &expected, 4)));
            }
            "mechanic invalid-repair" => {
                let expected = ok(ctx.player().inventory_stack(InventoryArea::Main, 0));
                for (area, points) in [
                    (InventoryArea::Main, 0),
                    (InventoryArea::Ui, 4),
                    (InventoryArea::EnderChest, 4),
                ] {
                    check(matches!(
                        ctx.player().repair_item(area, 0, &expected, points),
                        Err(AccessError::InvalidArgument)
                    ));
                }
                host::log(LogLevel::Info, "invalid-repair");
            }
            "mechanic poll" => {
                for operation in &state.operations {
                    let status = ok(operation.status());
                    host::log(
                        LogLevel::Info,
                        match status {
                            InventoryMechanicsStatus::Staged => "staged",
                            InventoryMechanicsStatus::Applied => "applied",
                            InventoryMechanicsStatus::Rejected(_) => "rejected",
                            InventoryMechanicsStatus::Cancelled => "cancelled",
                        },
                    );
                }
            }
            _ => check(false),
        });
    }

    fn on_player_damaged(ctx: &mut PlayerContext<'_>, damage: DamageCommitted) {
        check(damage.actual_health_loss > 0.0);
        STATE.with_borrow_mut(|state| {
            if let (Some(handle), Some(original)) = (&state.attacker, &state.original) {
                let attacker = ok(ctx.tx().player(handle));
                let operation = if state.reverse_on_hit {
                    attacker.reorder_hotbar([8, 7, 6, 5, 4, 3, 2, 1, 0])
                } else {
                    attacker.consume_selected(original, 1)
                };
                remember(state, ok(operation));
            }
        });
    }

    fn on_player_quit(_tx: &WorldTx<'_>, _event: QuitEvent) -> QuitDisposition {
        QuitDisposition::RetainEntity
    }

    fn on_world_task(tx: &WorldTx<'_>, _: u64, callback: u64) {
        if callback == 12 {
            STATE.with_borrow(|state| {
                for operation in &state.operations {
                    check(matches!(
                        ok(operation.status()),
                        InventoryMechanicsStatus::Applied
                    ));
                }
            });
            host::log(LogLevel::Info, "owned-applied");
            return;
        }
        check(callback == 11);
        STATE.with_borrow_mut(|state| {
            let player = ok(tx.player(state.attacker.as_ref().unwrap()));
            match player.reorder_hotbar([8, 7, 6, 5, 4, 3, 2, 1, 0]) {
                Ok(operation) => remember(state, operation),
                Err(AccessError::Unavailable) => host::log(LogLevel::Info, "unavailable"),
                Err(_) => check(false),
            }
        });
    }
}
