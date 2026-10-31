//! The ItemStackRequest engine: one transfer algorithm over the validator
//! table in [`super::containers`], applied to a per-request staging overlay
//! and committed only when every action succeeds.
//!
//! Behaviour copied from BDS 1.26.51 (scenario `inventory-moves`, fixture
//! `testdata/bds-inventory-moves.jsonl`, and the items research §2.5):
//! - all or nothing: any failure answers the status with no containers and
//!   changes nothing; no `InventoryContent` resync follows;
//! - non-negative/even request IDs are fatal protocol violations;
//! - transfer checks run in the order 47 (destination is the created
//!   output), 48 (same slot, aliases included), 49 (source), 51 (amount),
//!   50 (destination); an amount above the source count is clamped;
//! - a merge keeps the destination's stack network ID, a full move keeps the
//!   source's, a split destination gets a fresh one; swaps keep both;
//! - response containers and slots appear in first-touched order, keyed by
//!   the container name as requested, source before destination; an emptied
//!   slot reports amount 0 and no stack ID;
//! - Drop validates and stages a world throw; stale sources answer 59.
//!   Destroy outside creative or off an inventory/workbench screen answers 60.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use jolyne::valentine::{
    BedrockSafetyRedactableString, EnumsContainerEnumName as Name,
    EnumsItemStackNetResult as Status, FullContainerName, ItemStackRequestCerealSlotInfoData,
    ItemStackRequestPacketDataRequestData,
    ItemStackRequestPacketDataRequestDataActionsItem as Action, ItemStackResponseContainerInfo,
    ItemStackResponseInfo, ItemStackResponseSlotInfo,
    TypedClientNetIdstructItemStackRequestIdTagint32T0,
    TypedServerNetIdstructItemStackNetIdTagint32T0,
};

use super::containers::{CREATED_OUTPUT_SLOT, ContainerRule, ScreenKind, Store};
use crate::crafting::execution::{CraftingExecutor, CraftingGrid, ValidationError};
use crate::entity::components::{ArmourInventory, Inventory};
use crate::item::{ItemStack, fresh_net_id};

/// The player's addressable storage.
pub struct Stores<'a> {
    pub main: &'a mut Inventory,
    pub armor: &'a mut ArmourInventory,
    pub offhand: &'a mut ItemStack,
    pub ui: &'a mut Inventory,
    /// The open container block's window slots, when one is open.
    pub block: Option<&'a mut Inventory>,
}

static EMPTY: ItemStack = ItemStack::EMPTY;

impl Stores<'_> {
    fn get(&self, store: Store, index: usize) -> &ItemStack {
        match store {
            Store::Main => self.main.item(index),
            Store::Armor => self.armor.slot(index),
            Store::Offhand => Some(&*self.offhand),
            Store::Ui => self.ui.item(index),
            Store::Block => self.block.as_deref().and_then(|block| block.item(index)),
        }
        .unwrap_or(&EMPTY)
    }

    fn set(&mut self, store: Store, index: usize, stack: ItemStack) {
        match store {
            Store::Main => {
                self.main
                    .set_item(index, stack)
                    .expect("validated main slot");
            }
            // The armour rule was checked by the validator table.
            Store::Armor => {
                self.armor
                    .set_slot(index, stack)
                    .expect("validated armour slot");
            }
            Store::Offhand => *self.offhand = stack.into_slot(),
            Store::Ui => {
                self.ui.set_item(index, stack).expect("validated UI slot");
            }
            Store::Block => {
                self.block
                    .as_deref_mut()
                    .expect("block rules exist only while a block window is open")
                    .set_item(index, stack)
                    .expect("validated block slot");
            }
        }
    }
}

/// Slot changes of one request, applied on success.
#[derive(Default)]
struct Staging {
    slots: Vec<((Store, usize), ItemStack)>,
}

impl Staging {
    fn get<'a>(&'a self, stores: &'a Stores<'_>, key: (Store, usize)) -> &'a ItemStack {
        self.slots
            .iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| stores.get(key.0, key.1), |(_, stack)| stack)
    }

    fn set(&mut self, key: (Store, usize), stack: ItemStack) {
        let stack = if stack.is_empty() {
            ItemStack::empty()
        } else {
            stack
        };
        match self.slots.iter_mut().find(|(k, _)| *k == key) {
            Some((_, slot)) => *slot = stack,
            None => self.slots.push((key, stack)),
        }
    }

    fn changed(&self, key: (Store, usize)) -> bool {
        self.slots.iter().any(|(k, _)| *k == key)
    }
}

/// How long BDS keeps a request's resulting stack IDs addressable by the
/// request ID.
pub const REQUEST_ID_LIFETIME: Duration = Duration::from_secs(5);

/// Stack IDs that successful requests left in the slots they touched, so a
/// later request can address a stack by the earlier request's (negative) ID
/// before the client has seen the response.
#[derive(bevy_ecs::component::Component, Debug, Default, Clone, PartialEq, Eq)]
pub struct RequestIdAssignments {
    entries: VecDeque<(Instant, i32, SlotIds)>,
}

/// Stack IDs per slot one request left behind.
type SlotIds = Vec<((Store, usize), i32)>;

impl RequestIdAssignments {
    fn expire(&mut self, now: Instant) {
        while self
            .entries
            .front()
            .is_some_and(|(at, _, _)| now.duration_since(*at) > REQUEST_ID_LIFETIME)
        {
            self.entries.pop_front();
        }
    }

    fn resolve(&self, request_id: i32, key: (Store, usize)) -> Option<i32> {
        self.entries
            .iter()
            .rev()
            .find(|(_, id, _)| *id == request_id)
            .and_then(|(_, _, slots)| slots.iter().find(|(k, _)| *k == key).map(|(_, id)| *id))
    }
}

/// Request-independent inputs.
pub struct Context<'a> {
    pub screen: ScreenKind,
    pub creative: bool,
    /// The live thrower has Rotation and RuntimeId, as spawn_item_drop requires.
    pub can_drop: bool,
    pub now: Instant,
    /// Stack for a creative item network ID (`CraftCreative`).
    pub creative_item: &'a dyn Fn(u32) -> Option<ItemStack>,
    pub crafting: &'a CraftingExecutor,
}

/// Slots of equipment containers a committed request changed; BDS follows
/// such changes with its own slot packets.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EquipmentChanges {
    pub armor: Vec<usize>,
    pub offhand: bool,
}

/// What processing one request produced.
#[derive(Debug)]
pub struct Outcome {
    /// Fatal request IDs do not receive a stack response.
    pub response: Option<ItemStackResponseInfo>,
    pub equipment: EquipmentChanges,
    /// Only successful frames expose drops to the world commit step.
    pub drops: Vec<ItemStack>,
    pub fatal_invalid_id: bool,
}

/// A slot a request addressed, as the client named it.
struct Touched {
    name: FullContainerName,
    slots: Vec<(u8, Option<(Store, usize)>)>,
}

struct ExpectedConsumption {
    ui_slot: u8,
    stack: ItemStack,
    remaining: u8,
}

/// Unsupported native contracts stay distinct from a mismatching grid. In
/// particular, no client result descriptor can supply missing server output.
#[derive(Debug)]
enum GridCraftError {
    Validation(ValidationError),
    MultipleOutputs,
    UnrepresentableOutput,
    AutomaticSources,
    IngredientRemainders,
}

impl GridCraftError {
    fn status(self) -> Status {
        match self {
            Self::Validation(ValidationError::UnknownRecipe(_)) => {
                Status::Failedtofindrecipebynetid
            }
            Self::Validation(ValidationError::UnsupportedRecipe(_))
            | Self::AutomaticSources
            | Self::IngredientRemainders => Status::Invalidrequestcraftactiontype,
            Self::Validation(ValidationError::ZeroCrafts) => Status::Invalidnumcrafts,
            Self::Validation(ValidationError::RequiresThreeByThree) => {
                Status::Mismatchedcraftingsize
            }
            Self::Validation(
                ValidationError::GridMismatch | ValidationError::InsufficientInput { .. },
            ) => Status::Mismatchedrecipeforinputgriditems,
            // Native statuses for multi-result placement and totals above u8
            // have not been measured. Fail the whole request, never truncate.
            Self::Validation(ValidationError::OutputCountOverflow)
            | Self::MultipleOutputs
            | Self::UnrepresentableOutput => Status::Error,
        }
    }
}

