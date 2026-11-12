//! The sound BDS 1.26.51 plays for placing with items whose sound does not
//! follow the general rule in `blocks::place_sound` (round 2 of the blocks
//! lane, `testdata/bds-placements-*.json`): which sound, with which state
//! and where.

use glam::{IVec3, Vec3};

/// Where a placement sound plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// The placed block's centre.
    PlacedCentre,
    /// The placed block's corner (its integer position).
    PlacedCorner,
    /// The centre of the block that was clicked.
    ClickedCentre,
    /// The centre of the cell on the clicked face of the clicked block.
    FaceCentre,
}

/// A placement sound: event name, `Some(state)` for the placed state or
/// `None` for data -1, and where it plays; `repeat` sounds play twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlaceSound {
    pub name: &'static str,
    pub placed_state: bool,
    pub anchor: Anchor,
    pub repeat: bool,
    /// The sound carries the placed block with this property set (mushroom
    /// blocks capped on every side, glow lichen on every face), whatever was
    /// placed.
    pub fixed: Option<(&'static str, i32)>,
    /// No sound at all (a lily pad).
    pub silent: bool,
}

const fn sound(name: &'static str, placed_state: bool, anchor: Anchor) -> PlaceSound {
    PlaceSound {
        name,
        placed_state,
        anchor,
        repeat: false,
        fixed: None,
        silent: false,
    }
}

/// The measured sound of placing `placed` (block identifier) with `item`,
/// `onto` naming the block that was there when the click grew it (a
/// stacking block, a candle cake). `None`: the general rule applies.
pub(crate) fn place_sound(item: &str, placed: &str, onto: Option<&str>) -> Option<PlaceSound> {
    use Anchor::*;
    let item = item.strip_prefix("minecraft:")?;
    if placed.ends_with("candle_cake") && onto == Some("minecraft:cake") {
        return Some(sound("cake.add_candle", false, PlacedCorner));
    }
    Some(match item {
        "wheat_seeds" | "carrot" | "potato" | "beetroot_seeds" | "melon_seeds"
        | "pumpkin_seeds" | "torchflower_seeds" | "pitcher_pod" | "sweet_berries"
        | "nether_wart" | "glow_berries" => sound("place", true, ClickedCentre),
        "cocoa_beans" | "pink_petals" | "wildflowers" | "leaf_litter"
            if onto.is_some() || item == "cocoa_beans" =>
        {
            sound("place", true, PlacedCorner)
        }
        "bamboo" => sound("block.bamboo_sapling.place", false, PlacedCorner),
        "sea_pickle" => sound("item.use.on", true, FaceCentre),
        _ if item.ends_with("candle") => sound("item.use.on", true, FaceCentre),
        "turtle_egg" => sound("place", true, PlacedCentre),
        "brown_mushroom_block" | "red_mushroom_block" => PlaceSound {
            fixed: Some(("huge_mushroom_bits", 14)),
            ..sound("place", true, PlacedCentre)
        },
        "glow_lichen" => PlaceSound {
            fixed: Some(("multi_face_direction_bits", 63)),
            ..sound("place", true, PlacedCentre)
        },
        "waterlily" => PlaceSound {
            silent: true,
            ..sound("", false, PlacedCentre)
        },
        "frog_spawn" => sound("lay_spawn", false, PlacedCorner),
        _ if item.ends_with("_skull")
            || item.ends_with("_head")
            || item.ends_with("_coral_fan") =>
        {
            sound("place", true, PlacedCentre)
        }
        _ if item.ends_with("_coral") => PlaceSound {
            repeat: true,
            ..sound("place", true, PlacedCentre)
        },
        _ => return None,
    })
}

/// Where `anchor` is for a placement at `placed` from a click on `clicked`
/// through `face`.
pub(crate) fn position(anchor: Anchor, placed: IVec3, clicked: IVec3, face: u8) -> Vec3 {
    let centre = |pos: IVec3| pos.as_vec3() + Vec3::splat(0.5);
    match anchor {
        Anchor::PlacedCentre => centre(placed),
        Anchor::PlacedCorner => placed.as_vec3(),
        Anchor::ClickedCentre => centre(clicked),
        Anchor::FaceCentre => {
            centre(clicked + super::placement::face_offset(face).unwrap_or(IVec3::ZERO))
        }
    }
}

/// Play the measured placement sound of `placed_state` at `placed`, placed
/// with `item` from a click on `clicked` through `face` into a block that was
/// `onto` (when the click grew it). Returns `false` when the general rule
/// applies instead.
pub(crate) fn play(
    world: &bevy_ecs::world::World,
    item: &str,
    placed_state: u32,
    placed: IVec3,
    onto: Option<&str>,
    clicked: IVec3,
    face: u8,
) -> bool {
    use jolyne::valentine::{LevelSoundEventPacket, McpePacket, Vec3 as WireVec3};
    let Some(name) = world
        .get_resource::<super::types::BlockRegistryResource>()
        .and_then(|registry| registry.0.get_by_runtime_id(placed_state))
        .map(|entry| entry.string_id.clone())
    else {
        return false;
    };
    let Some(sound) = place_sound(item, &name, onto) else {
        return false;
    };
    let at = position(sound.anchor, placed, clicked, face);
    if sound.silent {
        return true;
    }
    let state = match sound.fixed {
        Some((property, value)) => world
            .get_resource::<super::types::BlockRegistryResource>()
            .and_then(|registry| registry.0.get_by_runtime_id(placed_state))
            .and_then(|entry| {
                entry.with_state_value(
                    placed_state,
                    property,
                    crate::registry::block::StateValue::Int(value),
                )
            })
            .unwrap_or(placed_state),
        None => placed_state,
    };
    let data = if sound.placed_state {
        super::types::block_network_ids(world).to_wire(state) as i32
    } else {
        -1
    };
    let packet = McpePacket::from(LevelSoundEventPacket {
        sound_event: sound.name.into(),
        position: WireVec3 {
            x: at.x,
            y: at.y,
            z: at.z,
        },
        data,
        actor_identifier: "".into(),
        is_baby: false,
        is_global: false,
        actor_unique_id: -1,
        fire_at_position: None,
    });
    for _ in 0..if sound.repeat { 2 } else { 1 } {
        super::block_actors::broadcast(world, placed, &packet, None);
    }
    // A turtle egg added to a clutch also sends level event 2005 at its corner.
    if item == "minecraft:turtle_egg" && onto.is_some() {
        let event = McpePacket::from(jolyne::valentine::LevelEventPacket {
            event_id: 2005,
            position: WireVec3 {
                x: placed.x as f32,
                y: placed.y as f32,
                z: placed.z as f32,
            },
            data: super::types::block_network_ids(world).to_wire(placed_state) as i32,
        });
        super::block_actors::broadcast(world, placed, &event, None);
    }
    true
}
