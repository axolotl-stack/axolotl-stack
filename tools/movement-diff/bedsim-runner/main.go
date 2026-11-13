// Command bedsim-runner plays the movement corpus through oomph-ac/bedsim
// (MIT, pinned in go.mod) and writes its traces in the harness's format.
//
//	go run . -corpus resolved.json -out bedsim.json
//
// The corpus must have every block state spelled out
// (`movement-diff resolve`). Blocks come from Dragonfly's registry, their
// collision boxes from Dragonfly's block models: this is bedsim as an
// integrator would run it. Input flags are derived each tick from the keys
// and bedsim's own state, as a vanilla client derives them from its own
// prediction.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"math"
	"os"
	"runtime/debug"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/oomph-ac/bedsim"
	bedsimblock "github.com/oomph-ac/bedsim/block"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// networkOffset is the wire position's height above the feet.
const networkOffset = float32(1.62001)

type corpus struct {
	Scenarios []scenario `json:"scenarios"`
}

type scenario struct {
	Name   string    `json:"name"`
	World  []fill    `json:"world"`
	Start  start     `json:"start"`
	Inputs []segment `json:"inputs"`
	Events []event   `json:"events"`
}

type fill struct {
	From  [3]int    `json:"from"`
	To    [3]int    `json:"to"`
	Block blockSpec `json:"block"`
}

type blockSpec struct {
	Name   string         `json:"name"`
	States map[string]any `json:"states"`
}

type start struct {
	Feet    [3]float32   `json:"feet"`
	Yaw     float32      `json:"yaw"`
	Pitch   float32      `json:"pitch"`
	Effects []effectSpec `json:"effects"`
	Elytra  bool         `json:"elytra"`
}

type effectSpec struct {
	Effect    string `json:"effect"`
	Amplifier int32  `json:"amplifier"`
}

type segment struct {
	Ticks   int      `json:"ticks"`
	Forward float32  `json:"forward"`
	Strafe  float32  `json:"strafe"`
	Jump    bool     `json:"jump"`
	Sneak   bool     `json:"sneak"`
	Sprint  bool     `json:"sprint"`
	Glide   bool     `json:"glide"`
	Yaw     *float32 `json:"yaw"`
	Pitch   *float32 `json:"pitch"`
	Turn    float32  `json:"turn"`
	Tilt    float32  `json:"tilt"`
}

type event struct {
	After    int         `json:"after"`
	Velocity *[3]float32 `json:"velocity"`
}

// exact is the harness's f32 encoding: the value and its bits.
type exact struct {
	Value []float32 `json:"value"`
	Bits  []string  `json:"bits"`
}

func exactOf(values ...float32) exact {
	out := exact{Value: values}
	for _, v := range values {
		out.Bits = append(out.Bits, fmt.Sprintf("%08x", math.Float32bits(v)))
	}
	return out
}

type tickState struct {
	Tick                int   `json:"tick"`
	Position            exact `json:"position"`
	Velocity            exact `json:"velocity"`
	OnGround            bool  `json:"on_ground"`
	HorizontalCollision bool  `json:"horizontal_collision"`
	VerticalCollision   bool  `json:"vertical_collision"`
	FallDistance        exact `json:"fall_distance"`
	// Outcome is bedsim's simulation outcome when it is not normal.
	Outcome string `json:"outcome,omitempty"`
}

type scenarioTrace struct {
	Ticks []tickState `json:"ticks"`
}

type traces struct {
	Implementation string                   `json:"implementation"`
	Source         string                   `json:"source"`
	Scenarios      map[string]scenarioTrace `json:"scenarios"`
}

// blockWorld is a scenario's blocks; everything else is air.
type blockWorld map[cube.Pos]world.Block

func (w blockWorld) Block(pos cube.Pos) world.Block {
	if b, ok := w[pos]; ok {
		return b
	}
	return block.Air{}
}

func (w blockWorld) BlockCollisions(pos cube.Pos) []cube.BBox32 {
	var out []cube.BBox32
	for _, box := range w.Block(pos).Model().BBox(pos, w) {
		min, max := box.Min(), box.Max()
		out = append(out, cube.Box32(float32(min[0]), float32(min[1]), float32(min[2]),
			float32(max[0]), float32(max[1]), float32(max[2])))
	}
	return out
}