struct Request<'s, 'a> {
    id: i32,
    stores: &'s Stores<'a>,
    staging: Staging,
    touched: Vec<Touched>,
    cache: &'s RequestIdAssignments,
    ctx: &'s Context<'s>,
    crafted: bool,
    drops: Vec<ItemStack>,
    consumption: [Option<ExpectedConsumption>; 9],
}

/// Resolved slot address.
struct Slot {
    rule: ContainerRule,
    requested: u8,
    /// `None` for an address without storage (offhand slot 0).
    key: Option<(Store, usize)>,
}

impl Request<'_, '_> {
    fn touch(&mut self, info: &ItemStackRequestCerealSlotInfoData, key: Option<(Store, usize)>) {
        let name = &info.fullcontainername;
        let index = match self.touched.iter().position(|t| t.name == *name) {
            Some(index) => index,
            None => {
                self.touched.push(Touched {
                    name: name.clone(),
                    slots: Vec::new(),
                });
                self.touched.len() - 1
            }
        };
        let slots = &mut self.touched[index].slots;
        if !slots.iter().any(|(slot, _)| *slot == info.slot) {
            slots.push((info.slot, key));
        }
    }

    fn resolve(&self, info: &ItemStackRequestCerealSlotInfoData) -> Option<Slot> {
        if info.fullcontainername.dynamic_id.is_some() {
            return None;
        }
        let rule = self
            .ctx
            .screen
            .rule(info.fullcontainername.container_name)?;
        let index = rule.index(info.slot)?;
        Some(Slot {
            rule,
            requested: info.slot,
            key: index.map(|index| (rule.store, index)),
        })
    }

    fn stack(&self, slot: &Slot) -> &ItemStack {
        slot.key
            .map_or(&EMPTY, |key| self.staging.get(self.stores, key))
    }

    /// Whether the client's stack ID names the stack in `slot`.
    fn net_id_matches(&self, client: i32, slot: &Slot) -> bool {
        let stack = self.stack(slot);
        match client {
            0 => stack.is_empty(),
            id if id > 0 => !stack.is_empty() && stack.net_id == id,
            id if id == self.id => slot.key.is_some_and(|key| self.staging.changed(key)),
            id => slot
                .key
                .and_then(|key| self.cache.resolve(id, key))
                .is_some_and(|net_id| !stack.is_empty() && net_id == stack.net_id),
        }
    }

    fn source(&mut self, info: &ItemStackRequestCerealSlotInfoData) -> Result<Slot, Status> {
        let slot = self.resolve(info).ok_or(Status::Failedtovalidatesrcslot)?;
        if self.stack(&slot).is_empty() || !self.net_id_matches(info.net_id_variant, &slot) {
            return Err(Status::Failedtovalidatesrcslot);
        }
        self.touch(info, slot.key);
        Ok(slot)
    }

    fn destination(&mut self, info: &ItemStackRequestCerealSlotInfoData) -> Result<Slot, Status> {
        let slot = self.resolve(info).ok_or(Status::Failedtovalidatedstslot)?;
        if !self.net_id_matches(info.net_id_variant, &slot) {
            return Err(Status::Failedtovalidatedstslot);
        }
        self.touch(info, slot.key);
        Ok(slot)
    }

    /// Take and Place.
    fn transfer(
        &mut self,
        amount: u8,
        source: &ItemStackRequestCerealSlotInfoData,
        destination: &ItemStackRequestCerealSlotInfoData,
    ) -> Result<(), Status> {
        if destination.fullcontainername.container_name == Name::Createdoutputcontainer {
            return Err(Status::Dstcontainerequaltocreatedoutputcontainer);
        }
        if let (Some(a), Some(b)) = (self.resolve(source), self.resolve(destination))
            && a.key.is_some()
            && a.key == b.key
        {
            return Err(Status::Dstcontainerandslotequaltosrccontainerandslot);
        }
        let src = self.source(source)?;
        let from = self.stack(&src).clone();
        if amount == 0 || (self.crafted && amount > from.count) {
            return Err(Status::Invalidadjustedamount);
        }
        let amount = amount.min(from.count);
        let dst = self.destination(destination)?;
        let to = self.stack(&dst).clone();

        let (moved, placed) = if !to.is_empty() {
            if !to.comparable(&from) {
                return Err(Status::Cannotplaceitem);
            }
            let room = dst.rule.set_count(&from).saturating_sub(to.count);
            let moved = amount.min(room);
            if moved == 0 {
                return Err(Status::Cannotplaceitem);
            }
            let count = to.count + moved;
            (moved, to.with_count(count))
        } else {
            if dst.key.is_none() || !dst.rule.accepts(dst.requested, &from) {
                return Err(Status::Cannotplaceitem);
            }
            let moved = amount.min(dst.rule.set_count(&from));
            if moved == 0 {
                return Err(Status::Cannotplaceitem);
            }
            if moved == from.count {
                self.staging
                    .set(src.key.expect("source has storage"), ItemStack::empty());
                self.staging
                    .set(dst.key.expect("destination has storage"), from);
                return Ok(());
            }
            (
                moved,
                from.clone().with_count(moved).with_net_id(fresh_net_id()),
            )
        };
        let remaining = from.count - moved;
        self.staging.set(
            src.key.expect("source has storage"),
            from.with_count(remaining),
        );
        self.staging
            .set(dst.key.expect("destination has storage"), placed);
        Ok(())
    }

    fn swap(
        &mut self,
        source: &ItemStackRequestCerealSlotInfoData,
        destination: &ItemStackRequestCerealSlotInfoData,
    ) -> Result<(), Status> {
        let src = self.source(source)?;
        let dst = self.destination(destination)?;
        let (Some(src_key), Some(dst_key)) = (src.key, dst.key) else {
            return Err(Status::Cannotswapitem);
        };
        if src_key == dst_key {
            return Err(Status::Dstcontainerandslotequaltosrccontainerandslot);
        }
        let a = self.stack(&src).clone();
        let b = self.stack(&dst).clone();
        let fits = |stack: &ItemStack, slot: &Slot| {
            stack.is_empty()
                || (slot.rule.accepts(slot.requested, stack)
                    && stack.count <= slot.rule.set_count(stack))
        };
        if !fits(&a, &dst) || !fits(&b, &src) {
            return Err(Status::Cannotswapitem);
        }
        self.staging.set(src_key, b);
        self.staging.set(dst_key, a);
        Ok(())
    }

    fn drop_item(
        &mut self,
        amount: u8,
        source: &ItemStackRequestCerealSlotInfoData,
    ) -> Result<(), Status> {
        if !self.ctx.can_drop {
            return Err(Status::Cannotdropitem);
        }
        let src = self.source(source).map_err(|_| Status::Cannotdropitem)?;
        let from = self.stack(&src).clone();
        if amount == 0 {
            return Err(Status::Invalidadjustedamount);
        }
        let amount = amount.min(from.count);
        let remaining = from.count - amount;
        let key = src.key.expect("source has storage");
        let dropped = if remaining == 0 {
            self.staging.set(key, ItemStack::empty());
            from
        } else {
            let dropped = from.clone().with_count(amount).with_net_id(fresh_net_id());
            self.staging.set(key, from.with_count(remaining));
            dropped
        };
        self.drops.push(dropped);
        Ok(())
    }

    fn destroy(
        &mut self,
        amount: u8,
        source: &ItemStackRequestCerealSlotInfoData,
    ) -> Result<(), Status> {
        if !self.ctx.creative || !self.ctx.screen.allows_destroy() {
            return Err(Status::Cannotdestroyitem);
        }
        let src = self.source(source)?;
        let from = self.stack(&src).clone();
        if amount == 0 {
            return Err(Status::Invalidadjustedamount);
        }
        let remaining = from.count - amount.min(from.count);
        self.staging.set(
            src.key.expect("source has storage"),
            from.with_count(remaining),
        );
        Ok(())
    }

    fn craft_creative(&mut self, creative_net_id: u32) -> Result<(), Status> {
        if !self.ctx.creative {
            return Err(Status::Error);
        }
        let stack =
            (self.ctx.creative_item)(creative_net_id).ok_or(Status::Failedtocraftcreative)?;
        let count = stack.max_stack_size();
        self.staging.set(
            (Store::Ui, CREATED_OUTPUT_SLOT),
            stack.with_count(count).with_net_id(fresh_net_id()),
        );
        Ok(())
    }

