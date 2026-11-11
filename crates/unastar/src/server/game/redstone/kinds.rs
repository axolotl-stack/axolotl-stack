//! What each block state is to a circuit: dust, a power source, a diode
//! (repeater, comparator), a torch, an observer, a block the circuit acts on
//! (lamps, doors, rails, ...), and whether it can be powered or cut dust.
//!
//! The classification is built once per block registry and indexed by
//! runtime id. Which blocks conduct power, cut dust that climbs past them and
//! let dust climb onto or come down from them is measured
//! (`unastar_data::redstone`); the rest follows the block states.

use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::block_update::support::Face;

/// The six faces in protocol order (down, up, north, south, west, east).
pub(crate) const FACES: [Face; 6] = [
    Face::Down,
    Face::Up,
    Face::North,
    Face::South,
    Face::West,
    Face::East,
];

/// The four horizontal faces.
pub(crate) const SIDES: [Face; 4] = [Face::North, Face::South, Face::West, Face::East];

/// What a circuit acts on when its power changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Consumer {
    /// Lights at once, goes out two redstone ticks after losing power.
    Lamp {
        lit: bool,
    },
    /// Doors: either half's power opens the door; the lower half stores it.
    Door {
        upper: bool,
        open: bool,
    },
    /// Trapdoors and fence gates: `open_bit` follows the power.
    Opens {
        open: bool,
    },
    /// Copper bulbs: `powered_bit` follows the power a tick after the
    /// update; each rising edge toggles `lit`.
    Bulb {
        lit: bool,
        powered: bool,
    },
    /// Hoppers: `toggle_bit` follows the power (a powered hopper is locked).
    Hopper {
        locked: bool,
    },
    /// Dispensers, droppers and crafters: `triggered_bit` follows the power.
    Triggered {
        triggered: bool,
    },
    /// Powered and activator rails: `rail_data_bit`, carried along rails.
    Rail {
        activator: bool,
        powered: bool,
    },
    /// TNT and note blocks: act on a rising edge only.
    Tnt,
    NoteBlock,
}

/// A block's part in a circuit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Not part of a circuit (it may still conduct power or cut dust).
    None,
    Dust {
        signal: u8,
    },
    /// A redstone block: 15 to every neighbour, powering no block.
    RedstoneBlock,
    /// Levers and buttons: 15 to every neighbour while on, strong power into
    /// the block they are attached to.
    Switch {
        attached: Face,
        on: bool,
    },
    /// Pressure plates: their signal to every neighbour, strong power into
    /// the block below.
    Plate {
        signal: u8,
    },
    /// Tripwire hooks: like a switch.
    Hook {
        attached: Face,
        on: bool,
    },
    /// Daylight detectors: their signal to every neighbour.
    Daylight {
        signal: u8,
    },
    /// Redstone torches: 15 to every neighbour but the one they hang on,
    /// strong power into the block above; off while that block is powered.
    Torch {
        attached: Face,
        lit: bool,
    },
    /// Repeaters: read the side named by `cardinal_direction` and power the
    /// opposite side after `delay` + 1 redstone ticks.
    Repeater {
        input: Face,
        delay: u8,
        powered: bool,
    },
    /// Comparators: read the same side as a repeater; side inputs compare or
    /// subtract.
    Comparator {
        input: Face,
        subtract: bool,
        lit: bool,
    },
    /// Observers: watch the block they face and pulse out of their back.
    Observer {
        facing: Face,
        powered: bool,
    },
    Consumer(Consumer),
}

impl Kind {
    pub(crate) fn is_dust(self) -> bool {
        matches!(self, Kind::Dust { .. })
    }

    /// Whether a dust beside this block connects to it from `toward` (the
    /// direction from the dust to this block): dust points at it.
    pub(crate) fn connects_dust(self, toward: Face) -> bool {
        match self {
            Kind::Dust { .. }
            | Kind::RedstoneBlock
            | Kind::Switch { .. }
            | Kind::Plate { .. }
            | Kind::Hook { .. }
            | Kind::Daylight { .. }
            | Kind::Torch { .. }
            | Kind::Comparator { .. } => true,
            Kind::Repeater { input, .. } => toward == input || toward == input.opposite(),
            Kind::Observer { facing, .. } => toward == facing,
            Kind::None | Kind::Consumer(_) => false,
        }
    }
}

