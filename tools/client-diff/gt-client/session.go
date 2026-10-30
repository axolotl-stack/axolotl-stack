package main

import (
	"fmt"
	"math"
	"os"
	"strconv"
	"strings"
	"sync"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// eyeHeight converts between the eye position used by StartGame, MovePlayer and
// PlayerAuthInput and the block the player stands in.
const eyeHeight = 1.62

// Player inventory slots 0-8 are the hotbar and 9-35 the main inventory; this
// is the slot numbering of InventoryContent for window 0.
const inventorySize = 36

// session tracks just enough server state to issue the interaction steps of a
// scenario: own position and rotation, inventory contents with their stack
// network IDs, block runtime IDs the server sent, the last runtime ID and
// position per entity type and the crafting recipes.
type session struct {
	mu         sync.Mutex
	runtimeID  uint64
	pos        mgl32.Vec3
	pitch, yaw float32
	tick       uint64
	anchor     *protocol.BlockPos
	selected   int
	inventory  map[int]protocol.ItemInstance
	// cursor is the UI cursor stack (PlayerUI container slot 0).
	cursor    protocol.ItemInstance
	blocks    map[protocol.BlockPos]uint32
	entities  map[string]uint64
	positions map[uint64]mgl32.Vec3
	recipes   map[string]craftRecipe
	requestID int32
	// window, windowType and block are the open block container window
	// (ContainerOpen) and its contents (InventoryContent/InventorySlot for
	// that window, then accepted ItemStackResponses).
	window      byte
	windowType  byte
	windowItems map[int]protocol.ItemInstance
	// grid is the inventory crafting grid (PlayerUI slots 28-31) as accepted
	// ItemStackResponses report it.
	grid map[int]protocol.ItemInstance
}

// blockSlots bounds block window slots (a double chest has 54).
const blockSlots = 54

// craftingGridFirst and craftingGridLast bound the PlayerUI slots of the
// inventory's 2x2 crafting grid.
const (
	craftingGridFirst = 28
	craftingGridLast  = 31
)

// containerIDs names the ContainerEnumName of each addressable container.
// Block window slots use the PlayerUI numbering: anvil input 1 and material
// 2, smithing table input 51, material 52 and template 53, grindstone input
// 16 and additional 17.
var containerIDs = map[string]byte{
	"armor":                 protocol.ContainerArmor,
	"offhand":               protocol.ContainerOffhand,
	"chest":                 protocol.ContainerLevelEntity,
	"barrel":                protocol.ContainerBarrel,
	"ingredient":            protocol.ContainerFurnaceIngredient,
	"fuel":                  protocol.ContainerFurnaceFuel,
	"result":                protocol.ContainerFurnaceResult,
	"crafting":              protocol.ContainerCraftingInput,
	"anvil_input":           protocol.ContainerAnvilInput,
	"anvil_material":        protocol.ContainerAnvilMaterial,
	"smithing_input":        protocol.ContainerSmithingTableInput,
	"smithing_material":     protocol.ContainerSmithingTableMaterial,
	"smithing_template":     protocol.ContainerSmithingTableTemplate,
	"grindstone_input":      protocol.ContainerGrindstoneInput,
	"grindstone_additional": protocol.ContainerGrindstoneAdditional,
}

// inBlock reports whether a ContainerEnumName addresses the open block window.
func inBlock(container byte) bool {
	switch container {
	case protocol.ContainerLevelEntity, protocol.ContainerBarrel, protocol.ContainerFurnaceIngredient,
		protocol.ContainerFurnaceFuel, protocol.ContainerFurnaceResult,
		protocol.ContainerAnvilInput, protocol.ContainerAnvilMaterial,
		protocol.ContainerSmithingTableInput, protocol.ContainerSmithingTableMaterial, protocol.ContainerSmithingTableTemplate,
		protocol.ContainerGrindstoneInput, protocol.ContainerGrindstoneAdditional:
		return true
	}
	return false
}

// windowSlotOffset is the PlayerUI slot of index 0 of a block window's
// InventoryContent: BDS lists the anvil, smithing table and grindstone inputs
// from index 0 although requests address them as UI slots 1, 51 and 16.
func windowSlotOffset(windowType byte) int {
	switch int(windowType) {
	case protocol.ContainerTypeAnvil:
		return 1
	case protocol.ContainerTypeSmithingTable:
		return 51
	case protocol.ContainerTypeGrindstone:
		return 16
	}
	return 0
}

func newSession(conn *minecraft.Conn) *session {
	data := conn.GameData()
	return &session{
		runtimeID:   data.EntityRuntimeID,
		pos:         data.PlayerPosition,
		inventory:   make(map[int]protocol.ItemInstance),
		blocks:      make(map[protocol.BlockPos]uint32),
		entities:    make(map[string]uint64),
		positions:   make(map[uint64]mgl32.Vec3),
		recipes:     make(map[string]craftRecipe),
		windowItems: make(map[int]protocol.ItemInstance),
		grid:        make(map[int]protocol.ItemInstance),
	}
}

func feetBlock(eye mgl32.Vec3) protocol.BlockPos {
	return protocol.BlockPos{
		int32(math.Floor(float64(eye[0]))),
		int32(math.Floor(float64(eye[1] - eyeHeight + 0.01))),
		int32(math.Floor(float64(eye[2]))),
	}
}

// anchorBlock fixes the anchor on first use rather than at StartGame: in a
// fresh world StartGame carries a placeholder height (y=32767) that the
// server corrects with MovePlayer once the spawn is resolved. The caller
// holds s.mu.
func (s *session) anchorBlock() protocol.BlockPos {
	if s.anchor == nil {
		anchor := feetBlock(s.pos)
		s.anchor = &anchor
	}
	return *s.anchor
}

// expand substitutes the anchor block coordinates for {x}, {y} and {z}.
func (s *session) expand(text string) string {
	s.mu.Lock()
	anchor := s.anchorBlock()
	s.mu.Unlock()
	return strings.NewReplacer(
		"{x}", strconv.Itoa(int(anchor[0])),
		"{y}", strconv.Itoa(int(anchor[1])),
		"{z}", strconv.Itoa(int(anchor[2])),
	).Replace(text)
}

func (s *session) block(st step) protocol.BlockPos {
	s.mu.Lock()
	defer s.mu.Unlock()
	anchor := s.anchorBlock()
	return protocol.BlockPos{anchor[0] + st.X, anchor[1] + st.Y, anchor[2] + st.Z}
}

// handle updates the tracked state from one server packet and returns the
// packets the client must answer with (respawn and dimension handshakes).
func (s *session) handle(pk packet.Packet) []packet.Packet {
	s.mu.Lock()
	defer s.mu.Unlock()
	switch pk := pk.(type) {
	case *packet.MovePlayer:
		if pk.EntityRuntimeID == s.runtimeID {
			s.pos = pk.Position
		}
	case *packet.CorrectPlayerMovePrediction:
		s.pos = pk.Position
	case *packet.ChangeDimension:
		s.pos = pk.Position
		return []packet.Packet{&packet.PlayerAction{EntityRuntimeID: s.runtimeID, ActionType: protocol.PlayerActionDimensionChangeDone}}
	case *packet.Respawn:
		switch pk.State {
		case packet.RespawnStateSearchingForSpawn:
			return []packet.Packet{
				&packet.Respawn{Position: pk.Position, State: packet.RespawnStateClientReadyToSpawn, EntityRuntimeID: s.runtimeID},
				&packet.PlayerAction{EntityRuntimeID: s.runtimeID, ActionType: protocol.PlayerActionRespawn, BlockFace: -1},
			}
		case packet.RespawnStateReadyToSpawn:
			s.pos = pk.Position
		}
	case *packet.ContainerOpen:
		s.window, s.windowType = pk.WindowID, pk.ContainerType
		s.windowItems = make(map[int]protocol.ItemInstance)
	case *packet.InventoryContent:
		if pk.WindowID == protocol.WindowIDInventory {
			for i, item := range pk.Content {
				if i < inventorySize {
					s.inventory[i] = item
				}
			}
		} else if s.window != 0 && pk.WindowID == uint32(s.window) {
			offset := windowSlotOffset(s.windowType)
			for i, item := range pk.Content {
				s.windowItems[offset+i] = item
			}
		}
	case *packet.InventorySlot:
		if pk.WindowID == protocol.WindowIDInventory && pk.Slot < inventorySize {
			s.inventory[int(pk.Slot)] = pk.NewItem
		} else if s.window != 0 && pk.WindowID == uint32(s.window) {
			s.windowItems[windowSlotOffset(s.windowType)+int(pk.Slot)] = pk.NewItem
		}
	case *packet.InventoryTransaction:
		// Commands such as /enchant replace inventory stacks (with new stack
		// network IDs) only through an s2c transaction.
		for _, action := range pk.Actions {
			window, ok := action.WindowID.Value()
			if action.SourceType == protocol.InventoryActionSourceContainer && ok && window == protocol.WindowIDInventory && action.InventorySlot < inventorySize {
				s.inventory[int(action.InventorySlot)] = action.NewItem
			}
		}
	case *packet.UpdateBlock:
		if pk.Layer == 0 {
			s.blocks[pk.Position] = pk.NewBlockRuntimeID
		}
	case *packet.AddActor:
		s.entities[pk.EntityType] = pk.EntityRuntimeID
		s.positions[pk.EntityRuntimeID] = pk.Position
	case *packet.MoveActorAbsolute:
		s.positions[pk.EntityRuntimeID] = pk.Position
	case *packet.CraftingData:
		s.addRecipes(pk)
	case *packet.ItemStackResponse:
		for _, response := range pk.Responses {
			if response.Status != protocol.ItemStackResponseStatusOK {
				fmt.Fprintf(os.Stderr, "item stack request %d rejected with status %d\n", response.RequestID, response.Status)
				continue
			}
			for _, info := range response.ContainerInfo {
				for _, slot := range info.SlotInfo {
					s.applySlot(info.Container.ContainerID, slot)
				}
			}
		}
	}
	return nil
}

// applySlot records the server-assigned player inventory stack after an
// accepted request.
func (s *session) applySlot(container byte, info protocol.StackResponseSlotInfo) {
	var item protocol.ItemInstance
	switch container {
	case protocol.ContainerHotBar, protocol.ContainerInventory, protocol.ContainerCombinedHotBarAndInventory:
		item = s.inventory[int(info.Slot)]
	case protocol.ContainerCursor:
		item = s.cursor
	case protocol.ContainerCraftingInput:
		item = s.grid[int(info.Slot)]
	default:
		if !inBlock(container) {
			return
		}
		item = s.windowItems[int(info.Slot)]
	}
	if info.Count == 0 {
		item = protocol.ItemInstance{}
	} else {
		item.Stack.Count = uint16(info.Count)
		item.StackNetworkID = info.StackNetworkID
	}
	switch {
	case container == protocol.ContainerCursor:
		s.cursor = item
	case container == protocol.ContainerCraftingInput:
		s.grid[int(info.Slot)] = item
	case inBlock(container):
		s.windowItems[int(info.Slot)] = item
	default:
		s.inventory[int(info.Slot)] = item
	}
}

// predict records the item type a request moves into an empty destination,
// as the client does, so that later steps can address it; counts and stack
// network IDs come from the server's response (applySlot).
func predict(dest *protocol.ItemInstance, source protocol.ItemInstance) {
	if dest.Stack.Count == 0 {
		*dest = protocol.ItemInstance{Stack: protocol.ItemStack{ItemType: source.Stack.ItemType, BlockRuntimeID: source.Stack.BlockRuntimeID, NBTData: source.Stack.NBTData}}
	}
}

// otherSlot addresses an empty armour or offhand slot (stack network ID 0);
// the bot does not track those containers, the responses show the result.
func otherSlot(container string, slot int) protocol.StackRequestSlotInfo {
	return protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: containerIDs[container]}, Slot: byte(slot)}
}

