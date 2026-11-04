//! The shared scenario script: `tools/client-diff/scenarios.json`, run by the
//! gophertunnel bot (`tools/client-diff/gt-client`) against BDS and by
//! [`crate::driver`] against any server. Field names, defaults and
//! validation mirror the bot's `step` and `scenario` types so both drivers
//! accept exactly the same scripts.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Player inventory slots: 0-8 hotbar, 9-35 main inventory.
pub const INVENTORY_SIZE: i32 = 36;
/// Block window slots are bounded by a double chest.
pub const BLOCK_SLOTS: i32 = 54;
/// PlayerUI slots of the inventory's 2x2 crafting grid.
pub const CRAFTING_GRID: std::ops::RangeInclusive<i32> = 28..=31;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScenariosFile {
    pub identity: Identity,
    pub scenarios: BTreeMap<String, Scenario>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Identity {
    pub display_name: String,
    pub uuid: Uuid,
    pub device_id: String,
    pub client_random_id: i64,
    pub device_os: i32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Scenario {
    pub idle_ms: u64,
    pub steps: Vec<Step>,
    /// Tick like a vanilla client (PlayerAuthInput every 50 ms, respawn and
    /// dimension-change acknowledgements). Interaction steps require it.
    #[serde(default)]
    pub auth_input: bool,
}

/// What a step does; see the bot's `session.interaction` for the packets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    #[default]
    Chat,
    Command,
    Move,
    MoveItem,
    Drop,
    Craft,
    TakeCursor,
    PlaceCursor,
    Swap,
    DropCursor,
    Destroy,
    OpenInventory,
    CloseInventory,
    CloseContainer,
    MapInfo,
    WaitEvent,
    Book,
    ToggleCrafter,
    Select,
    Attack,
    Interact,
    Break,
    Click,
    Use,
    Release,
    Consume,
}

impl StepKind {
    /// The snake_case name used in scenarios.json.
    pub fn name(self) -> &'static str {
        use StepKind::*;
        match self {
            Chat => "chat",
            Command => "command",
            Move => "move",
            MoveItem => "move_item",
            Drop => "drop",
            Craft => "craft",
            TakeCursor => "take_cursor",
            PlaceCursor => "place_cursor",
            Swap => "swap",
            DropCursor => "drop_cursor",
            Destroy => "destroy",
            OpenInventory => "open_inventory",
            CloseInventory => "close_inventory",
            CloseContainer => "close_container",
            MapInfo => "map_info",
            WaitEvent => "wait_event",
            Book => "book",
            ToggleCrafter => "toggle_crafter",
            Select => "select",
            Attack => "attack",
            Interact => "interact",
            Break => "break",
            Click => "click",
            Use => "use",
            Release => "release",
            Consume => "consume",
        }
    }

    /// The client-to-server packet a step of this kind sends (one per step),
    /// if any: steps are located in a recording by these packets. `move`,
    /// `break` and `wait_event` only shape `PlayerAuthInput` ticks.
    pub fn marker(self) -> Option<&'static str> {
        use StepKind::*;
        Some(match self {
            Chat => "Text",
            Command => "CommandRequest",
            Select => "MobEquipment",
            OpenInventory => "Interact",
            CloseInventory | CloseContainer => "ContainerClose",
            Book => "BookEdit",
            ToggleCrafter => "PlayerToggleCrafterSlotRequest",
            MapInfo => "MapInfoRequest",
            MoveItem | Drop | Craft | TakeCursor | PlaceCursor | Swap | DropCursor | Destroy => {
                "ItemStackRequest"
            }
            Click | Attack | Interact | Use | Release | Consume => "InventoryTransaction",
            Move | Break | WaitEvent => return None,
        })
    }
}

/// One scenario action. `chat`/`command` use `text` (`{x}`, `{y}`, `{z}`
/// expand to the spawn feet block); interaction block coordinates `x`/`y`/`z`
/// are offsets from that block. Slots 0-8 are the hotbar, 9-35 the main
/// inventory; the remaining fields are documented on the bot's `step` type.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Step {
    pub after_ms: u64,
    pub kind: StepKind,
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub face: i32,
    pub slot: i32,
    pub to: i32,
    pub count: i32,
    pub ticks: i32,
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
    pub entity: String,
    pub recipe: String,
    pub stale: bool,
    pub to_container: String,
    pub from_container: String,
    pub consume: Vec<SlotRef>,
    pub action: String,
    pub recipe_network_id: u32,
    pub cost: i32,
    pub option: i32,
    pub pattern: String,
    pub result: String,
    pub result_nbt: Option<serde_json::Map<String, serde_json::Value>>,
    pub flags: Vec<String>,
    pub event: i32,
    pub timeout_ms: u64,
    pub page: i32,
    pub title: String,
    pub author: String,
    pub disabled: bool,
    pub lenient: bool,
}

