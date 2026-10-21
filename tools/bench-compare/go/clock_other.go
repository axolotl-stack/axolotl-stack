//go:build !windows

package main

import "time"

type benchmarkClock struct {
	name      string
	frequency int64
	origin    time.Time
}

func newBenchmarkClock() (*benchmarkClock, error) {
	return &benchmarkClock{name: "time.Since monotonic", frequency: 1_000_000_000, origin: time.Now()}, nil
}

func (clock *benchmarkClock) now() (int64, error) {
	return time.Since(clock.origin).Nanoseconds(), nil
}
