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
	} {
		if err := scenario.validate(); err == nil {
			t.Fatalf("accepted invalid scenario: %+v", scenario)
		}
	}
	if err := (scenario{IdleMS: 10, Steps: []step{{AfterMS: 1, Kind: "chat", Text: "hello"}, {Kind: "command", Text: "/say hi"}}}).validate(); err != nil {
		t.Fatal(err)
	}
}
