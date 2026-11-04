//! Client-side state the scenario steps need, tracked from server packets the
//! way `tools/client-diff/gt-client/session.go` tracks it: own position and
//! rotation, inventory contents with their stack network IDs, block runtime
//! IDs the server sent, the last runtime ID and position per entity type,
//! the open block window and the crafting recipes.

use std::collections::HashMap;

use jolyne::valentine::*;

/// Eye height above the feet in StartGame/MovePlayer/PlayerAuthInput positions.
pub const EYE_HEIGHT: f32 = 1.62;

/// An inventory item as the server sends it (stack network ID included).
pub type Item = CerealizerNetworkItemStackDescriptorSerializedData;

pub fn count(item: &Item) -> u16 {
    if item.id == 0 { 0 } else { item.stacksize }
}

pub fn stack_id(item: &Item) -> i32 {
    item.net_id_variant.unwrap_or(0)
}

fn set_stack_id(item: &mut Item, id: i32) {
    item.net_id_variant = (id != 0).then_some(id);
}

/// A recipe the `craft` step can use: shaped/shapeless (crafting grid,
/// stonecutter), multi (keyed by UUID, e.g. grid repair) or smithing.
#[derive(Clone, Debug, Default)]
pub struct CraftRecipe {
    pub network_id: u32,
    pub output_count: u16,
    /// The crafted item (empty for multi and smithing trim recipes).
    pub output: Item,
}

pub struct Session {
    pub runtime_id: u64,
    pub pos: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub tick: u64,
    anchor: Option<[i32; 3]>,
    pub selected: i32,
    pub inventory: HashMap<i32, Item>,
    /// The UI cursor stack (PlayerUI container slot 0).
    pub cursor: Item,
    pub blocks: HashMap<[i32; 3], u32>,
    pub entities: HashMap<String, u64>,
    pub positions: HashMap<u64, [f32; 3]>,
    pub recipes: HashMap<String, CraftRecipe>,
    pub request_id: i32,
    /// The open block window (ContainerOpen) and its contents.
    pub window: u8,
    pub window_type: u8,
    pub window_items: HashMap<i32, Item>,
    /// The inventory crafting grid (PlayerUI 28-31) as responses report it.
    pub grid: HashMap<i32, Item>,
    /// Recipe network IDs of the options of the last PlayerEnchantOptions.
    pub enchant_options: Vec<u32>,
    /// Problems the bot logs on stderr (rejected requests, refused windows):
    /// kept with the run so a report can show them.
    pub notes: Vec<String>,
}

/// Player inventory window.
const WINDOW_INVENTORY: u32 = 0;
const INVENTORY_SIZE: i32 = crate::scenario::INVENTORY_SIZE;

/// Window types (ContainerOpen) whose InventoryContent lists the inputs from
/// index 0 although requests address them by PlayerUI slot: the PlayerUI
/// slot of index 0.
pub fn window_slot_offset(window_type: u8) -> i32 {
    match window_type {
        5 => 1,   // anvil
        33 => 51, // smithing table
        26 => 16, // grindstone
        3 => 14,  // enchanting table
        29 => 3,  // stonecutter
        24 => 9,  // loom
        30 => 12, // cartography table
        _ => 0,
    }
}

/// The container a step names (see `scenario::CONTAINERS`).
pub fn container(name: &str) -> EnumsContainerEnumName {
    use EnumsContainerEnumName::*;
    match name {
        "armor" => Armorcontainer,
        "offhand" => Offhandcontainer,
        "chest" => Levelentitycontainer,
        "barrel" => Barrelcontainer,
        "ingredient" => Furnaceingredientcontainer,
        "fuel" => Furnacefuelcontainer,
        "result" => Furnaceresultcontainer,
        "crafting" => Craftinginputcontainer,
        "anvil_input" => Anvilinputcontainer,
        "anvil_material" => Anvilmaterialcontainer,
        "smithing_input" => Smithingtableinputcontainer,
        "smithing_material" => Smithingtablematerialcontainer,
        "smithing_template" => Smithingtabletemplatecontainer,
        "grindstone_input" => Grindstoneinputcontainer,
        "grindstone_additional" => Grindstoneadditionalcontainer,
        "enchanting_input" => Enchantinginputcontainer,
        "enchanting_material" => Enchantingmaterialcontainer,
        "brewing_input" => Brewingstandinputcontainer,
        "brewing_result" => Brewingstandresultcontainer,
        "brewing_fuel" => Brewingstandfuelcontainer,
        "shulker_box" => Shulkerboxcontainer,
        "crafter" => Crafterlevelentitycontainer,
        "stonecutter_input" => Stonecutterinputcontainer,
        "loom_input" => Loominputcontainer,
        "loom_dye" => Loomdyecontainer,
        "loom_material" => Loommaterialcontainer,
        "cartography_input" => Cartographyinputcontainer,
        "cartography_additional" => Cartographyadditionalcontainer,
        other => unreachable!("container {other:?} passed validation"),
    }
}

