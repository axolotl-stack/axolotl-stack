//! Actual scoped SDK final authorization over native selected destinations.

use std::cell::RefCell;
use unastar_api::{
    Handler, LogLevel, PlayerContext, TravelCause, TravelCommitted, TravelOutcome, TravelProposal,
    TravelResult, Vec3, WorldHandle, host, server,
};

struct Fixture;
unastar_api::export_handler!(Fixture);
thread_local! {
    static STATE: RefCell<(u8, i32, Vec<WorldHandle>)> = const { RefCell::new((0, 0, Vec::new())) };
    static LAST: RefCell<Option<TravelCommitted>> = const { RefCell::new(None) };
}

/// Convert fixture assertions to ordinary managed guest traps.
fn check(value: bool) {
    if !value {
        core::arch::wasm32::unreachable();
    }
}

impl Handler for Fixture {
    fn on_tick(_: u64) {
        if let Some(event) = LAST.with_borrow_mut(Option::take) {
            check(
                event
                    .identity
                    .as_ref()
                    .is_ok_and(|identity| identity.name == "Traveller"),
            );
            host::log(LogLevel::Info, "owned-after-departure");
        }
    }
    fn on_player_travelled(event: TravelCommitted) {
        STATE.with_borrow(|state| {
            check(event.identity.as_ref().is_ok_and(|identity| {
                identity.name == "Traveller"
                    && identity.authenticated_xuid == Some(73)
                    && identity.connection_id == Some(73)
            }));
            check(event.origin.dimension == 0 && event.destination.dimension == state.1);
            check(
                event.origin_world_lifetime.is_some() && event.destination_world_lifetime.is_some(),
            );
            check(
                (event.origin_world_lifetime == event.destination_world_lifetime) == (state.1 == 0),
            );
            check(event.captured_tick.is_some());
            check(
                event.correlation
                    == if matches!(state.0, 6 | 7) {
                        None
                    } else {
                        Some(87)
                    },
            );
            host::log(
                LogLevel::Info,
                match event.cause {
                    TravelCause::Command => "committed:command",
                    TravelCause::Projectile => "committed:projectile",
                    TravelCause::ChorusFruit => "committed:chorus-fruit",
                    TravelCause::WorldTransfer => "committed:world-transfer",
                },
            );
            check(!matches!(state.0, 8 | 9));
        });
        LAST.with_borrow_mut(|last| *last = Some(event));
    }
    fn on_load() {
        let configuration = host::configuration();
        STATE.with_borrow_mut(|state| {
            state.0 = configuration[0];
            state.1 = i32::from(configuration[1]);
            state.2 = server::worlds().unwrap();
        });
    }
    fn on_player_jump(ctx: &mut PlayerContext<'_>) {
        STATE.with_borrow(|state| {
            let destination = state
                .2
                .iter()
                .find(|world| world.dimension().unwrap() == state.1)
                .unwrap();
            ctx.player()
                .relocate(
                    destination,
                    Vec3 {
                        x: 8.5,
                        y: 64.0,
                        z: 8.5,
                    },
                    4,
                    87,
                )
                .unwrap();
        });
    }
    fn on_player_travel(ctx: &mut PlayerContext<'_>, proposal: &TravelProposal) {
        STATE.with_borrow(|state| {
            let source = ctx.tx().world().unwrap();
            check(source.dimension().unwrap() == proposal.origin.dimension);
            let position = ctx.player().position().unwrap();
            check(
                (position.x, position.y, position.z)
                    == (
                        proposal.origin.feet.x,
                        proposal.origin.feet.y,
                        proposal.origin.feet.z,
                    ),
            );
            check(proposal.destination.dimension == state.1);
            check(!proposal.origin_world_lifetime.is_empty());
            check(!proposal.destination_world_lifetime.is_empty());
            check(
                proposal.correlation
                    == if matches!(state.0, 6 | 7) {
                        None
                    } else {
                        Some(87)
                    },
            );
            check(matches!(proposal.cause, TravelCause::WorldTransfer) == (state.1 != 0));
            if state.0 == 2 {
                check(proposal.destination.feet.y == 65.0);
            }
            check(!matches!(state.0, 3 | 4));
            if matches!(state.0, 1 | 2 | 7) {
                ctx.cancel();
            }
            host::log(LogLevel::Info, "guard");
        });
    }
    fn on_player_travel_result(event: TravelResult) {
        check(event.correlation == 87);
        host::log(
            LogLevel::Info,
            match event.outcome {
                TravelOutcome::Moved(_) => "result:moved",
                TravelOutcome::NotApplied(_) => "result:rejected",
                TravelOutcome::Uncertain => "result:uncertain",
            },
        );
    }
}