/// Facts about one block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Facts {
    pub kind: Kind,
    /// Can be powered (strongly or weakly) and power what touches it.
    pub conductor: bool,
    /// Stops dust climbing past it (above the lower dust).
    pub cuts: bool,
    /// Dust on top of it carries power up from the dust below beside it.
    pub climb_up: bool,
    /// Dust on top of it carries power down to the dust below beside it.
    pub climb_down: bool,
    /// Has a comparator reading of its own (`analog`), or is a container
    /// whose reading comes from its contents.
    pub analog: Analog,
}

/// What a comparator behind which this block stands reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Analog {
    None,
    /// A reading from the block state.
    State(u8),
    /// A container: the reading comes from its contents.
    Container,
}

fn byte(entry: &BlockEntry, state: u32, property: &str) -> Option<bool> {
    match entry.state_value(state, property)? {
        StateValue::Byte(value) => Some(value != 0),
        _ => None,
    }
}

fn int(entry: &BlockEntry, state: u32, property: &str) -> Option<i32> {
    match entry.state_value(state, property)? {
        StateValue::Int(value) => Some(value),
        _ => None,
    }
}

fn text(entry: &BlockEntry, state: u32, property: &str) -> Option<&'static str> {
    match entry.state_value(state, property)? {
        StateValue::Str(value) => Some(value),
        _ => None,
    }
}

fn facing_ordinal(facing: i32) -> Option<Face> {
    Some(match facing {
        0 => Face::Down,
        1 => Face::Up,
        2 => Face::North,
        3 => Face::South,
        4 => Face::West,
        5 => Face::East,
        _ => return None,
    })
}

fn signal(entry: &BlockEntry, state: u32) -> u8 {
    int(entry, state, "redstone_signal").map_or(0, |value| value.clamp(0, 15) as u8)
}

