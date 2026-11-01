//! Real scoped container plans, with no native actor-buffer authority.
use std::cell::RefCell;
use unastar_api::{
    BlockFace, BlockPos, ContainerCreateKind, ContainerPlan, DurableBlockError,
    DurableBlockRequest, Handler, ItemValue, LogLevel, PlayerContext, WorldTx, server,
};
#[derive(Default)]
struct State {
    plan: Option<ContainerPlan>,
    ticket: Option<DurableBlockRequest>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
struct Containers;
unastar_api::export_handler!(Containers);
/// Exact slot values remain ordinary immutable typed stack resources.
fn build(
    tx: &WorldTx<'_>,
    kind: ContainerCreateKind,
    position: BlockPos,
) -> Result<ContainerPlan, DurableBlockError> {
    let expected = tx.block(position).map_err(DurableBlockError::Block)?;
    let plan = tx.plan_container(&expected, kind, BlockFace::North)?;
    let item = tx
        .item("minecraft:paper", 63)
        .map_err(DurableBlockError::Access)?;
    let item = item
        .with_name(Some("Workshop record"))
        .map_err(DurableBlockError::Access)?;
    let item = item
        .with_lore(&["First line".into(), "Second line".into()])
        .map_err(DurableBlockError::Access)?;
    let item = item
        .with_value(
            &["extension".into(), "serial".into()],
            Some(&ItemValue::Long(739)),
        )
        .map_err(DurableBlockError::Access)?;
    plan.set_slot(0, &item)?;
    let block = tx
        .item("minecraft:obsidian", 7)
        .map_err(DurableBlockError::Access)?;
    plan.set_slot(26, &block)?;
    Ok(plan)
}
/// Diagnostics are observations, never strings dispatched into native actions.
fn log(value: impl AsRef<str>) {
    server::log(LogLevel::Info, value.as_ref());
}
impl Handler for Containers {
    fn on_player_command(ctx: &mut PlayerContext<'_>, line: String, _request_id: Option<String>) {
        let words: Vec<_> = line.trim_start_matches('/').split_whitespace().collect();
        let command = words.first().copied().unwrap_or("");
        let key = words.get(1).copied().unwrap_or("container-1");
        let tx = ctx.tx();
        match command {
            "barrel" | "chest" | "edge-chest" | "trap-container" | "cancel-container"
            | "drop-container" | "bad-slot" | "prepare-container" => {
                let kind = if command == "chest" || command == "edge-chest" {
                    ContainerCreateKind::SingleChest
                } else {
                    ContainerCreateKind::Barrel
                };
                let position = if command == "edge-chest" {
                    BlockPos {
                        x: 15,
                        y: 21,
                        z: 15,
                    }
                } else {
                    BlockPos { x: 1, y: 21, z: 1 }
                };
                match build(tx, kind, position) {
                    Err(error) => log(format!("plan-error:{error:?}")),
                    Ok(plan) => {
                        log(format!("intent:{:?}", tx.container_intent(&plan)));
                        if command == "prepare-container" {
                            STATE.with(|state| state.borrow_mut().plan = Some(plan));
                        } else if command == "bad-slot" {
                            let item = tx.item("minecraft:paper", 1).unwrap();
                            log(format!("bad-slot:{:?}", plan.set_slot(27, &item)));
                        } else {
                            match tx.commit_container(key, &plan) {
                                Err(error) => log(format!("stage-error:{error:?}")),
                                Ok(ticket) => {
                                    log(format!("status:{:?}", ticket.status()));
                                    if command == "cancel-container" {
                                        log(format!("cancel:{:?}", ticket.cancel()));
                                    }
                                    if command != "drop-container" {
                                        STATE
                                            .with(|state| state.borrow_mut().ticket = Some(ticket));
                                    }
                                    if command == "trap-container" {
                                        core::arch::wasm32::unreachable();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "create-container" => STATE.with(|state| {
                let mut state = state.borrow_mut();
                match tx.commit_container(key, state.plan.as_ref().unwrap()) {
                    Ok(ticket) => state.ticket = Some(ticket),
                    Err(error) => log(format!("stage-error:{error:?}")),
                }
            }),
            "status" => STATE.with(|state| {
                if let Some(ticket) = &state.borrow().ticket {
                    log(format!("status:{:?}", ticket.status()));
                }
            }),
            "receipt" => log(format!("receipt:{:?}", tx.block_receipt(key))),
            "resume" => match tx.resume_blocks(key) {
                Ok(ticket) => STATE.with(|state| state.borrow_mut().ticket = Some(ticket)),
                Err(error) => log(format!("resume-error:{error:?}")),
            },
            _ => return,
        }
        ctx.cancel();
    }
}
