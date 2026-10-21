package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// batchCases does not depend on typed packet support: framing preserves opaque
// packet bytes, including headers, and validates against the committed corpus.
func batchCases(dir string) ([]codecCase, error) {
	manifest, err := os.ReadFile(filepath.Join(dir, "manifest.json"))
	if err != nil {
		return nil, err
	}
	var fixtures []fixture
	if err := json.Unmarshal(manifest, &fixtures); err != nil {
		return nil, err
	}
	var cases []codecCase
	count := 0
	for _, f := range fixtures {
		if f.Set != "batch" {
			continue
		}
		count++
		batch, err := os.ReadFile(filepath.Join(dir, f.File))
		if err != nil {
			return nil, err
		}
		sum := sha256.Sum256(batch)
		if hex.EncodeToString(sum[:]) != f.SHA256 || len(batch) == 0 || batch[0] != 0xfe {
			return nil, fmt.Errorf("%s: invalid corpus digest or marker", f.Name)
		}
		parts, err := packet.NewDecoder(&batchSource{batch}).Decode()
		if err != nil {
			return nil, err
		}
		if len(parts) != len(f.Packets) {
			return nil, fmt.Errorf("%s: packet count differs", f.Name)
		}
		for _, mode := range []struct {
			encode, decode string
			compression    packet.Compression
		}{
			{"framing_encode", "framing_decode", nil},
			{"deflate_encode", "deflate_decode", packet.FlateCompression},
			{"snappy_encode", "snappy_decode", packet.SnappyCompression},
		} {
			var out bytes.Buffer
			encoder := packet.NewEncoder(&out)
			if mode.compression != nil {
				encoder.EnableCompression(mode.compression, 0)
			}
			encode := func() { out.Reset(); must(encoder.Encode(parts)) }
			encode()
			encoded := bytes.Clone(out.Bytes())
			if mode.compression == nil && !bytes.Equal(encoded, batch) {
				return nil, fmt.Errorf("%s: raw framing differs from corpus", f.Name)
			}
			decoder := packet.NewDecoder(&batchSource{encoded})
			if mode.compression != nil {
				decoder.EnableCompression(mode.compression, len(batch)-1)
			}
			decoded, err := decoder.Decode()
			if err != nil {
				return nil, err
			}
			if len(decoded) != len(parts) {
				return nil, fmt.Errorf("%s: roundtrip count differs", f.Name)
			}
			for i := range parts {
				if !bytes.Equal(decoded[i], parts[i]) {
					return nil, fmt.Errorf("%s: roundtrip packet %d differs", f.Name, i)
				}
			}
			cases = append(cases,
				codecCase{"batch", f.Name, mode.encode, len(batch), encode},
				codecCase{"batch", f.Name, mode.decode, len(batch), func() {
					data, err := decoder.Decode()
					must(err)
					runtime.KeepAlive(data)
				}},
			)
		}
		plain := batch[1:]
		compressed, err := packet.FlateCompression.Compress(plain)
		if err != nil {
			return nil, err
		}
		decoded, err := packet.FlateCompression.Decompress(compressed, len(plain))
		if err != nil {
			return nil, err
		}
		if !bytes.Equal(decoded, plain) {
			return nil, fmt.Errorf("%s: DEFLATE roundtrip differs", f.Name)
		}
		cases = append(cases,
			codecCase{"batch", f.Name, "compress", len(plain), func() {
				data, err := packet.FlateCompression.Compress(plain)
				must(err)
				runtime.KeepAlive(data)
			}},
			codecCase{"batch", f.Name, "decompress", len(plain), func() {
				data, err := packet.FlateCompression.Decompress(compressed, len(plain))
				must(err)
				runtime.KeepAlive(data)
			}},
		)
		for _, operation := range []string{"encrypt", "decrypt"} {
			data, err := json.Marshal(map[string]any{
				"area": "coverage", "name": f.Name, "operation": operation,
				"implementation": "go", "status": "unavailable",
				"reason": "isolated encryption/checksum helpers are private; public Encoder/Decoder also frame packets",
			})
			if err != nil {
				return nil, err
			}
			fmt.Fprintln(os.Stderr, string(data))
		}
	}
	if count != 4 {
		return nil, fmt.Errorf("expected four batch fixtures, got %d", count)
	}
	return cases, nil
}