// blockSlot addresses slot of the open block window with the stack ID the
// server last reported there. The caller holds s.mu.
func (s *session) blockSlot(container string, slot int) protocol.StackRequestSlotInfo {
	info := otherSlot(container, slot)
	info.StackNetworkID = s.windowItems[slot].StackNetworkID
	return info
}

func cursorSlot(stack int32) protocol.StackRequestSlotInfo {
	return protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: protocol.ContainerCursor}, Slot: 0, StackNetworkID: stack}
}

// gridSlot addresses a slot of the inventory crafting grid (PlayerUI 28-31).
func gridSlot(slot int, stack int32) protocol.StackRequestSlotInfo {
	return protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: protocol.ContainerCraftingInput}, Slot: byte(slot), StackNetworkID: stack}
}

// containerSlot addresses a crafting grid or block window slot with the stack
// the server last reported there. The caller holds s.mu.
func (s *session) containerSlot(ref slotRef) (protocol.StackRequestSlotInfo, protocol.ItemInstance) {
	if ref.Container == "crafting" {
		item := s.grid[ref.Slot]
		return gridSlot(ref.Slot, item.StackNetworkID), item
	}
	return s.blockSlot(ref.Container, ref.Slot), s.windowItems[ref.Slot]
}

// nbtValue converts a decoded JSON value into NBT-encodable values: integral
// numbers become int32 (TAG_Int, as item tags such as Damage, RepairCost
// and ench entries use), other numbers float32, and lists typed slices.
func nbtValue(v any) any {
	switch v := v.(type) {
	case float64:
		if v == math.Trunc(v) {
			return int32(v)
		}
		return float32(v)
	case map[string]any:
		out := make(map[string]any, len(v))
		for k, e := range v {
			out[k] = nbtValue(e)
		}
		return out
	case []any:
		if len(v) == 0 {
			return []int32{}
		}
		switch v[0].(type) {
		case map[string]any:
			out := make([]map[string]any, len(v))
			for i, e := range v {
				out[i] = nbtValue(e).(map[string]any)
			}
			return out
		case string:
			out := make([]string, len(v))
			for i, e := range v {
				out[i] = e.(string)
			}
			return out
		}
		out := make([]int32, len(v))
		for i, e := range v {
			out[i] = nbtValue(e).(int32)
		}
		return out
	}
	return v
}

