//! Detached typed single-container construction uses the receipt-backed column coordinator.

use super::*;
use crate::{
    block_entity::ContainerKind,
    item::{ItemStack, enchant},
    registry::block::StateValue,
    server::game::{container_creation::Creation, types::ItemRegistryResource},
};
use sha2::{Digest, Sha256};
use zuri_nbt::{NBTTag, encoding::LittleEndian};

type Failure = wit::DurableBlockError;
type ItemValue = Arc<values::Value<ItemStack>>;

/// Builders contain only charged owned observations and immutable slot values.
pub(in crate::plugin::manager) struct Plan {
    world: WorldReference,
    cells: Vec<OwnedReplacement>,
    kind: ContainerKind,
    slots: [Option<ItemValue>; 27],
    staged: bool,
}

/// Reserve every single-chest neighbor rather than silently preparing a pair.
fn neighbors(position: IVec3, kind: ContainerKind) -> Result<Vec<IVec3>, Failure> {
    if kind != ContainerKind::Chest {
        return Ok(Vec::new());
    }
    [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
        .into_iter()
        .map(|offset| {
            Ok(IVec3::new(
                position
                    .x
                    .checked_add(offset.x)
                    .ok_or(Failure::Block(Error::OutOfBounds))?,
                position.y,
                position
                    .z
                    .checked_add(offset.z)
                    .ok_or(Failure::Block(Error::OutOfBounds))?,
            ))
        })
        .collect()
}

/// Resolve a native state from typed orientation without a guest block-actor buffer.
fn state(
    registry: &BlockRegistry,
    kind: ContainerKind,
    facing: wit::BlockFace,
) -> Result<u32, Failure> {
    let values = match kind {
        ContainerKind::Barrel => vec![
            (
                "facing_direction",
                StateValue::Int(match facing {
                    wit::BlockFace::Down => 0,
                    wit::BlockFace::Up => 1,
                    wit::BlockFace::North => 2,
                    wit::BlockFace::South => 3,
                    wit::BlockFace::West => 4,
                    wit::BlockFace::East => 5,
                }),
            ),
            ("open_bit", StateValue::Byte(0)),
        ],
        ContainerKind::Chest => vec![(
            "minecraft:cardinal_direction",
            StateValue::Str(match facing {
                wit::BlockFace::North => "north",
                wit::BlockFace::South => "south",
                wit::BlockFace::West => "west",
                wit::BlockFace::East => "east",
                _ => return Err(Failure::Access(wit::AccessError::InvalidArgument)),
            }),
        )],
        _ => return Err(Failure::Block(Error::Unsupported)),
    };
    let identifier = if kind == ContainerKind::Barrel {
        "minecraft:barrel"
    } else {
        "minecraft:chest"
    };
    let block = registry
        .get_by_name(identifier)
        .ok_or(Failure::Block(Error::Unsupported))?;
    let default = registry
        .default_state_id_by_name(identifier)
        .ok_or(Failure::Block(Error::Unsupported))?;
    if values.iter().any(|(name, _)| {
        !block
            .data
            .properties
            .iter()
            .any(|property| property.name == *name)
    }) {
        return Err(Failure::Block(Error::Unsupported));
    }
    (block.min_state_id..=block.max_state_id)
        .find(|state| {
            block.data.properties.iter().all(|property| {
                let expected = values
                    .iter()
                    .find(|(name, _)| *name == property.name)
                    .map(|(_, value)| *value)
                    .or_else(|| block.state_value(default, property.name));
                block.state_value(*state, property.name) == expected
            })
        })
        .ok_or(Failure::Block(Error::Unsupported))
}

/// Plan only from an original loaded actor-free family, keeping every footprint witness.
pub(in crate::plugin::manager) fn plan(
    context: &mut HostContext,
    tx: u32,
    expected: u32,
    kind: wit::ContainerCreateKind,
    facing: wit::BlockFace,
) -> Result<Resource<wit::ContainerPlan>, Failure> {
    let scope = context.transaction(tx).map_err(Failure::Access)?;
    context
        .require(PluginCapability::WriteDurableContainers)
        .map_err(Failure::Access)?;
    let Some(Entry::BlockCell(cell)) = context.resources.entries.get(&expected) else {
        return Err(Failure::Access(wit::AccessError::InvalidArgument));
    };
    let cell = cell.clone();
    if cell.world != scope.world {
        return Err(Failure::Access(wit::AccessError::WrongWorld));
    }
    if cell.facts.edit_support != wit::BlockEditSupport::Supported {
        return Err(Failure::Block(Error::Unsupported));
    }
    cell.current(unsafe { context.world() })
        .map_err(Failure::Block)?;
    let kind = match kind {
        wit::ContainerCreateKind::Barrel => ContainerKind::Barrel,
        wit::ContainerCreateKind::SingleChest => ContainerKind::Chest,
    };
    let primary = state(&cell.registry, kind, facing)?;
    let mut cells = Vec::new();
    let positions = neighbors(cell.position, kind)?;
    cells.push(
        context
            .resources
            .value_budget
            .own(
                Replacement {
                    expected: cell.clone(),
                    proposed: Layers {
                        primary,
                        secondary: cell.layers.secondary,
                    },
                },
                REPLACEMENT_BYTES,
            )
            .map_err(Failure::Access)?,
    );
    for position in positions {
        let resource = capture(
            context,
            tx,
            wit::BlockPos {
                x: position.x,
                y: position.y,
                z: position.z,
            },
        )
        .map_err(Failure::Block)?;
        let Some(Entry::BlockCell(neighbor)) = context.resources.entries.remove(&resource.rep())
        else {
            unreachable!()
        };
        if neighbor.facts.edit_support != wit::BlockEditSupport::Supported {
            return Err(Failure::Block(Error::Unsupported));
        }
        neighbor
            .current(unsafe { context.world() })
            .map_err(Failure::Block)?;
        cells.push(
            context
                .resources
                .value_budget
                .own(
                    Replacement {
                        proposed: neighbor.layers,
                        expected: neighbor,
                    },
                    REPLACEMENT_BYTES,
                )
                .map_err(Failure::Access)?,
        );
    }
    context
        .resources
        .allocate(
            Entry::ContainerPlan(Plan {
                world: scope.world,
                cells,
                kind,
                slots: std::array::from_fn(|_| None),
                staged: false,
            }),
            context.api_limits.max_owned_resources,
        )
        .map(Resource::new_own)
        .map_err(Failure::Access)
}

/// Validate callback scope and retain historical plan facts without recapturing them.
fn candidate(context: &mut HostContext, tx: u32, rep: u32) -> Result<(), Failure> {
    let scope = context.transaction(tx).map_err(Failure::Access)?;
    context
        .require(PluginCapability::WriteDurableContainers)
        .map_err(Failure::Access)?;
    let Some(Entry::ContainerPlan(plan)) = context.resources.entries.get(&rep) else {
        return Err(Failure::Access(wit::AccessError::InvalidArgument));
    };
    if plan.staged {
        return Err(Failure::Block(Error::AlreadyStaged));
    }
    if plan.world != scope.world {
        return Err(Failure::Access(wit::AccessError::WrongWorld));
    }
    Ok(())
}

/// Hash every semantic native field; only transient network IDs and map order are omitted.
fn item(hash: &mut Sha256, stack: &ItemStack, registry: &BlockRegistry) -> Result<(), Failure> {
    use crate::storage::column_writer::fingerprint;
    if stack.is_empty() {
        hash.update([0]);
        return Ok(());
    }
    if !enchant::valid_user_data(stack) {
        return Err(Failure::Access(wit::AccessError::InvalidArgument));
    }
    hash.update([1]);
    hash.update((stack.item_id().len() as u64).to_le_bytes());
    hash.update(stack.item_id().as_bytes());
    hash.update([stack.count]);
    hash.update(stack.damage.to_le_bytes());
    if stack.runtime_id != 0 {
        hash.update([1]);
        let value = super::super::super::blocks::registry_value(registry, stack.runtime_id as u32)
            .map_err(Failure::Access)?;
        hash.update((value.identifier.len() as u64).to_le_bytes());
        hash.update(value.identifier.as_bytes());
        hash.update((value.properties.len() as u64).to_le_bytes());
        for property in value.properties {
            hash.update((property.name.len() as u64).to_le_bytes());
            hash.update(property.name.as_bytes());
            match property.value {
                wit::BlockPropertyValue::Byte(value) => hash.update([1, value]),
                wit::BlockPropertyValue::Integer(value) => {
                    hash.update([2]);
                    hash.update(value.to_le_bytes());
                }
                wit::BlockPropertyValue::Text(value) => {
                    hash.update([3]);
                    hash.update((value.len() as u64).to_le_bytes());
                    hash.update(value.as_bytes());
                }
            }
        }
    } else {
        hash.update([0]);
    }
    let buffer = stack.user_data();
    let marker = i16::from_le_bytes([buffer[0], buffer[1]]);
    hash.update(marker.to_le_bytes());
    let tail = if marker == -1 {
        let mut input = &buffer[3..];
        let tag = NBTTag::read(&mut input, LittleEndian)
            .map_err(|_| Failure::Access(wit::AccessError::InvalidArgument))?;
        fingerprint::tag(hash, &tag, 0).map_err(durable::failure)?;
        input
    } else {
        &buffer[2..]
    };
    hash.update((tail.len() as u64).to_le_bytes());
    hash.update(tail);
    Ok(())
}

/// Logical intent excludes newly captured whole-column or moving-actor payloads.
fn digest(context: &HostContext, plan: &Plan) -> Result<[u8; 32], Failure> {
    let mut hash = Sha256::new();
    hash.update(b"unastar-container-create-v1");
    hash.update(durable::digest(
        context,
        unsafe { context.world() },
        &plan.cells,
    )?);
    for value in &plan.slots {
        item(
            &mut hash,
            value.as_ref().map_or(&ItemStack::EMPTY, |value| &**value),
            &plan.cells[0].expected.registry,
        )?;
    }
    Ok(hash.finalize().into())
}

/// Compute before persisting a caller intent, without allocating native IO or pins.
pub(in crate::plugin::manager) fn intent(
    context: &mut HostContext,
    tx: u32,
    rep: u32,
) -> Result<Vec<u8>, Failure> {
    candidate(context, tx, rep)?;
    let Some(Entry::ContainerPlan(plan)) = context.resources.entries.get(&rep) else {
        unreachable!()
    };
    digest(context, plan).map(Vec::from)
}

/// New actor authority is typed and bounded before shared durable staging is reached.
pub(in crate::plugin::manager) fn stage(
    context: &mut HostContext,
    tx: u32,
    key: &str,
    rep: u32,
) -> Result<Resource<wit::DurableBlockRequest>, Failure> {
    candidate(context, tx, rep)?;
    let Some(Entry::ContainerPlan(plan)) = context.resources.entries.get(&rep) else {
        unreachable!()
    };
    let digest = digest(context, plan)?;
    let registry = plan.cells[0].expected.registry.clone();
    let items = unsafe { context.world() }
        .get_resource::<ItemRegistryResource>()
        .ok_or(Failure::Access(wit::AccessError::Unavailable))?;
    let items = items.0.clone();
    let mut bytes = 8192usize;
    for value in plan.slots.iter().flatten() {
        if value.is_empty() {
            continue;
        }
        if value.item().is_none_or(|entry| {
            items
                .item_type(value.item_id())
                .is_none_or(|current| !std::ptr::eq(entry, &*current))
        }) {
            return Err(Failure::Conflict);
        }
        // Standard item tags and the full native envelope both enter actor storage.
        bytes += 4 * super::super::inventory::native_stack_bytes(value);
    }
    let reservation = context
        .resources
        .value_budget
        .reserve(bytes)
        .map_err(Failure::Access)?;
    let slots = plan
        .slots
        .iter()
        .map(|value| {
            value
                .as_ref()
                .map_or_else(ItemStack::empty, |value| (**value).clone())
        })
        .collect();
    let creation = Creation::new(
        plan.kind,
        plan.cells[0].expected.position,
        plan.cells[0].proposed.primary,
        slots,
        registry,
        items,
    )
    .map_err(durable::failure)?;
    if creation.retained_bytes() > reservation.bytes() {
        return Err(Failure::Access(wit::AccessError::LimitExceeded));
    }
    let creation = reservation.bind(creation);
    let payload = durable::Payload {
        world: plan.world,
        cells: plan.cells.clone(),
        creation: Some(creation),
        digest,
    };
    let (request, staged) = durable::enqueue(context, key, payload)?;
    if staged {
        let Some(Entry::ContainerPlan(plan)) = context.resources.entries.get_mut(&rep) else {
            unreachable!()
        };
        plan.staged = true;
        plan.cells.clear();
        plan.slots = std::array::from_fn(|_| None);
    }
    Ok(request)
}

impl wit::HostContainerPlan for HostContext {
    fn set_slot(
        &mut self,
        plan: Resource<wit::ContainerPlan>,
        slot: u8,
        item: Resource<wit::Stack>,
    ) -> Result<(), Failure> {
        self.operation().map_err(Failure::Access)?;
        self.require(PluginCapability::WriteDurableContainers)
            .map_err(Failure::Access)?;
        self.require(PluginCapability::ItemValues)
            .map_err(Failure::Access)?;
        if slot >= 27 {
            return Err(Failure::Access(wit::AccessError::InvalidArgument));
        }
        let Some(Entry::Item(value)) = self.resources.entries.get(&item.rep()) else {
            return Err(Failure::Access(wit::AccessError::InvalidArgument));
        };
        let value = value.clone();
        let Some(Entry::ContainerPlan(plan)) = self.resources.entries.get_mut(&plan.rep()) else {
            return Err(Failure::Access(wit::AccessError::InvalidArgument));
        };
        if plan.staged {
            return Err(Failure::Block(Error::AlreadyStaged));
        }
        plan.slots[usize::from(slot)] = Some(value);
        Ok(())
    }
    fn drop(&mut self, plan: Resource<wit::ContainerPlan>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&plan.rep());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::stack;

    /// A deliberately ordered encoded compound proves byte order is not logical identity.
    fn ordered(names: &[&str]) -> ItemStack {
        let mut data = vec![255, 255, 1, 10, 0, 0];
        for name in names {
            data.push(3);
            data.extend_from_slice(&(name.len() as u16).to_le_bytes());
            data.extend_from_slice(name.as_bytes());
            data.extend_from_slice(&17i32.to_le_bytes());
        }
        data.push(0);
        data.extend_from_slice(&[0; 8]);
        stack("minecraft:paper", 3).with_nbt(data)
    }
    /// Use exactly the native semantic digest helper used by accepted plans.
    fn hash(stack: &ItemStack) -> [u8; 32] {
        let mut hash = Sha256::new();
        item(&mut hash, stack, BlockRegistry::vanilla()).unwrap();
        hash.finalize().into()
    }

    #[test]
    fn compound_order_and_transient_network_ids_do_not_change_container_intent() {
        let first = ordered(&["alpha", "beta"]).with_net_id(12);
        let second = ordered(&["beta", "alpha"]).with_net_id(13);
        assert_ne!(first.nbt, second.nbt);
        assert_eq!(hash(&first), hash(&second));
        assert_ne!(hash(&first), hash(&first.clone().with_count(2)));
        assert_ne!(hash(&first), hash(&first.clone().with_damage(1)));
    }
    #[test]
    fn block_restrictions_shield_tick_and_typed_values_are_part_of_the_logical_intent() {
        let plain = stack("minecraft:shield", 1);
        let restricted = enchant::with_block_lists(
            &plain,
            &["minecraft:stone".into()],
            &["minecraft:dirt".into()],
        );
        let mut data = restricted.user_data().to_vec();
        let tail = data.len() - 8;
        data[tail..].copy_from_slice(&17i64.to_le_bytes());
        let active = restricted.clone().with_nbt(data);
        assert_ne!(hash(&plain), hash(&restricted));
        assert_ne!(hash(&restricted), hash(&active));
        let tag = enchant::with_item_tag(
            &active,
            zuri_nbt::tag::Compound(
                [("value".into(), NBTTag::Short(zuri_nbt::tag::Short(1)))].into(),
            ),
        );
        let different = enchant::with_item_tag(
            &active,
            zuri_nbt::tag::Compound([("value".into(), NBTTag::Int(zuri_nbt::tag::Int(1)))].into()),
        );
        assert_ne!(hash(&tag), hash(&different));
    }
}
