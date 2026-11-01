//! Detached-value tests exercise the real bounded host resource implementation.

use super::super::super::wit_bindings::unastar::plugin::types::HostStack;
use super::*;
use crate::plugin::manifest::{PluginId, PluginLimits};
use wasmtime_wasi::WasiCtx;

/// Construct an isolated host context with no live world pointer or player mutation API.
fn context(maximum: u32) -> HostContext {
    let mut context = HostContext::new(
        WasiCtx::builder().build(),
        PluginId("example.items".into()),
        &PluginLimits {
            max_owned_resources: maximum,
            ..Default::default()
        },
    );
    context.capabilities.insert(PluginCapability::ItemValues);
    context
}

/// Build item values through an actual frozen test registry.
fn stack(identifier: &str, maximum: u8) -> Stack {
    crate::registry::item::test_support::registry(&[(identifier, 1, maximum)])
        .stack(identifier, 1)
        .unwrap()
}

/// Borrow an owned value for one host import without transferring its ownership.
fn borrow(rep: u32) -> Resource<wit::Stack> {
    Resource::new_borrow(rep)
}

#[test]
fn detached_edits_keep_original_values_and_typed_properties_independent() {
    let mut host = context(32);
    let original = host
        .own_item(stack("minecraft:diamond_sword", 1))
        .unwrap()
        .rep();
    let named = HostStack::with_name(&mut host, borrow(original), Some("§6Example".into()))
        .unwrap()
        .rep();
    let lore = HostStack::with_lore(
        &mut host,
        borrow(named),
        vec!["First".into(), "Second".into()],
    )
    .unwrap()
    .rep();
    let enchanted = HostStack::with_enchantments(
        &mut host,
        borrow(lore),
        vec![wit::Enchantment { id: 17, level: 3 }],
    )
    .unwrap()
    .rep();
    let signed = HostStack::with_value(
        &mut host,
        borrow(enchanted),
        vec!["extension".into(), "signature".into()],
        Some(wit::ItemValue::Bytes(vec![7; 32])),
    )
    .unwrap()
    .rep();
    assert_eq!(HostStack::name(&mut host, borrow(original)).unwrap(), None);
    assert_eq!(
        HostStack::name(&mut host, borrow(signed))
            .unwrap()
            .as_deref(),
        Some("§6Example")
    );
    assert_eq!(
        HostStack::lore(&mut host, borrow(signed)).unwrap(),
        ["First", "Second"]
    );
    let enchants = HostStack::enchantments(&mut host, borrow(signed)).unwrap();
    assert!(matches!(
        enchants.as_slice(),
        [wit::Enchantment { id: 17, level: 3 }]
    ));
    assert!(
        matches!(HostStack::value(&mut host, borrow(signed), vec!["extension".into(),"signature".into()]).unwrap(), Some(wit::ItemValue::Bytes(bytes)) if bytes == vec![7;32])
    );
    assert_eq!(host.item_value(original).unwrap().nbt, None);
    assert!(host.world_ptr.is_null());
}