func (w blockWorld) GetNearbyBBoxes(aabb cube.BBox32) []cube.BBox32 {
	var out []cube.BBox32
	min, max := aabb.Min(), aabb.Max()
	for x := floor(min[0]); x <= floor(max[0]); x++ {
		// One block lower: fences and walls reach 1.5 blocks up.
		for y := floor(min[1]) - 1; y <= floor(max[1]); y++ {
			for z := floor(min[2]); z <= floor(max[2]); z++ {
				pos := cube.Pos{x, y, z}
				for _, local := range w.BlockCollisions(pos) {
					box := local.Translate(mgl32.Vec3{float32(x), float32(y), float32(z)})
					if box.IntersectsWith(aabb) {
						out = append(out, box)
					}
				}
			}
		}
	}
	return out
}

func (w blockWorld) IsChunkLoaded(int32, int32) bool { return true }

func (w blockWorld) Liquid(pos cube.Pos) (world.Liquid, bool) {
	liquid, ok := w.Block(pos).(world.Liquid)
	return liquid, ok
}

func floor(v float32) int { return int(math.Floor(float64(v))) }

// worldCell is what one placed block is to the runner: its collision boxes
// in world coordinates and bedsim's ground friction.
type worldCell struct {
	Block    string       `json:"block"`
	Boxes    [][6]float32 `json:"boxes"`
	Friction float32      `json:"friction"`
}

// dumpWorld lists the scenario's placed blocks as the runner sees them, but
// plain full cubes with the ordinary 0.6 friction (harness validation:
// compared with unastar's world by compare.py --worlds).
func dumpWorld(w blockWorld) map[string]worldCell {
	out := map[string]worldCell{}
	for pos, b := range w {
		name, _ := b.EncodeBlock()
		cell := worldCell{Block: name, Boxes: [][6]float32{}, Friction: bedsimblock.Friction(b, name)}
		for _, box := range w.BlockCollisions(pos) {
			min, max := box.Min(), box.Max()
			cell.Boxes = append(cell.Boxes, [6]float32{
				min[0] + float32(pos[0]), min[1] + float32(pos[1]), min[2] + float32(pos[2]),
				max[0] + float32(pos[0]), max[1] + float32(pos[1]), max[2] + float32(pos[2])})
		}
		full := [6]float32{float32(pos[0]), float32(pos[1]), float32(pos[2]),
			float32(pos[0] + 1), float32(pos[1] + 1), float32(pos[2] + 1)}
		_, liquid := b.(world.Liquid)
		if !liquid && cell.Friction == 0.6 && len(cell.Boxes) == 1 && cell.Boxes[0] == full {
			continue
		}
		out[fmt.Sprintf("%d,%d,%d", pos[0], pos[1], pos[2])] = cell
	}
	return out
}

type effects map[int32]int32

func (e effects) GetEffect(id int32) (int32, bool) {
	amplifier, ok := e[id]
	return amplifier, ok
}

type inventory struct{ elytra bool }

func (i inventory) HasElytra() bool { return i.elytra }

var effectIDs = map[string]int32{
	"speed":        packet.EffectSpeed,
	"slowness":     packet.EffectSlowness,
	"jump_boost":   packet.EffectJumpBoost,
	"slow_falling": packet.EffectSlowFalling,
	"levitation":   packet.EffectLevitation,
}

// newerStates are BDS 1.26.51 block states Dragonfly does not have; a block
// is looked up without them (Dragonfly computes stair corners and fence
// connections from the neighbours).
var newerStates = []string{
	"minecraft:corner",
	"minecraft:connection_east", "minecraft:connection_north",
	"minecraft:connection_south", "minecraft:connection_west",
}

// states converts the corpus's JSON states to Dragonfly's property types.
func states(in map[string]any) map[string]any {
	out := make(map[string]any, len(in))
	for k, v := range in {
		switch v := v.(type) {
		case float64:
			out[k] = int32(v)
		default:
			out[k] = v
		}
	}
	return out
}

