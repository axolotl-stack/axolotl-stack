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
	blocks     map[protocol.BlockPos]uint32
	entities   map[string]uint64
	positions  map[uint64]mgl32.Vec3
	recipes    map[string]singleRecipe
	requestID  int32
}

func newSession(conn *minecraft.Conn) *session {
	data := conn.GameData()
	return &session{
		runtimeID: data.EntityRuntimeID,
		pos:       data.PlayerPosition,
		inventory: make(map[int]protocol.ItemInstance),
		blocks:    make(map[protocol.BlockPos]uint32),
		entities:  make(map[string]uint64),
		positions: make(map[uint64]mgl32.Vec3),
		recipes:   make(map[string]singleRecipe),
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
	case *packet.InventoryContent:
		if pk.WindowID == protocol.WindowIDInventory {
			for i, item := range pk.Content {
				if i < inventorySize {
					s.inventory[i] = item
				}
			}
		}
	case *packet.InventorySlot:
		if pk.WindowID == protocol.WindowIDInventory && pk.Slot < inventorySize {
			s.inventory[int(pk.Slot)] = pk.NewItem
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
	switch container {
	case protocol.ContainerHotBar, protocol.ContainerInventory, protocol.ContainerCombinedHotBarAndInventory:
	default:
		return
	}
	slot := int(info.Slot)
	item := s.inventory[slot]
	if info.Count == 0 {
		item = protocol.ItemInstance{}
	} else {
		item.Stack.Count = uint16(info.Count)
		item.StackNetworkID = info.StackNetworkID
	}
	s.inventory[slot] = item
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

func (s *session) entity(st step) (uint64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	id, ok := s.entities[st.Entity]
	if !ok {
		return 0, fmt.Errorf("no %s has been added", st.Entity)
	}
	return id, nil
}

// singleRecipe is a crafting recipe with exactly one ingredient, which the
// craft step can consume from one inventory slot.
type singleRecipe struct {
	networkID   uint32
	outputCount uint16
}

// addRecipes records the single-ingredient recipes of a CraftingData packet by
// recipe ID (e.g. minecraft:oak_planks, a 1x1 shaped recipe). The caller holds
// s.mu.
func (s *session) addRecipes(pk *packet.CraftingData) {
	for _, recipe := range pk.ShapedRecipes {
		if recipe.Width*recipe.Height == 1 && len(recipe.Output) == 1 {
			s.recipes[recipe.RecipeID] = singleRecipe{recipe.RecipeNetworkID, recipe.Output[0].Count}
		}
	}
	for _, recipe := range pk.ShapelessRecipes {
		if len(recipe.Input) == 1 && len(recipe.Output) == 1 {
			s.recipes[recipe.RecipeID] = singleRecipe{recipe.RecipeNetworkID, recipe.Output[0].Count}
		}
	}
}

func (s *session) recipe(id string) (singleRecipe, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	recipe, ok := s.recipes[id]
	if !ok {
		return recipe, fmt.Errorf("no single-ingredient recipe %q among %d", id, len(s.recipes))
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
	case "move_item":
		item, err := s.item(st.Slot)
		if err != nil {
			return nil, err
		}
		take := &protocol.TakeStackRequestAction{}
		take.Count = byte(st.Count)
		take.Source = inventorySlot(st.Slot, item.StackNetworkID)
		take.Destination = inventorySlot(st.To, 0)
		return []packet.Packet{&packet.ItemStackRequest{Requests: []protocol.ItemStackRequest{{RequestID: s.nextRequestID(), Actions: []protocol.StackRequestAction{take}}}}}, nil
	case "drop":
		item, err := s.item(st.Slot)
		if err != nil {
			return nil, err
		}
		return []packet.Packet{&packet.ItemStackRequest{Requests: []protocol.ItemStackRequest{{RequestID: s.nextRequestID(), Actions: []protocol.StackRequestAction{
			&protocol.DropStackRequestAction{Count: byte(st.Count), Source: inventorySlot(st.Slot, item.StackNetworkID)},
		}}}}}, nil
	case "craft":
		recipe, err := s.recipe(st.Recipe)
		if err != nil {
			return nil, err
		}
		item, err := s.item(st.Slot)
		if err != nil {
			return nil, err
		}
		requestID := s.nextRequestID()
		consume := &protocol.ConsumeStackRequestAction{}
		consume.Count = 1
		consume.Source = inventorySlot(st.Slot, item.StackNetworkID)
		take := &protocol.TakeStackRequestAction{}
		take.Count = byte(recipe.outputCount)
		// The crafted stack is addressed by the request ID until placed.
		take.Source = protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: protocol.ContainerCreatedOutput}, Slot: 50, StackNetworkID: requestID}
		take.Destination = inventorySlot(st.To, 0)
		return []packet.Packet{&packet.ItemStackRequest{Requests: []protocol.ItemStackRequest{{RequestID: requestID, Actions: []protocol.StackRequestAction{
			&protocol.CraftRecipeStackRequestAction{RecipeNetworkID: recipe.networkID, NumberOfCrafts: 1},
			consume,
			take,
		}}}}}, nil
	}
	return nil, fmt.Errorf("unsupported interaction %q", st.Kind)
}
