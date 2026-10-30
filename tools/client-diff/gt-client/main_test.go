package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestCaptureIncludesHeaderAndBothDirections(t *testing.T) {
	var out bytes.Buffer
	ctx, cancel := context.WithCancelCause(context.Background())
	defer cancel(nil)
	remote := &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 19132}
	local := &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 40000}
	capture := newCapture(&out, remote.String(), cancel)
	header := packet.Header{PacketID: 193, SenderSubClient: 1, TargetSubClient: 2}
	capture.packet(header, []byte{0xab, 0xcd}, local, remote)
	capture.packet(header, []byte{0xef}, remote, local)
	decoder := json.NewDecoder(&out)
	for i, direction := range []string{"c2s", "s2c"} {
		var record captureRecord
		if err := decoder.Decode(&record); err != nil {
			t.Fatal(err)
		}
		wantHex := "c149abcd"
		if i == 1 {
			wantHex = "c149ef"
		}
		if record.Seq != uint64(i) || record.Dir != direction || record.ID != 193 || record.Hex != wantHex || record.TimeMS < 0 {
			t.Fatalf("unexpected record: %+v", record)
		}
	}
	if ctx.Err() != nil {
		t.Fatal(context.Cause(ctx))
	}
}

type brokenWriter struct{}

func (brokenWriter) Write([]byte) (int, error) { return 0, errors.New("capture disk failure") }

func TestCaptureFailureCancelsSession(t *testing.T) {
	ctx, cancel := context.WithCancelCause(context.Background())
	defer cancel(nil)
	capture := newCapture(brokenWriter{}, "server", cancel)
	capture.packet(packet.Header{PacketID: packet.IDText}, nil, &net.UDPAddr{}, &net.UDPAddr{})
	if context.Cause(ctx) == nil || capture.err == nil {
		t.Fatal("capture failure was not propagated")
	}
}

func TestScenarioValidation(t *testing.T) {
	for _, scenario := range []scenario{
		{IdleMS: -1},
		{Steps: []step{{AfterMS: -1, Kind: "chat", Text: "hello"}}},
		{Steps: []step{{Kind: "unknown", Text: "hello"}}},
		{Steps: []step{{Kind: "chat"}}},
		// Interactions only make sense while the client ticks like vanilla.
		{Steps: []step{{Kind: "click"}}},
		{Steps: []step{{Kind: "use"}}},
		{AuthInput: true, Steps: []step{{Kind: "move"}}},
		{AuthInput: true, Steps: []step{{Kind: "drop", Slot: 36, Count: 1}}},
		{AuthInput: true, Steps: []step{{Kind: "select", Slot: 9}}},
		{AuthInput: true, Steps: []step{{Kind: "craft", Recipe: "r", Count: 1, Consume: []slotRef{{Container: "crafting", Slot: 27}}}}},
		{AuthInput: true, Steps: []step{{Kind: "craft", Recipe: "r", Count: 1, Consume: []slotRef{{Container: "offhand", Slot: 1}}}}},
		{AuthInput: true, Steps: []step{{Kind: "craft", Action: "bogus", Count: 1}}},
		{AuthInput: true, Steps: []step{{Kind: "move_item", Count: 1, To: 32, ToContainer: "crafting"}}},
	} {
		if err := scenario.validate(); err == nil {
			t.Fatalf("accepted invalid scenario: %+v", scenario)
		}
	}
	if err := (scenario{IdleMS: 10, Steps: []step{{AfterMS: 1, Kind: "chat", Text: "hello"}, {Kind: "command", Text: "/say hi"}}}).validate(); err != nil {
		t.Fatal(err)
	}
	if err := (scenario{AuthInput: true, Steps: []step{{Kind: "move", Ticks: 2}, {Kind: "click", Face: 1}, {Kind: "drop", Slot: 35, Count: 64}, {Kind: "use"}, {Kind: "release"}, {Kind: "consume"}}}).validate(); err != nil {
		t.Fatal(err)
	}
	if err := (scenario{AuthInput: true, Steps: []step{{Kind: "move_item", Count: 1, To: 28, ToContainer: "crafting"}, {Kind: "craft", Recipe: "r", Count: 1, Consume: []slotRef{{Container: "crafting", Slot: 28}, {Container: "crafting", Slot: 31}}}}}).validate(); err != nil {
		t.Fatal(err)
	}
	if err := (scenario{AuthInput: true, Steps: []step{{Kind: "move_item", Count: 1, To: 1, ToContainer: "anvil_input"}, {Kind: "craft", Action: "optional", Text: "Renamed", Count: 1, Consume: []slotRef{{Container: "anvil_input", Slot: 1}}}}}).validate(); err != nil {
		t.Fatal(err)
	}
}

func TestNBTValueTypesJSON(t *testing.T) {
	var decoded map[string]any
	if err := json.Unmarshal([]byte(`{"Damage":9,"display":{"Name":"Blade"},"ench":[{"id":9,"lvl":3}]}`), &decoded); err != nil {
		t.Fatal(err)
	}
	got := nbtValue(decoded).(map[string]any)
	if got["Damage"] != int32(9) || got["display"].(map[string]any)["Name"] != "Blade" || got["ench"].([]map[string]any)[0]["lvl"] != int32(3) {
		t.Fatalf("unexpected conversion %#v", got)
	}
}
