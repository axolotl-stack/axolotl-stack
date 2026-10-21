package main

import "fmt"

// Counter values remain in their native units until after subtraction: converting
// absolute QPC ticks to nanoseconds can overflow on machines with long uptimes.
func counterNanoseconds(start, end, frequency int64) float64 {
	delta := end - start
	return float64(delta/frequency)*1e9 + float64(delta%frequency)*1e9/float64(frequency)
}

func validateCounterFrequency(frequency int64) error {
	// Compare in counter units, not rounded integer nanoseconds per tick.
	if frequency <= 1_000_000 {
		return fmt.Errorf("benchmark clock frequency %d Hz must exceed 1000000 Hz (resolution < 1us)", frequency)
	}
	return nil
}

type clockEvidence struct {
	Event              string  `json:"event"`
	Clock              string  `json:"clock"`
	FrequencyHz        int64   `json:"frequency_hz"`
	TickNS             float64 `json:"tick_ns"`
	MinimumPositiveNS  float64 `json:"minimum_positive_ns"`
	PositiveIncrements int     `json:"positive_increments"`
	Samples            int     `json:"samples"`
}

func measureClockResolution(name string, frequency int64, samples int, now func() (int64, error)) (clockEvidence, error) {
	evidence := clockEvidence{Event: "benchmark_clock", Clock: name, FrequencyHz: frequency, Samples: samples}
	if err := validateCounterFrequency(frequency); err != nil {
		return evidence, err
	}
	evidence.TickNS = 1e9 / float64(frequency)
	previous, err := now()
	if err != nil {
		return evidence, err
	}
	for range samples {
		current, err := now()
		if err != nil {
			return evidence, err
		}
		if current < previous {
			return evidence, fmt.Errorf("benchmark clock %s moved backwards", name)
		}
		if current > previous {
			ns := counterNanoseconds(previous, current, frequency)
			if evidence.PositiveIncrements == 0 || ns < evidence.MinimumPositiveNS {
				evidence.MinimumPositiveNS = ns
			}
			evidence.PositiveIncrements++
		}
		previous = current
	}
	if evidence.PositiveIncrements == 0 || evidence.MinimumPositiveNS >= 1000 {
		return evidence, fmt.Errorf("benchmark clock %s did not observe a positive increment below 1us: %+v", name, evidence)
	}
	return evidence, nil
}

func (clock *benchmarkClock) validate() (clockEvidence, error) {
	return measureClockResolution(clock.name, clock.frequency, 100_000, clock.now)
}

func (clock *benchmarkClock) nanoseconds(start, end int64) float64 {
	return counterNanoseconds(start, end, clock.frequency)
}
