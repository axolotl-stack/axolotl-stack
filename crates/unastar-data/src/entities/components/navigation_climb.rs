use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:navigation.climb`. Allows this entity to generate paths that include vertical walls like the vanilla Spiders do.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct NavigationClimb {
    ///Tells the pathfinder to avoid blocks that cause damage when finding a path.
    pub avoid_damage_blocks: Option<bool>,
    ///Tells the pathfinder to avoid portals (like nether portals) when finding a path.
    pub avoid_portals: Option<bool>,
    ///Whether or not the pathfinder should avoid tiles that are exposed to the sun when creating paths.
    pub avoid_sun: Option<bool>,
    ///Tells the pathfinder to avoid water when creating a path.
    pub avoid_water: Option<bool>,
    ///Tells the pathfinder which blocks to avoid when creating a path.
    pub blocks_to_avoid: Option<Vec<crate::types::BedrockValue>>,
    ///Tells the pathfinder whether or not it can jump out of water (like a dolphin).
    pub can_breach: Option<bool>,
    ///Tells the pathfinder that it can path through a closed door and break it.
    pub can_break_doors: Option<bool>,
    ///Tells the pathfinder whether or not it can jump up blocks.
    pub can_jump: Option<bool>,
    ///Tells the pathfinder that it can path through a closed door assuming the AI will open the door.
    pub can_open_doors: Option<bool>,
    ///Tells the pathfinder that it can path through a closed iron door assuming the AI will open the door.
    pub can_open_iron_doors: Option<bool>,
    ///Whether a path can be created through a door.
    pub can_pass_doors: Option<bool>,
    ///Tells the pathfinder that it can start pathing when in the air.
    pub can_path_from_air: Option<bool>,
    ///Tells the pathfinder whether or not it can travel on the surface of the lava.
    pub can_path_over_lava: Option<bool>,
    ///Tells the pathfinder whether or not it can travel on the surface of the water.
    pub can_path_over_water: Option<bool>,
    ///Tells the pathfinder whether or not it will be pulled down by gravity while in water.
    pub can_sink: Option<bool>,
    ///Tells the pathfinder whether or not it can path anywhere through water and plays swimming animation along that path.
    pub can_swim: Option<bool>,
    ///Tells the pathfinder whether or not it can walk on the ground outside water.
    pub can_walk: Option<bool>,
    ///Tells the pathfinder whether or not it can travel in lava like walking on ground.
    pub can_walk_in_lava: Option<bool>,
    ///Tells the pathfinder whether or not it can walk on the ground underwater.
    pub is_amphibious: Option<bool>,
}
impl Default for NavigationClimb {
    fn default() -> Self {
        Self {
            avoid_damage_blocks: Some(false),
            avoid_portals: Some(false),
            avoid_sun: Some(false),
            avoid_water: Some(false),
            blocks_to_avoid: None,
            can_breach: Some(false),
            can_break_doors: Some(false),
            can_jump: Some(true),
            can_open_doors: Some(false),
            can_open_iron_doors: Some(false),
            can_pass_doors: Some(true),
            can_path_from_air: Some(false),
            can_path_over_lava: Some(false),
            can_path_over_water: Some(false),
            can_sink: Some(true),
            can_swim: Some(false),
            can_walk: Some(true),
            can_walk_in_lava: Some(false),
            is_amphibious: Some(false),
        }
    }
}
