//! Independent instances use identical opaque guest IDs without sharing ownership.

use std::cell::Cell;
use unastar_api::{
    HandledItemAction, Handler, ItemActionCommitted, ItemActionDecision, ItemActionOutcome,
    ItemActionProposal, ItemActionStage, ItemActionTarget, LogLevel, Player, PlayerContext,
    WorldTx, host, server,
};

struct Fixture;
unastar_api::export_handler!(Fixture);
thread_local! { static MODE: Cell<u8> = const { Cell::new(0) }; }

impl Handler for Fixture {
    fn on_load() {
        MODE.set(host::configuration().first().copied().unwrap_or(0));
    }

    fn on_item_action(
        ctx: &mut PlayerContext<'_>,
        _target: Option<&Player<'_>>,
        proposal: &ItemActionProposal,
    ) -> ItemActionDecision {
        let mode = MODE.get();
        if mode >= 7 {
            assert_eq!(proposal.stage, ItemActionStage::Punch);
            assert!(matches!(proposal.target, ItemActionTarget::Air));
            assert_eq!(ctx.player().flags().unwrap().sneaking, mode != 8);
        }
        if matches!(mode, 5 | 9) {
            ctx.cancel();
        }
        assert!(!matches!(mode, 6 | 10), "controlled callback trap");
        if mode >= 3 {
            ItemActionDecision::HandledWithoutConsumption(87)
        } else {
            ItemActionDecision::Handled(HandledItemAction {
                correlation: 87,
                consume: 1,
            })
        }
    }

    fn on_item_action_committed(_tx: &WorldTx<'_>, event: ItemActionCommitted) {
        let mode = MODE.get();
        if mode >= 7 {
            assert_eq!(event.stage, ItemActionStage::Punch);
            assert!(matches!(event.target, ItemActionTarget::Air));
        }
        assert_ne!(mode, 2, "conflicting claims must not commit");
        let ItemActionOutcome::Custom(outcome) = event.outcome else {
            panic!("expected a custom completion");
        };
        assert_eq!(
            outcome.correlation,
            if matches!(mode, 0 | 3 | 7 | 8 | 11) {
                Some(87)
            } else {
                None
            }
        );
        let consumed = u8::from(mode < 3);
        assert_eq!(outcome.consumed, consumed);
        let items = event.items.unwrap();
        let original = if mode == 11 { 0 } else { 3 };
        assert_eq!(items.selected_before.count().unwrap(), original);
        assert_eq!(
            items.selected_after.as_ref().unwrap().count().unwrap(),
            original - consumed
        );
        server::log(
            LogLevel::Info,
            if matches!(mode, 0 | 3 | 7 | 8 | 11) {
                "owner"
            } else {
                "observer"
            },
        );
    }
}
