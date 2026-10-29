//! Slot validators and screen allow-lists for player-owned containers and the
//! container block a window shows.

use crate::block_entity::ContainerKind;
pub use crate::entity::components::ScreenKind;
use crate::entity::components::{BlockScreen, StationKind};
use crate::item::{ItemStack, equipment};
use jolyne::valentine::EnumsContainerEnumName as Name;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Store {
    Main,
    Armor,
    Offhand,
    Ui,
    /// The open container block's slots (both halves of a double chest, or
    /// the player's ender chest).
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Plain,
    Armor,
    Offhand,
    CreatedOutput,
    /// Furnace-family fuel slot (accepts only items that burn).
    FurnaceFuel,
    /// Grindstone input: items with durability or enchantments. BDS refused
    /// an enchanted book without enchantments with `CannotPlaceItem`
    /// (client-diff grindstone-book); other unenchantable items follow the
    /// public reference.
    GrindstoneInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerRule {
    pub store: Store,
    pub first: u8,
    pub len: u8,
    pub kind: SlotKind,
}

impl ContainerRule {
    /// Offhand slot zero is addressable but has no backing storage.
    pub fn index(self, requested: u8) -> Option<Option<usize>> {
        if requested < self.first || requested >= self.first.saturating_add(self.len) {
            return None;
        }
        Some(match self.kind {
            SlotKind::Offhand => (requested == 1).then_some(0),
            _ => Some(usize::from(requested)),
        })
    }

    pub fn accepts(self, requested: u8, stack: &ItemStack) -> bool {
        match self.kind {
            SlotKind::Plain => true,
            SlotKind::Armor => equipment::accepts_armour(stack, usize::from(requested)),
            SlotKind::Offhand => requested == 1 && equipment::allows_offhand(stack),
            SlotKind::CreatedOutput => false,
            // An empty bucket has no burn time; a furnace leaves it behind
            // after lava, from where it can only be taken.
            SlotKind::FurnaceFuel => crate::block_entity::burn_ticks(stack, 200) > 0,
            SlotKind::GrindstoneInput => stack.item().is_some_and(|item| {
                super::super::durability::max_durability(item) > 0
                    || !crate::item::enchant::enchantments(stack).is_empty()
            }),
        }
    }

    pub fn set_count(self, stack: &ItemStack) -> u8 {
        match self.kind {
            SlotKind::Armor => 1,
            SlotKind::FurnaceFuel
                if matches!(
                    stack.item_id(),
                    "minecraft:bucket" | "minecraft:lava_bucket"
                ) =>
            {
                1
            }
            _ => stack.max_stack_size(),
        }
    }
}

const fn rule(store: Store, first: u8, len: u8, kind: SlotKind) -> ContainerRule {
    ContainerRule {
        store,
        first,
        len,
        kind,
    }
}

const VALIDATORS: &[(Name, ContainerRule)] = &[
    (
        Name::Hotbarcontainer,
        rule(Store::Main, 0, 9, SlotKind::Plain),
    ),
    (
        Name::Inventorycontainer,
        rule(Store::Main, 9, 27, SlotKind::Plain),
    ),
    (
        Name::Combinedhotbarandinventorycontainer,
        rule(Store::Main, 0, 36, SlotKind::Plain),
    ),
    (
        Name::Cursorcontainer,
        rule(Store::Ui, 0, 1, SlotKind::Plain),
    ),
    (
        Name::Armorcontainer,
        rule(Store::Armor, 0, 4, SlotKind::Armor),
    ),
    (
        Name::Offhandcontainer,
        rule(Store::Offhand, 0, 2, SlotKind::Offhand),
    ),
    (
        Name::Craftinginputcontainer,
        rule(Store::Ui, 28, 4, SlotKind::Plain),
    ),
    (
        Name::Createdoutputcontainer,
        rule(Store::Ui, 50, 1, SlotKind::CreatedOutput),
    ),
];

struct ScreenPolicy {
    screen: ScreenKind,
    allowed: &'static [Name],
    crafting_first: u8,
    crafting_len: u8,
    destroy: bool,
}

const OPEN_CONTAINERS: &[Name] = &[
    Name::Hotbarcontainer,
    Name::Inventorycontainer,
    Name::Combinedhotbarandinventorycontainer,
    Name::Cursorcontainer,
    Name::Armorcontainer,
    Name::Offhandcontainer,
    Name::Craftinginputcontainer,
    Name::Createdoutputcontainer,
];
const SCREENS: &[ScreenPolicy] = &[
    ScreenPolicy {
        screen: ScreenKind::None,
        allowed: &[Name::Hotbarcontainer],
        crafting_first: 0,
        crafting_len: 0,
        destroy: false,
    },
    ScreenPolicy {
        screen: ScreenKind::Inventory,
        allowed: OPEN_CONTAINERS,
        crafting_first: 28,
        crafting_len: 4,
        destroy: true,
    },
    ScreenPolicy {
        screen: ScreenKind::Workbench,
        allowed: OPEN_CONTAINERS,
        crafting_first: 32,
        crafting_len: 9,
        destroy: true,
    },
];

/// Player containers every block-container screen also allows (BDS "+std").
const BLOCK_SCREEN_STANDARD: &[Name] = &[
    Name::Combinedhotbarandinventorycontainer,
    Name::Hotbarcontainer,
    Name::Inventorycontainer,
    Name::Cursorcontainer,
];

/// The block's own containers on its screen: chest, ender chest and hopper
/// windows address `LevelEntityContainer`, the barrel `BarrelContainer`, and
/// furnaces their ingredient (by block), fuel and result containers.
fn block_rule(screen: BlockScreen, name: Name) -> Option<ContainerRule> {
    let block = |first, len, kind| Some(rule(Store::Block, first, len, kind));
    match screen.kind {
        ContainerKind::Chest
        | ContainerKind::TrappedChest
        | ContainerKind::EnderChest
        | ContainerKind::Hopper => {
            (name == Name::Levelentitycontainer).then_some(())?;
            block(0, screen.size, SlotKind::Plain)
        }
        ContainerKind::Barrel => {
            (name == Name::Barrelcontainer).then_some(())?;
            block(0, screen.size, SlotKind::Plain)
        }
        ContainerKind::Furnace | ContainerKind::BlastFurnace | ContainerKind::Smoker => {
            let smelter = screen.kind.smelter().expect("furnace kinds smelt");
            match name {
                _ if name == smelter.ingredient => block(0, 1, SlotKind::Plain),
                Name::Furnacefuelcontainer => block(1, 1, SlotKind::FurnaceFuel),
                // BDS does not forbid placing into the result slot.
                Name::Furnaceresultcontainer => block(2, 1, SlotKind::Plain),
                _ => None,
            }
        }
    }
}

/// UI slots of a station's inputs, as BDS's client addresses them
/// (client-diff scenarios `anvil`, `smithing-table`, `grindstone`).
pub const ANVIL_INPUT_SLOT: u8 = 1;
pub const ANVIL_MATERIAL_SLOT: u8 = 2;
pub const SMITHING_INPUT_SLOT: u8 = 51;
pub const SMITHING_MATERIAL_SLOT: u8 = 52;
pub const SMITHING_TEMPLATE_SLOT: u8 = 53;
pub const GRINDSTONE_INPUT_SLOT: u8 = 16;
pub const GRINDSTONE_ADDITIONAL_SLOT: u8 = 17;

/// A station's own containers: single UI slots plus the created output.
fn station_rule(kind: StationKind, name: Name) -> Option<ContainerRule> {
    let ui = |slot| Some(rule(Store::Ui, slot, 1, SlotKind::Plain));
    match (kind, name) {
        (_, Name::Createdoutputcontainer) => Some(rule(
            Store::Ui,
            CREATED_OUTPUT_SLOT as u8,
            1,
            SlotKind::CreatedOutput,
        )),
        (StationKind::Anvil, Name::Anvilinputcontainer) => ui(ANVIL_INPUT_SLOT),
        (StationKind::Anvil, Name::Anvilmaterialcontainer) => ui(ANVIL_MATERIAL_SLOT),
        (StationKind::SmithingTable, Name::Smithingtableinputcontainer) => ui(SMITHING_INPUT_SLOT),
        (StationKind::SmithingTable, Name::Smithingtablematerialcontainer) => {
            ui(SMITHING_MATERIAL_SLOT)
        }
        (StationKind::SmithingTable, Name::Smithingtabletemplatecontainer) => {
            ui(SMITHING_TEMPLATE_SLOT)
        }
        (StationKind::Grindstone, Name::Grindstoneinputcontainer) => Some(rule(
            Store::Ui,
            GRINDSTONE_INPUT_SLOT,
            1,
            SlotKind::GrindstoneInput,
        )),
        (StationKind::Grindstone, Name::Grindstoneadditionalcontainer) => Some(rule(
            Store::Ui,
            GRINDSTONE_ADDITIONAL_SLOT,
            1,
            SlotKind::GrindstoneInput,
        )),
        _ => None,
    }
}

impl ScreenKind {
    fn policy(self) -> &'static ScreenPolicy {
        SCREENS
            .iter()
            .find(|row| row.screen == self)
            .expect("every player screen has a policy")
    }

    pub fn rule(self, name: Name) -> Option<ContainerRule> {
        if let ScreenKind::Block(_) | ScreenKind::Station(_) = self
            && BLOCK_SCREEN_STANDARD.contains(&name)
        {
            return VALIDATORS
                .iter()
                .find(|(kind, _)| *kind == name)
                .map(|row| row.1);
        }
        match self {
            ScreenKind::Block(screen) => return block_rule(screen, name),
            ScreenKind::Station(screen) => return station_rule(screen.kind, name),
            _ => {}
        }
        let screen = self.policy();
        if !screen.allowed.contains(&name) {
            return None;
        }
        let mut rule = VALIDATORS.iter().find(|(kind, _)| *kind == name)?.1;
        if name == Name::Craftinginputcontainer {
            rule.first = screen.crafting_first;
            rule.len = screen.crafting_len;
        }
        Some(rule)
    }

    /// Destroy is only legal on the inventory and crafting-table screens.
    pub fn allows_destroy(self) -> bool {
        !matches!(self, ScreenKind::Block(_) | ScreenKind::Station(_)) && self.policy().destroy
    }
}