func (s *session) nextRequestID() int32 {
	s.mu.Lock()
	defer s.mu.Unlock()
	// Vanilla clients number item stack requests with negative odd IDs.
	s.requestID -= 2
	return s.requestID + 1
}

func inventorySlot(slot int, stack int32) protocol.StackRequestSlotInfo {
	container := byte(protocol.ContainerInventory)
	if slot < 9 {
		container = protocol.ContainerHotBar
	}
	return protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: container}, Slot: byte(slot), StackNetworkID: stack}
}

// authInput builds the per-tick movement packet; vanilla clients send one every
// tick and BDS only simulates (e.g. picks up items for) players that do.
func (s *session) authInput(delta mgl32.Vec3, extra func(*packet.PlayerAuthInput)) *packet.PlayerAuthInput {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.pos = s.pos.Add(delta)
	s.tick++
	pk := &packet.PlayerAuthInput{
		Pitch:            s.pitch,
		Yaw:              s.yaw,
		HeadYaw:          s.yaw,
		Position:         s.pos,
		InputData:        protocol.NewInputFlags(packet.InputFlagCount),
		InputMode:        packet.InputModeMouse,
		PlayMode:         packet.PlayModeNormal,
		InteractionModel: packet.InteractionModelCrosshair,
		InteractPitch:    s.pitch,
		InteractYaw:      s.yaw,
		Tick:             s.tick,
		Delta:            delta,
	}
	if delta[0] != 0 || delta[2] != 0 {
		pk.MoveVector = mgl32.Vec2{0, 1}
		pk.RawMoveVector = pk.MoveVector
		pk.AnalogueMoveVector = pk.MoveVector
		pk.InputData.Set(packet.InputFlagUp)
	}
	pitch, yaw := float64(mgl32.DegToRad(s.pitch)), float64(mgl32.DegToRad(s.yaw))
	pk.CameraOrientation = mgl32.Vec3{
		float32(-math.Sin(yaw) * math.Cos(pitch)),
		float32(-math.Sin(pitch)),
		float32(math.Cos(yaw) * math.Cos(pitch)),
	}
	if extra != nil {
		extra(pk)
	}
	return pk
}

