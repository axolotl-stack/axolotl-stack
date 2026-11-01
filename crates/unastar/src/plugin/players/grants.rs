//! Owned exact-quantity descriptions; successful callbacks alone may submit them.

use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::item_grants::{Grant, Placement};
use sha2::{Digest, Sha256};

pub(super) struct Plan {
    world: uuid::Uuid,
    dimension: i32,
    grants: Vec<Grant>,
}

pub(super) fn request_key(plugin: &str, request: &str) -> Result<String, wit::InventoryError> {
    super::inventory_transactions::request_key(plugin, request)?;
    let mut hash = Sha256::new();
    hash.update(b"unastar.grant.v1\0");
    hash.update((plugin.len() as u64).to_be_bytes());
    hash.update(plugin.as_bytes());
    hash.update((request.len() as u64).to_be_bytes());
    hash.update(request.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

impl HostContext {
    pub(super) fn new_grant_plan(
        &mut self,
        tx: u32,
    ) -> Result<Resource<wit::GrantPlan>, wit::InventoryError> {
        self.validate_transaction(tx)
            .map_err(wit::InventoryError::Access)?;
        self.require(PluginCapability::GrantItems)
            .map_err(wit::InventoryError::Access)?;
        self.require(PluginCapability::ReadInventory)
            .map_err(wit::InventoryError::Access)?;
        self.require(PluginCapability::ItemValues)
            .map_err(wit::InventoryError::Access)?;
        let world = unsafe { self.world() };
        let provider = world
            .get_resource::<crate::server::game::types::PlayerProviderResource>()
            .ok_or(wit::InventoryError::NotInstalled)?;
        if !provider.0.atomic_durable_items() {
            return Err(wit::InventoryError::UnsupportedProvider);
        }
        crate::server::game::inventory_effects::available(world)
            .map_err(super::inventory_transactions::error)?;
        if crate::server::game::item_grants::recovery_required(world) {
            return Err(wit::InventoryError::RecoveryRequired);
        }
        if !crate::server::game::item_grants::ready(world) {
            return Err(wit::InventoryError::Busy);
        }
        let identity = world
            .get_resource::<crate::world::WorldIdentity>()
            .ok_or(wit::InventoryError::UnsupportedProvider)?;
        self.own_grant_plan(Plan {
            world: identity.id,
            dimension: identity.dimension,
            grants: Vec::new(),
        })
        .map_err(wit::InventoryError::Access)
    }

    fn own_grant_plan(&mut self, plan: Plan) -> Result<Resource<wit::GrantPlan>, wit::AccessError> {
        let bytes = plan.grants.iter().fold(192usize, |used, grant| {
            used.saturating_add(super::inventory::native_stack_bytes(&grant.item) + 128)
        });
        let value = self.resources.value_budget.own(plan, bytes)?;
        let rep = self
            .resources
            .allocate(Entry::GrantPlan(value), self.api_limits.max_owned_resources)?;
        Ok(Resource::new_own(rep))
    }

    pub(super) fn grant_player_items(
        &mut self,
        player: u32,
        request: String,
        plan: u32,
    ) -> Result<Resource<wit::InventoryOperation>, wit::InventoryError> {
        self.operation().map_err(wit::InventoryError::Access)?;
        self.require(PluginCapability::GrantItems)
            .map_err(wit::InventoryError::Access)?;
        let grants = match self.resources.entries.get(&plan) {
            Some(Entry::GrantPlan(plan)) => {
                let world = unsafe { self.world() }
                    .get_resource::<crate::world::WorldIdentity>()
                    .ok_or(wit::InventoryError::UnsupportedProvider)?;
                if plan.world != world.id || plan.dimension != world.dimension {
                    return Err(wit::InventoryError::Access(wit::AccessError::WrongWorld));
                }
                plan.grants.clone()
            }
            _ => {
                return Err(wit::InventoryError::Access(
                    wit::AccessError::InvalidArgument,
                ));
            }
        };
        self.submit_grants(player, request, grants)
    }

    pub(super) fn grant_receipt(
        &mut self,
        player: u32,
        request: String,
    ) -> Result<Option<wit::InventoryReceipt>, wit::InventoryError> {
        let (_, player) = self
            .live_player(player)
            .map_err(wit::InventoryError::Access)?;
        self.require(PluginCapability::GrantItems)
            .map_err(wit::InventoryError::Access)?;
        let key = request_key(&self.plugin_id.0, &request)?;
        Ok(
            crate::server::game::inventory_effects::receipt(unsafe { self.world() }, player, &key)
                .map(|record| wit::InventoryReceipt {
                    revision: record.revision,
                    request_hash: record.request_hash.to_vec(),
                }),
        )
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostGrantPlan for HostContext {
    fn request_hash(
        &mut self,
        plan: Resource<wit::GrantPlan>,
    ) -> Result<Vec<u8>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::GrantItems)?;
        let Some(Entry::GrantPlan(plan)) = self.resources.entries.get(&plan.rep()) else {
            return Err(wit::AccessError::InvalidArgument);
        };
        crate::server::game::item_grants::intent(plan.world, plan.dimension, &plan.grants)
            .map(|hash| hash.to_vec())
            .map_err(|_| wit::AccessError::InvalidArgument)
    }

    fn with_item(
        &mut self,
        plan: Resource<wit::GrantPlan>,
        item: Resource<wit::Stack>,
        quantity: u32,
        placement: wit::GrantPlacement,
    ) -> Result<Resource<wit::GrantPlan>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::GrantItems)?;
        let item = self.item_value(item.rep())?;
        let (world, dimension, mut grants) = match self.resources.entries.get(&plan.rep()) {
            Some(Entry::GrantPlan(plan)) => (plan.world, plan.dimension, plan.grants.clone()),
            _ => return Err(wit::AccessError::InvalidArgument),
        };
        let placement = match placement {
            wit::GrantPlacement::AnyInventoryOrDrop => Placement::Any,
            wit::GrantPlacement::EmptyMainOrDrop(slot) => Placement::EmptyMainOrDrop(slot),
            wit::GrantPlacement::EmptyArmourOrDrop(slot) => Placement::EmptyArmourOrDrop(slot),
            wit::GrantPlacement::EmptyOffhandOrDrop => Placement::EmptyOffhandOrDrop,
        };
        grants.push(Grant {
            item,
            quantity,
            placement,
        });
        // The logical validation is shared with native submission; no quantities are clamped.
        crate::server::game::item_grants::intent(uuid::Uuid::nil(), 0, &grants)
            .map_err(|_| wit::AccessError::InvalidArgument)?;
        self.own_grant_plan(Plan {
            world,
            dimension,
            grants,
        })
    }

    fn drop(&mut self, plan: Resource<wit::GrantPlan>) -> wasmtime::Result<()> {
        if !matches!(
            self.resources.entries.get(&plan.rep()),
            Some(Entry::GrantPlan(_))
        ) {
            anyhow::bail!("invalid grant plan resource");
        }
        self.resources.entries.remove(&plan.rep());
        Ok(())
    }
}
