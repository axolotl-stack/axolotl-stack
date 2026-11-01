//! Detached item values share resource limits but never write live inventory slots.

use super::*;
use crate::item::ItemStack as Stack;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};
use zuri_nbt::{NBTTag, tag};

#[path = "items/metadata.rs"]
mod metadata;

const MAX_VALUE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ITEM_BYTES: usize = 131_072;
const MAX_PROPERTY_BYTES: usize = 4096;
const MAX_LORE: usize = 64;
const MAX_ENCHANTMENTS: usize = 128;

/// Account for retained native values independently of guest linear-memory limits.
fn size(stack: &Stack) -> usize {
    128 + stack.item_id().len() + stack.nbt.as_ref().map_or(0, |bytes| bytes.len())
}

impl HostContext {
    /// Resolve a registry identity only inside the supplied world transaction.
    pub(super) fn create_item(
        &mut self,
        tx: u32,
        identifier: &str,
        count: u8,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        self.validate_transaction(tx)?;
        self.require(PluginCapability::ItemValues)?;
        let stack = if count == 0 {
            Stack::empty()
        } else {
            if identifier.len() > 256 {
                return Err(wit::AccessError::InvalidArgument);
            }
            let world = unsafe { self.world() };
            let items = world
                .get_resource::<ItemRegistryResource>()
                .ok_or(wit::AccessError::Unavailable)?;
            let blocks = world
                .get_resource::<BlockRegistryResource>()
                .ok_or(wit::AccessError::Unavailable)?;
            items
                .0
                .stack_with_block(identifier, count, &blocks.0)
                .ok_or(wit::AccessError::InvalidArgument)?
        };
        self.own_item(stack)
    }

    /// Clone cheap immutable data without requiring a world pointer or a live player.
    fn item_value(&mut self, rep: u32) -> Result<Stack, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ItemValues)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::Item(stack)) => Ok(stack.clone()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }

    /// Charge resource and retained-byte limits before exposing an owned value.
    pub(in crate::plugin::manager) fn own_item(
        &mut self,
        stack: Stack,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        let stack = if stack.is_empty() {
            Stack::empty()
        } else {
            stack
        };
        if !stack.is_empty()
            && (stack
                .item()
                .is_none_or(|item| stack.count > item.max_stack_size())
                || stack.damage < 0
                || stack
                    .nbt
                    .as_ref()
                    .is_some_and(|bytes| bytes.len() > MAX_ITEM_BYTES)
                || !crate::item::enchant::valid_user_data(&stack))
        {
            return Err(wit::AccessError::InvalidArgument);
        }
        let bytes = size(&stack);
        let total = self
            .resources
            .item_value_bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_VALUE_BYTES)
            .ok_or(wit::AccessError::LimitExceeded)?;
        let rep = self
            .resources
            .allocate(Entry::Item(stack), self.api_limits.max_owned_resources)?;
        self.resources.item_value_bytes = total;
        Ok(Resource::new_own(rep))
    }

    /// Edit one detached compound while preserving its other native instance data.
    fn edit_item(
        &mut self,
        rep: u32,
        edit: impl FnOnce(&mut tag::Compound) -> Result<(), wit::AccessError>,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        let stack = self.item_value(rep)?;
        if stack.is_empty() {
            return Err(wit::AccessError::InvalidArgument);
        }
        let mut compound = crate::item::enchant::item_tag(&stack).unwrap_or_default();
        edit(&mut compound)?;
        self.own_item(metadata::replace(&stack, compound)?)
    }
}

/// Validate bounded field paths before reading or creating nested compounds.
fn path(path: &[String]) -> Result<(), wit::AccessError> {
    if path.is_empty()
        || path.len() > 8
        || path
            .iter()
            .any(|name| name.is_empty() || name.len() > 64 || name.chars().any(char::is_control))
    {
        return Err(wit::AccessError::InvalidArgument);
    }
    Ok(())
}

/// Read a nested value without interpreting a non-compound parent as missing data.
fn at<'a>(
    compound: &'a tag::Compound,
    path: &[String],
) -> Result<Option<&'a NBTTag>, wit::AccessError> {
    let (name, rest) = path
        .split_first()
        .ok_or(wit::AccessError::InvalidArgument)?;
    let Some(value) = compound.0.get(name) else {
        return Ok(None);
    };
    if rest.is_empty() {
        return Ok(Some(value));
    }
    match value {
        NBTTag::Compound(child) => at(child, rest),
        _ => Err(wit::AccessError::InvalidArgument),
    }
}