// lookAt turns the player's head towards target before a block or entity
// interaction, as a vanilla client does.
func (s *session) lookAt(target mgl32.Vec3) {
	s.mu.Lock()
	defer s.mu.Unlock()
	d := target.Sub(s.pos)
	horizontal := math.Hypot(float64(d[0]), float64(d[2]))
	s.yaw = float32(-math.Atan2(float64(d[0]), float64(d[2])) * 180 / math.Pi)
	s.pitch = float32(-math.Atan2(float64(d[1]), horizontal) * 180 / math.Pi)
}

// faceOffsets are the clicked positions within a block for faces 0-5
// (down, up, north, south, west, east).
var faceOffsets = [6]mgl32.Vec3{{0.5, 0, 0.5}, {0.5, 1, 0.5}, {0.5, 0.5, 0}, {0.5, 0.5, 1}, {0, 0.5, 0.5}, {1, 0.5, 0.5}}

func faceOffset(face int32) mgl32.Vec3 {
	if face < 0 || int(face) >= len(faceOffsets) {
		return mgl32.Vec3{0.5, 0.5, 0.5}
	}
	return faceOffsets[face]
}

func blockVec(pos protocol.BlockPos) mgl32.Vec3 {
	return mgl32.Vec3{float32(pos[0]), float32(pos[1]), float32(pos[2])}
}

