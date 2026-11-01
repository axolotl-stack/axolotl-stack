//! A real consumer compiles the ergonomic handler and its canonical scoped exports.

use std::sync::atomic::{AtomicU64, Ordering};
use unastar_api::{Guest, Handler, PlayerContext};
use unastar_api_macros::plugin;

static LAST_TICK: AtomicU64 = AtomicU64::new(0);
struct Scoped;
#[plugin]
impl Handler for Scoped {
    fn on_tick(tick: u64) {
        LAST_TICK.store(tick, Ordering::SeqCst);
    }
    fn on_player_admission(ctx: &mut PlayerContext<'_>) -> unastar_api::AdmissionDecision {
        ctx.cancel();
        unastar_api::AdmissionDecision::Allow
    }
    fn on_player_chat(ctx: &mut PlayerContext<'_>, _message: String) {
        ctx.cancel();
    }
    fn on_player_move(
        ctx: &mut PlayerContext<'_>,
        _old_position: unastar_api::Vec3,
        new_position: &mut unastar_api::Vec3,
    ) {
        new_position.x += 1.0;
        ctx.cancel();
    }
}

#[test]
fn generated_handler_dispatches_and_uses_the_canonical_transaction_signature() {
    let _: fn(&unastar_api::raw::WorldTx, &unastar_api::raw::Player, String) -> bool =
        <__UnastarHandlerBridge as Guest>::on_player_chat;
    let _: fn(
        &unastar_api::raw::WorldTx,
        &unastar_api::raw::Player,
        String,
        Option<String>,
    ) -> bool = <__UnastarHandlerBridge as Guest>::on_player_command;
    let _: fn(
        &unastar_api::raw::WorldTx,
        &unastar_api::raw::Player,
    ) -> unastar_api::AdmissionDecision = <__UnastarHandlerBridge as Guest>::on_player_admission;
    let _: fn(&unastar_api::raw::WorldTx, unastar_api::QuitEvent) -> unastar_api::QuitDisposition =
        <__UnastarHandlerBridge as Guest>::on_player_quit;
    let _: fn(
        &unastar_api::raw::WorldTx,
        &unastar_api::raw::Player,
        unastar_api::Vec3,
        unastar_api::Vec3,
    ) -> Option<unastar_api::Vec3> = <__UnastarHandlerBridge as Guest>::on_player_move;
    let _: fn(
        &unastar_api::raw::WorldTx,
        &unastar_api::raw::Player,
        unastar_api::MovementCommitted,
    ) = <__UnastarHandlerBridge as Guest>::on_player_moved;
    <__UnastarHandlerBridge as Guest>::on_tick(123);
    assert_eq!(LAST_TICK.load(Ordering::SeqCst), 123);
}