/// Replace or remove a typed value; absent parents are created only for insertion.
fn put(
    compound: &mut tag::Compound,
    path: &[String],
    value: Option<NBTTag>,
) -> Result<(), wit::AccessError> {
    let (name, rest) = path
        .split_first()
        .ok_or(wit::AccessError::InvalidArgument)?;
    if rest.is_empty() {
        if let Some(value) = value {
            compound.0.insert(name.clone(), value);
        } else {
            compound.0.remove(name);
        }
        return Ok(());
    }
    if !compound.0.contains_key(name) {
        if value.is_none() {
            return Ok(());
        }
        compound
            .0
            .insert(name.clone(), NBTTag::Compound(tag::Compound::default()));
    }
    let removing = value.is_none();
    match compound.0.get_mut(name) {
        Some(NBTTag::Compound(child)) => put(child, rest, value),
        _ => Err(wit::AccessError::InvalidArgument),
    }?;
    if removing
        && matches!(compound.0.get(name), Some(NBTTag::Compound(child)) if child.0.is_empty())
    {
        compound.0.remove(name);
    }
    Ok(())
}

/// Scalars retain their exact width so authenticated application fields stay stable.
fn scalar(value: wit::ItemValue) -> Result<NBTTag, wit::AccessError> {
    Ok(match value {
        wit::ItemValue::Byte(value) => NBTTag::Byte(tag::Byte(value)),
        wit::ItemValue::Short(value) => NBTTag::Short(tag::Short(value)),
        wit::ItemValue::Int(value) => NBTTag::Int(tag::Int(value)),
        wit::ItemValue::Long(value) => NBTTag::Long(tag::Long(value)),
        wit::ItemValue::Text(value) if value.len() <= MAX_PROPERTY_BYTES => {
            NBTTag::String(tag::String(value))
        }
        wit::ItemValue::Bytes(value) if value.len() <= MAX_PROPERTY_BYTES => {
            NBTTag::ByteArray(tag::ByteArray(value))
        }
        _ => return Err(wit::AccessError::LimitExceeded),
    })
}

/// Complex tags remain intact; a scalar query never silently changes their type.
fn property(value: &NBTTag) -> Result<wit::ItemValue, wit::AccessError> {
    Ok(match value {
        NBTTag::Byte(value) => wit::ItemValue::Byte(value.0),
        NBTTag::Short(value) => wit::ItemValue::Short(value.0),
        NBTTag::Int(value) => wit::ItemValue::Int(value.0),
        NBTTag::Long(value) => wit::ItemValue::Long(value.0),
        NBTTag::String(value) if value.0.len() <= MAX_PROPERTY_BYTES => {
            wit::ItemValue::Text(value.0.clone())
        }
        NBTTag::ByteArray(value) if value.0.len() <= MAX_PROPERTY_BYTES => {
            wit::ItemValue::Bytes(value.0.clone())
        }
        _ => return Err(wit::AccessError::InvalidArgument),
    })
}