/// Whether a container addresses the open block window.
fn in_block(container: EnumsContainerEnumName) -> bool {
    use EnumsContainerEnumName::*;
    matches!(
        container,
        Levelentitycontainer
            | Barrelcontainer
            | Furnaceingredientcontainer
            | Furnacefuelcontainer
            | Furnaceresultcontainer
            | Anvilinputcontainer
            | Anvilmaterialcontainer
            | Smithingtableinputcontainer
            | Smithingtablematerialcontainer
            | Smithingtabletemplatecontainer
            | Grindstoneinputcontainer
            | Grindstoneadditionalcontainer
            | Enchantinginputcontainer
            | Enchantingmaterialcontainer
            | Brewingstandinputcontainer
            | Brewingstandresultcontainer
            | Brewingstandfuelcontainer
            | Shulkerboxcontainer
            | Crafterlevelentitycontainer
            | Stonecutterinputcontainer
            | Loominputcontainer
            | Loomdyecontainer
            | Loommaterialcontainer
            | Cartographyinputcontainer
            | Cartographyadditionalcontainer
    )
}

fn vec3(v: &Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

impl Session {
    pub fn new(runtime_id: u64, position: &Vec3) -> Self {
        Self {
            runtime_id,
            pos: vec3(position),
            pitch: 0.0,
            yaw: 0.0,
            tick: 0,
            anchor: None,
            selected: 0,
            inventory: HashMap::new(),
            cursor: Item::default(),
            blocks: HashMap::new(),
            entities: HashMap::new(),
            positions: HashMap::new(),
            recipes: HashMap::new(),
            request_id: 0,
            window: 0,
            window_type: 0,
            window_items: HashMap::new(),
            grid: HashMap::new(),
            enchant_options: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// The spawn feet block, fixed on first use rather than at StartGame: a
    /// fresh world announces a placeholder height (y=32767) that MovePlayer
    /// corrects once the spawn is resolved.
    pub fn anchor(&mut self) -> [i32; 3] {
        let pos = self.pos;
        *self.anchor.get_or_insert_with(|| {
            [
                pos[0].floor() as i32,
                (pos[1] - EYE_HEIGHT + 0.01).floor() as i32,
                pos[2].floor() as i32,
            ]
        })
    }

    /// Substitutes the anchor block coordinates for `{x}`, `{y}` and `{z}`.
    pub fn expand(&mut self, text: &str) -> String {
        let [x, y, z] = self.anchor();
        text.replace("{x}", &x.to_string())
            .replace("{y}", &y.to_string())
            .replace("{z}", &z.to_string())
    }

    /// The block at the given offset from the anchor.
    pub fn block(&mut self, x: i32, y: i32, z: i32) -> [i32; 3] {
        let anchor = self.anchor();
        [anchor[0] + x, anchor[1] + y, anchor[2] + z]
    }

    /// Vanilla clients number item stack requests with negative odd IDs.
    pub fn next_request_id(&mut self) -> i32 {
        self.request_id -= 2;
        self.request_id + 1
    }

    /// Turns the head towards `target`.
    pub fn look_at(&mut self, target: [f32; 3]) {
        let d = [
            target[0] - self.pos[0],
            target[1] - self.pos[1],
            target[2] - self.pos[2],
        ];
        let horizontal = f64::from(d[0]).hypot(f64::from(d[2]));
        self.yaw = (-f64::from(d[0]).atan2(f64::from(d[2])) * 180.0 / std::f64::consts::PI) as f32;
        self.pitch = (-f64::from(d[1]).atan2(horizontal) * 180.0 / std::f64::consts::PI) as f32;
    }

    pub fn item(&self, slot: i32) -> Option<&Item> {
        self.inventory.get(&slot).filter(|item| count(item) > 0)
    }

    pub fn held(&self) -> Item {
        self.inventory
            .get(&self.selected)
            .cloned()
            .unwrap_or_default()
    }

    /// Updates the tracked state from one server packet and returns the
    /// packets the client must answer with (respawn and dimension handshakes).
    pub fn handle(&mut self, packet: McpePacketData) -> Vec<McpePacket> {
        use McpePacketData as P;
        match packet {
            P::MovePlayerPacket(p) if p.player_runtime_id.actor_runtime_id == self.runtime_id => {
                self.pos = vec3(&p.position);
            }
            P::CorrectPlayerMovePredictionPacket(p) => self.pos = vec3(&p.pos),
            P::ChangeDimensionPacket(p) => {
                self.pos = vec3(&p.position);
                return vec![self.action(EnumsPlayerActionType::Changedimensionack)];
            }
            P::RespawnPacket(p) => match p.state {
                EnumsPlayerRespawnState::Searchingforspawn => {
                    return vec![
                        McpePacket::from(RespawnPacket {
                            position: p.position,
                            state: EnumsPlayerRespawnState::Clientreadytospawn,
                            player_runtime_id: ActorRuntimeId {
                                actor_runtime_id: self.runtime_id,
                            },
                        }),
                        self.action(EnumsPlayerActionType::Respawn),
                    ];
                }
                EnumsPlayerRespawnState::Readytospawn => self.pos = vec3(&p.position),
                _ => {}
            },
            P::ContainerOpenPacket(p) => {
                self.window = p.container_id;
                self.window_type = p.container_type;
                self.window_items.clear();
            }
            P::InventoryContentPacket(p) => {
                if p.container_id == WINDOW_INVENTORY {
                    for (slot, item) in (0..INVENTORY_SIZE).zip(p.slots) {
                        self.inventory.insert(slot, item);
                    }
                } else if self.window != 0 && p.container_id == u32::from(self.window) {
                    let offset = window_slot_offset(self.window_type);
                    for (slot, item) in (offset..).zip(p.slots) {
                        self.window_items.insert(slot, item);
                    }
                }
            }
            P::InventorySlotPacket(p) => {
                let slot = p.slot as i32;
                if u32::from(p.container_id) == WINDOW_INVENTORY && slot < INVENTORY_SIZE {
                    self.inventory.insert(slot, p.item);
                } else if self.window != 0 && p.container_id == self.window {
                    self.window_items
                        .insert(window_slot_offset(self.window_type) + slot, p.item);
                }
            }
            P::InventoryTransactionPacket(p) => {
                // Commands such as /enchant replace inventory stacks (with new
                // stack network IDs) only through a server transaction.
                let actions = match p.transaction {
                    InventoryTransactionPacketTransaction::NormalTransactionData(t) => t.actions,
                    InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(t) => {
                        t.actions
                    }
                    InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
                        t,
                    ) => t.actions,
                    InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(t) => {
                        t.actions
                    }
                    InventoryTransactionPacketTransaction::InventoryMismatchData(_) => {
                        InventoryTransaction::default()
                    }
                };
                for action in actions.actions {
                    if action.source.source_type == EnumsInventorySourceType::Containerinventory
                        && action.source.container_id == Some(WINDOW_INVENTORY as i8)
                        && (action.slot as i32) < INVENTORY_SIZE
                    {
                        self.inventory.insert(action.slot as i32, action.to_item);
                    }
                }
            }
            P::UpdateBlockPacket(p) if p.layer == 0 => {
                let pos = p.block_position;
                self.blocks
                    .insert([pos.x, pos.y, pos.z], p.block_runtime_id);
            }
            P::AddActorPacket(p) => {
                let id = p.target_runtime_id.actor_runtime_id;
                self.entities.insert(p.actor_type.to_string(), id);
                self.positions.insert(id, vec3(&p.position));
            }
            P::MoveActorAbsolutePacket(p) => {
                self.positions.insert(
                    p.move_data.actor_runtime_id.actor_runtime_id,
                    vec3(&p.move_data.position),
                );
            }
            P::CraftingDataPacket(p) => self.add_recipes(*p),
            P::PlayerEnchantOptionsPacket(p) => {
                self.enchant_options = p
                    .options
                    .iter()
                    .map(|option| option.enchant_net_id.raw_id)
                    .collect();
            }
            P::ItemStackResponsePacket(p) => {
                for response in p.responses {
                    if response.result != EnumsItemStackNetResult::Success {
                        self.notes.push(format!(
                            "item stack request {} rejected with {:?}",
                            response.client_request_id.id, response.result
                        ));
                        continue;
                    }
                    for info in response.containers.unwrap_or_default() {
                        for slot in &info.slots {
                            self.apply_slot(info.full_container_name.container_name, slot);
                        }
                    }
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn action(&self, action: EnumsPlayerActionType) -> McpePacket {
        McpePacket::from(PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            action,
            face: -1,
            ..Default::default()
        })
    }

    /// Records the server-assigned stack after an accepted request.
    fn apply_slot(&mut self, container: EnumsContainerEnumName, info: &ItemStackResponseSlotInfo) {
        use EnumsContainerEnumName::*;
        let slot = i32::from(info.slot);
        let target = match container {
            Hotbarcontainer | Inventorycontainer | Combinedhotbarandinventorycontainer => {
                self.inventory.entry(slot).or_default()
            }
            Cursorcontainer => &mut self.cursor,
            Craftinginputcontainer => self.grid.entry(slot).or_default(),
            other if in_block(other) => self.window_items.entry(slot).or_default(),
            _ => return,
        };
        if info.amount == 0 {
            *target = Item::default();
        } else {
            target.stacksize = u16::from(info.amount);
            set_stack_id(
                target,
                info.item_stack_net_id.as_ref().map_or(0, |id| id.id),
            );
        }
    }

    /// Records shaped, shapeless and smithing recipes by recipe ID (e.g.
    /// `minecraft:oak_planks`) and multi recipes by UUID.
    fn add_recipes(&mut self, packet: CraftingDataPacket) {
        let output = |results: &[CerealizerNetworkItemInstanceDescriptorSerializedData]| {
            results
                .first()
                .map(|first| (first.stacksize, descriptor_item(first)))
        };
        for recipe in &packet.shaped_recipes {
            if let Some((output_count, output)) = output(&recipe.results) {
                self.recipes.insert(
                    recipe.recipe_id.to_string(),
                    CraftRecipe {
                        network_id: recipe.net_id.raw_id,
                        output_count,
                        output,
                    },
                );
            }
        }
        // User-data shapeless recipes include e.g. dyeing a shulker box.
        for recipe in packet
            .shapeless_recipes
            .iter()
            .chain(&packet.user_data_shapeless_recipes)
        {
            if let Some((output_count, output)) = output(&recipe.results) {
                self.recipes.insert(
                    recipe.recipe_id.to_string(),
                    CraftRecipe {
                        network_id: recipe.net_id.raw_id,
                        output_count,
                        output,
                    },
                );
            }
        }
        for recipe in &packet.multi_recipes {
            self.recipes.insert(
                recipe.multi_recipe_uuid.to_string(),
                CraftRecipe {
                    network_id: recipe.net_id.raw_id,
                    output_count: 1,
                    output: Item::default(),
                },
            );
        }
        for recipe in &packet.smithing_transform_recipes {
            self.recipes.insert(
                recipe.recipe_id.to_string(),
                CraftRecipe {
                    network_id: recipe.net_id.raw_id,
                    output_count: 1,
                    output: descriptor_item(&recipe.result),
                },
            );
        }
        for recipe in &packet.smithing_trim_recipes {
            self.recipes.insert(
                recipe.recipe_id.to_string(),
                CraftRecipe {
                    network_id: recipe.net_id.raw_id,
                    output_count: 1,
                    output: Item::default(),
                },
            );
        }
    }
}

/// A recipe output as an (unplaced, count 0) inventory item of that type.
fn descriptor_item(item: &CerealizerNetworkItemInstanceDescriptorSerializedData) -> Item {
    Item {
        id: item.id as i16,
        stacksize: item.stacksize,
        auxvalue: item.auxvalue,
        net_id_variant: None,
        block_runtime_id: item.block_runtime_id as u32,
        user_data_buffer: item.user_data_buffer.clone(),
    }
}

/// Records the item type a request moves into an empty destination, as the
/// client does, so later steps can address it; counts and stack network IDs
/// come from the server's response.
pub fn predict(dest: &mut Item, source: &Item) {
    if count(dest) == 0 {
        *dest = Item {
            id: source.id,
            stacksize: 0,
            auxvalue: source.auxvalue,
            net_id_variant: None,
            block_runtime_id: source.block_runtime_id,
            user_data_buffer: source.user_data_buffer.clone(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: i16, count: u16, stack: i32) -> Item {
        Item {
            id,
            stacksize: count,
            net_id_variant: Some(stack),
            ..Default::default()
        }
    }

    #[test]
    fn anchor_uses_the_corrected_spawn_and_respawn_is_acknowledged() {
        let mut session = Session::new(
            7,
            &Vec3 {
                x: 0.5,
                y: 32767.0,
                z: -0.5,
            },
        );
        session.handle(McpePacketData::MovePlayerPacket(MovePlayerPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: 7,
            },
            position: Vec3 {
                x: 0.5,
                y: -58.38,
                z: -0.5,
            },
            ..Default::default()
        }));
        assert_eq!(session.expand("/tp @s {x} {y} {z}"), "/tp @s 0 -60 -1");
        session.pos[0] = 20.0;
        assert_eq!(session.expand("{x}"), "0");
        let replies = session.handle(McpePacketData::RespawnPacket(RespawnPacket {
            state: EnumsPlayerRespawnState::Searchingforspawn,
            ..Default::default()
        }));
        assert!(matches!(
            &replies[0].data,
            McpePacketData::RespawnPacket(p)
                if p.state == EnumsPlayerRespawnState::Clientreadytospawn
                    && p.player_runtime_id.actor_runtime_id == 7
        ));
        assert!(matches!(
            &replies[1].data,
            McpePacketData::PlayerActionPacket(p) if p.action == EnumsPlayerActionType::Respawn
        ));
    }

    #[test]
    fn responses_assign_stack_ids_and_window_contents_use_ui_slots() {
        let mut session = Session::new(1, &Vec3::default());
        session.handle(McpePacketData::InventoryContentPacket(
            InventoryContentPacket {
                container_id: 0,
                slots: vec![stack(5, 3, 11)],
                ..Default::default()
            },
        ));
        session.handle(McpePacketData::ContainerOpenPacket(ContainerOpenPacket {
            container_id: 2,
            container_type: 5,
            ..Default::default()
        }));
        session.handle(McpePacketData::InventoryContentPacket(
            InventoryContentPacket {
                container_id: 2,
                slots: vec![stack(9, 1, 20)],
                ..Default::default()
            },
        ));
        // Anvil input index 0 is PlayerUI slot 1.
        assert_eq!(stack_id(&session.window_items[&1]), 20);
        let mut dest = session.inventory.get(&4).cloned().unwrap_or_default();
        predict(&mut dest, &session.inventory[&0]);
        session.inventory.insert(4, dest);
        session.handle(McpePacketData::ItemStackResponsePacket(
            ItemStackResponsePacket {
                responses: vec![ItemStackResponseInfo {
                    result: EnumsItemStackNetResult::Success,
                    containers: Some(vec![ItemStackResponseContainerInfo {
                        full_container_name: FullContainerName {
                            container_name: EnumsContainerEnumName::Hotbarcontainer,
                            dynamic_id: None,
                        },
                        slots: vec![
                            ItemStackResponseSlotInfo {
                                slot: 0,
                                amount: 0,
                                ..Default::default()
                            },
                            ItemStackResponseSlotInfo {
                                slot: 4,
                                amount: 3,
                                item_stack_net_id: Some(
                                    TypedServerNetIdstructItemStackNetIdTagint32T0 { id: 12 },
                                ),
                                ..Default::default()
                            },
                        ],
                    }]),
                    ..Default::default()
                }],
            },
        ));
        assert_eq!(count(&session.inventory[&0]), 0);
        let moved = &session.inventory[&4];
        assert_eq!((moved.id, count(moved), stack_id(moved)), (5, 3, 12));
    }
}
