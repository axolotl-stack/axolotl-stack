package main

import (
	"math"
	"testing"
)

func TestCounterNanoseconds(t *testing.T) {
	for _, tc := range []struct {
		name                  string
		start, end, frequency int64
		want                  float64
	}{
		{"zero", 123, 123, 10_000_000, 0},
		{"one QPC tick", 123, 124, 10_000_000, 100},
		{"fractional nanosecond", 0, 1, 3_000_000, 1000.0 / 3},
		{"nondivisor frequency", 0, 3_000_001, 3_000_001, 1e9},
		{"large uptime", math.MaxInt64 - 20, math.MaxInt64 - 3, 10_000_000, 1700},
		{"large delta", 0, 100_000_000_001, 10_000_000, 10_000_000_000_100},
	} {
		t.Run(tc.name, func(t *testing.T) {
			got := counterNanoseconds(tc.start, tc.end, tc.frequency)
			if math.Abs(got-tc.want) > math.Max(1e-9, tc.want*1e-15) {
				t.Fatalf("nanoseconds = %.17g, want %.17g", got, tc.want)
			}
		})
	}
}

func TestCounterFrequencyResolution(t *testing.T) {
	for _, frequency := range []int64{0, -1, 999_999, 1_000_000} {
		if err := validateCounterFrequency(frequency); err == nil {
			t.Errorf("frequency %d must fail submicrosecond requirement", frequency)
		}
	}
	for _, frequency := range []int64{1_000_001, 3_000_001, 10_000_000} {
		if err := validateCounterFrequency(frequency); err != nil {
			t.Errorf("frequency %d: %v", frequency, err)
		}
	}
}

func TestClockResolutionValidation(t *testing.T) {
	for _, tc := range []struct {
		name        string
		ticks       []int64
		wantError   bool
		wantMinimum float64
	}{
		{"positive submicrosecond", []int64{100, 100, 110, 113, 113}, false, 300},
		{"no positive increments", []int64{100, 100, 100}, true, 0},
		{"microsecond increments", []int64{100, 110, 120}, true, 0},
		{"backwards", []int64{100, 99, 101}, true, 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			i := 0
			evidence, err := measureClockResolution("test", 10_000_000, len(tc.ticks)-1, func() (int64, error) {
				value := tc.ticks[i]
				i++
				return value, nil
			})
			if (err != nil) != tc.wantError {
				t.Fatalf("error = %v, wantError %v", err, tc.wantError)
			}
			if err == nil && evidence.MinimumPositiveNS != tc.wantMinimum {
				t.Fatalf("minimum = %g, want %g", evidence.MinimumPositiveNS, tc.wantMinimum)
			}
		})
	}
}

func TestBenchmarkClockStartup(t *testing.T) {
	clock, err := newBenchmarkClock()
	if err != nil {
		t.Fatal(err)
	}
	evidence, err := clock.validate()
	if err != nil {
		t.Fatal(err)
	}
	if evidence.MinimumPositiveNS <= 0 || evidence.MinimumPositiveNS >= 1000 {
		t.Fatalf("clock is not empirically submicrosecond: %+v", evidence)
	}
	t.Logf("clock evidence: %+v", evidence)
}
