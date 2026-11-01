//! Independent instances use identical opaque guest IDs without sharing ownership.

use std::cell::Cell;
use unastar_api::{
    HandledItemAction, Handler, ItemActionCommitted, ItemActionDecision, ItemActionOutcome,
    ItemActionProposal, LogLevel, Player, PlayerContext, WorldTx, host, server,
};

struct Fixture;
unastar_api::export_handler!(Fixture);
thread_local! { static MODE: Cell<u8> = const { Cell::new(0) }; }

impl Handler for Fixture {
    fn on_load() {
        MODE.set(host::configuration().first().copied().unwrap_or(0));
    }

    fn on_item_action(
        _ctx: &mut PlayerContext<'_>,
        _target: Option<&Player<'_>>,
        _proposal: &ItemActionProposal,
    ) -> ItemActionDecision {
        ItemActionDecision::Handled(HandledItemAction {
            correlation: 87,
            consume: 1,
        })
    }

    fn on_item_action_committed(_tx: &WorldTx<'_>, event: ItemActionCommitted) {
        let mode = MODE.get();
        assert_ne!(mode, 2, "conflicting claims must not commit");
        let ItemActionOutcome::Custom(outcome) = event.outcome else {
            panic!("expected a custom completion");
        };
        assert_eq!(outcome.correlation, if mode == 0 { Some(87) } else { None });
        assert_eq!(outcome.consumed, 1);
        let items = event.items.unwrap();
        assert_eq!(items.selected_before.count().unwrap(), 3);
        assert_eq!(items.selected_after.as_ref().unwrap().count().unwrap(), 2);
        server::log(LogLevel::Info, if mode == 0 { "owner" } else { "observer" });
    }
}
