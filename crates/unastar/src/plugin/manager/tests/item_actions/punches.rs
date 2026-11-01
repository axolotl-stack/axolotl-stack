//! Encoded missed-swing reports remain distinct from animations and right-use gestures.

use super::*;
use crate::entity::components::{PlayerInput, PlayerSession, PlayerState, Position};
use crate::server::game::item_actions::PlayerItemAction;
use crate::server::game::player_events::{PlayerMove, PlayerMoved};

impl Harness {
    /// Pass the real codec's decoded packet through the owning connection's ordinary router.
    async fn packet(
        &mut self,
        packet: McpePacket,
    ) -> Result<(), crate::server::worlds::WorldTickError> {
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        self.router.route_packet(
            42,
            McpePacketArgs,
            BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
        );
        self.router.tick().await
    }

    /// Report actual PlayerAuthInput flags at the current native eye position.
    async fn auth(
        &mut self,
        flags: &[EnumsPlayerAuthInputPacketPayloadInputData],
        move_x: f32,
    ) -> Result<(), crate::server::worlds::WorldTickError> {
        let position = self.world().get::<Position>(self.player).unwrap().0;
        self.packet(
            PlayerAuthInputPacket {
                position: Vec3 {
                    x: position.x as f32 + move_x,
                    y: position.y as f32,
                    z: position.z as f32,
                },
                input_data: flags.to_vec(),
                ..Default::default()
            }
            .into(),
        )
        .await
    }

    /// Preserve complete authoritative stack values, including existing network identities.
    fn slots(&self) -> Vec<(crate::item::ItemStack, i32)> {
        self.world()
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .slots()
            .iter()
            .map(|stack| (stack.clone(), stack.net_id))
            .collect()
    }
}

/// The report contains both the held sneak state and its transition before the swing.
fn sneaking_punch() -> [EnumsPlayerAuthInputPacketPayloadInputData; 3] {
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    [Flag::StartSneaking, Flag::Sneaking, Flag::MissedSwing]
}

