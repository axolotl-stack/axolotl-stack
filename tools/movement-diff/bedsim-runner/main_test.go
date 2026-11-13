package main

import (
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

func TestExactWritesTheBits(t *testing.T) {
	got := exactOf(0.1, -0)
	if got.Bits[0] != "3dcccccd" || got.Bits[1] != "00000000" {
		t.Fatalf("bits %v", got.Bits)
	}
}

func TestStatesBecomeDragonflyTypes(t *testing.T) {
	got := states(map[string]any{"weirdo_direction": float64(2), "upside_down_bit": false, "half": "top"})
	if got["weirdo_direction"] != int32(2) || got["upside_down_bit"] != false || got["half"] != "top" {
		t.Fatalf("states %v", got)
	}
}

func TestAFloorCollidesInWorldSpace(t *testing.T) {
	world.DefaultBlockRegistry.Finalize()
	w, err := buildWorld(scenario{World: []fill{{
		From: [3]int{0, 99, 0}, To: [3]int{1, 99, 1}, Block: blockSpec{Name: "minecraft:stone"},
	}}})
	if err != nil {
		t.Fatal(err)
	}
	boxes := w.GetNearbyBBoxes(cube.Box32(0.2, 99.9, 0.2, 0.8, 101.7, 0.8))
	if len(boxes) != 1 || boxes[0].Max()[1] != 100 {
		t.Fatalf("boxes %v", boxes)
	}
}