func (s *session) item(slot int) (protocol.ItemInstance, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	item, ok := s.inventory[slot]
	if !ok || item.Stack.Count == 0 {
		return item, fmt.Errorf("inventory slot %d is empty", slot)
	}
	return item, nil
}

// useItemData describes clicking a block face with the selected hotbar item
// (placing it, or opening the block's container). The clicked block's runtime
// ID is the one the server last sent for that position (see handle). The
// caller holds s.mu.
func (s *session) useItemData(pos protocol.BlockPos, face int32) protocol.UseItemTransactionData {
	return protocol.UseItemTransactionData{
		ActionType:       protocol.UseItemActionClickBlock,
		TriggerType:      protocol.TriggerTypePlayerInput,
		BlockPosition:    pos,
		BlockFace:        face,
		HotBarSlot:       int32(s.selected),
		HeldItem:         s.inventory[s.selected],
		Position:         s.pos,
		ClickedPosition:  faceOffset(face),
		BlockRuntimeID:   s.blocks[pos],
		ClientPrediction: protocol.ClientPredictionSuccess,
	}
}

// useAirData describes using the selected hotbar item without a target block
// (eating, drinking, drawing a bow, throwing): no block position, face, click
// position or block runtime ID. The caller holds s.mu.
func (s *session) useAirData() protocol.UseItemTransactionData {
	return protocol.UseItemTransactionData{
		ActionType:       protocol.UseItemActionClickAir,
		TriggerType:      protocol.TriggerTypePlayerInput,
		HotBarSlot:       int32(s.selected),
		HeldItem:         s.inventory[s.selected],
		Position:         s.pos,
		ClientPrediction: protocol.ClientPredictionSuccess,
	}
}

func (s *session) entity(st step) (uint64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	id, ok := s.entities[st.Entity]
	if !ok {
		return 0, fmt.Errorf("no %s has been added", st.Entity)
	}
	return id, nil
}

// craftRecipe is a crafting recipe the craft step can use: one with exactly
// one ingredient, consumed from one inventory slot, a multi recipe (e.g.
// repairing two items in the crafting grid) consumed from grid slots, or a
// smithing transform/trim recipe consumed from the smithing table.
type craftRecipe struct {
	networkID   uint32
	outputCount uint16
}

// addRecipes records the single-ingredient and smithing recipes of a
// CraftingData packet by recipe ID (e.g. minecraft:oak_planks, a 1x1 shaped
// recipe, or minecraft:smithing_netherite_sword) and the multi recipes by
// UUID (e.g. 00000000-0000-0000-0000-000000000001). The caller holds s.mu.
func (s *session) addRecipes(pk *packet.CraftingData) {
	for _, recipe := range pk.ShapedRecipes {
		if recipe.Width*recipe.Height == 1 && len(recipe.Output) == 1 {
			s.recipes[recipe.RecipeID] = craftRecipe{recipe.RecipeNetworkID, recipe.Output[0].Count}
		}
	}
	for _, recipe := range pk.ShapelessRecipes {
		if len(recipe.Input) == 1 && len(recipe.Output) == 1 {
			s.recipes[recipe.RecipeID] = craftRecipe{recipe.RecipeNetworkID, recipe.Output[0].Count}
		}
	}
	for _, recipe := range pk.MultiRecipes {
		s.recipes[recipe.UUID.String()] = craftRecipe{recipe.RecipeNetworkID, 1}
	}
	for _, recipe := range pk.SmithingTransformRecipes {
		s.recipes[recipe.RecipeID] = craftRecipe{recipe.RecipeNetworkID, 1}
	}
	for _, recipe := range pk.SmithingTrimRecipes {
		s.recipes[recipe.RecipeID] = craftRecipe{recipe.RecipeNetworkID, 1}
	}
}

func (s *session) recipe(id string) (craftRecipe, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	recipe, ok := s.recipes[id]
	if !ok {
		return recipe, fmt.Errorf("no single-ingredient or multi recipe %q among %d", id, len(s.recipes))
	}
	return recipe, nil
}

// inputSpec is one PlayerAuthInput tick requested by a step. The tick loop
// builds it when sending so that tick numbers and positions stay monotonic:
// it first turns the head to look (if set), sends the input, then the packets
// after returns in the same batch.
type inputSpec struct {
	delta mgl32.Vec3
	look  *mgl32.Vec3
	extra func(*packet.PlayerAuthInput)
	after func() []packet.Packet
}