func buildWorld(s scenario) (blockWorld, error) {
	w := blockWorld{}
	for _, f := range s.World {
		props := states(f.Block.States)
		b, ok := world.BlockByName(f.Block.Name, props)
		if !ok {
			// States newer than Dragonfly's block palette.
			for _, newer := range newerStates {
				delete(props, newer)
			}
			b, ok = world.BlockByName(f.Block.Name, props)
		}
		if !ok {
			return nil, fmt.Errorf("dragonfly has no block %s %v", f.Block.Name, f.Block.States)
		}
		lo := [3]int{min(f.From[0], f.To[0]), min(f.From[1], f.To[1]), min(f.From[2], f.To[2])}
		hi := [3]int{max(f.From[0], f.To[0]), max(f.From[1], f.To[1]), max(f.From[2], f.To[2])}
		for x := lo[0]; x <= hi[0]; x++ {
			for y := lo[1]; y <= hi[1]; y++ {
				for z := lo[2]; z <= hi[2]; z++ {
					w[cube.Pos{x, y, z}] = b
				}
			}
		}
	}
	return w, nil
}

// movementSpeed is the movement attribute with the speed and slowness
// effects, before sprinting.
func movementSpeed(s scenario) float32 {
	speed := float32(0.1)
	for _, e := range s.Start.Effects {
		switch e.Effect {
		case "speed":
			speed *= 1 + 0.2*float32(e.Amplifier+1)
		case "slowness":
			speed *= 1 - 0.15*float32(e.Amplifier+1)
		}
	}
	return speed
}

func run(s scenario) (scenarioTrace, error) {
	w, err := buildWorld(s)
	if err != nil {
		return scenarioTrace{}, err
	}
	fx := effects{}
	for _, e := range s.Start.Effects {
		// Effects without a movement effect (fire resistance) are skipped.
		if id, ok := effectIDs[e.Effect]; ok {
			fx[id] = e.Amplifier
		}
	}
	sim := bedsim.Simulator{
		World:     w,
		Liquids:   w,
		Effects:   fx,
		Inventory: inventory{elytra: s.Start.Elytra},
		Options: bedsim.SimulationOptions{
			Mode:                       bedsim.SimulationModePassive,
			IgnoreClientStepTiebreaker: true,
		},
	}
	speed := movementSpeed(s)
	state := bedsim.MovementState{
		Pos:                  mgl32.Vec3{s.Start.Feet[0], s.Start.Feet[1], s.Start.Feet[2]},
		Size:                 bedsim.DefaultPlayerSize(),
		MovementSpeed:        speed,
		DefaultMovementSpeed: speed,
		AirSpeed:             bedsim.WalkAirSpeed,
		Gravity:              bedsim.NormalGravity,
		HasGravity:           true,
		// BDS's teleport leaves the player on the ground until the first move.
		OnGround:            true,
		Ready:               true,
		Alive:               true,
		HasFirstInput:       true,
		GameMode:            packet.GameTypeSurvival,
		TicksSinceKnockback: 1,
		TicksSinceTeleport:  1,
		Rotation:            mgl32.Vec3{s.Start.Pitch, s.Start.Yaw, s.Start.Yaw},
	}
	state.Client.Pos = state.Pos
	velocities := map[int]mgl32.Vec3{}
	for _, e := range s.Events {
		if e.Velocity != nil {
			velocities[e.After] = mgl32.Vec3{e.Velocity[0], e.Velocity[1], e.Velocity[2]}
		}
	}
	yaw, pitch := s.Start.Yaw, s.Start.Pitch
	var trace scenarioTrace
	tick := 0
	var sneaking bool
	for _, seg := range s.Inputs {
		if seg.Yaw != nil {
			yaw = *seg.Yaw
		}
		if seg.Pitch != nil {
			pitch = *seg.Pitch
		}
		for i := 0; i < seg.Ticks; i++ {
			tick++
			yaw += seg.Turn
			pitch += seg.Tilt
			if v, ok := velocities[tick-1]; ok {
				state.QueueKnockback(v)
			}
			glide := seg.Glide && i == 0
			jump := seg.Jump || glide
			canSprint := seg.Forward > 0 && !seg.Sneak
			wantSprint := canSprint && (seg.Sprint || state.Sprinting)
			input := bedsim.InputState{
				MoveVector:      mgl32.Vec2{seg.Strafe, seg.Forward},
				MoveVectorIsRaw: true,
				Pitch:           pitch,
				Yaw:             yaw,
				HeadYaw:         yaw,
				ClientPos:       state.Pos,
				ClientVel:       state.Vel,
				SprintDown:      seg.Sprint,
				StartSprinting:  wantSprint && !state.Sprinting,
				StopSprinting:   !wantSprint && state.Sprinting,
				SneakDown:       seg.Sneak,
				Sneaking:        seg.Sneak,
				StartSneaking:   seg.Sneak && !sneaking,
				StopSneaking:    !seg.Sneak && sneaking,
				Jumping:         jump,
				StartJumping:    jump && state.OnGround && state.JumpDelay == 0,
				StartGliding:    glide && !state.OnGround && !state.Gliding,
			}
			sneaking = seg.Sneak
			result := sim.Simulate(&state, input)
			ts := tickState{
				Tick:                tick,
				Position:            exactOf(state.Pos[0], state.Pos[1]+networkOffset, state.Pos[2]),
				Velocity:            exactOf(state.Vel[0], state.Vel[1], state.Vel[2]),
				OnGround:            state.OnGround,
				HorizontalCollision: state.CollideX || state.CollideZ,
				VerticalCollision:   state.CollideY,
				FallDistance:        exactOf(state.FallDistance),
			}
			if result.Outcome != bedsim.SimulationOutcomeNormal {
				ts.Outcome = fmt.Sprint(result.Outcome)
			}
			trace.Ticks = append(trace.Ticks, ts)
		}
	}
	return trace, nil
}

