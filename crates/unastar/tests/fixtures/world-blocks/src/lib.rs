//! Real scoped SDK calls exercise owned terrain witnesses and physical replacement tickets.

use std::cell::RefCell;
use unastar_api::{
    BlockCell, BlockEdit, BlockEditRequest, BlockLayers, BlockPos, BlockProperty,
    BlockPropertyValue, Handler, ItemActionDecision, ItemActionProposal, LogLevel, PlayerContext,
    WorldBlockError, WorldTx, host, server,
};

#[derive(Default)]
struct State {
    cells: Vec<BlockCell>,
    edit: Option<BlockEdit>,
    ticket: Option<BlockEditRequest>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
struct Blocks;
unastar_api::export_handler!(Blocks);

/// Tests use separate source and material cells, so proposed properties come from native observations.
fn position(x: i32) -> BlockPos {
    BlockPos { x, y: 21, z: 1 }
}

/// Preserve complete canonical properties and the source's actual secondary layer.
fn plan(tx: &WorldTx<'_>, material: i32, invalid: bool) -> Result<BlockEdit, WorldBlockError> {
    let material = tx
        .block(position(material))?
        .layers()
        .map_err(WorldBlockError::Access)?
        .primary;
    let edit = tx.edit_blocks()?;
    for x in [1, 2] {
        let cell = tx.block(position(x))?;
        let layers = cell.layers().map_err(WorldBlockError::Access)?;
        let mut primary = material.clone();
        if invalid {
            primary.properties.push(BlockProperty {
                name: "invalid".into(),
                value: BlockPropertyValue::Integer(0),
            });
        }
        edit.replace(
            &cell,
            &BlockLayers {
                primary,
                secondary: layers.secondary,
            },
        )?;
    }
    Ok(edit)
}

/// Observations are ordinary diagnostic messages, never instructions interpreted by the host.
fn report(message: impl AsRef<str>) {
    server::log(LogLevel::Info, message.as_ref());
}

impl Handler for Blocks {
    fn on_player_command(ctx: &mut PlayerContext<'_>, line: String, _request_id: Option<String>) {
        let words: Vec<_> = line.trim_start_matches('/').split_whitespace().collect();
        let command = words.first().copied().unwrap_or("");
        let tx = ctx.tx();
        match command {
            "build" | "trap" | "quota" | "prepare" | "unsupported" | "invalid" => {
                let material = if command == "unsupported" { 5 } else { 4 };
                let result = plan(tx, material, command == "invalid");
                match result {
                    Ok(edit) if command == "prepare" => {
                        STATE.with(|state| state.borrow_mut().edit = Some(edit))
                    }
                    Ok(edit) => match tx.apply_blocks(&edit) {
                        Ok(ticket) => {
                            report(format!("stage:{:?}", ticket.status()));
                            STATE.with(|state| state.borrow_mut().ticket = Some(ticket));
                            if command == "quota" {
                                STATE.with(|state| state.borrow_mut().ticket.take());
                                report(format!(
                                    "quota:{:?}",
                                    plan(tx, 4, false)
                                        .and_then(|next| tx.apply_blocks(&next).map(|_| ()))
                                ));
                            }
                        }
                        Err(error) => report(format!("stage-error:{error:?}")),
                    },
                    Err(error) => report(format!("plan-error:{error:?}")),
                }
                if command == "trap" {
                    core::arch::wasm32::unreachable();
                }
            }
            "apply" => STATE.with(|state| {
                let mut state = state.borrow_mut();
                let result = tx.apply_blocks(state.edit.as_ref().expect("prepared builder"));
                match result {
                    Ok(ticket) => {
                        report(format!("stage:{:?}", ticket.status()));
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
            "cancel" => STATE.with(|state| {
                if let Some(ticket) = &state.borrow().ticket {
                    report(format!("cancel:{:?}", ticket.cancel()));
                }
            }),
            "drop" => STATE.with(|state| {
                state.borrow_mut().ticket.take();
            }),
            "read" => {
                let coords: Vec<i32> = words
                    .iter()
                    .skip(1)
                    .filter_map(|word| word.parse().ok())
                    .collect();
                if let [x, y, z] = coords.as_slice() {
                    match tx.block(BlockPos {
                        x: *x,
                        y: *y,
                        z: *z,
                    }) {
                        Ok(cell) => {
                            let layers = cell.layers().unwrap();
                            report(format!(
                                "read:{}:{}:{:?}",
                                layers.primary.identifier,
                                layers.secondary.identifier,
                                cell.facts().unwrap().edit_support
                            ));
                            STATE.with(|state| state.borrow_mut().cells.push(cell));
                        }
                        Err(error) => report(format!("read-error:{error:?}")),
                    }
                }
            }
            "image" => STATE.with(|state| {
                for cell in &state.borrow().cells {
                    report(format!("image:{:?}:{:?}", cell.position(), cell.layers()));
                }
            }),
            _ => return,
        }
        ctx.cancel();
    }

    fn on_item_action(
        ctx: &mut PlayerContext<'_>,
        _target: Option<&unastar_api::Player<'_>>,
        _proposal: &ItemActionProposal,
    ) -> ItemActionDecision {
        if host::configuration() == b"invalid-item" {
            let edit = plan(ctx.tx(), 4, false).unwrap();
            let ticket = ctx.tx().apply_blocks(&edit).unwrap();
            STATE.with(|state| state.borrow_mut().ticket = Some(ticket));
            ItemActionDecision::Handled(unastar_api::HandledItemAction {
                correlation: 7,
                consume: 0,
            })
        } else {
            ItemActionDecision::Pass
        }
    }
}
