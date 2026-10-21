//go:build windows

package main

import (
	"fmt"
	"syscall"
	"unsafe"
)

type benchmarkClock struct {
	name      string
	frequency int64
	counter   *syscall.LazyProc
}

func newBenchmarkClock() (*benchmarkClock, error) {
	kernel := syscall.NewLazyDLL("kernel32.dll")
	frequencyProc := kernel.NewProc("QueryPerformanceFrequency")
	counter := kernel.NewProc("QueryPerformanceCounter")
	if err := frequencyProc.Find(); err != nil {
		return nil, fmt.Errorf("QueryPerformanceFrequency unavailable: %w", err)
	}
	if err := counter.Find(); err != nil {
		return nil, fmt.Errorf("QueryPerformanceCounter unavailable: %w", err)
	}
	var frequency int64
	ok, _, callErr := frequencyProc.Call(uintptr(unsafe.Pointer(&frequency)))
	if ok == 0 {
		return nil, fmt.Errorf("QueryPerformanceFrequency failed: %v", callErr)
	}
	if err := validateCounterFrequency(frequency); err != nil {
		return nil, err
	}
	return &benchmarkClock{name: "QueryPerformanceCounter", frequency: frequency, counter: counter}, nil
}

func (clock *benchmarkClock) now() (int64, error) {
	var tick int64
	ok, _, callErr := clock.counter.Call(uintptr(unsafe.Pointer(&tick)))
	if ok == 0 {
		return 0, fmt.Errorf("QueryPerformanceCounter failed: %v", callErr)
	}
	return tick, nil
}