func main() {
	corpusPath := flag.String("corpus", "", "resolved corpus (movement-diff resolve)")
	outPath := flag.String("out", "", "traces to write")
	worldPath := flag.String("world-out", "", "also write each scenario's blocks as the runner sees them")
	flag.Parse()
	if *corpusPath == "" || *outPath == "" {
		flag.Usage()
		os.Exit(2)
	}
	world.DefaultBlockRegistry.Finalize()
	data, err := os.ReadFile(*corpusPath)
	if err != nil {
		fail(err)
	}
	var c corpus
	if err := json.Unmarshal(data, &c); err != nil {
		fail(err)
	}
	out := traces{
		Implementation: "bedsim",
		Source:         "oomph-ac/bedsim " + bedsimVersion(),
		Scenarios:      map[string]scenarioTrace{},
	}
	worlds := map[string]map[string]worldCell{}
	for _, s := range c.Scenarios {
		if *worldPath != "" {
			w, err := buildWorld(s)
			if err != nil {
				fail(fmt.Errorf("scenario %s: %w", s.Name, err))
			}
			worlds[s.Name] = dumpWorld(w)
		}
		trace, err := run(s)
		if err != nil {
			fail(fmt.Errorf("scenario %s: %w", s.Name, err))
		}
		out.Scenarios[s.Name] = trace
	}
	if *worldPath != "" {
		encoded, err := json.MarshalIndent(map[string]any{"runner": "bedsim", "scenarios": worlds}, "", " ")
		if err != nil {
			fail(err)
		}
		if err := os.WriteFile(*worldPath, append(encoded, '\n'), 0o644); err != nil {
			fail(err)
		}
	}
	encoded, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		fail(err)
	}
	if err := os.WriteFile(*outPath, append(encoded, '\n'), 0o644); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, "bedsim-runner:", err)
	os.Exit(1)
}

// bedsimVersion is the pinned bedsim module version.
func bedsimVersion() string {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return "(unknown version)"
	}
	for _, dep := range info.Deps {
		if dep.Path == "github.com/oomph-ac/bedsim" {
			return dep.Version
		}
	}
	return "(unknown version)"
}