/// Derive an enum discriminant from the current WIT instead of freezing its numeric ordering.
fn stage_index(name: &str) -> u8 {
    let layout = Layout::new();
    layout
        .resolve
        .types
        .iter()
        .find_map(|(_, ty)| match &ty.kind {
            wit_parser::TypeDefKind::Enum(cases)
                if ty.name.as_deref() == Some("item-action-stage") =>
            {
                cases
                    .cases
                    .iter()
                    .position(|case| case.name == name)
                    .map(|index| u8::try_from(index).unwrap())
            }
            _ => None,
        })
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_flag_animation_and_right_use_never_become_a_punch() {
    let layout = Layout::new();
    let stage =
        layout.param("on-item-action", "proposal") + layout.field("item-action-proposal", "stage");
    let proposal = format!(
        "local.get 0 i32.load8_u offset={stage} i32.const {} i32.ne if unreachable end {}",
        stage_index("use"),
        decision(None)
    );
    let mut h = Harness::new(proposal, completion()).await;
    h.held("minecraft:paper", 3);
    let before = h.slots();
    h.auth(&[], 0.0).await.unwrap();
    h.auth(
        &[EnumsPlayerAuthInputPacketPayloadInputData::StartSneaking],
        0.0,
    )
    .await
    .unwrap();
    h.packet(
        AnimatePacket {
            action: EnumsAnimatePacketPayloadAction::Swing,
            target_actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: 42,
            },
            ..Default::default()
        }
        .into(),
    )
    .await
    .unwrap();
    h.packet(
        PlayerActionPacket {
            action: EnumsPlayerActionType::MissedSwing,
            ..Default::default()
        }
        .into(),
    )
    .await
    .unwrap();
    assert!(h.logs().is_empty());
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    // The right-use proposal is still real, but inert paper has no completion.
    assert_eq!(h.logs(), ["P"]);
    assert_eq!(h.slots(), before);
    assert_eq!(h.projectiles(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_sdk_punch_observes_applied_sneak_toggle_and_commits_without_consumption() {
    for (mode, flags) in [
        (7, sneaking_punch().to_vec()),
        (
            8,
            vec![
                EnumsPlayerAuthInputPacketPayloadInputData::StopSneaking,
                EnumsPlayerAuthInputPacketPayloadInputData::MissedSwing,
            ],
        ),
    ] {
        let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
        correlation_instances(&mut h, &[("example.items", mode, true)]);
        h.held("minecraft:snowball", 3);
        let before = h.slots();
        h.auth(&flags, 0.0).await.unwrap();
        assert_eq!(h.logs(), ["owner"]);
        assert_eq!(h.slots(), before);
        assert_eq!(h.projectiles(), 0);
        assert_eq!(
            h.world().get::<PlayerState>(h.player).unwrap().sneaking,
            mode == 7
        );
        assert_eq!(
            h.world().get::<PlayerInput>(h.player).unwrap().sneaking,
            mode == 7
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn passed_punch_is_a_proposal_with_no_fabricated_completion_or_default_cost() {
    let mut h = Harness::new(decision(None), "unreachable".into()).await;
    h.held("minecraft:snowball", 3);
    let before = h.slots();
    h.auth(&sneaking_punch(), 0.0).await.unwrap();
    assert_eq!(h.logs(), ["P"]);
    assert_eq!(h.slots(), before);
    assert_eq!(h.projectiles(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_selected_cell_still_reports_a_real_handled_punch_without_minting_an_item() {
    let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
    correlation_instances(&mut h, &[("example.items", 11, true)]);
    let before = h.slots();
    assert_eq!(h.count(), 0);
    h.auth(&sneaking_punch(), 0.0).await.unwrap();
    assert_eq!(h.logs(), ["owner"]);
    assert_eq!(h.slots(), before);
    assert_eq!(h.projectiles(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn component_punch_denial_and_late_native_veto_or_staleness_emit_no_completion() {
    let mut denied = Harness::new(decision(Some(stage_index("punch"))), "unreachable".into()).await;
    denied.held("minecraft:snowball", 3);
    let before = denied.slots();
    denied.auth(&sneaking_punch(), 0.0).await.unwrap();
    assert_eq!(denied.logs(), ["P"]);
    assert_eq!(denied.slots(), before);

    for variant in 0..3 {
        let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
        correlation_instances(&mut h, &[("example.items", 7, true)]);
        h.held("minecraft:snowball", 3);
        let before = h.slots();
        h.world_mut().add_observer(
            move |mut event: On<PlayerItemAction>,
                  mut slots: Query<&mut HeldSlot>,
                  mut sessions: Query<&mut PlayerSession>| {
                match variant {
                    0 => event.cancel(),
                    1 => slots.get_mut(event.player()).unwrap().0 = 1,
                    _ => sessions.get_mut(event.player()).unwrap().session_id = 99,
                }
            },
        );
        h.auth(&sneaking_punch(), 0.0).await.unwrap();
        assert!(h.logs().is_empty());
        assert_eq!(h.slots(), before);
        assert_eq!(h.projectiles(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_or_replaced_movement_input_cannot_authorize_a_punch() {
    for replaced in [false, true] {
        let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
        h.held("minecraft:snowball", 3);
        let before = h.slots();
        h.world_mut().add_observer(
            move |mut event: On<PlayerMove>, mut sessions: Query<&mut PlayerSession>| {
                if replaced {
                    sessions.get_mut(event.player()).unwrap().session_id = 99;
                } else {
                    event.cancel();
                }
            },
        );
        h.auth(&sneaking_punch(), 0.25).await.unwrap();
        assert!(h.logs().is_empty());
        assert_eq!(h.slots(), before);
        assert!(!h.world().get::<PlayerState>(h.player).unwrap().sneaking);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_replaced_by_a_committed_move_never_receives_the_punch() {
    let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
    h.held("minecraft:snowball", 3);
    h.world_mut().add_observer(
        |event: On<PlayerMoved>, mut sessions: Query<&mut PlayerSession>| {
            sessions.get_mut(event.player()).unwrap().session_id = 99;
        },
    );
    h.auth(&sneaking_punch(), 0.25).await.unwrap();
    assert!(h.logs().is_empty());
    assert!(h.world().get::<PlayerState>(h.player).unwrap().sneaking);
    assert_eq!(h.count(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_punch_cancellation_and_required_trap_never_emit_a_custom_completion() {
    for mode in [9, 10] {
        let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
        correlation_instances(&mut h, &[("example.items", mode, true)]);
        h.held("minecraft:snowball", 3);
        let before = h.slots();
        assert_eq!(h.auth(&sneaking_punch(), 0.0).await.is_err(), mode == 10);
        assert!(h.logs().is_empty());
        assert_eq!(h.projectiles(), 0);
        if mode == 9 {
            assert_eq!(h.slots(), before);
            assert!(h.manager.lock().unwrap().check_required().is_ok());
        } else {
            assert!(crate::server::game::inventory_effects::is_recovery_fenced(
                h.world(),
                h.player
            ));
        }
    }
}
