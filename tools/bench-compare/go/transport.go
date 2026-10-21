package main

import (
	"bytes"
	"fmt"
	"io"
	"os"
	"sort"
	"time"

	"github.com/sandertv/go-raknet"
)

var sizes = []int{32, 512, 4096, 65536}

const networkTimeout = 30 * time.Second

func payload(size int) []byte {
	p := make([]byte, size)
	for i := range p {
		p[i] = byte(i % 251)
	}
	p[0] = 0xfe
	return p
}

// The fork explicitly does not implement Conn deadlines. Bound each exchange
// group externally without inserting goroutine/channel overhead into each RTT.
func bounded(timeout time.Duration, f func() error) error {
	done := make(chan error, 1)
	go func() { done <- f() }()
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	select {
	case err := <-done:
		return err
	case <-timer.C:
		return fmt.Errorf("RakNet operation timed out after %s", timeout)
	}
}

// Close is asynchronous in the pinned fork. Keep the UDP listener alive until
// the reliable queue drains and the connection lifecycle actually ends.
func closeAndDrain(conn *raknet.Conn) error {
	if err := conn.Close(); err != nil {
		return err
	}
	<-conn.Context().Done()
	return nil
}

func serveListener(listener *raknet.Listener, count int) error {
	for _, size := range sizes {
		err := bounded(networkTimeout+time.Duration(count)*time.Second, func() error {
			conn, err := listener.Accept()
			if err != nil {
				return err
			}
			defer conn.Close()
			rc, ok := conn.(*raknet.Conn)
			if !ok {
				return fmt.Errorf("unexpected RakNet connection type %T", conn)
			}
			want := payload(size)
			for i := range count + 1 {
				got, err := rc.ReadPacket()
				if err != nil {
					return err
				}
				if !bytes.Equal(got, want) {
					return fmt.Errorf("size %d message %d: received payload differs", size, i)
				}
				n, err := rc.Write(got)
				if err != nil {
					return err
				}
				if n != len(got) {
					return io.ErrShortWrite
				}
			}
			return closeAndDrain(rc)
		})
		if err != nil {
			return err
		}
	}
	return nil
}

func serve(address string, count int) error {
	listener, err := raknet.Listen(address)
	if err != nil {
		return err
	}
	defer listener.Close()
	fmt.Fprintln(os.Stderr, "READY")
	return serveListener(listener, count)
}

func client(address string, count int) error {
	for _, size := range sizes {
		var result row
		err := bounded(networkTimeout+time.Duration(count)*time.Second, func() error {
			conn, err := raknet.DialTimeout(address, networkTimeout)
			if err != nil {
				return err
			}
			defer conn.Close()
			want := payload(size)
			samples := make([]float64, count)
			start := time.Now()
			for i := -1; i < count; i++ {
				tick := time.Now()
				n, err := conn.Write(want)
				if err != nil {
					return err
				}
				if n != len(want) {
					return io.ErrShortWrite
				}
				got, err := conn.ReadPacket()
				if err != nil {
					return err
				}
				if !bytes.Equal(got, want) {
					return fmt.Errorf("size %d message %d: echoed payload differs", size, i)
				}
				// The first echo proves application readiness after connection
				// setup and the previous session's asynchronous drain.
				if i < 0 {
					start = time.Now()
					continue
				}
				samples[i] = float64(time.Since(tick).Nanoseconds()) / 1000
			}
			elapsed := time.Since(start)
			sort.Float64s(samples)
			p50, p99 := samples[(count*50+99)/100-1], samples[(count*99+99)/100-1]
			result = row{Area: "loopback", Name: fmt.Sprintf("%d", size), Operation: "echo", Implementation: "go", NSPerOp: float64(elapsed.Nanoseconds()) / float64(count), PayloadBytes: size, MsgsPerSec: float64(count) / elapsed.Seconds(), MBPerSec: float64(count) * float64(size) / elapsed.Seconds() / 1e6, P50US: &p50, P99US: &p99}
			return nil
		})
		if err != nil {
			return err
		}
		if err := emit(result); err != nil {
			return err
		}
	}
	return nil
}

func loopback(count int) error {
	listener, err := raknet.Listen("127.0.0.1:0")
	if err != nil {
		return err
	}
	defer listener.Close()
	done := make(chan error, 1)
	go func() { done <- serveListener(listener, count) }()
	if err := client(listener.Addr().String(), count); err != nil {
		return err
	}
	return <-done
}
