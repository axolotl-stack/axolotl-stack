use super::*;
use tokio::sync::mpsc;

/// Build a trusted host-ingress fixture without creating any authentication token.
fn owner(
    world: &mut World,
    xuid: Option<&str>,
    uuid: Uuid,
    session_uuid: Option<String>,
) -> Entity {
    let (tx, _) = mpsc::channel(8);
    world
        .spawn((
            Player,
            PlayerUuid(uuid),
            PlayerSession::new(7, "Owner".into(), xuid.map(str::to_owned), session_uuid, tx),
        ))
        .id()
}

#[test]
fn authenticated_identity_requires_canonical_nonzero_account_and_matching_uuid_binding() {
    let uuid = Uuid::from_u128(7);
    for xuid in [
        None,
        Some("0"),
        Some("00123"),
        Some("+123"),
        Some("-123"),
        Some("unknown"),
        Some("18446744073709551616"),
    ] {
        let mut world = World::new();
        let actor = owner(&mut world, xuid, uuid, Some(uuid.to_string()));
        let attribution = DamageAttribution::capture(&world, Some(actor), Some(actor));
        assert!(attribution.owner().unwrap().is_player());
        assert!(attribution.authenticated_owner().is_none(), "{xuid:?}");
    }
    for session_uuid in [
        None,
        Some("invalid".into()),
        Some(Uuid::from_u128(8).to_string()),
    ] {
        let mut world = World::new();
        let actor = owner(&mut world, Some("12345"), uuid, session_uuid);
        assert!(
            DamageAttribution::capture(&world, Some(actor), Some(actor))
                .authenticated_owner()
                .is_none()
        );
    }
    let mut world = World::new();
    let actor = owner(&mut world, Some("12345"), uuid, Some(uuid.to_string()));
    let attribution = DamageAttribution::capture(&world, Some(actor), Some(actor));
    let identity = attribution.authenticated_owner().unwrap();
    assert_eq!(identity.uuid(), uuid);
    assert_eq!(identity.xuid(), 12345);
    assert_eq!(attribution.owner().unwrap().session_id(), Some(7));
}

#[test]
fn same_entity_bits_in_another_world_never_resolve_as_the_original_owner() {
    let mut source = World::new();
    let uuid = Uuid::from_u128(7);
    let actor = owner(&mut source, Some("123"), uuid, Some(uuid.to_string()));
    let attribution = DamageAttribution::capture(&source, Some(actor), Some(actor));
    let mut other = World::new();
    let alias = owner(&mut other, Some("456"), uuid, Some(uuid.to_string()));
    assert_eq!(alias, actor);
    assert_eq!(attribution.live_owner(&source), Some(actor));
    assert_eq!(attribution.live_owner(&other), None);
    source.despawn(actor);
    assert_eq!(attribution.live_owner(&source), None);
    assert_eq!(attribution.authenticated_owner().unwrap().xuid(), 123);
}

#[test]
fn launch_position_is_immutable_and_replaced_session_is_never_a_live_owner() {
    let mut world = World::new();
    let uuid = Uuid::from_u128(7);
    let actor = owner(&mut world, Some("123"), uuid, Some(uuid.to_string()));
    world
        .entity_mut(actor)
        .insert(crate::entity::components::Position(glam::DVec3::new(
            4.0, 65.62, 4.0,
        )));
    let attribution = DamageAttribution::capture(&world, Some(actor), Some(actor));
    assert_eq!(
        attribution.owner().unwrap().feet_position(),
        Some(glam::DVec3::new(4.0, 64.0, 4.0))
    );
    world
        .get_mut::<crate::entity::components::Position>(actor)
        .unwrap()
        .0
        .x = 9.0;
    assert_eq!(attribution.live_owner(&world), Some(actor));
    assert_eq!(attribution.owner().unwrap().feet_position().unwrap().x, 4.0);
    world.get_mut::<PlayerSession>(actor).unwrap().session_id = 8;
    assert_eq!(attribution.live_owner(&world), None);
    assert_eq!(attribution.authenticated_owner().unwrap().xuid(), 123);
}

#[test]
fn absent_or_nil_uuid_and_nonplayer_sources_never_claim_an_authenticated_account() {
    for player in [true, false] {
        for uuid in [None, Some(Uuid::nil())] {
            let mut world = World::new();
            let actor = owner(
                &mut world,
                Some("123"),
                Uuid::nil(),
                Some(Uuid::nil().to_string()),
            );
            if uuid.is_none() {
                world.entity_mut(actor).remove::<PlayerUuid>();
            }
            if !player {
                world.entity_mut(actor).remove::<Player>();
            }
            assert!(
                DamageAttribution::capture(&world, Some(actor), Some(actor))
                    .authenticated_owner()
                    .is_none()
            );
        }
    }
    let mut world = World::new();
    let uuid = Uuid::from_u128(7);
    let actor = owner(&mut world, Some("123"), uuid, Some(uuid.to_string()));
    world.entity_mut(actor).remove::<Player>();
    assert!(
        DamageAttribution::capture(&world, Some(actor), Some(actor))
            .authenticated_owner()
            .is_none()
    );
}
