package main

import (
	"context"
	"encoding/json"
	"net"
	"testing"
	"time"

	nethernet "github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/discovery"
)

func TestNethernetSignalFixtures(t *testing.T) {
	for _, fixture := range nethernetSignalFixtures() {
		var signal nethernet.Signal
		if err := signal.UnmarshalText([]byte(fixture.text)); err != nil {
			t.Fatal(err)
		}
		if signal.String() != fixture.text {
			t.Fatalf("%s failed round trip", fixture.name)
		}
	}
}

func TestNethernetDiscoveryFixture(t *testing.T) {
	packet := nethernetDiscoveryPacket()
	encoded := discovery.Marshal(packet, 1)
	decoded, sender, err := discovery.Unmarshal(encoded)
	if err != nil {
		t.Fatal(err)
	}
	message, ok := decoded.(*discovery.MessagePacket)
	if !ok || sender != 1 || message.RecipientID != 2 || message.Data != "CONNECTERROR 42 2" {
		t.Fatalf("unexpected discovery round trip: %#v, %d", decoded, sender)
	}
	encoded[len(encoded)-1] ^= 1
	if _, _, err := discovery.Unmarshal(encoded); err == nil {
		t.Fatal("corrupted authenticated packet accepted")
	}
}

type signalReceiver chan *nethernet.Signal

func (r signalReceiver) NotifySignal(signal *nethernet.Signal) bool { r <- signal; return true }

func TestNethernetBridgePreservesMultilineSignal(t *testing.T) {
	a, b := net.Pipe()
	defer b.Close()
	bridge := newNethernetBridge(a, "1", "2")
	defer bridge.Close()
	received := make(signalReceiver, 1)
	stop := bridge.Notify(received)
	defer stop()
	go bridge.readSignals()
	text := nethernetSignalFixtures()[0].text
	if err := json.NewEncoder(b).Encode(text); err != nil {
		t.Fatal(err)
	}
	select {
	case signal := <-received:
		if signal.String() != text || signal.NetworkID != "2" {
			t.Fatalf("unexpected signal: %#v", signal)
		}
	case <-time.After(time.Second):
		t.Fatal("signal not delivered")
	}
	stop()
	stop()
}

func TestNethernetBridgeWritesJSONStrings(t *testing.T) {
	a, b := net.Pipe()
	defer b.Close()
	bridge := newNethernetBridge(a, "2", "1")
	defer bridge.Close()
	signal := &nethernet.Signal{Type: nethernet.SignalTypeOffer, ConnectionID: 42, Data: "v=0\r\ns=-\r\n"}
	done := make(chan error, 1)
	go func() { done <- bridge.Signal(context.Background(), signal) }()
	var text string
	if err := json.NewDecoder(b).Decode(&text); err != nil {
		t.Fatal(err)
	}
	if text != signal.String() {
		t.Fatalf("unexpected wire string %q", text)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}

func TestNethernetDefaultCounts(t *testing.T) {
	t.Setenv("BENCH_SPLIT_COUNT", "")
	for _, size := range sizes {
		want := 100
		if size >= 4096 {
			want = 10
		}
		got, err := nethernetCount(size, 100)
		if err != nil || got != want {
			t.Fatalf("size %d: count %d, err %v", size, got, err)
		}
	}
}

// This exercises the reference transports and final-echo shutdown, not a mock.
func TestNethernetFinalLargeEcho(t *testing.T) {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	server := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			server <- err
			return
		}
		server <- nethernetServeRound(conn, 65536, 1)
	}()
	raw, err := net.DialTimeout("tcp4", listener.Addr().String(), networkTimeout)
	if err != nil {
		t.Fatal(err)
	}
	bridge := newNethernetBridge(raw, "2", "1")
	defer bridge.Close()
	go bridge.readSignals()
	ctx, cancel := context.WithTimeout(context.Background(), networkTimeout)
	defer cancel()
	conn, err := (nethernet.Dialer{API: nethernetAPI(), DisableTrickleICE: true, AllowIdentitylessServer: true}).DialContext(ctx, "1", bridge)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	timer := time.AfterFunc(networkTimeout, func() { _ = conn.Close() })
	defer timer.Stop()
	want := payload(65536)
	for range 2 {
		if n, err := conn.Write(want); err != nil || n != len(want) {
			t.Fatalf("write: %d, %v", n, err)
		}
		got, err := conn.ReadPacket()
		if err != nil {
			t.Fatal(err)
		}
		if string(got) != string(want) {
			t.Fatal("final echo differs")
		}
	}
	if err := raw.(*net.TCPConn).CloseWrite(); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-server:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(networkTimeout):
		t.Fatal("server did not finish after delivery acknowledgement")
	}
}
