package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func snappyNoise(n int) []byte {
	state := uint32(0x12345678)
	out := make([]byte, n)
	for i := range out {
		state ^= state << 13
		state ^= state >> 17
		state ^= state << 5
		out[i] = byte(state)
	}
	return out
}

func TestJolyneSnappyGoldenInteroperability(t *testing.T) {
	root := filepath.Join("..", "..", "..", "crates", "jolyne", "tests", "fixtures", "snappy")
	cases := map[string][]byte{
		"long-literal": snappyNoise(65537),
		"copy4":        append(append(snappyNoise(4096), make([]byte, 70000)...), snappyNoise(4096)...),
	}
	for _, name := range []string{"spawn", "server_tick", "chunk_radius", "client_tick"} {
		plain, err := os.ReadFile(filepath.Join(defaultCorpus(), "synthetic_batch_"+name+".bin"))
		if err != nil {
			t.Fatal(err)
		}
		cases[name] = plain[1:]
	}
	for name, want := range cases {
		t.Run(name, func(t *testing.T) {
			encoded, err := os.ReadFile(filepath.Join(root, name+".snappy"))
			if err != nil {
				t.Fatal(err)
			}
			got, err := packet.SnappyCompression.Decompress(encoded, len(want))
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(got, want) {
				t.Fatalf("jolyne output decoded incorrectly: got %d bytes, want %d", len(got), len(want))
			}
			if len(want) > 0 {
				if _, err := packet.SnappyCompression.Decompress(encoded, len(want)-1); err == nil {
					t.Fatal("decoder accepted output exceeding its configured limit")
				}
			}
		})
	}
}
