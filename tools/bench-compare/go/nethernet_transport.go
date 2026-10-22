package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"sort"
	"strconv"
	"time"

	nethernet "github.com/df-mc/go-nethernet"
)

func nethernetCount(size, count int) (int, error) {
	if size < 4096 {
		return count, nil
	}
	value := os.Getenv("BENCH_SPLIT_COUNT")
	if value == "" {
		return 10, nil
	}
	n, err := strconv.Atoi(value)
	if err != nil || n <= 0 {
		return 0, fmt.Errorf("BENCH_SPLIT_COUNT must be positive")
	}
	return n, nil
}

func nethernetServe(address string, count int) error {
	listener, err := net.Listen("tcp4", address)
	if err != nil {
		return err
	}
	defer listener.Close()
	fmt.Fprintln(os.Stderr, "READY")
	return nethernetServeListener(listener, count)
}

func nethernetServeListener(tcp net.Listener, count int) error {
	for _, size := range sizes {
		n, err := nethernetCount(size, count)
		if err != nil {
			return err
		}
		if listener, ok := tcp.(*net.TCPListener); ok {
			if err := listener.SetDeadline(time.Now().Add(networkTimeout)); err != nil {
				return err
			}
		}
		raw, err := tcp.Accept()
		if err != nil {
			return err
		}
		err = nethernetServeRound(raw, size, n)
		if err != nil {
			return err
		}
	}
	return nil
}

func nethernetServeRound(raw net.Conn, size, count int) error {
	bridge := newNethernetBridge(raw, "1", "2")
	defer bridge.Close()
	timeout := time.AfterFunc(networkTimeout+time.Duration(count)*time.Second, func() { _ = bridge.Close() })
	defer timeout.Stop()
	listener, err := (nethernet.ListenConfig{
		API: nethernetAPI(), Log: networkLog, AllowAnonymous: true, DisableTrickleICE: true,
	}).Listen(bridge)
	if err != nil {
		return err
	}
	defer listener.Close()
	go bridge.readSignals()
	accepted, err := listener.Accept()
	if err != nil {
		return err
	}
	conn, ok := accepted.(*nethernet.Conn)
	if !ok {
		accepted.Close()
		return fmt.Errorf("unexpected NetherNet connection %T", accepted)
	}
	defer conn.Close()
	stopRead := time.AfterFunc(networkTimeout+time.Duration(count)*time.Second, func() { _ = conn.Close() })
	defer stopRead.Stop()
	want := payload(size)
	for i := 0; i <= count; i++ {
		got, err := conn.ReadPacket()
		if err != nil {
			return err
		}
		if !bytes.Equal(got, want) {
			return fmt.Errorf("NetherNet size %d message %d payload differs", size, i)
		}
		written, err := conn.Write(got)
		if err != nil {
			return err
		}
		if written != len(got) {
			return io.ErrShortWrite
		}
	}
	// Keep signaling and RTC live until final application delivery and the
	// server's final Write have both completed.
	completionCtx, cancelCompletion := context.WithTimeout(context.Background(), networkTimeout)
	defer cancelCompletion()
	if err := bridge.waitDone(completionCtx); err != nil {
		return err
	}
	return bridge.sendDone(completionCtx)
}

func nethernetClient(address string, count int) error {
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
		n, err := nethernetCount(size, count)
		if err != nil {
			return err
		}
		setup, echo, err := nethernetClientRound(address, size, n, clock)
		if err != nil {
			return err
		}
		if err := emit(setup); err != nil {
			return err
		}
		if err := emit(echo); err != nil {
			return err
		}
	}
	return nil
}

func nethernetClientRound(address string, size, count int, clock *benchmarkClock) (row, row, error) {
	fail := func(err error) (row, row, error) { return row{}, row{}, err }
	start, err := clock.now()
	if err != nil {
		return fail(err)
	}
	raw, err := net.DialTimeout("tcp4", address, networkTimeout)
	if err != nil {
		return fail(err)
	}
	bridge := newNethernetBridge(raw, "2", "1")
	defer bridge.Close()
	go bridge.readSignals()
	ctx, cancel := context.WithTimeout(context.Background(), networkTimeout)
	defer cancel()
	conn, err := (nethernet.Dialer{API: nethernetAPI(), Log: networkLog, DisableTrickleICE: true, AllowIdentitylessServer: true}).DialContext(ctx, "1", bridge)
	if err != nil {
		return fail(err)
	}
	defer conn.Close()
	end, err := clock.now()
	if err != nil {
		return fail(err)
	}
	setupNS := clock.nanoseconds(start, end)
	if setupNS <= 0 {
		return fail(fmt.Errorf("nonpositive NetherNet setup duration"))
	}
	setup := row{Area: "nethernet_loopback", Name: strconv.Itoa(size), Operation: "setup", Implementation: "go", NSPerOp: setupNS, PayloadBytes: size, MsgsPerSec: 1e9 / setupNS}
	timer := time.AfterFunc(networkTimeout+time.Duration(count)*time.Second, func() { _ = conn.Close() })
	defer timer.Stop()
	want := payload(size)
	exchange := func() error {
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
			return fmt.Errorf("NetherNet size %d echoed payload differs", size)
		}
		return nil
	}
	// The application-readiness echo is entirely outside setup and echo timing.
	if err := exchange(); err != nil {
		return fail(err)
	}
	samples := make([]float64, count)
	start, err = clock.now()
	if err != nil {
		return fail(err)
	}
	for i := range count {
		tick, err := clock.now()
		if err != nil {
			return fail(err)
		}
		if err := exchange(); err != nil {
			return fail(err)
		}
		end, err := clock.now()
		if err != nil {
			return fail(err)
		}
		samples[i] = clock.nanoseconds(tick, end) / 1000
		if samples[i] <= 0 {
			return fail(fmt.Errorf("nonpositive NetherNet echo duration"))
		}
	}
	end, err = clock.now()
	if err != nil {
		return fail(err)
	}
	elapsed := clock.nanoseconds(start, end)
	if elapsed <= 0 {
		return fail(fmt.Errorf("nonpositive NetherNet total duration"))
	}
	sort.Float64s(samples)
	p50, p99, maximum := samples[(count*50+99)/100-1], samples[(count*99+99)/100-1], samples[count-1]
	echo := row{Area: "nethernet_loopback", Name: strconv.Itoa(size), Operation: "echo", Implementation: "go", NSPerOp: elapsed / float64(count), PayloadBytes: size, MsgsPerSec: float64(count) * 1e9 / elapsed, MBPerSec: float64(count) * float64(size) * 1e3 / elapsed, P50US: &p50, P99US: &p99, MaxUS: &maximum}
	// Completion is outside measurement, with both transports kept alive until
	// the server confirms its final echo send has completed.
	completionCtx, cancelCompletion := context.WithTimeout(context.Background(), networkTimeout)
	defer cancelCompletion()
	if err := bridge.sendDone(completionCtx); err != nil {
		return fail(err)
	}
	if err := bridge.waitDone(completionCtx); err != nil {
		return fail(err)
	}
	return setup, echo, nil
}

func nethernetLoopback(count int) error {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		return err
	}
	defer listener.Close()
	done := make(chan error, 1)
	go func() { done <- nethernetServeListener(listener, count) }()
	if err := nethernetClient(listener.Addr().String(), count); err != nil {
		return err
	}
	return <-done
}