impl super::super::wit_bindings::unastar::plugin::types::HostStack for HostContext {
    fn identifier(&mut self, item: Resource<wit::Stack>) -> Result<String, wit::AccessError> {
        self.item_value(item.rep())
            .map(|item| item.item_id().into())
    }
    fn count(&mut self, item: Resource<wit::Stack>) -> Result<u8, wit::AccessError> {
        self.item_value(item.rep()).map(|item| item.count)
    }
    fn max_count(&mut self, item: Resource<wit::Stack>) -> Result<u8, wit::AccessError> {
        self.item_value(item.rep())
            .map(|item| item.item().map_or(0, |item| item.max_stack_size()))
    }
    fn damage(&mut self, item: Resource<wit::Stack>) -> Result<u16, wit::AccessError> {
        self.item_value(item.rep()).and_then(|item| {
            u16::try_from(item.damage).map_err(|_| wit::AccessError::InvalidArgument)
        })
    }
    fn with_count(
        &mut self,
        item: Resource<wit::Stack>,
        count: u8,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        let item = self.item_value(item.rep())?;
        if count > 0 && item.is_empty() {
            return Err(wit::AccessError::InvalidArgument);
        }
        self.own_item(item.with_count(count))
    }
    fn with_damage(
        &mut self,
        item: Resource<wit::Stack>,
        damage: u16,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        let item = self.item_value(item.rep())?;
        if item.is_empty() {
            return Err(wit::AccessError::InvalidArgument);
        }
        let damage = i16::try_from(damage).map_err(|_| wit::AccessError::InvalidArgument)?;
        self.own_item(item.with_damage(damage))
    }
    fn value(
        &mut self,
        item: Resource<wit::Stack>,
        names: Vec<String>,
    ) -> Result<Option<wit::ItemValue>, wit::AccessError> {
        path(&names)?;
        let item = self.item_value(item.rep())?;
        let compound = crate::item::enchant::item_tag(&item).unwrap_or_default();
        at(&compound, &names)?.map(property).transpose()
    }
    fn with_value(
        &mut self,
        item: Resource<wit::Stack>,
        names: Vec<String>,
        value: Option<wit::ItemValue>,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        path(&names)?;
        let value = value.map(scalar).transpose()?;
        self.edit_item(item.rep(), |compound| put(compound, &names, value))
    }
    fn name(&mut self, item: Resource<wit::Stack>) -> Result<Option<String>, wit::AccessError> {
        match self.value(item, vec!["display".into(), "Name".into()])? {
            Some(wit::ItemValue::Text(value)) => Ok(Some(value)),
            None => Ok(None),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn with_name(
        &mut self,
        item: Resource<wit::Stack>,
        name: Option<String>,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        self.with_value(
            item,
            vec!["display".into(), "Name".into()],
            name.map(wit::ItemValue::Text),
        )
    }
    fn lore(&mut self, item: Resource<wit::Stack>) -> Result<Vec<String>, wit::AccessError> {
        let item = self.item_value(item.rep())?;
        let compound = crate::item::enchant::item_tag(&item).unwrap_or_default();
        let Some(value) = at(&compound, &["display".into(), "Lore".into()])? else {
            return Ok(Vec::new());
        };
        let NBTTag::List(lines) = value else {
            return Err(wit::AccessError::InvalidArgument);
        };
        if lines.0.len() > MAX_LORE {
            return Err(wit::AccessError::LimitExceeded);
        }
        lines
            .0
            .iter()
            .map(|line| match line {
                NBTTag::String(value) if value.0.len() <= MAX_PROPERTY_BYTES => Ok(value.0.clone()),
                _ => Err(wit::AccessError::InvalidArgument),
            })
            .collect()
    }
    fn with_lore(
        &mut self,
        item: Resource<wit::Stack>,
        lines: Vec<String>,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        if lines.len() > MAX_LORE || lines.iter().any(|line| line.len() > MAX_PROPERTY_BYTES) {
            return Err(wit::AccessError::LimitExceeded);
        }
        let value = (!lines.is_empty()).then(|| {
            NBTTag::List(tag::List(
                lines
                    .into_iter()
                    .map(|line| NBTTag::String(tag::String(line)))
                    .collect(),
            ))
        });
        self.edit_item(item.rep(), |compound| {
            put(compound, &["display".into(), "Lore".into()], value)
        })
    }
    fn enchantments(
        &mut self,
        item: Resource<wit::Stack>,
    ) -> Result<Vec<wit::Enchantment>, wit::AccessError> {
        let item = self.item_value(item.rep())?;
        let compound = crate::item::enchant::item_tag(&item).unwrap_or_default();
        let Some(value) = compound.0.get("ench") else {
            return Ok(Vec::new());
        };
        let NBTTag::List(values) = value else {
            return Err(wit::AccessError::InvalidArgument);
        };
        if values.0.len() > MAX_ENCHANTMENTS {
            return Err(wit::AccessError::LimitExceeded);
        }
        values
            .0
            .iter()
            .map(|value| {
                let NBTTag::Compound(value) = value else {
                    return Err(wit::AccessError::InvalidArgument);
                };
                match (value.0.get("id"), value.0.get("lvl")) {
                    (Some(NBTTag::Short(id)), Some(NBTTag::Short(level)))
                        if id.0 >= 0 && level.0 > 0 =>
                    {
                        Ok(wit::Enchantment {
                            id: id.0,
                            level: level.0,
                        })
                    }
                    _ => Err(wit::AccessError::InvalidArgument),
                }
            })
            .collect()
    }
    fn with_enchantments(
        &mut self,
        item: Resource<wit::Stack>,
        mut values: Vec<wit::Enchantment>,
    ) -> Result<Resource<wit::Stack>, wit::AccessError> {
        if values.len() > MAX_ENCHANTMENTS
            || values.iter().any(|value| value.id < 0 || value.level <= 0)
            || values
                .iter()
                .map(|value| value.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != values.len()
        {
            return Err(wit::AccessError::InvalidArgument);
        }
        values.sort_by_key(|value| value.id);
        let value = (!values.is_empty()).then(|| {
            NBTTag::List(tag::List(
                values
                    .into_iter()
                    .map(|value| {
                        NBTTag::Compound(tag::Compound(
                            [
                                ("id".into(), NBTTag::Short(tag::Short(value.id))),
                                ("lvl".into(), NBTTag::Short(tag::Short(value.level))),
                            ]
                            .into(),
                        ))
                    })
                    .collect(),
            ))
        });
        self.edit_item(item.rep(), |compound| {
            put(compound, &["ench".into()], value)
        })
    }
    fn drop(&mut self, item: Resource<wit::Stack>) -> wasmtime::Result<()> {
        if !matches!(
            self.resources.entries.get(&item.rep()),
            Some(Entry::Item(_))
        ) {
            anyhow::bail!("invalid item value resource");
        }
        if let Some(Entry::Item(value)) = self.resources.entries.remove(&item.rep()) {
            self.resources.item_value_bytes =
                self.resources.item_value_bytes.saturating_sub(size(&value));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "items/tests.rs"]
mod tests;
