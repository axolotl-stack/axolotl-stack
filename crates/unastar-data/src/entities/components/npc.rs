use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct NpcNpcDataPickerOffsets {
    ///UNDOCUMENTED.
    pub scale: Option<Vec<f32>>,
    ///UNDOCUMENTED.
    pub translate: Option<Vec<f32>>,
}
impl Default for NpcNpcDataPickerOffsets {
    fn default() -> Self {
        Self {
            scale: None,
            translate: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct NpcNpcDataPortraitOffsets {
    ///UNDOCUMENTED.
    pub scale: Option<Vec<f32>>,
    ///UNDOCUMENTED.
    pub translate: Option<Vec<f32>>,
}
impl Default for NpcNpcDataPortraitOffsets {
    fn default() -> Self {
        Self {
            scale: None,
            translate: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct NpcNpcDataSkinList {
    ///UNDOCUMENTED.
    pub mark_variant: Option<i32>,
    ///UNDOCUMENTED.
    pub variant: Option<i32>,
}
impl Default for NpcNpcDataSkinList {
    fn default() -> Self {
        Self {
            mark_variant: None,
            variant: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct NpcNpcData {
    ///UNDOCUMENTED.
    pub picker_offsets: Option<NpcNpcDataPickerOffsets>,
    ///UNDOCUMENTED.
    pub portrait_offsets: Option<NpcNpcDataPortraitOffsets>,
    ///UNDOCUMENTED.
    pub skin_list: Option<Vec<NpcNpcDataSkinList>>,
}
impl Default for NpcNpcData {
    fn default() -> Self {
        Self {
            picker_offsets: None,
            portrait_offsets: None,
            skin_list: None,
        }
    }
}
/// Bedrock component `minecraft:npc`. Sets this entity as an NPC
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Npc {
    ///The data belonging to this npc.
    pub npc_data: Option<NpcNpcData>,
}
impl Default for Npc {
    fn default() -> Self {
        Self { npc_data: None }
    }
}
