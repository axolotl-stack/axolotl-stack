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
    fn on_player_chat(ctx: &mut PlayerContext<'_>, _message: String) {
        ctx.cancel();
    }
}

#[test]
fn generated_handler_dispatches_and_uses_the_canonical_transaction_signature() {
    let _: fn(&unastar_api::raw::WorldTx, &unastar_api::raw::Player, String) -> bool =
        <__UnastarHandlerBridge as Guest>::on_player_chat;
    <__UnastarHandlerBridge as Guest>::on_tick(123);
    assert_eq!(LAST_TICK.load(Ordering::SeqCst), 123);
}