pub const CREATED_OUTPUT_SLOT: usize = 50;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::stack;

    #[test]
    fn hud_allows_only_hotbar_and_open_screens_reject_foreign_containers() {
        for &(name, _) in VALIDATORS {
            assert_eq!(
                ScreenKind::None.rule(name).is_some(),
                name == Name::Hotbarcontainer
            );
        }
        assert!(
            ScreenKind::Inventory
                .rule(Name::Levelentitycontainer)
                .is_none()
        );
        assert!(
            ScreenKind::Workbench
                .rule(Name::Furnacefuelcontainer)
                .is_none()
        );
    }

    #[test]
    fn container_slot_ranges_follow_screen_data() {
        let inventory = ScreenKind::Inventory
            .rule(Name::Inventorycontainer)
            .unwrap();
        assert_eq!(
            (inventory.index(8), inventory.index(9), inventory.index(36)),
            (None, Some(Some(9)), None)
        );
        let grid = ScreenKind::Inventory
            .rule(Name::Craftinginputcontainer)
            .unwrap();
        assert_eq!((grid.index(28), grid.index(32)), (Some(Some(28)), None));
        let grid = ScreenKind::Workbench
            .rule(Name::Craftinginputcontainer)
            .unwrap();
        assert_eq!(
            (grid.index(32), grid.index(40), grid.index(41)),
            (Some(Some(32)), Some(Some(40)), None)
        );
        let offhand = ScreenKind::Inventory.rule(Name::Offhandcontainer).unwrap();
        assert_eq!(
            (offhand.index(0), offhand.index(1), offhand.index(2)),
            (Some(None), Some(Some(0)), None)
        );
    }

    #[test]
    fn equipment_slots_use_compiled_permissions() {
        let shield = stack("minecraft:shield", 1);
        let helmet = stack("minecraft:iron_helmet", 1);
        let sword = stack("minecraft:diamond_sword", 1);
        let offhand = ScreenKind::Inventory.rule(Name::Offhandcontainer).unwrap();
        assert!(offhand.accepts(1, &shield));
        assert!(!offhand.accepts(0, &shield));
        assert!(!offhand.accepts(1, &sword));
        let armor = ScreenKind::Inventory.rule(Name::Armorcontainer).unwrap();
        assert!(armor.accepts(0, &helmet));
        assert!(!armor.accepts(1, &helmet));
        assert!(!armor.accepts(0, &sword));
        assert_eq!(armor.set_count(&helmet), 1);
        assert!(
            !ScreenKind::Inventory
                .rule(Name::Createdoutputcontainer)
                .unwrap()
                .accepts(50, &shield)
        );
    }
}