// interaction converts an interaction step into the packets a vanilla client
// sends for it; movement, aiming and block actions are PlayerAuthInput ticks.
func (s *session) interaction(st step) ([]packet.Packet, []inputSpec, error) {
	switch st.Kind {
	case "move":
		delta := mgl32.Vec3{st.DX / float32(st.Ticks), st.DY / float32(st.Ticks), st.DZ / float32(st.Ticks)}
		out := make([]inputSpec, st.Ticks)
		for i := range out {
			out[i].delta = delta
		}
		return nil, out, nil
	case "break":
		pos := s.block(st)
		target := blockVec(pos).Add(faceOffset(st.Face))
		return nil, []inputSpec{{look: &target}, {extra: func(pk *packet.PlayerAuthInput) {
			pk.InputData.Set(packet.InputFlagPerformBlockActions)
			pk.BlockActions = protocol.Option([]protocol.PlayerBlockAction{
				{Action: protocol.PlayerActionStartBreak, BlockPos: pos, Face: st.Face},
				{Action: protocol.PlayerActionPredictDestroyBlock, BlockPos: pos, Face: st.Face},
			})
		}}}, nil
	case "click":
		pos := s.block(st)
		target := blockVec(pos).Add(faceOffset(st.Face))
		return nil, []inputSpec{{look: &target}, {after: func() []packet.Packet {
			s.mu.Lock()
			defer s.mu.Unlock()
			data := s.useItemData(pos, st.Face)
			return []packet.Packet{&packet.InventoryTransaction{TransactionData: &data}}
		}}}, nil
	case "attack", "interact":
		target, err := s.entity(st)
		if err != nil {
			return nil, nil, err
		}
		s.mu.Lock()
		aim := s.positions[target].Add(mgl32.Vec3{0, 1, 0})
		s.mu.Unlock()
		action := int32(protocol.UseItemOnEntityActionAttack)
		if st.Kind == "interact" {
			action = protocol.UseItemOnEntityActionInteract
		}
		return nil, []inputSpec{{look: &aim}, {after: func() []packet.Packet {
			s.mu.Lock()
			defer s.mu.Unlock()
			return []packet.Packet{&packet.InventoryTransaction{TransactionData: &protocol.UseItemOnEntityTransactionData{
				TargetEntityRuntimeID: target,
				ActionType:            action,
				HotBarSlot:            int32(s.selected),
				HeldItem:              s.inventory[s.selected],
				Position:              s.pos,
				ClickedPosition:       mgl32.Vec3{0, 1, 0},
			}}}
		}}}, nil
	case "use":
		// The client sends the transaction while processing input and flags
		// the PlayerAuthInput of the tick that follows with StartUsingItem.
		var specs []inputSpec
		if st.X != 0 || st.Y != 0 || st.Z != 0 {
			target := blockVec(s.block(st)).Add(mgl32.Vec3{0.5, 0.5, 0.5})
			specs = append(specs, inputSpec{look: &target})
		}
		return nil, append(specs, inputSpec{after: func() []packet.Packet {
			s.mu.Lock()
			defer s.mu.Unlock()
			data := s.useAirData()
			return []packet.Packet{&packet.InventoryTransaction{TransactionData: &data}}
		}}, inputSpec{extra: func(pk *packet.PlayerAuthInput) {
			pk.InputData.Set(packet.InputFlagStartUsingItem)
		}}), nil
	case "release", "consume":
		action := int32(protocol.ReleaseItemActionRelease)
		if st.Kind == "consume" {
			action = protocol.ReleaseItemActionConsume
		}
		return nil, []inputSpec{{after: func() []packet.Packet {
			s.mu.Lock()
			defer s.mu.Unlock()
			return []packet.Packet{&packet.InventoryTransaction{TransactionData: &protocol.ReleaseItemTransactionData{
				ActionType:   action,
				HotBarSlot:   int32(s.selected),
				HeldItem:     s.inventory[s.selected],
				HeadPosition: s.pos,
			}}}
		}}}, nil
	}
	packets, err := s.packets(st)
	return packets, nil, err
}