#[test]
fn scalar_widths_removal_and_noncompound_parents_are_explicit() {
    let mut host = context(32);
    let base = host.own_item(stack("minecraft:paper", 64)).unwrap().rep();
    for (name, value, expected) in [
        (
            "byte",
            wit::ItemValue::Byte(255),
            NBTTag::Byte(tag::Byte(255)),
        ),
        (
            "short",
            wit::ItemValue::Short(-7),
            NBTTag::Short(tag::Short(-7)),
        ),
        (
            "int",
            wit::ItemValue::Int(-123),
            NBTTag::Int(tag::Int(-123)),
        ),
        (
            "long",
            wit::ItemValue::Long(i64::MAX),
            NBTTag::Long(tag::Long(i64::MAX)),
        ),
    ] {
        let changed =
            HostStack::with_value(&mut host, borrow(base), vec![name.into()], Some(value))
                .unwrap()
                .rep();
        let compound = crate::item::enchant::item_tag(&host.item_value(changed).unwrap()).unwrap();
        assert_eq!(compound.0[name], expected);
        assert!(matches!(
            HostStack::with_value(
                &mut host,
                borrow(changed),
                vec![name.into(), "child".into()],
                Some(wit::ItemValue::Byte(1))
            ),
            Err(wit::AccessError::InvalidArgument)
        ));
        let removed = HostStack::with_value(&mut host, borrow(changed), vec![name.into()], None)
            .unwrap()
            .rep();
        assert!(
            HostStack::value(&mut host, borrow(removed), vec![name.into()])
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn equivalent_typed_construction_has_identical_native_receipt_images() {
    let build = |reverse: bool| {
        let mut host = context(32);
        let mut value = host.own_item(stack("minecraft:paper", 64)).unwrap().rep();
        let mut names = ["zeta", "alpha", "middle"];
        if reverse {
            names.reverse();
        }
        for name in names {
            value = HostStack::with_value(
                &mut host,
                borrow(value),
                vec!["instance".into(), name.into()],
                Some(wit::ItemValue::Long(42)),
            )
            .unwrap()
            .rep();
        }
        crate::storage::StoredItemStack::from(&host.item_value(value).unwrap())
    };
    assert_eq!(build(false), build(true));
    assert_eq!(build(false), build(false));
}

#[test]
fn removing_last_property_restores_default_and_enchantment_order_is_canonical() {
    let mut host = context(32);
    let base = host.own_item(stack("minecraft:paper", 64)).unwrap().rep();
    let named = HostStack::with_name(&mut host, borrow(base), Some("Example".into()))
        .unwrap()
        .rep();
    let cleared = HostStack::with_name(&mut host, borrow(named), None)
        .unwrap()
        .rep();
    assert_eq!(
        host.item_value(base).unwrap(),
        host.item_value(cleared).unwrap()
    );
    let first = HostStack::with_enchantments(
        &mut host,
        borrow(base),
        vec![
            wit::Enchantment { id: 17, level: 3 },
            wit::Enchantment { id: 15, level: 2 },
        ],
    )
    .unwrap()
    .rep();
    let second = HostStack::with_enchantments(
        &mut host,
        borrow(base),
        vec![
            wit::Enchantment { id: 15, level: 2 },
            wit::Enchantment { id: 17, level: 3 },
        ],
    )
    .unwrap()
    .rep();
    assert_eq!(
        host.item_value(first).unwrap(),
        host.item_value(second).unwrap()
    );
}

#[test]
fn resource_limits_and_released_values_do_not_reuse_old_representations() {
    let mut host = context(2);
    let first = host.own_item(stack("minecraft:paper", 64)).unwrap().rep();
    let second = HostStack::with_count(&mut host, borrow(first), 2)
        .unwrap()
        .rep();
    assert!(matches!(
        HostStack::with_count(&mut host, borrow(first), 3),
        Err(wit::AccessError::LimitExceeded)
    ));
    HostStack::drop(&mut host, Resource::new_own(second)).unwrap();
    let third = HostStack::with_count(&mut host, borrow(first), 3)
        .unwrap()
        .rep();
    assert!(third > second);
    assert!(matches!(
        HostStack::count(&mut host, borrow(second)),
        Err(wit::AccessError::InvalidArgument)
    ));
    assert_eq!(HostStack::count(&mut host, borrow(first)).unwrap(), 1);
    HostStack::drop(&mut host, Resource::new_own(third)).unwrap();
    HostStack::drop(&mut host, Resource::new_own(first)).unwrap();
    assert_eq!(host.resources.value_budget.used(), 0);
}

#[test]
fn invalid_fields_permissions_and_property_limits_do_not_allocate_values() {
    let mut host = context(32);
    let first = host
        .own_item(stack("minecraft:diamond_sword", 1))
        .unwrap()
        .rep();
    let before = host.resources.entries.len();
    assert!(matches!(
        HostStack::with_count(&mut host, borrow(first), 2),
        Err(wit::AccessError::InvalidArgument)
    ));
    assert!(matches!(
        HostStack::with_damage(&mut host, borrow(first), u16::MAX),
        Err(wit::AccessError::InvalidArgument)
    ));
    assert!(matches!(
        HostStack::with_value(&mut host, borrow(first), vec![], None),
        Err(wit::AccessError::InvalidArgument)
    ));
    assert!(matches!(
        HostStack::with_name(
            &mut host,
            borrow(first),
            Some("x".repeat(MAX_PROPERTY_BYTES + 1))
        ),
        Err(wit::AccessError::LimitExceeded)
    ));
    assert_eq!(host.resources.entries.len(), before);
    host.capabilities.clear();
    assert!(matches!(
        HostStack::count(&mut host, borrow(first)),
        Err(wit::AccessError::PermissionDenied)
    ));
    assert!(matches!(
        HostStack::with_count(&mut host, borrow(first), 1),
        Err(wit::AccessError::PermissionDenied)
    ));
}

#[test]
fn native_retained_bytes_are_bounded_and_dropping_values_releases_capacity() {
    let mut host = context(4096);
    let data = metadata::replace(
        &stack("minecraft:paper", 64),
        tag::Compound(
            [(
                "bytes".into(),
                NBTTag::ByteArray(tag::ByteArray(vec![1; 8192])),
            )]
            .into(),
        ),
    )
    .unwrap();
    let mut values = Vec::new();
    loop {
        match host.own_item(data.clone()) {
            Ok(value) => values.push(value.rep()),
            Err(wit::AccessError::LimitExceeded) => break,
            _ => panic!("unexpected value failure"),
        }
    }
    assert!(values.len() < 4096);
    let first = values.remove(0);
    HostStack::drop(&mut host, Resource::new_own(first)).unwrap();
    assert!(host.own_item(data).is_ok());
}

#[test]
fn canonical_encoding_preserves_all_scalar_types_lists_and_native_restrictions() {
    let original = crate::item::enchant::with_block_lists(
        &stack("minecraft:shield", 1),
        &["minecraft:stone".into()],
        &["minecraft:dirt".into()],
    );
    let compound = tag::Compound(
        [
            (
                "list".into(),
                NBTTag::List(tag::List(vec![NBTTag::Compound(tag::Compound(
                    [
                        ("z".into(), NBTTag::Int(tag::Int(1))),
                        ("a".into(), NBTTag::Int(tag::Int(2))),
                    ]
                    .into(),
                ))])),
            ),
            ("float".into(), NBTTag::Float(tag::Float(1.25))),
            ("double".into(), NBTTag::Double(tag::Double(-2.5))),
            ("ints".into(), NBTTag::IntArray(tag::IntArray(vec![-1, 2]))),
            (
                "longs".into(),
                NBTTag::LongArray(tag::LongArray(vec![i64::MIN, i64::MAX])),
            ),
        ]
        .into(),
    );
    let updated = metadata::replace(&original, compound.clone()).unwrap();
    assert_eq!(crate::item::enchant::item_tag(&updated), Some(compound));
    assert_eq!(
        crate::item::enchant::block_lists(&updated),
        crate::item::enchant::block_lists(&original)
    );
    assert!(crate::item::enchant::valid_user_data(&updated));
    assert_eq!(
        &updated.user_data()[updated.user_data().len() - 8..],
        &original.user_data()[original.user_data().len() - 8..]
    );
}