/// A container slot named like [`Step::to_container`]; `count` (default 1)
/// is how many items `craft` consumes from it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SlotRef {
    pub container: String,
    pub slot: i32,
    pub count: i32,
}

/// Container names a step may address (see the bot's `containerIDs`).
pub const CONTAINERS: &[&str] = &[
    "armor",
    "offhand",
    "chest",
    "barrel",
    "ingredient",
    "fuel",
    "result",
    "crafting",
    "anvil_input",
    "anvil_material",
    "smithing_input",
    "smithing_material",
    "smithing_template",
    "grindstone_input",
    "grindstone_additional",
    "enchanting_input",
    "enchanting_material",
    "brewing_input",
    "brewing_result",
    "brewing_fuel",
    "shulker_box",
    "crafter",
    "stonecutter_input",
    "loom_input",
    "loom_dye",
    "loom_material",
    "cartography_input",
    "cartography_additional",
];

/// `PlayerAuthInput` flags a `move` step may set.
pub const INPUT_FLAGS: &[&str] = &[
    "sneaking",
    "sneak_down",
    "start_sneaking",
    "stop_sneaking",
    "sneak_pressed_raw",
    "sneak_current_raw",
    "jumping",
    "jump_down",
    "start_jumping",
    "jump_pressed_raw",
    "jump_current_raw",
    "want_up",
    "start_gliding",
    "stop_gliding",
    "sprinting",
    "start_sprinting",
];

/// `book` step actions.
pub const BOOK_ACTIONS: &[&str] = &["replace", "add", "delete", "swap", "sign"];

impl Scenario {
    /// The bot's `scenario.validate`, so a script either runs on both drivers
    /// or on neither.
    pub fn validate(&self) -> Result<()> {
        for (i, step) in self.steps.iter().enumerate() {
            validate_step(step).with_context(|| format!("step {i}"))?;
            ensure!(
                matches!(step.kind, StepKind::Chat | StepKind::Command) || self.auth_input,
                "step {i}: {} requires auth_input",
                step.kind.name()
            );
        }
        Ok(())
    }

    /// The first `count` steps, then idle for the `after_ms` of the first
    /// step left out (or the scenario's own idle time when none is): the
    /// window a retained recording covers (see [`crate::parity`]).
    pub fn prefix(&self, count: usize) -> Result<Scenario> {
        ensure!(
            count <= self.steps.len(),
            "prefix of {count} steps, scenario has {}",
            self.steps.len()
        );
        Ok(Scenario {
            idle_ms: self
                .steps
                .get(count)
                .map_or(self.idle_ms, |step| step.after_ms),
            steps: self.steps[..count].to_vec(),
            auth_input: self.auth_input,
        })
    }
}