fn classify(entry: &BlockEntry, state: u32) -> Kind {
    let id = entry.string_id.as_str();
    match id {
        "minecraft:redstone_wire" => {
            return Kind::Dust {
                signal: signal(entry, state),
            };
        }
        "minecraft:redstone_block" => return Kind::RedstoneBlock,
        "minecraft:redstone_torch" | "minecraft:unlit_redstone_torch" => {
            let attached = match text(entry, state, "torch_facing_direction") {
                Some("top") => Face::Down,
                Some(other) => match Face::from_name(other) {
                    Some(face) => face,
                    None => return Kind::None,
                },
                None => return Kind::None,
            };
            return Kind::Torch {
                attached,
                lit: id == "minecraft:redstone_torch",
            };
        }
        "minecraft:unpowered_repeater" | "minecraft:powered_repeater" => {
            let Some(input) =
                text(entry, state, "minecraft:cardinal_direction").and_then(Face::from_name)
            else {
                return Kind::None;
            };
            return Kind::Repeater {
                input,
                delay: int(entry, state, "repeater_delay").unwrap_or(0).clamp(0, 3) as u8,
                powered: id == "minecraft:powered_repeater",
            };
        }
        "minecraft:unpowered_comparator" | "minecraft:powered_comparator" => {
            let Some(input) =
                text(entry, state, "minecraft:cardinal_direction").and_then(Face::from_name)
            else {
                return Kind::None;
            };
            return Kind::Comparator {
                input,
                subtract: byte(entry, state, "output_subtract_bit") == Some(true),
                lit: byte(entry, state, "output_lit_bit") == Some(true),
            };
        }
        "minecraft:observer" => {
            let Some(facing) =
                text(entry, state, "minecraft:facing_direction").and_then(Face::from_name)
            else {
                return Kind::None;
            };
            return Kind::Observer {
                facing,
                powered: byte(entry, state, "powered_bit") == Some(true),
            };
        }
        "minecraft:lever" => {
            let attached = match text(entry, state, "lever_direction") {
                Some(direction) if direction.starts_with("up_") => Face::Down,
                Some(direction) if direction.starts_with("down_") => Face::Up,
                Some(direction) => match Face::from_name(direction) {
                    Some(face) => face.opposite(),
                    None => return Kind::None,
                },
                None => return Kind::None,
            };
            return Kind::Switch {
                attached,
                on: byte(entry, state, "open_bit") == Some(true),
            };
        }
        "minecraft:tripwire_hook" => {
            let attached = [Face::North, Face::East, Face::South, Face::West]
                .get(
                    usize::try_from(int(entry, state, "direction").unwrap_or(-1))
                        .unwrap_or(usize::MAX),
                )
                .copied();
            return attached.map_or(Kind::None, |attached| Kind::Hook {
                attached,
                on: byte(entry, state, "powered_bit") == Some(true),
            });
        }
        "minecraft:daylight_detector" | "minecraft:daylight_detector_inverted" => {
            return Kind::Daylight {
                signal: signal(entry, state),
            };
        }
        "minecraft:redstone_lamp" => return Kind::Consumer(Consumer::Lamp { lit: false }),
        "minecraft:lit_redstone_lamp" => return Kind::Consumer(Consumer::Lamp { lit: true }),
        "minecraft:hopper" => {
            return Kind::Consumer(Consumer::Hopper {
                locked: byte(entry, state, "toggle_bit") == Some(true),
            });
        }
        "minecraft:dispenser" | "minecraft:dropper" | "minecraft:crafter" => {
            return Kind::Consumer(Consumer::Triggered {
                triggered: byte(entry, state, "triggered_bit") == Some(true),
            });
        }
        "minecraft:golden_rail" | "minecraft:activator_rail" => {
            return Kind::Consumer(Consumer::Rail {
                activator: id == "minecraft:activator_rail",
                powered: byte(entry, state, "rail_data_bit") == Some(true),
            });
        }
        "minecraft:tnt" => return Kind::Consumer(Consumer::Tnt),
        "minecraft:noteblock" => return Kind::Consumer(Consumer::NoteBlock),
        _ => {}
    }
    if id.ends_with("_button") {
        let attached = int(entry, state, "facing_direction")
            .and_then(facing_ordinal)
            .map(Face::opposite);
        return attached.map_or(Kind::None, |attached| Kind::Switch {
            attached,
            on: byte(entry, state, "button_pressed_bit") == Some(true),
        });
    }
    if id.ends_with("_pressure_plate") {
        return Kind::Plate {
            signal: signal(entry, state),
        };
    }
    if id.ends_with("copper_bulb") {
        return Kind::Consumer(Consumer::Bulb {
            lit: byte(entry, state, "lit") == Some(true),
            powered: byte(entry, state, "powered_bit") == Some(true),
        });
    }
    if entry.state_value(state, "door_hinge_bit").is_some() {
        return Kind::Consumer(Consumer::Door {
            upper: byte(entry, state, "upper_block_bit") == Some(true),
            open: byte(entry, state, "open_bit") == Some(true),
        });
    }
    if (id.ends_with("_trapdoor") || id == "minecraft:trapdoor" || id.ends_with("fence_gate"))
        && let Some(open) = byte(entry, state, "open_bit")
    {
        return Kind::Consumer(Consumer::Opens { open });
    }
    Kind::None
}

/// The comparator reading of a block that has one from its state.
fn analog(entry: &BlockEntry, state: u32) -> Analog {
    let id = entry.string_id.as_str();
    let level = |property: &str| int(entry, state, property).unwrap_or(0);
    match id {
        // BDS reads composters 0-8 as their level (7 becomes 8 when ready).
        "minecraft:composter" => Analog::State(level("composter_fill_level").clamp(0, 8) as u8),
        // Each bite takes 2 from 14.
        "minecraft:cake" => Analog::State((14 - 2 * level("bite_counter")).clamp(0, 14) as u8),
        // Six cauldron levels read 0-3: (level + 1) / 2 (BDS 1.26.51).
        "minecraft:cauldron" => Analog::State(((level("fill_level") + 1) / 2).clamp(0, 15) as u8),
        "minecraft:end_portal_frame" => {
            Analog::State(if byte(entry, state, "end_portal_eye_bit") == Some(true) {
                15
            } else {
                0
            })
        }
        "minecraft:respawn_anchor" => Analog::State(match level("respawn_anchor_charge") {
            0 => 0,
            1 => 3,
            2 => 7,
            3 => 11,
            _ => 15,
        }),
        "minecraft:beehive" | "minecraft:bee_nest" => {
            Analog::State(level("honey_level").clamp(0, 15) as u8)
        }
        // The last slot a player used decides it; nothing was used yet.
        "minecraft:chiseled_bookshelf" => Analog::State(0),
        "minecraft:chest"
        | "minecraft:trapped_chest"
        | "minecraft:barrel"
        | "minecraft:hopper"
        | "minecraft:dispenser"
        | "minecraft:dropper"
        | "minecraft:furnace"
        | "minecraft:lit_furnace"
        | "minecraft:blast_furnace"
        | "minecraft:lit_blast_furnace"
        | "minecraft:smoker"
        | "minecraft:lit_smoker"
        | "minecraft:brewing_stand" => Analog::Container,
        _ if id.ends_with("shulker_box") => Analog::Container,
        _ => Analog::None,
    }
}