    fn craft_recipe(&mut self, net_id: u32, num_crafts: u32) -> Result<(), Status> {
        let executor = self.ctx.crafting;
        let large = executor
            .requires_3x3(net_id)
            .map_err(|error| GridCraftError::Validation(error).status())?;
        if large && self.ctx.screen != ScreenKind::Workbench {
            return Err(Status::Mismatchedcraftingsize);
        }
        let grid = if self.ctx.screen == ScreenKind::Workbench {
            CraftingGrid::Workbench(std::array::from_fn(|index| {
                self.staging.get(self.stores, (Store::Ui, 32 + index))
            }))
        } else {
            CraftingGrid::Inventory(std::array::from_fn(|index| {
                self.staging.get(self.stores, (Store::Ui, 28 + index))
            }))
        };
        let plan = executor
            .validate(net_id, grid, num_crafts)
            .map_err(|error| GridCraftError::Validation(error).status())?;
        let mut outputs = plan.outputs();
        let output = outputs.next().ok_or(Status::Emptycraftresults)?;
        if outputs.next().is_some() {
            return Err(GridCraftError::MultipleOutputs.status());
        }
        let count = u8::try_from(output.count)
            .ok()
            .filter(|count| *count != 0)
            .ok_or_else(|| GridCraftError::UnrepresentableOutput.status())?;
        // Preserve the packet's aux, exact block state, NBT and registered
        // item handle. Consume actions, not this validation step, remove input.
        let output = output
            .template
            .clone()
            .with_count(count)
            .with_net_id(fresh_net_id());
        self.consumption = std::array::from_fn(|index| {
            let remaining = plan.consumption[index];
            (remaining != 0).then(|| {
                let ui_slot = plan
                    .grid_size
                    .ui_slot(index)
                    .expect("bounded occupied cell");
                ExpectedConsumption {
                    ui_slot,
                    stack: self
                        .staging
                        .get(self.stores, (Store::Ui, usize::from(ui_slot)))
                        .clone(),
                    remaining,
                }
            })
        });
        self.staging.set((Store::Ui, CREATED_OUTPUT_SLOT), output);
        Ok(())
    }

    fn consume(
        &mut self,
        amount: u8,
        source: &ItemStackRequestCerealSlotInfoData,
    ) -> Result<(), Status> {
        let slot = self
            .resolve(source)
            .ok_or(Status::Consumeditemoutofallowedslotrange)?;
        let Some((Store::Ui, index)) = slot.key else {
            return Err(Status::Consumeditemoutofallowedslotrange);
        };
        let expected_index = self
            .consumption
            .iter()
            .position(|expected| {
                expected
                    .as_ref()
                    .is_some_and(|expected| usize::from(expected.ui_slot) == index)
            })
            .ok_or(Status::Consumeditemnotallowed)?;
        let expected = self.consumption[expected_index]
            .as_ref()
            .expect("matched expected input");
        if expected.remaining == 0 {
            return Err(Status::Itemalreadyconsumedfromslot);
        }
        if amount == 0 || amount > expected.remaining {
            return Err(Status::Consumedtoomuchfromslot);
        }
        let from = self.stack(&slot);
        if !from.comparable(&expected.stack) || from.count < amount {
            return Err(Status::Mismatchslotexpectedconsumeditem);
        }
        if from.net_id != expected.stack.net_id
            || !self.net_id_matches(source.net_id_variant, &slot)
        {
            return Err(Status::Mismatchslotexpectedconsumeditemnetidvariant);
        }
        let remaining = from.count - amount;
        let from = from.clone().with_count(remaining);
        self.touch(source, slot.key);
        self.staging
            .set(slot.key.expect("validated UI input"), from);
        self.consumption[expected_index]
            .as_mut()
            .expect("matched expected input")
            .remaining -= amount;
        Ok(())
    }

    fn finish(&self) -> Result<(), Status> {
        if self
            .consumption
            .iter()
            .flatten()
            .any(|expected| expected.remaining != 0)
        {
            return Err(Status::Expecteditemslotnotfullyconsumed);
        }
        Ok(())
    }

    fn action(&mut self, action: &Action) -> Result<(), Status> {
        match action {
            Action::TakeActionData(take) => {
                self.transfer(take.amount, &take.source, &take.destination)
            }
            Action::PlaceActionData(place) => {
                self.transfer(place.amount, &place.source, &place.destination)
            }
            Action::SwapActionData(swap) if !self.crafted => {
                self.swap(&swap.source, &swap.destination)
            }
            Action::SwapActionData(_) => Err(Status::Actionrequestnotallowed),
            Action::DropActionData(drop) => self.drop_item(drop.amount, &drop.source),
            Action::DestroyActionData(destroy) => self.destroy(destroy.amount, &destroy.source),
            Action::CraftCreativeActionData(_)
            | Action::CraftRecipeActionData(_)
            | Action::CraftRecipeAutoActionData(_)
            | Action::CraftRecipeOptionalActionData(_)
            | Action::CraftRepairAndDisenchantActionData(_)
            | Action::CraftLoomActionData(_)
            | Action::CraftNonImplementedActionData(_)
                if self.crafted =>
            {
                Err(Status::Actionrequestnotallowed)
            }
            Action::CraftCreativeActionData(craft) => {
                self.crafted = true;
                self.craft_creative(craft.creative_item_net_id)
            }
            Action::CraftRecipeActionData(craft) => {
                self.crafted = true;
                self.craft_recipe(
                    craft.recipe_net_id.raw_id,
                    u32::from(craft.numberofrequestedcrafts),
                )
            }
            Action::CraftRecipeAutoActionData(_) => Err(GridCraftError::AutomaticSources.status()),
            Action::ConsumeActionData(_)
            | Action::CreateActionData(_)
            | Action::CraftResultsActionData(_)
                if !self.crafted =>
            {
                Err(Status::Actionrequestnotallowed)
            }
            Action::ConsumeActionData(consume) => self.consume(consume.amount, &consume.source),
            Action::CreateActionData(_) => Err(GridCraftError::IngredientRemainders.status()),
            // Deprecated client predictions are neither recipes nor authority
            // for output identity/count. The server's compiled plan owns both.
            Action::CraftResultsActionData(_) => Ok(()),
            Action::CraftRecipeOptionalActionData(_)
            | Action::CraftRepairAndDisenchantActionData(_)
            | Action::CraftLoomActionData(_)
            | Action::CraftNonImplementedActionData(_) => {
                Err(Status::Invalidrequestcraftactiontype)
            }
            _ => Err(Status::Error),
        }
    }

    fn slot_info(&self, requested: u8, key: Option<(Store, usize)>) -> ItemStackResponseSlotInfo {
        let stack = key.map_or(&EMPTY, |key| self.staging.get(self.stores, key));
        ItemStackResponseSlotInfo {
            requested_slot: requested,
            slot: requested,
            amount: if stack.is_empty() { 0 } else { stack.count },
            item_stack_net_id: (!stack.is_empty())
                .then_some(TypedServerNetIdstructItemStackNetIdTagint32T0 { id: stack.net_id }),
            custom_name: BedrockSafetyRedactableString {
                unredacted: custom_name(stack).into(),
                redacted: None,
            },
            durability_correction: i32::from(stack.damage),
        }
    }
}

fn custom_name(stack: &ItemStack) -> String {
    use zuri_nbt::NBTTag;
    let Some(compound) = crate::item::enchant::item_tag(stack) else {
        return String::new();
    };
    let Some(NBTTag::Compound(display)) = compound.0.get("display") else {
        return String::new();
    };
    match display.0.get("Name") {
        Some(NBTTag::String(name)) => name.0.clone(),
        _ => String::new(),
    }
}

