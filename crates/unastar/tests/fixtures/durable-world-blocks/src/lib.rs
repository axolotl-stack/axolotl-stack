//! Actual scoped SDK operations for durable loaded terrain and receipt recovery.

use std::cell::RefCell;
use unastar_api::{
    BlockEdit, BlockLayers, BlockPos, DurableBlockRequest, Handler, LogLevel, PlayerContext,
    WorldBlockError, WorldTx, server,
};

#[derive(Default)]
struct State {
    edit: Option<BlockEdit>,
    ticket: Option<DurableBlockRequest>,
    key: String,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
struct Durable;
unastar_api::export_handler!(Durable);

/// Material and expected properties come from actual native observations.
fn plan(tx: &WorldTx<'_>) -> Result<BlockEdit, WorldBlockError> {
    let material = tx
        .block(BlockPos { x: 4, y: 21, z: 1 })?
        .layers()
        .map_err(WorldBlockError::Access)?
        .primary;
    let edit = tx.edit_blocks()?;
    for x in [1, 2] {
        let cell = tx.block(BlockPos { x, y: 21, z: 1 })?;
        let old = cell.layers().map_err(WorldBlockError::Access)?;
        edit.replace(
            &cell,
            &BlockLayers {
                primary: material.clone(),
                secondary: old.secondary,
            },
        )?;
    }
    Ok(edit)
}
/// Log typed outcomes only; no native action is dispatched from these strings.
fn report(value: impl AsRef<str>) {
    server::log(LogLevel::Info, value.as_ref());
}

impl Handler for Durable {
    fn on_player_command(ctx: &mut PlayerContext<'_>, line: String, _request_id: Option<String>) {
        let words: Vec<_> = line.trim_start_matches('/').split_whitespace().collect();
        let command = words.first().copied().unwrap_or("");
        let key = words.get(1).copied().unwrap_or("operation-1");
        let tx = ctx.tx();
        match command {
            "build" | "trap" | "cancel" | "drop" | "quota" => match plan(tx) {
                Err(error) => report(format!("plan-error:{error:?}")),
                Ok(edit) => {
                    report(format!("intent:{:?}", tx.block_intent(&edit)));
                    match tx.commit_blocks(key, &edit) {
                        Err(error) => report(format!("stage-error:{error:?}")),
                        Ok(ticket) => {
                            report(format!("staged:{:?}", ticket.status()));
                            if command == "cancel" {
                                report(format!("cancel:{:?}", ticket.cancel()));
                            }
                            if command != "drop" {
                                STATE.with(|state| {
                                    let mut state = state.borrow_mut();
                                    state.ticket = Some(ticket);
                                    state.key = key.into();
                                });
                            }
                            if command == "quota" {
                                STATE.with(|state| {
                                    state.borrow_mut().ticket.take();
                                });
                                report(format!(
                                    "quota:{:?}",
                                    plan(tx).map(|next| tx
                                        .commit_blocks("operation-2", &next)
                                        .map(|_| ()))
                                ));
                            }
                            if command == "trap" {
                                core::arch::wasm32::unreachable();
                            }
                        }
                    }
                }
            },
            "prepare" => {
                let edit = plan(tx).unwrap();
                report(format!("intent:{:?}", tx.block_intent(&edit)));
                STATE.with(|state| state.borrow_mut().edit = Some(edit));
            }
            "apply" => STATE.with(|state| {
                let mut state = state.borrow_mut();
                match tx.commit_blocks(key, state.edit.as_ref().unwrap()) {
                    Ok(ticket) => {
                        report(format!("staged:{:?}", ticket.status()));
                        state.ticket = Some(ticket);
                    }
                    Err(error) => report(format!("stage-error:{error:?}")),
                }
            }),
            "status" => STATE.with(|state| {
                if let Some(ticket) = &state.borrow().ticket {
                    report(format!("status:{:?}", ticket.status()));
                }
            }),
            "receipt" => report(format!("receipt:{:?}", tx.block_receipt(key))),
            "resume" => match tx.resume_blocks(key) {
                Ok(ticket) => {
                    report(format!("resumed:{:?}", ticket.status()));
                    STATE.with(|state| state.borrow_mut().ticket = Some(ticket));
                }
                Err(error) => report(format!("resume-error:{error:?}")),
            },
            _ => return,
        }
        ctx.cancel();
    }
}