func (s *session) packets(st step) ([]packet.Packet, error) {
	switch st.Kind {
	case "select":
		s.mu.Lock()
		s.selected = st.Slot
		item := s.inventory[st.Slot]
		runtimeID := s.runtimeID
		s.mu.Unlock()
		return []packet.Packet{&packet.MobEquipment{EntityRuntimeID: runtimeID, NewItem: item, InventorySlot: byte(st.Slot), HotBarSlot: byte(st.Slot), WindowID: protocol.WindowIDInventory}}, nil
	case "move_item", "take_cursor", "place_cursor", "swap", "drop", "drop_cursor", "destroy":
		return s.stackRequest(st)
	case "open_inventory":
		// The vanilla client opens its own inventory with Interact
		// OpenInventory targeting itself.
		s.mu.Lock()
		runtimeID := s.runtimeID
		s.mu.Unlock()
		return []packet.Packet{&packet.Interact{ActionType: packet.InteractActionOpenInventory, TargetEntityRuntimeID: runtimeID}}, nil
	case "close_inventory":
		// Window 0 with type 0xff (NONE) as the vanilla client sends it.
		return []packet.Packet{&packet.ContainerClose{WindowID: 0, ContainerType: 0xff}}, nil
	case "close_container":
		s.mu.Lock()
		defer s.mu.Unlock()
		if s.window == 0 {
			// The server refused to open it (e.g. an obstructed chest): the
			// capture shows the refusal, there is nothing to close.
			fmt.Fprintln(os.Stderr, "close_container: no block window is open")
			return nil, nil
		}
		window, kind := s.window, s.windowType
		s.window = 0
		return []packet.Packet{&packet.ContainerClose{WindowID: window, ContainerType: kind}}, nil
	case "craft":
		recipe := craftRecipe{st.RecipeNetworkID, 1}
		if st.Recipe != "" {
			var err error
			if recipe, err = s.recipe(st.Recipe); err != nil {
				return nil, err
			}
		}
		var consumes []protocol.StackRequestAction
		var firstStack int32
		if len(st.Consume) > 0 {
			s.mu.Lock()
			for i, ref := range st.Consume {
				info, item := s.containerSlot(ref)
				if item.Stack.Count == 0 {
					s.mu.Unlock()
					return nil, fmt.Errorf("%s slot %d is empty", ref.Container, ref.Slot)
				}
				if i == 0 {
					firstStack = item.StackNetworkID
				}
				consume := &protocol.ConsumeStackRequestAction{}
				consume.Count = byte(max(ref.Count, 1))
				consume.Source = info
				consumes = append(consumes, consume)
			}
			s.mu.Unlock()
		} else {
			item, err := s.item(st.Slot)
			if err != nil {
				return nil, err
			}
			firstStack = item.StackNetworkID
			consume := &protocol.ConsumeStackRequestAction{}
			consume.Count = 1
			consume.Source = inventorySlot(st.Slot, item.StackNetworkID)
			consumes = append(consumes, consume)
		}
		requestID := s.nextRequestID()
		request := protocol.ItemStackRequest{RequestID: requestID}
		switch st.Action {
		case "optional":
			// The anvil sends the item name as the request's filter string.
			request.FilterStrings = []string{st.Text}
			request.FilterCause = protocol.FilterCauseAnvilText
			request.Actions = append(request.Actions, &protocol.CraftRecipeOptionalStackRequestAction{RecipeNetworkID: recipe.networkID, FilterStringIndex: 0})
		case "grindstone":
			// The grindstone's "recipe" ID is an item stack network ID: BDS
			// 1.26.51 accepts only the stack network ID of the first input.
			id := recipe.networkID
			if id == 0 {
				id = uint32(firstStack)
			}
			request.Actions = append(request.Actions, &protocol.CraftGrindstoneRecipeStackRequestAction{RecipeNetworkID: id, NumberOfCrafts: 1, Cost: st.Cost})
		default:
			request.Actions = append(request.Actions, &protocol.CraftRecipeStackRequestAction{RecipeNetworkID: recipe.networkID, NumberOfCrafts: 1})
		}
		if st.Result != "" {
			result := protocol.StackRequestItem{Identifier: st.Result, Count: recipe.outputCount}
			if st.ResultNBT != nil {
				result.NBTData = nbtValue(st.ResultNBT).(map[string]any)
			}
			request.Actions = append(request.Actions, &protocol.CraftResultsDeprecatedStackRequestAction{ResultItems: []protocol.StackRequestItem{result}, TimesCrafted: 1})
		}
		request.Actions = append(request.Actions, consumes...)
		take := &protocol.TakeStackRequestAction{}
		take.Count = byte(recipe.outputCount)
		// The crafted stack is addressed by the request ID until placed; it
		// may merge into a matching stack already in the destination.
		take.Source = protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: protocol.ContainerCreatedOutput}, Slot: 50, StackNetworkID: requestID}
		s.mu.Lock()
		take.Destination = inventorySlot(st.To, s.inventory[st.To].StackNetworkID)
		s.mu.Unlock()
		request.Actions = append(request.Actions, take)
		return []packet.Packet{&packet.ItemStackRequest{Requests: []protocol.ItemStackRequest{request}}}, nil
	}
	return nil, fmt.Errorf("unsupported interaction %q", st.Kind)
}