fn validate_step(step: &Step) -> Result<()> {
    use StepKind::*;
    let known = |name: &str| name.is_empty() || CONTAINERS.contains(&name);
    match step.kind {
        Chat | Command => ensure!(!step.text.is_empty(), "text must not be empty"),
        Move => {
            ensure!(step.ticks > 0, "move needs positive ticks");
            for flag in &step.flags {
                ensure!(
                    INPUT_FLAGS.contains(&flag.as_str()),
                    "unknown input flag {flag:?}"
                );
            }
        }
        MoveItem | Drop | Craft | TakeCursor | PlaceCursor | Swap | DropCursor | Destroy => {
            ensure!(
                (1..=64).contains(&step.count)
                    && (0..BLOCK_SLOTS).contains(&step.slot)
                    && (0..BLOCK_SLOTS).contains(&step.to),
                "{} needs count 1..64 and slots 0..53",
                step.kind.name()
            );
            ensure!(
                !(step.from_container.is_empty() && step.slot >= INVENTORY_SIZE
                    || step.to_container.is_empty() && step.to >= INVENTORY_SIZE),
                "player inventory slots are 0..35"
            );
            for name in [&step.to_container, &step.from_container] {
                ensure!(known(name), "unknown container {name:?}");
            }
            ensure!(
                !(!step.from_container.is_empty() && step.kind != MoveItem
                    || matches!(
                        step.from_container.as_str(),
                        "armor" | "offhand" | "crafting"
                    )),
                "from_container is for move_item out of a block window"
            );
            ensure!(
                !(step.kind == Craft && step.recipe.is_empty() && step.action.is_empty()),
                "craft needs a recipe"
            );
            match step.action.as_str() {
                "" | "optional" | "grindstone" | "enchant" => {}
                "loom" => ensure!(!step.pattern.is_empty(), "loom needs a pattern"),
                other => bail!("unknown craft action {other:?}"),
            }
            ensure!(
                !(step.kind == Craft
                    && matches!(step.to_container.as_str(), "armor" | "offhand" | "crafting")),
                "craft places into the inventory or the open block window"
            );
            for slot in &step.consume {
                ensure!(
                    step.kind == Craft
                        && !slot.container.is_empty()
                        && known(&slot.container)
                        && !matches!(slot.container.as_str(), "armor" | "offhand"),
                    "consume is for craft, from the crafting grid or a block window"
                );
                ensure!(
                    !(slot.container == "crafting" && !CRAFTING_GRID.contains(&slot.slot)
                        || !(0..BLOCK_SLOTS).contains(&slot.slot)),
                    "consume slot {} out of range",
                    slot.slot
                );
            }
            ensure!(
                step.to_container != "crafting" || CRAFTING_GRID.contains(&step.to),
                "crafting grid slots are 28..31"
            );
            ensure!(
                !(step.stale && step.kind == Craft),
                "craft cannot use a stale stack ID"
            );
        }
        OpenInventory | CloseInventory | CloseContainer | MapInfo => {}
        WaitEvent => ensure!(
            step.timeout_ms > 0,
            "wait_event needs a positive timeout_ms"
        ),
        Book => {
            ensure!(
                (0..INVENTORY_SIZE).contains(&step.slot),
                "book needs an inventory slot 0..35"
            );
            ensure!(
                BOOK_ACTIONS.contains(&step.action.as_str()),
                "unknown book action {:?}",
                step.action
            );
        }
        ToggleCrafter => ensure!(
            (0..=8).contains(&step.slot),
            "toggle_crafter needs a crafter slot 0..8"
        ),
        Select => ensure!(
            (0..=8).contains(&step.slot),
            "select needs a hotbar slot 0..8"
        ),
        Attack | Interact => ensure!(
            !step.entity.is_empty(),
            "{} needs an entity type",
            step.kind.name()
        ),
        Break => ensure!(step.ticks >= 0, "break ticks must not be negative"),
        Click | Use | Release | Consume => {}
    }
    Ok(())
}

impl ScenariosFile {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let file: Self =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let id = &file.identity;
        ensure!(
            !id.display_name.is_empty()
                && !id.device_id.is_empty()
                && id.client_random_id != 0
                && (1..=15).contains(&id.device_os),
            "scenario identity requires display_name, device_id, nonzero client_random_id and device_os in 1..15"
        );
        Ok(file)
    }

    /// The named scenario, validated.
    pub fn scenario(&self, name: &str) -> Result<&Scenario> {
        let scenario = self
            .scenarios
            .get(name)
            .with_context(|| format!("unknown scenario {name:?}"))?;
        scenario
            .validate()
            .with_context(|| format!("scenario {name:?}"))?;
        Ok(scenario)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scenario in the shared script parses and validates.
    #[test]
    fn shared_scenarios_validate() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/client-diff/scenarios.json");
        let file = ScenariosFile::load(&path).unwrap();
        assert!(!file.scenarios.is_empty());
        for name in file.scenarios.keys() {
            file.scenario(name).unwrap();
        }
    }

    #[test]
    fn invalid_steps_are_rejected() {
        let interact = Step {
            kind: StepKind::Use,
            ..Step::default()
        };
        let scenario = Scenario {
            steps: vec![interact],
            ..Scenario::default()
        };
        assert!(
            scenario.validate().is_err(),
            "interaction without auth_input"
        );
        let bad_flag = Step {
            kind: StepKind::Move,
            ticks: 1,
            flags: vec!["flying".into()],
            ..Step::default()
        };
        assert!(validate_step(&bad_flag).is_err());
        let bad_slot = Step {
            kind: StepKind::MoveItem,
            count: 1,
            slot: 40,
            ..Step::default()
        };
        assert!(validate_step(&bad_slot).is_err());
        assert!(serde_json::from_str::<Step>(r#"{"kind":"teleport"}"#).is_err());
    }

    #[test]
    fn prefix_idles_until_the_next_step() {
        let step = |after_ms| Step {
            after_ms,
            kind: StepKind::Command,
            text: "/say".into(),
            ..Step::default()
        };
        let scenario = Scenario {
            idle_ms: 9,
            steps: vec![step(1), step(2), step(3)],
            auth_input: true,
        };
        let prefix = scenario.prefix(2).unwrap();
        assert_eq!((prefix.steps.len(), prefix.idle_ms), (2, 3));
        assert_eq!(scenario.prefix(3).unwrap().idle_ms, 9);
        assert!(scenario.prefix(4).is_err());
    }
}
