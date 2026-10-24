package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"os"
	"sort"
	"strconv"
	"time"

	"github.com/sandertv/go-raknet"
)

var sizes = []int{32, 512, 4096, 65536}

const networkTimeout = 30 * time.Second

var networkLog = slog.New(slog.NewTextHandler(os.Stderr, nil))

func countForSize(size, count int) (int, error) {
	if size < 4096 {
		return count, nil
	}
	value, ok := os.LookupEnv("BENCH_SPLIT_COUNT")
	if !ok {
		return count, nil
	}
	count, err := strconv.Atoi(value)
	if err != nil || count <= 0 {
		return 0, fmt.Errorf("BENCH_SPLIT_COUNT must be a positive integer")
	}
	return count, nil
}

func listenBenchmark(address string) (*raknet.Listener, error) {
	config := raknet.ListenConfig{ErrorLog: networkLog}
	if os.Getenv("BENCH_TRACE_SPLITS") == "1" {
		config.UpstreamPacketListener = traceListener{}
	}
	return config.Listen(address)
}

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

// The peer closes only after its final application read. Starting Close here
// would cancel that read even when transport-level ACKs have already arrived.
func finishEcho(conn *raknet.Conn) error {
	_, err := conn.ReadPacket()
	if err == nil {
		return fmt.Errorf("unexpected application message after final echo")
	}
	if !errors.Is(err, net.ErrClosed) {
		return err
	}
	return closeAndDrain(conn)
}

func serveListener(listener *raknet.Listener, count int) error {
	for _, size := range sizes {
		count, err := countForSize(size, count)
		if err != nil {
			return err
		}
		err = bounded(networkTimeout+time.Duration(count)*time.Second, func() error {
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
			return finishEcho(rc)
		})
		if err != nil {
			return err
		}
	}
	return nil
}

func serve(address string, count int) error {
	listener, err := listenBenchmark(address)
	if err != nil {
		return err
	}
	defer listener.Close()
	fmt.Fprintln(os.Stderr, "READY")
	return serveListener(listener, count)
}

func client(address string, count int) error {
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
	for _, size := range sizes {
		count, err := countForSize(size, count)
		if err != nil {
			return err
		}
		var result, setup row
		err = bounded(networkTimeout+time.Duration(count)*time.Second, func() error {
			// setup: the complete RakNet handshake until DialTimeout returns a
			// connected Conn; one sample per fresh connection.
			setupStart, err := clock.now()
			if err != nil {
				return err
			}
			conn, err := (raknet.Dialer{ErrorLog: networkLog}).DialTimeout(address, networkTimeout)
			if err != nil {
				return err
			}
			defer conn.Close()
			setupEnd, err := clock.now()
			if err != nil {
				return err
			}
			setupNS := clock.nanoseconds(setupStart, setupEnd)
			if setupNS <= 0 {
				return fmt.Errorf("benchmark clock measured a nonpositive setup duration")
			}
			setup = row{Area: "loopback", Name: fmt.Sprintf("%d", size), Operation: "setup", Implementation: "go", NSPerOp: setupNS, PayloadBytes: size, MsgsPerSec: 1e9 / setupNS}
			want := payload(size)
			samples := make([]float64, count)
			var start int64
			for i := -1; i < count; i++ {
				tick, err := clock.now()
				if err != nil {
					return err
				}
				n, err := conn.Write(want)
				if err != nil {
					return err
				}
				if n != len(want) {
					return io.ErrShortWrite
				}
				got, err := conn.ReadPacket()
				if err != nil {
					return fmt.Errorf("size %d measured message %d: %w", size, i, err)
				}
				if !bytes.Equal(got, want) {
					return fmt.Errorf("size %d message %d: echoed payload differs", size, i)
				}
				end, err := clock.now()
				if err != nil {
					return err
				}
				// The first echo proves application readiness after connection
				// setup and the previous session's asynchronous drain.
				if i < 0 {
					start = end
					continue
				}
				samples[i] = clock.nanoseconds(tick, end) / 1000
				if samples[i] <= 0 {
					return fmt.Errorf("benchmark clock measured a nonpositive echo duration")
				}
			}
			end, err := clock.now()
			if err != nil {
				return err
			}
			elapsedNS := clock.nanoseconds(start, end)
			if elapsedNS <= 0 {
				return fmt.Errorf("benchmark clock measured a nonpositive total duration")
			}
			elapsedSeconds := elapsedNS / 1e9
			sort.Float64s(samples)
			p50, p99 := samples[(count*50+99)/100-1], samples[(count*99+99)/100-1]
			maximum := samples[count-1]
			result = row{Area: "loopback", Name: fmt.Sprintf("%d", size), Operation: "echo", Implementation: "go", NSPerOp: elapsedNS / float64(count), PayloadBytes: size, MsgsPerSec: float64(count) / elapsedSeconds, MBPerSec: float64(count) * float64(size) / elapsedSeconds / 1e6, P50US: &p50, P99US: &p99, MaxUS: &maximum}
			return nil
		})
		if err != nil {
			return err
		}
		if err := emit(result); err != nil {
			return err
		}
		if err := emit(setup); err != nil {
			return err
		}
	}
	return nil
}

func loopback(count int) error {
	listener, err := listenBenchmark("127.0.0.1:0")
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
