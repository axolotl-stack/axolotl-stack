//! Slot validators and screen allow-lists for player-owned containers.

pub use crate::entity::components::ScreenKind;
use crate::item::{ItemStack, equipment};
use jolyne::valentine::EnumsContainerEnumName as Name;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Store {
    Main,
    Armor,
    Offhand,
    Ui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Plain,
    Armor,
    Offhand,
    CreatedOutput,
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
        }
    }

    pub fn set_count(self, stack: &ItemStack) -> u8 {
        match self.kind {
            SlotKind::Armor => 1,
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

impl ScreenKind {
    fn policy(self) -> &'static ScreenPolicy {
        SCREENS
            .iter()
            .find(|row| row.screen == self)
            .expect("every screen has a policy")
    }

    pub fn rule(self, name: Name) -> Option<ContainerRule> {
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

    pub fn allows_destroy(self) -> bool {
        self.policy().destroy
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