/// Whether power climbs onto dust on top of a block state the sweep did not
/// measure, and comes down its far side: BDS carries it both ways over any
/// block dust stands on, except down from glowstone, hoppers, top slabs and
/// upside-down stairs (`redstone_dust_blocks.json`).
fn unmeasured_climb(entry: &BlockEntry, state: u32) -> (bool, bool) {
    let no_down = matches!(
        entry.string_id.as_str(),
        "minecraft:glowstone" | "minecraft:hopper"
    ) || text(entry, state, "minecraft:vertical_half") == Some("top")
        || byte(entry, state, "upside_down_bit") == Some(true);
    (true, !no_down)
}

/// The other names of a block that only changes its name with power or light
/// (`lit_`, `unlit_`, `powered_`, `unpowered_`).
fn variant_names(id: &str) -> Vec<String> {
    let Some(rest) = id.strip_prefix("minecraft:") else {
        return Vec::new();
    };
    let base = ["lit_", "unlit_", "powered_", "unpowered_"]
        .iter()
        .find_map(|prefix| rest.strip_prefix(prefix))
        .unwrap_or(rest);
    ["", "lit_", "unlit_", "powered_", "unpowered_"]
        .iter()
        .map(|prefix| format!("minecraft:{prefix}{base}"))
        .filter(|name| name != id)
        .collect()
}

/// Per-runtime-id facts for one registry.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    facts: Vec<Facts>,
}

const NOTHING: Facts = Facts {
    kind: Kind::None,
    conductor: false,
    cuts: false,
    climb_up: false,
    climb_down: false,
    analog: Analog::None,
};

impl Table {
    pub(crate) fn build(registry: &BlockRegistry) -> Self {
        let measured = unastar_data::redstone::blocks();
        let mut facts = Vec::new();
        for entry in registry.iter() {
            let id = entry.string_id.as_str();
            // A lit lamp, a powered repeater or an unlit torch is the same
            // block to a circuit as the variant the sweep measured.
            let variants = variant_names(id);
            let listed = |set: &std::collections::HashSet<String>| {
                set.contains(id) || variants.iter().any(|name| set.contains(name))
            };
            let conductor = listed(&measured.conductors);
            let cuts = listed(&measured.cuts);
            for state in entry.min_state_id..=entry.max_state_id {
                let index = state as usize;
                if facts.len() <= index {
                    facts.resize(index + 1, NOTHING);
                }
                let climb = measured
                    .climb(id, |property| {
                        entry.state_value(state, property).map(|value| match value {
                            StateValue::Byte(b) => serde_json::Value::Bool(b != 0),
                            StateValue::Int(i) => serde_json::Value::from(i),
                            StateValue::Str(s) => serde_json::Value::from(s),
                        })
                    })
                    .unwrap_or_else(|| unmeasured_climb(entry, state));
                facts[index] = Facts {
                    kind: classify(entry, state),
                    conductor,
                    cuts,
                    climb_up: climb.0,
                    climb_down: climb.1,
                    analog: analog(entry, state),
                };
            }
        }
        Self { facts }
    }

    pub(crate) fn get(&self, state: u32) -> Facts {
        self.facts.get(state as usize).copied().unwrap_or(NOTHING)
    }
}