/// Process one request against `stores`, committing it on success.
pub fn process(
    request: &ItemStackRequestPacketDataRequestData,
    stores: &mut Stores<'_>,
    cache: &mut RequestIdAssignments,
    ctx: &Context<'_>,
) -> Outcome {
    let id = request.client_request_id.id;
    let mut equipment = EquipmentChanges::default();
    if id >= 0 || id % 2 == 0 {
        return Outcome {
            response: None,
            equipment,
            drops: Vec::new(),
            fatal_invalid_id: true,
        };
    }
    cache.expire(ctx.now);
    let response = |result, containers| ItemStackResponseInfo {
        result,
        client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
        containers,
    };
    if request.actions.is_empty() {
        return Outcome {
            response: Some(response(Status::Error, None)),
            equipment,
            drops: Vec::new(),
            fatal_invalid_id: false,
        };
    }

    let mut req = Request {
        id,
        stores,
        staging: Staging::default(),
        touched: Vec::new(),
        cache,
        ctx,
        crafted: false,
        consumption: std::array::from_fn(|_| None),
        drops: Vec::new(),
    };
    for action in &request.actions {
        if let Err(status) = req.action(action) {
            return Outcome {
                response: Some(response(status, None)),
                equipment,
                drops: Vec::new(),
                fatal_invalid_id: false,
            };
        }
    }
    if let Err(status) = req.finish() {
        return Outcome {
            response: Some(response(status, None)),
            equipment,
            drops: Vec::new(),
            fatal_invalid_id: false,
        };
    }
    // The created output only lives for the request that filled it.
    req.staging
        .set((Store::Ui, CREATED_OUTPUT_SLOT), ItemStack::empty());

    let containers: Vec<ItemStackResponseContainerInfo> = req
        .touched
        .iter()
        .filter(|t| t.name.container_name != Name::Createdoutputcontainer)
        .map(|t| ItemStackResponseContainerInfo {
            full_container_name: t.name.clone(),
            slots: t
                .slots
                .iter()
                .map(|(requested, key)| req.slot_info(*requested, *key))
                .collect(),
        })
        .collect();
    let assignments: SlotIds = req
        .staging
        .slots
        .iter()
        .map(|(key, stack)| (*key, stack.net_id))
        .collect();
    let drops = std::mem::take(&mut req.drops);
    let staging = std::mem::take(&mut req.staging);
    for ((store, index), stack) in staging.slots {
        match store {
            Store::Armor => equipment.armor.push(index),
            Store::Offhand => equipment.offhand = true,
            Store::Main | Store::Ui | Store::Block => {}
        }
        stores.set(store, index, stack);
    }
    equipment.armor.sort_unstable();
    cache.entries.push_back((ctx.now, id, assignments));
    Outcome {
        response: Some(response(Status::Success, Some(containers))),
        equipment,
        drops,
        fatal_invalid_id: false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use jolyne::valentine::McpePacketData;

    use super::*;
    use crate::entity::components::PlayerUi;
    use crate::item::test_support::VANILLA;

    use super::super::capture::{BDS_INVENTORY_MOVES as CAPTURE, decode};

    /// Pairs BDS stack network IDs with ours: the replay must reuse an ID
    /// exactly where BDS does and mint a fresh one exactly where BDS does.
    #[derive(Default)]
    struct IdMap {
        ours: HashMap<i32, i32>,
        theirs: HashMap<i32, i32>,
    }

    impl IdMap {
        fn pair(&mut self, bds: i32, ours: i32, context: &str) {
            match (self.ours.get(&bds), self.theirs.get(&ours)) {
                (Some(&o), _) => assert_eq!(o, ours, "{context}: BDS reused {bds}"),
                (None, Some(&b)) => {
                    panic!("{context}: we reused {ours} where BDS minted {bds} (was {b})")
                }
                (None, None) => {
                    self.ours.insert(bds, ours);
                    self.theirs.insert(ours, bds);
                }
            }
        }

        fn to_ours(&self, bds: i32) -> i32 {
            if bds <= 0 {
                bds
            } else {
                // An ID BDS never assigned (the stale-ID step) stays unknown.
                self.ours.get(&bds).copied().unwrap_or(i32::MAX)
            }
        }
    }

    fn stack_of(
        wire: &jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData,
        ids: &mut IdMap,
    ) -> ItemStack {
        if wire.id == 0 {
            return ItemStack::empty();
        }
        let item = VANILLA.item_type_by_network_id(i32::from(wire.id)).unwrap();
        let bds = wire.net_id_variant.unwrap();
        // A resent stack keeps the ID it already has.
        let known = ids.ours.get(&bds).copied().unwrap_or(0);
        let stack = ItemStack::new(item, wire.stacksize as u8)
            .with_damage(wire.auxvalue as i16)
            .with_runtime_id(wire.block_runtime_id as i32)
            .with_nbt(wire.user_data_buffer.clone())
            .with_net_id(known)
            .into_slot();
        ids.pair(bds, stack.net_id, "InventoryContent");
        stack
    }

    fn set_window(
        container: u32,
        slots: &[jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData],
        stores: &mut Stores<'_>,
        ids: &mut IdMap,
    ) {
        for (index, wire) in slots.iter().enumerate() {
            set_slot(container, index, wire, stores, ids);
        }
    }

    fn set_slot(
        container: u32,
        index: usize,
        wire: &jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData,
        stores: &mut Stores<'_>,
        ids: &mut IdMap,
    ) {
        let stack = stack_of(wire, ids);
        match container {
            0 => drop(stores.main.set_item(index, stack)),
            119 => *stores.offhand = stack,
            120 if index < 4 => drop(stores.armor.set_slot(index, stack)),
            124 => drop(stores.ui.set_item(index, stack)),
            _ => {}
        }
    }

    fn translate(request: &mut ItemStackRequestPacketDataRequestData, ids: &IdMap) {
        let fix = |info: &mut ItemStackRequestCerealSlotInfoData| {
            info.net_id_variant = ids.to_ours(info.net_id_variant);
        };
        for action in &mut request.actions {
            match action {
                Action::TakeActionData(a) => {
                    fix(&mut a.source);
                    fix(&mut a.destination);
                }
                Action::PlaceActionData(a) => {
                    fix(&mut a.source);
                    fix(&mut a.destination);
                }
                Action::SwapActionData(a) => {
                    fix(&mut a.source);
                    fix(&mut a.destination);
                }
                Action::DropActionData(a) => fix(&mut a.source),
                Action::DestroyActionData(a) => fix(&mut a.source),
                Action::ConsumeActionData(a) => fix(&mut a.source),
                _ => {}
            }
        }
    }

    /// Replays every request of the capture through the engine and compares
    /// each response with BDS's: status, container order, slot order,
    /// amounts, and stack-ID reuse versus fresh IDs. Armour changes must
    /// produce the armour-window slot update BDS sends.
    #[test]
    fn engine_answers_like_bds_inventory_moves() {
        let mut main = Inventory::new(36);
        let mut armor = ArmourInventory::default();
        let mut offhand = ItemStack::empty();
        let mut ui = PlayerUi::default().0;
        let mut stores = Stores {
            main: &mut main,
            armor: &mut armor,
            offhand: &mut offhand,
            ui: &mut ui,
            block: None,
        };
        let mut cache = RequestIdAssignments::default();
        let mut ids = IdMap::default();
        let mut screen = ScreenKind::None;
        let mut pending: Option<(i32, Outcome)> = None;
        let mut replayed = 0;
        let mut crafting_replayed = 0;
        let no_creative = |_: u32| None;

        for line in CAPTURE.lines() {
            let entry: serde_json::Value = serde_json::from_str(line).unwrap();
            let seq = entry["seq"].as_u64().unwrap();
            match decode(entry["hex"].as_str().unwrap()) {
                McpePacketData::InventoryContentPacket(content) => {
                    set_window(content.container_id, &content.slots, &mut stores, &mut ids);
                }
                McpePacketData::InteractPacket(_) => screen = ScreenKind::Inventory,
                McpePacketData::ContainerClosePacket(close) if entry["dir"] == "c2s" => {
                    assert_eq!(close.container_id, 0);
                    screen = ScreenKind::None;
                }
                McpePacketData::ItemStackRequestPacket(packet) => {
                    assert_eq!(packet.requests.len(), 1);
                    let mut request = packet.requests[0].clone();
                    for action in &mut request.actions {
                        if let Action::CraftRecipeActionData(craft) = action {
                            // Preserved original scenarios.json inventory-moves
                            // selects minecraft:oak_planks (step after drop_cursor).
                            // Recipe numbering is server-local, unlike item IDs.
                            craft.recipe_net_id.raw_id =
                                super::super::test_support::recipe_id("minecraft:oak_planks");
                            crafting_replayed += 1;
                        }
                    }
                    translate(&mut request, &ids);
                    let ctx = Context {
                        screen,
                        creative: false,
                        // BDS answered every drop of this scripted
                        // (gophertunnel) client with Cannotdropitem: cursor,
                        // hotbar and inventory sources, screen open or closed.
                        // Real-client drops succeed (bds-1.26.51-player-drop);
                        // the replay models the refused thrower, not a rule
                        // about these slots.
                        can_drop: false,
                        now: Instant::now(),
                        creative_item: &no_creative,
                        crafting: &super::super::test_support::CRAFTING.1,
                    };
                    let outcome = process(&request, &mut stores, &mut cache, &ctx);
                    pending = Some((request.client_request_id.id, outcome));
                }
                McpePacketData::InventorySlotPacket(slot) => match &pending {
                    Some((_, outcome)) => {
                        assert_eq!(slot.container_id, 120, "seq {seq}");
                        assert!(
                            outcome.equipment.armor.contains(&(slot.slot as usize)),
                            "seq {seq}"
                        );
                    }
                    None => set_slot(
                        u32::from(slot.container_id),
                        slot.slot as usize,
                        &slot.item,
                        &mut stores,
                        &mut ids,
                    ),
                },
                McpePacketData::ItemStackResponsePacket(packet) => {
                    let Some((id, outcome)) = pending.take() else {
                        continue;
                    };
                    let bds = &packet.responses[0];
                    let ours = outcome.response.expect("negative odd IDs are answered");
                    assert_eq!(bds.client_request_id.id, id);
                    let context = format!("seq {seq} request {id}");
                    assert_eq!(ours.result, bds.result, "{context}");
                    let (theirs, ours) = (
                        bds.containers.clone().unwrap_or_default(),
                        ours.containers.unwrap_or_default(),
                    );
                    assert_eq!(ours.len(), theirs.len(), "{context}: containers");
                    for (o, t) in ours.iter().zip(&theirs) {
                        assert_eq!(o.full_container_name, t.full_container_name, "{context}");
                        assert_eq!(o.slots.len(), t.slots.len(), "{context}: slots");
                        for (o, t) in o.slots.iter().zip(&t.slots) {
                            assert_eq!(
                                (o.requested_slot, o.slot, o.amount, o.durability_correction),
                                (t.requested_slot, t.slot, t.amount, t.durability_correction),
                                "{context}"
                            );
                            match (&o.item_stack_net_id, &t.item_stack_net_id) {
                                (Some(o), Some(t)) => ids.pair(t.id, o.id, &context),
                                (None, None) => {}
                                other => panic!("{context}: stack IDs {other:?}"),
                            }
                        }
                    }
                    replayed += 1;
                }
                _ => {}
            }
        }
        assert_eq!((replayed, crafting_replayed), (22, 1));
    }
}

#[cfg(test)]
mod regressions {
    use super::*;
    use crate::item::test_support::stack;
    use jolyne::valentine::{
        ItemStackRequestCerealConsumeActionData, ItemStackRequestCerealDestroyActionData,
        ItemStackRequestCerealDropActionData, ItemStackRequestCerealSwapActionData,
        ItemStackRequestCerealTakeActionData,
    };

    struct Harness {
        main: Inventory,
        armor: ArmourInventory,
        offhand: ItemStack,
        ui: Inventory,
        block: Option<Inventory>,
        cache: RequestIdAssignments,
        now: Instant,
        screen: ScreenKind,
        can_drop: bool,
        creative: bool,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                main: Inventory::new(36),
                armor: ArmourInventory::default(),
                offhand: ItemStack::empty(),
                ui: Inventory::new(54),
                block: None,
                cache: RequestIdAssignments::default(),
                now: Instant::now(),
                screen: ScreenKind::Inventory,
                can_drop: true,
                creative: false,
            }
        }

        fn run(&mut self, id: i32, actions: Vec<Action>) -> Outcome {
            let mut stores = Stores {
                main: &mut self.main,
                armor: &mut self.armor,
                offhand: &mut self.offhand,
                ui: &mut self.ui,
                block: self.block.as_mut(),
            };
            process(
                &ItemStackRequestPacketDataRequestData {
                    client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
                    actions,
                    ..Default::default()
                },
                &mut stores,
                &mut self.cache,
                &Context {
                    screen: self.screen,
                    creative: self.creative,
                    can_drop: self.can_drop,
                    now: self.now,
                    creative_item: &|_| None,
                    crafting: &super::super::test_support::CRAFTING.1,
                },
            )
        }

        fn seed(&mut self, slot: usize, count: u8) -> i32 {
            self.main
                .set_item(slot, stack("minecraft:stone", count))
                .unwrap();
            self.main.item(slot).unwrap().net_id
        }
    }

    fn slot(name: Name, slot: u8, id: i32) -> ItemStackRequestCerealSlotInfoData {
        ItemStackRequestCerealSlotInfoData {
            fullcontainername: FullContainerName {
                container_name: name,
                dynamic_id: None,
            },
            slot,
            net_id_variant: id,
        }
    }

    fn main(slot_index: u8, id: i32) -> ItemStackRequestCerealSlotInfoData {
        slot(Name::Combinedhotbarandinventorycontainer, slot_index, id)
    }

    fn take(
        amount: u8,
        source: ItemStackRequestCerealSlotInfoData,
        destination: ItemStackRequestCerealSlotInfoData,
    ) -> Action {
        Action::TakeActionData(ItemStackRequestCerealTakeActionData {
            amount,
            source,
            destination,
            ..Default::default()
        })
    }

    fn drop_action(amount: u8, source: ItemStackRequestCerealSlotInfoData) -> Action {
        Action::DropActionData(ItemStackRequestCerealDropActionData {
            amount,
            source,
            ..Default::default()
        })
    }

    fn status(outcome: &Outcome) -> Status {
        outcome.response.as_ref().unwrap().result
    }

    #[test]
    fn split_full_move_merge_and_swap_preserve_the_right_stack_identity() {
        let mut h = Harness::new();
        let original = h.seed(0, 12);
        assert_eq!(
            status(&h.run(-1, vec![take(4, main(0, original), main(1, 0))])),
            Status::Success
        );
        let split = h.main.item(1).unwrap().net_id;
        assert_ne!(split, original);
        assert_eq!(h.main.item(0).unwrap().net_id, original);
        assert_eq!(
            status(&h.run(-3, vec![take(4, main(1, split), main(2, 0))])),
            Status::Success
        );
        assert!(h.main.item(1).unwrap().is_empty());
        assert_eq!(h.main.item(2).unwrap().net_id, split);
        assert_eq!(
            status(&h.run(-5, vec![take(4, main(2, split), main(0, original))])),
            Status::Success
        );
        assert!(h.main.item(2).unwrap().is_empty());
        assert_eq!(
            (
                h.main.item(0).unwrap().count,
                h.main.item(0).unwrap().net_id
            ),
            (12, original)
        );
        h.main.set_item(3, stack("minecraft:diamond", 2)).unwrap();
        let diamond = h.main.item(3).unwrap().net_id;
        let outcome = h.run(
            -7,
            vec![Action::SwapActionData(
                ItemStackRequestCerealSwapActionData {
                    source: main(0, original),
                    destination: main(3, diamond),
                    ..Default::default()
                },
            )],
        );
        assert_eq!(status(&outcome), Status::Success);
        assert_eq!(
            (
                h.main.item(0).unwrap().net_id,
                h.main.item(3).unwrap().net_id
            ),
            (diamond, original)
        );
    }

    #[test]
    fn later_action_failure_rolls_back_moves_cache_equipment_and_drops() {
        let mut h = Harness::new();
        let original = h.seed(0, 8);
        let outcome = h.run(
            -1,
            vec![
                drop_action(2, main(0, original)),
                take(3, main(0, -1), main(1, 0)),
                take(0, main(1, -1), main(2, 0)),
            ],
        );
        assert_eq!(status(&outcome), Status::Invalidadjustedamount);
        assert!(outcome.response.unwrap().containers.is_none());
        assert!(outcome.drops.is_empty());
        assert_eq!(outcome.equipment, EquipmentChanges::default());
        assert_eq!(
            (
                h.main.item(0).unwrap().count,
                h.main.item(0).unwrap().net_id
            ),
            (8, original)
        );
        assert!(h.main.item(1).unwrap().is_empty());
        assert!(h.cache.entries.is_empty());
    }

    #[test]
    fn valid_hotbar_inventory_and_whole_stack_drops_are_staged_once() {
        for (screen, name, amount) in [
            (ScreenKind::None, Name::Hotbarcontainer, 1),
            (
                ScreenKind::Inventory,
                Name::Combinedhotbarandinventorycontainer,
                2,
            ),
            (
                ScreenKind::Inventory,
                Name::Combinedhotbarandinventorycontainer,
                8,
            ),
        ] {
            let mut h = Harness::new();
            h.screen = screen;
            let original = h.seed(0, 8);
            let outcome = h.run(-1, vec![drop_action(amount, slot(name, 0, original))]);
            assert_eq!(status(&outcome), Status::Success);
            assert_eq!(outcome.drops.len(), 1);
            assert_eq!(outcome.drops[0].count, amount);
            assert_eq!(outcome.drops[0].item_id(), "minecraft:stone");
            assert_eq!(h.main.item(0).unwrap().count, 8 - amount);
            if amount < 8 {
                assert_eq!(h.main.item(0).unwrap().net_id, original);
                assert_ne!(outcome.drops[0].net_id, original);
            } else {
                assert!(h.main.item(0).unwrap().is_empty());
                assert_eq!(outcome.drops[0].net_id, original);
            }
        }
    }

    #[test]
    fn stale_drop_source_and_missing_thrower_preconditions_preserve_inventory() {
        for can_drop in [false, true] {
            let mut h = Harness::new();
            h.can_drop = can_drop;
            let original = h.seed(0, 8);
            let supplied = if can_drop {
                original + 10_000
            } else {
                original
            };
            let outcome = h.run(-1, vec![drop_action(1, main(0, supplied))]);
            assert_eq!(status(&outcome), Status::Cannotdropitem);
            assert!(outcome.drops.is_empty());
            assert_eq!(h.main.item(0).unwrap().count, 8);
        }
    }

    #[test]
    fn prediction_ids_are_slot_specific_stale_checked_and_expire_at_five_seconds() {
        let mut h = Harness::new();
        let original = h.seed(0, 8);
        assert_eq!(
            status(&h.run(-1, vec![take(3, main(0, original), main(1, 0))])),
            Status::Success
        );
        assert_eq!(
            status(&h.run(-3, vec![take(3, main(1, -1), main(2, 0))])),
            Status::Success
        );
        let stale = h.run(-5, vec![take(1, main(2, -1), main(3, 0))]);
        assert_eq!(status(&stale), Status::Failedtovalidatesrcslot);
        h.now += REQUEST_ID_LIFETIME + Duration::from_nanos(1);
        let expired = h.run(-7, vec![take(1, main(2, -3), main(3, 0))]);
        assert_eq!(status(&expired), Status::Failedtovalidatesrcslot);
        assert_eq!(h.main.item(2).unwrap().count, 3);
    }

    #[test]
    fn response_uses_first_touch_order_and_final_cursor_state() {
        let mut h = Harness::new();
        let original = h.seed(0, 8);
        let outcome = h.run(
            -1,
            vec![
                take(
                    2,
                    slot(Name::Hotbarcontainer, 0, original),
                    slot(Name::Cursorcontainer, 0, 0),
                ),
                take(2, slot(Name::Cursorcontainer, 0, -1), main(9, 0)),
                take(1, main(9, -1), slot(Name::Hotbarcontainer, 0, original)),
            ],
        );
        assert_eq!(status(&outcome), Status::Success);
        let containers = outcome.response.unwrap().containers.unwrap();
        assert_eq!(
            containers
                .iter()
                .map(|c| c.full_container_name.container_name)
                .collect::<Vec<_>>(),
            [
                Name::Hotbarcontainer,
                Name::Cursorcontainer,
                Name::Combinedhotbarandinventorycontainer,
            ]
        );
        assert_eq!(containers[0].slots.len(), 1);
        assert_eq!(containers[0].slots[0].amount, 7);
        assert_eq!(containers[1].slots[0].amount, 0);
        assert!(containers[1].slots[0].item_stack_net_id.is_none());
        assert_eq!(containers[2].slots[0].amount, 1);
    }

    #[test]
    fn transfer_validation_order_aliases_and_screen_constraints_are_stable() {
        let mut h = Harness::new();
        let original = h.seed(0, 8);
        assert_eq!(
            status(&h.run(
                -1,
                vec![take(
                    0,
                    main(0, 999),
                    slot(Name::Createdoutputcontainer, 50, 0)
                )]
            )),
            Status::Dstcontainerequaltocreatedoutputcontainer
        );
        assert_eq!(
            status(&h.run(
                -3,
                vec![take(0, main(0, 999), slot(Name::Hotbarcontainer, 0, 0))]
            )),
            Status::Dstcontainerandslotequaltosrccontainerandslot
        );
        assert_eq!(
            status(&h.run(-5, vec![take(0, main(0, 999), main(1, 0))])),
            Status::Failedtovalidatesrcslot
        );
        assert_eq!(
            status(&h.run(-7, vec![take(0, main(0, original), main(99, 0))])),
            Status::Invalidadjustedamount
        );
        assert_eq!(
            status(&h.run(-9, vec![take(1, main(0, original), main(99, 0))])),
            Status::Failedtovalidatedstslot
        );
        h.screen = ScreenKind::None;
        assert_eq!(
            status(&h.run(
                -11,
                vec![take(
                    1,
                    slot(Name::Hotbarcontainer, 0, original),
                    main(1, 0)
                )]
            )),
            Status::Failedtovalidatedstslot
        );
        let mut dynamic = slot(Name::Hotbarcontainer, 1, 0);
        dynamic.fullcontainername.dynamic_id = Some(1);
        assert_eq!(
            status(&h.run(
                -13,
                vec![take(1, slot(Name::Hotbarcontainer, 0, original), dynamic)]
            )),
            Status::Failedtovalidatedstslot
        );
        assert_eq!(h.main.item(0).unwrap().count, 8);
    }

    #[test]
    fn destroy_is_creative_screen_gated_and_consume_is_not_a_free_removal() {
        let mut h = Harness::new();
        let original = h.seed(0, 8);
        let destroy = || {
            Action::DestroyActionData(ItemStackRequestCerealDestroyActionData {
                amount: 2,
                source: main(0, original),
                ..Default::default()
            })
        };
        assert_eq!(
            status(&h.run(-1, vec![destroy()])),
            Status::Cannotdestroyitem
        );
        h.creative = true;
        h.screen = ScreenKind::None;
        assert_eq!(
            status(&h.run(-3, vec![destroy()])),
            Status::Cannotdestroyitem
        );
        h.screen = ScreenKind::Inventory;
        let consume = Action::ConsumeActionData(ItemStackRequestCerealConsumeActionData {
            amount: 1,
            source: main(0, original),
            ..Default::default()
        });
        assert_eq!(
            status(&h.run(-5, vec![destroy(), consume])),
            Status::Actionrequestnotallowed
        );
        assert_eq!(h.main.item(0).unwrap().count, 8);
        assert_eq!(status(&h.run(-7, vec![destroy()])), Status::Success);
        assert_eq!(h.main.item(0).unwrap().count, 6);
    }

    #[test]
    fn split_retains_metadata_and_response_name_and_merge_obeys_capacity() {
        use zuri_nbt::{NBTTag, tag};
        let mut h = Harness::new();
        let display = tag::Compound(
            [("Name".into(), NBTTag::String("Named stone".into()))]
                .into_iter()
                .collect(),
        );
        let compound = tag::Compound(
            [("display".into(), NBTTag::Compound(display))]
                .into_iter()
                .collect(),
        );
        let original = crate::item::enchant::with_item_tag(
            &stack("minecraft:stone", 8)
                .with_damage(3)
                .with_runtime_id(42),
            compound,
        );
        h.main.set_item(0, original).unwrap();
        let source = h.main.item(0).unwrap().clone();
        let outcome = h.run(-1, vec![take(3, main(0, source.net_id), main(1, 0))]);
        assert_eq!(status(&outcome), Status::Success);
        let split = h.main.item(1).unwrap().clone();
        assert_eq!(
            (split.damage, split.runtime_id, split.nbt.as_deref()),
            (3, 42, source.nbt.as_deref())
        );
        let response = outcome.response.unwrap().containers.unwrap();
        assert_eq!(response[0].slots[0].custom_name.unredacted, "Named stone");
        assert_eq!(response[0].slots[1].custom_name.unredacted, "Named stone");
        assert_eq!(response[0].slots[1].durability_correction, 3);
        h.main
            .set_item(2, source.with_count(63).with_net_id(0))
            .unwrap();
        let destination_id = h.main.item(2).unwrap().net_id;
        assert_eq!(
            status(&h.run(
                -3,
                vec![take(3, main(1, split.net_id), main(2, destination_id))]
            )),
            Status::Success
        );
        assert_eq!(h.main.item(1).unwrap().count, 2);
        assert_eq!(
            (
                h.main.item(2).unwrap().count,
                h.main.item(2).unwrap().net_id
            ),
            (64, destination_id)
        );
        assert_eq!(
            status(&h.run(
                -5,
                vec![take(
                    1,
                    main(1, split.net_id),
                    main(2, destination_id + 10_000)
                )]
            )),
            Status::Failedtovalidatedstslot
        );
        assert_eq!(
            status(&h.run(
                -7,
                vec![take(1, main(1, split.net_id), main(2, destination_id))]
            )),
            Status::Cannotplaceitem
        );
        assert_eq!(h.main.item(1).unwrap().count, 2);
    }

    #[test]
    fn trusted_non_equipment_head_contents_remain_removable() {
        let mut h = Harness::new();
        h.armor
            .inventory_mut()
            .set_item(0, stack("minecraft:stone", 1))
            .unwrap();
        let id = h.armor.slot(0).unwrap().net_id;
        let outcome = h.run(
            -1,
            vec![take(1, slot(Name::Armorcontainer, 0, id), main(0, 0))],
        );
        assert_eq!(status(&outcome), Status::Success);
        assert!(h.armor.slot(0).unwrap().is_empty());
        assert_eq!(h.main.item(0).unwrap().item_id(), "minecraft:stone");
        assert_eq!(h.main.item(0).unwrap().net_id, id);
    }

    #[test]
    fn unknown_recipe_rejects_and_rolls_back_the_entire_frame() {
        let mut h = Harness::new();
        let original = h.seed(0, 8);
        let outcome = h.run(
            -1,
            vec![
                take(3, main(0, original), main(1, 0)),
                Action::CraftRecipeActionData(Default::default()),
            ],
        );
        assert_eq!(status(&outcome), Status::Failedtofindrecipebynetid);
        assert!(outcome.response.unwrap().containers.is_none());
        assert_eq!(h.main.item(0).unwrap().count, 8);
        assert!(h.main.item(1).unwrap().is_empty());
        assert!(h.cache.entries.is_empty());
    }

    #[test]
    fn empty_request_errors_but_ordinary_oversized_transfer_clamps() {
        let mut h = Harness::new();
        let original = h.seed(0, 8);
        assert_eq!(status(&h.run(-1, Vec::new())), Status::Error);
        assert_eq!(
            status(&h.run(-3, vec![take(255, main(0, original), main(1, 0))])),
            Status::Success
        );
        assert!(h.main.item(0).unwrap().is_empty());
        assert_eq!(
            (
                h.main.item(1).unwrap().count,
                h.main.item(1).unwrap().net_id
            ),
            (8, original)
        );
    }

    #[test]
    fn invalid_request_ids_are_fatal_and_cannot_mutate_or_assign_cache() {
        for id in [0, 1, 2, -2, i32::MIN] {
            let mut h = Harness::new();
            let original = h.seed(0, 8);
            let outcome = h.run(id, vec![take(1, main(0, original), main(1, 0))]);
            assert!(outcome.fatal_invalid_id);
            assert!(outcome.response.is_none());
            assert!(outcome.drops.is_empty());
            assert!(h.cache.entries.is_empty());
            assert_eq!(h.main.item(0).unwrap().count, 8);
            assert!(h.main.item(1).unwrap().is_empty());
        }
    }

    #[cfg(test)]
    mod grid_crafting_tests {
        use super::*;
        use crate::item::test_support::stack;
        use jolyne::valentine::{
            ItemStackRequestCerealConsumeActionData, ItemStackRequestCerealCraftRecipeActionData,
            ItemStackRequestCerealCraftResultsActionData, McpePacketData,
            TypedServerNetIdstructRecipeNetIdTag,
        };

        use super::super::super::{capture, test_support};

        fn craft(net_id: u32, count: u8) -> Action {
            Action::CraftRecipeActionData(ItemStackRequestCerealCraftRecipeActionData {
                recipe_net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: net_id },
                numberofrequestedcrafts: count,
                ..Default::default()
            })
        }

        fn consume(count: u8, index: u8, net_id: i32) -> Action {
            Action::ConsumeActionData(ItemStackRequestCerealConsumeActionData {
                amount: count,
                source: slot(Name::Craftinginputcontainer, index, net_id),
                ..Default::default()
            })
        }

        fn output(count: u8, request_id: i32, destination: u8) -> Action {
            take(
                count,
                slot(Name::Createdoutputcontainer, 50, request_id),
                main(destination, 0),
            )
        }

        fn seed(h: &mut Harness, index: usize, name: &str, count: u8) -> i32 {
            h.ui.set_item(index, stack(name, count)).unwrap();
            h.ui.item(index).unwrap().net_id
        }

        fn planks() -> u32 {
            test_support::recipe_id("minecraft:oak_planks")
        }

        #[test]
        fn recorded_plank_actions_succeed_with_matched_grid_and_exact_consume_source() {
            let entry: serde_json::Value = capture::BDS_INVENTORY_MOVES
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .find(|entry| entry["seq"] == 645)
                .unwrap();
            let McpePacketData::ItemStackRequestPacket(packet) =
                capture::decode(entry["hex"].as_str().unwrap())
            else {
                panic!("recorded craft request")
            };
            let mut request = packet.requests[0].clone();
            let mut h = Harness::new();
            let input_id = seed(&mut h, 28, "minecraft:oak_log", 2);
            // The capture did not move main9 into the grid: BDS answered 35.
            // Repair only that setup/address, not action order, count or destination.
            for action in &mut request.actions {
                match action {
                    Action::CraftRecipeActionData(action) => action.recipe_net_id.raw_id = planks(),
                    Action::ConsumeActionData(action) => {
                        action.source = slot(Name::Craftinginputcontainer, 28, input_id);
                    }
                    _ => {}
                }
            }
            let outcome = h.run(request.client_request_id.id, request.actions);
            assert_eq!(status(&outcome), Status::Success);
            assert_eq!(
                (h.ui.item(28).unwrap().count, h.ui.item(28).unwrap().net_id),
                (1, input_id)
            );
            let result = h.main.item(13).unwrap();
            assert_eq!(
                (result.item_id(), result.count),
                ("minecraft:oak_planks", 4)
            );
            assert!(result.net_id > 0);
            assert_ne!(result.net_id, input_id);
            assert!(h.ui.item(CREATED_OUTPUT_SLOT).unwrap().is_empty());
            let containers = outcome.response.unwrap().containers.unwrap();
            assert_eq!(containers.len(), 2);
            assert_eq!(
                containers[0].full_container_name.container_name,
                Name::Craftinginputcontainer
            );
            assert_eq!(containers[0].slots[0].slot, 28);
            // The recorded destination addresses InventoryContainer slot 13; BDS
            // echoes the requested container name (seq 591/592), it does not
            // normalise to the combined container.
            assert_eq!(
                containers[1].full_container_name.container_name,
                Name::Inventorycontainer
            );
            assert_eq!(containers[1].slots[0].slot, 13);
        }

        #[test]
        fn wrong_recipe_grid_size_and_insufficient_input_reject_without_mutation() {
            for (net_id, name, count, crafts, expected) in [
                (
                    u32::MAX,
                    "minecraft:oak_log",
                    2,
                    1,
                    Status::Failedtofindrecipebynetid,
                ),
                (
                    planks(),
                    "minecraft:stone",
                    2,
                    1,
                    Status::Mismatchedrecipeforinputgriditems,
                ),
                (
                    planks(),
                    "minecraft:oak_log",
                    1,
                    2,
                    Status::Mismatchedrecipeforinputgriditems,
                ),
            ] {
                let mut h = Harness::new();
                let id = seed(&mut h, 28, name, count);
                let outcome = h.run(-1, vec![craft(net_id, crafts)]);
                assert_eq!(status(&outcome), expected);
                assert_eq!(
                    (h.ui.item(28).unwrap().count, h.ui.item(28).unwrap().net_id),
                    (count, id)
                );
                assert!(h.cache.entries.is_empty());
            }
            let packet = test_support::CRAFTING.0.packet();
            let large = packet
                .shaped_recipes
                .iter()
                .find(|recipe| recipe.width == 3)
                .unwrap();
            let mut h = Harness::new();
            assert_eq!(
                status(&h.run(-1, vec![craft(large.net_id.raw_id, 1)])),
                Status::Mismatchedcraftingsize
            );
            assert_eq!(
                status(&h.run(-3, vec![craft(packet.multi_recipes[0].net_id.raw_id, 1)])),
                Status::Invalidrequestcraftactiontype
            );
            assert_eq!(
                status(&h.run(-5, vec![craft(planks(), 0)])),
                Status::Invalidnumcrafts
            );
        }

        #[test]
        fn missing_partial_extra_and_unrelated_consume_never_commit() {
            for mode in 0..5 {
                let mut h = Harness::new();
                let id = seed(&mut h, 28, "minecraft:oak_log", 4);
                let unrelated = h.seed(0, 3);
                let mut actions = vec![craft(planks(), 2)];
                let expected = match mode {
                    0 => Status::Expecteditemslotnotfullyconsumed,
                    1 => {
                        actions.push(consume(1, 28, id));
                        Status::Expecteditemslotnotfullyconsumed
                    }
                    2 => {
                        actions.extend([consume(2, 28, id), consume(1, 28, -1)]);
                        Status::Itemalreadyconsumedfromslot
                    }
                    3 => {
                        actions.push(consume(3, 28, id));
                        Status::Consumedtoomuchfromslot
                    }
                    _ => {
                        actions.push(Action::ConsumeActionData(
                            ItemStackRequestCerealConsumeActionData {
                                amount: 2,
                                source: main(0, unrelated),
                                ..Default::default()
                            },
                        ));
                        Status::Consumeditemoutofallowedslotrange
                    }
                };
                actions.push(output(8, -1, 1));
                let outcome = h.run(-1, actions);
                assert_eq!(status(&outcome), expected, "mode {mode}");
                assert!(outcome.response.unwrap().containers.is_none());
                assert_eq!(h.ui.item(28).unwrap().count, 4);
                assert_eq!(h.main.item(0).unwrap().count, 3);
                assert!(h.main.item(1).unwrap().is_empty());
                assert!(h.cache.entries.is_empty());
            }
        }

        #[test]
        fn exact_multiple_crafts_consume_once_and_keep_first_touch_order() {
            let mut h = Harness::new();
            let input = seed(&mut h, 29, "minecraft:oak_log", 4);
            let first = h.seed(0, 3);
            let outcome = h.run(
                -1,
                vec![
                    take(1, main(0, first), slot(Name::Cursorcontainer, 0, 0)),
                    craft(planks(), 2),
                    consume(2, 29, input),
                    output(8, -1, 2),
                ],
            );
            assert_eq!(status(&outcome), Status::Success);
            assert_eq!(h.ui.item(29).unwrap().count, 2);
            assert_eq!(h.main.item(2).unwrap().count, 8);
            let containers = outcome.response.unwrap().containers.unwrap();
            assert_eq!(
                containers
                    .iter()
                    .map(|container| container.full_container_name.container_name)
                    .collect::<Vec<_>>(),
                [
                    Name::Combinedhotbarandinventorycontainer,
                    Name::Cursorcontainer,
                    Name::Craftinginputcontainer
                ]
            );
            assert_eq!(
                containers[0]
                    .slots
                    .iter()
                    .map(|slot| slot.slot)
                    .collect::<Vec<_>>(),
                [0, 2]
            );
            assert_eq!(containers[2].slots[0].slot, 29);
        }

        #[test]
        fn input_identity_second_craft_and_late_failure_discard_the_whole_frame() {
            let mut h = Harness::new();
            let input = seed(&mut h, 28, "minecraft:oak_log", 3);
            let dropped = h.seed(0, 5);
            let outcome = h.run(
                -1,
                vec![
                    super::drop_action(2, main(0, dropped)),
                    craft(planks(), 1),
                    consume(1, 28, input),
                    output(4, -1, 1),
                    take(0, main(1, -1), main(2, 0)),
                ],
            );
            assert_eq!(status(&outcome), Status::Invalidadjustedamount);
            assert!(outcome.drops.is_empty());
            assert!(h.cache.entries.is_empty());
            assert_eq!(h.main.item(0).unwrap().count, 5);
            assert!(h.main.item(1).unwrap().is_empty());
            assert_eq!(h.ui.item(28).unwrap().count, 3);
            assert_eq!(
                status(&h.run(-3, vec![craft(planks(), 1), consume(1, 28, input + 10_000)])),
                Status::Mismatchslotexpectedconsumeditemnetidvariant
            );
            assert_eq!(
                status(&h.run(-5, vec![craft(planks(), 1), craft(planks(), 1)])),
                Status::Actionrequestnotallowed
            );
            assert_eq!(
                status(&h.run(
                    -7,
                    vec![
                        craft(planks(), 1),
                        Action::SwapActionData(Default::default())
                    ]
                )),
                Status::Actionrequestnotallowed
            );
        }

        #[test]
        fn packet_output_metadata_survives_and_client_predictions_cannot_replace_it() {
            let packet = test_support::CRAFTING.0.packet();
            // Native builtin_recipes.json bed_color_0 is 3x2: black wool over
            // planks, yielding bed aux 15. No client result supplies this aux.
            let recipe = packet
                .shaped_recipes
                .iter()
                .find(|recipe| &*recipe.recipe_id == "bed_color_0")
                .unwrap();
            let mut h = Harness::new();
            h.screen = ScreenKind::Workbench;
            let mut actions = vec![craft(recipe.net_id.raw_id, 1)];
            for index in 0..6 {
                let id = seed(
                    &mut h,
                    32 + index,
                    if index < 3 {
                        "minecraft:black_wool"
                    } else {
                        "minecraft:oak_planks"
                    },
                    2,
                );
                actions.push(consume(1, u8::try_from(32 + index).unwrap(), id));
            }
            let expected = &recipe.results[0];
            actions.extend([
                output(u8::try_from(expected.stacksize).unwrap(), -1, 0),
                Action::CraftResultsActionData(ItemStackRequestCerealCraftResultsActionData {
                    num_crafts: 255,
                    craft_results: vec![Default::default()],
                    ..Default::default()
                }),
            ]);
            let outcome = h.run(-1, actions);
            assert_eq!(status(&outcome), Status::Success);
            let actual = h.main.item(0).unwrap();
            assert_eq!(actual.item().unwrap().network_id(), expected.id);
            assert_eq!(u32::try_from(actual.damage).unwrap(), expected.auxvalue);
            assert_eq!(actual.runtime_id, expected.block_runtime_id);
            assert_eq!(
                actual.nbt.as_deref(),
                Some(expected.user_data_buffer.as_ref())
            );
            assert_eq!(u16::from(actual.count), expected.stacksize);
            assert_eq!(actual.damage, 15);
            assert!(!expected.user_data_buffer.is_empty());
        }

        #[test]
        fn small_recipe_uses_the_open_workbench_grid_including_bottom_right_offset() {
            let mut h = Harness::new();
            h.screen = ScreenKind::Workbench;
            let input = seed(&mut h, 40, "minecraft:oak_log", 2);
            // These inactive inventory-grid contents must not participate.
            seed(&mut h, 28, "minecraft:stone", 2);
            let outcome = h.run(
                -1,
                vec![craft(planks(), 1), consume(1, 40, input), output(4, -1, 0)],
            );
            assert_eq!(status(&outcome), Status::Success);
            assert_eq!(h.ui.item(40).unwrap().count, 1);
            assert_eq!(h.ui.item(28).unwrap().count, 2);
            assert_eq!(h.main.item(0).unwrap().count, 4);
        }

        #[test]
        fn unrepresentable_output_auto_and_remainder_create_fail_closed() {
            let mut h = Harness::new();
            seed(&mut h, 28, "minecraft:oak_log", 64);
            assert_eq!(status(&h.run(-1, vec![craft(planks(), 64)])), Status::Error);
            assert_eq!(h.ui.item(28).unwrap().count, 64);
            assert_eq!(
                status(&h.run(
                    -3,
                    vec![Action::CraftRecipeAutoActionData(Default::default())]
                )),
                Status::Invalidrequestcraftactiontype
            );
            assert_eq!(
                status(&h.run(
                    -5,
                    vec![
                        craft(planks(), 1),
                        Action::CreateActionData(Default::default())
                    ]
                )),
                Status::Invalidrequestcraftactiontype
            );
            assert_eq!(h.ui.item(28).unwrap().count, 64);
        }
    }
}