// staleStackID is a stack network ID the server never assigned; steps with
// "stale": true send it as the source ID to capture the rejection.
const staleStackID = 999999

// stackRequest builds the ItemStackRequest of an inventory step and predicts
// the destination item type (see predict). Slots 0-35 are the player
// inventory; the cursor is the PlayerUI container's slot 0.
func (s *session) stackRequest(st step) ([]packet.Packet, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	source := s.inventory[st.Slot]
	if st.Kind == "place_cursor" || st.Kind == "drop_cursor" {
		source = s.cursor
	}
	if st.FromContainer != "" {
		source = s.windowItems[st.Slot]
	}
	if source.Stack.Count == 0 {
		return nil, fmt.Errorf("%s: source slot is empty", st.Kind)
	}
	sourceID := source.StackNetworkID
	if st.Stale {
		sourceID = staleStackID
	}
	var action protocol.StackRequestAction
	switch st.Kind {
	case "move_item":
		take := &protocol.TakeStackRequestAction{}
		take.Count = byte(st.Count)
		take.Source = inventorySlot(st.Slot, sourceID)
		if st.FromContainer != "" {
			take.Source = s.blockSlot(st.FromContainer, st.Slot)
			take.Source.StackNetworkID = sourceID
		}
		if st.ToContainer == "armor" || st.ToContainer == "offhand" {
			take.Destination = otherSlot(st.ToContainer, st.To)
		} else if st.ToContainer == "crafting" {
			dest := s.grid[st.To]
			take.Destination = gridSlot(st.To, dest.StackNetworkID)
			predict(&dest, source)
			s.grid[st.To] = dest
		} else if st.ToContainer != "" {
			take.Destination = s.blockSlot(st.ToContainer, st.To)
			dest := s.windowItems[st.To]
			predict(&dest, source)
			s.windowItems[st.To] = dest
		} else {
			dest := s.inventory[st.To]
			take.Destination = inventorySlot(st.To, dest.StackNetworkID)
			predict(&dest, source)
			s.inventory[st.To] = dest
		}
		action = take
	case "take_cursor":
		take := &protocol.TakeStackRequestAction{}
		take.Count = byte(st.Count)
		take.Source = inventorySlot(st.Slot, sourceID)
		take.Destination = cursorSlot(s.cursor.StackNetworkID)
		predict(&s.cursor, source)
		action = take
	case "place_cursor":
		place := &protocol.PlaceStackRequestAction{}
		place.Count = byte(st.Count)
		place.Source = cursorSlot(sourceID)
		if st.ToContainer != "" {
			place.Destination = otherSlot(st.ToContainer, st.To)
		} else {
			dest := s.inventory[st.To]
			place.Destination = inventorySlot(st.To, dest.StackNetworkID)
			predict(&dest, source)
			s.inventory[st.To] = dest
		}
		action = place
	case "swap":
		dest := s.inventory[st.To]
		action = &protocol.SwapStackRequestAction{Source: inventorySlot(st.Slot, sourceID), Destination: inventorySlot(st.To, dest.StackNetworkID)}
		s.inventory[st.Slot], s.inventory[st.To] = dest, source
	case "drop":
		action = &protocol.DropStackRequestAction{Count: byte(st.Count), Source: inventorySlot(st.Slot, sourceID)}
	case "drop_cursor":
		action = &protocol.DropStackRequestAction{Count: byte(st.Count), Source: cursorSlot(sourceID)}
	case "destroy":
		action = &protocol.DestroyStackRequestAction{Count: byte(st.Count), Source: inventorySlot(st.Slot, sourceID)}
	}
	s.requestID -= 2
	return []packet.Packet{&packet.ItemStackRequest{Requests: []protocol.ItemStackRequest{{RequestID: s.requestID + 1, Actions: []protocol.StackRequestAction{action}}}}}, nil
}
