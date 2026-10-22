package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"

	nethernet "github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/discovery"
)

type nethernetSignalFixture struct{ name, text string }

func nethernetSignalFixtures() []nethernetSignalFixture {
	offer := "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na=setup:actpass\r\n"
	return []nethernetSignalFixture{
		{"connect_request", "CONNECTREQUEST 42 " + offer},
		{"connect_response", "CONNECTRESPONSE 42 " + strings.Replace(offer, "actpass", "active", 1)},
		{"candidate_add", "CANDIDATEADD 42 candidate:1 1 UDP 2130706431 127.0.0.1 50000 typ host generation 0 ufrag bench network-id 0 network-cost 0"},
		{"connect_error", "CONNECTERROR 42 2"},
	}
}

func nethernetDiscoveryPacket() *discovery.MessagePacket {
	return &discovery.MessagePacket{RecipientID: 2, Data: "CONNECTERROR 42 2"}
}

// Sinks keep observable results alive without replacing the reference API.
var nethernetSignalSink nethernet.Signal
var nethernetTextSink string
var nethernetPacketSink discovery.Packet
var nethernetBytesSink []byte

func nethernetMicro(iterations int) error {
	clock, err := newBenchmarkClock()
	if err != nil {
		return err
	}
	evidence, err := clock.validate()
	if err != nil {
		return err
	}
	if err := json.NewEncoder(os.Stderr).Encode(evidence); err != nil {
		return err
	}
	measure := func(area, name, operation string, size int, run func() error) error {
		for range 32 {
			if err := run(); err != nil {
				return err
			}
		}
		start, err := clock.now()
		if err != nil {
			return err
		}
		for range iterations {
			if err := run(); err != nil {
				return err
			}
		}
		end, err := clock.now()
		if err != nil {
			return err
		}
		ns := clock.nanoseconds(start, end) / float64(iterations)
		if ns <= 0 {
			return fmt.Errorf("nonpositive NetherNet micro duration")
		}
		return emit(row{Area: area, Name: name, Operation: operation, Implementation: "go", NSPerOp: ns, PayloadBytes: size, MsgsPerSec: 1e9 / ns, MBPerSec: float64(size) * 1e3 / ns})
	}
	for _, missing := range []struct{ name, reason string }{
		{"nethernet_framing", "Framing encode, reassembly, invalid segment counts and oversize rejection are private; no public micro API for 32/512/4096/65536/1048576-byte messages."},
		{"nethernet_signaling/ice_candidate", "NetherNet ICE candidate formatting/parsing is private; CANDIDATEADD measures only public Signal text parsing/formatting."},
	} {
		if err := json.NewEncoder(os.Stderr).Encode(map[string]string{"area": "coverage", "status": "unavailable", "name": missing.name, "implementation": "go", "reason": missing.reason}); err != nil {
			return err
		}
	}
	for _, fixture := range nethernetSignalFixtures() {
		encoded := []byte(fixture.text)
		var signal nethernet.Signal
		if err := signal.UnmarshalText(encoded); err != nil {
			return err
		}
		if err := measure("nethernet_signaling", fixture.name, "parse", len(encoded), func() error { return nethernetSignalSink.UnmarshalText(encoded) }); err != nil {
			return err
		}
		if err := measure("nethernet_signaling", fixture.name, "format", len(encoded), func() error { nethernetTextSink = signal.String(); return nil }); err != nil {
			return err
		}
	}
	packet := nethernetDiscoveryPacket()
	encoded := discovery.Marshal(packet, 1)
	if err := measure("nethernet_discovery", "packet", "encode", len(encoded), func() error { nethernetBytesSink = discovery.Marshal(packet, 1); return nil }); err != nil {
		return err
	}
	return measure("nethernet_discovery", "packet", "decode", len(encoded), func() error {
		var err error
		nethernetPacketSink, _, err = discovery.Unmarshal(encoded)
		return err
	})
}
