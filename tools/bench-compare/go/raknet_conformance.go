package main

import (
	"bytes"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/sandertv/go-raknet"
)

// Peers for crates/tokio-raknet/tests/go_conformance.rs. The Rust test runs
// the same scripted client through a recording UDP relay twice, once with
// this go-raknet client and once with tokio-raknet in Vanilla mode, and
// compares the recorded datagram sequences and timings.

// conformanceServe echoes every message of every connection until killed.
func conformanceServe(address string) error {
	listener, err := raknet.ListenConfig{ErrorLog: networkLog}.Listen(address)
	if err != nil {
		return err
	}
	defer listener.Close()
	fmt.Fprintf(os.Stderr, "READY %s\n", listener.Addr())
	for {
		conn, err := listener.Accept()
		if err != nil {
			return err
		}
		rc, ok := conn.(*raknet.Conn)
		if !ok {
			return fmt.Errorf("unexpected RakNet connection type %T", conn)
		}
		go func() {
			for {
				message, err := rc.ReadPacket()
				if err != nil {
					return
				}
				if _, err := rc.Write(message); err != nil {
					return
				}
			}
		}()
	}
}

// parseSizes parses a comma-separated list of positive message sizes.
func parseSizes(list string) ([]int, error) {
	var out []int
	for _, field := range strings.Split(list, ",") {
		size, err := strconv.Atoi(strings.TrimSpace(field))
		if err != nil || size <= 0 {
			return nil, fmt.Errorf("invalid message size %q", field)
		}
		out = append(out, size)
	}
	return out, nil
}

// conformanceClient dials, echoes each scripted message in turn, stays idle
// for hold (keepalive/ACK cadence), then closes and waits for the close to
// complete.
func conformanceClient(address string, sizes []int, hold time.Duration) error {
	conn, err := (raknet.Dialer{ErrorLog: networkLog}).DialTimeout(address, 15*time.Second)
	if err != nil {
		return err
	}
	for i, size := range sizes {
		want := payload(size)
		if _, err := conn.Write(want); err != nil {
			return err
		}
		got, err := conn.ReadPacket()
		if err != nil {
			return fmt.Errorf("message %d: %w", i, err)
		}
		if !bytes.Equal(got, want) {
			return fmt.Errorf("message %d: echoed payload differs", i)
		}
	}
	time.Sleep(hold)
	if err := conn.Close(); err != nil {
		return err
	}
	<-conn.Context().Done()
	fmt.Println("DONE")
	return nil
}
