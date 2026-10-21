package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"testing"
)

type row struct {
	Area           string   `json:"area"`
	Name           string   `json:"name"`
	Operation      string   `json:"operation"`
	Implementation string   `json:"implementation"`
	NSPerOp        float64  `json:"ns_per_op"`
	AllocsPerOp    *float64 `json:"allocs_per_op"`
	BytesPerOp     *float64 `json:"bytes_per_op"`
	PayloadBytes   int      `json:"payload_bytes"`
	MsgsPerSec     float64  `json:"msgs_per_sec"`
	MBPerSec       float64  `json:"mb_per_sec"`
	P50US          *float64 `json:"p50_us"`
	P99US          *float64 `json:"p99_us"`
	MaxUS          *float64 `json:"max_us"`
}

func emit(r row) error { return json.NewEncoder(os.Stdout).Encode(r) }

func run() error {
	if len(os.Args) < 2 {
		return fmt.Errorf("usage: bench-compare codec|micro|loopback|serve|client [options]")
	}
	fs := flag.NewFlagSet(os.Args[1], flag.ContinueOnError)
	address := fs.String("address", "127.0.0.1:19142", "RakNet address")
	count := fs.Int("count", 100, "sequential echoes per payload size")
	corpus := fs.String("corpus", defaultCorpus(), "committed benchmark corpus directory")
	if err := fs.Parse(os.Args[2:]); err != nil {
		return err
	}
	if fs.NArg() != 0 {
		return fmt.Errorf("unexpected arguments: %v", fs.Args())
	}
	if *count <= 0 {
		return fmt.Errorf("count must be positive")
	}
	switch os.Args[1] {
	case "codec":
		cases, err := codecCases(*corpus)
		if err != nil {
			return err
		}
		for _, c := range cases {
			result := testing.Benchmark(func(b *testing.B) {
				b.ReportAllocs()
				b.SetBytes(int64(c.size))
				for b.Loop() {
					c.run()
				}
			})
			ns := float64(result.T.Nanoseconds()) / float64(result.N)
			allocs, allocated := float64(result.MemAllocs)/float64(result.N), float64(result.MemBytes)/float64(result.N)
			if err := emit(row{Area: c.area, Name: c.name, Operation: c.operation, Implementation: "go", NSPerOp: ns, AllocsPerOp: &allocs, BytesPerOp: &allocated, PayloadBytes: c.size, MsgsPerSec: 1e9 / ns, MBPerSec: float64(c.size) * 1e3 / ns}); err != nil {
				return err
			}
		}
		return nil
	case "micro":
		fmt.Fprintln(os.Stderr, "Go RakNet datagram, split and ACK APIs are private; no comparable micro rows. See CAVEATS.md.")
		cases, err := batchCases(*corpus)
		if err != nil {
			return err
		}
		for _, c := range cases {
			result := testing.Benchmark(func(b *testing.B) {
				b.ReportAllocs()
				b.SetBytes(int64(c.size))
				for b.Loop() {
					c.run()
				}
			})
			ns := float64(result.T.Nanoseconds()) / float64(result.N)
			allocs, allocated := float64(result.MemAllocs)/float64(result.N), float64(result.MemBytes)/float64(result.N)
			if err := emit(row{Area: c.area, Name: c.name, Operation: c.operation, Implementation: "go", NSPerOp: ns, AllocsPerOp: &allocs, BytesPerOp: &allocated, PayloadBytes: c.size, MsgsPerSec: 1e9 / ns, MBPerSec: float64(c.size) * 1e3 / ns}); err != nil {
				return err
			}
		}
		return nil
	case "serve":
		return serve(*address, *count)
	case "client":
		return client(*address, *count)
	case "loopback":
		return loopback(*count)
	default:
		return fmt.Errorf("unknown command %q", os.Args[1])
	}
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
