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

	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type fixture struct {
	Name    string   `json:"name"`
	File    string   `json:"file"`
	Sender  string   `json:"sender"`
	Set     string   `json:"set"`
	Packets []uint32 `json:"packets"`
	SHA256  string   `json:"sha256"`
}

type codecCase struct {
	name, operation string
	size            int
	run             func()
}
type entry struct {
	header          packet.Header
	pk              packet.Packet
	payload, framed []byte
}
type batchSource struct{ data []byte }

func (s *batchSource) Read(p []byte) (int, error)  { return copy(p, s.data), nil }
func (s *batchSource) ReadPacket() ([]byte, error) { return s.data, nil }

func defaultCorpus() string {
	if dir := os.Getenv("BENCH_CORPUS"); dir != "" {
		return dir
	}
	_, file, _, _ := runtime.Caller(0)
	return filepath.Join(filepath.Dir(file), "../../../crates/valentine_gen/protocolgen/testdata/bench")
}

func must(err error) {
	if err != nil {
		panic(err)
	}
}

func decodeEntry(p packet.Pool, data []byte, limits bool) entry {
	return decodeBuffer(p, bytes.NewBuffer(data), data, limits)
}

func decodeBuffer(p packet.Pool, buf *bytes.Buffer, data []byte, limits bool) entry {
	var h packet.Header
	must(h.Read(buf))
	newPacket, ok := p[h.PacketID]
	if !ok {
		panic(fmt.Sprintf("no typed packet for id %d", h.PacketID))
	}
	pk := newPacket()
	pk.Marshal(protocol.NewReader(buf, 0, limits))
	if buf.Len() != 0 {
		panic(fmt.Sprintf("packet %d: %d trailing bytes", h.PacketID, buf.Len()))
	}
	return entry{header: h, pk: pk, payload: data}
}

func prepareEntry(p packet.Pool, data []byte, limits bool) (e entry, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("typed decode: %v", recovered)
		}
	}()
	return decodeEntry(p, data, limits), nil
}

func unavailable(name string, err error) {
	data, marshalErr := json.Marshal(map[string]any{"area": "coverage", "name": name, "implementation": "go", "status": "unavailable", "reason": err.Error()})
	if marshalErr != nil {
		panic(marshalErr)
	}
	fmt.Fprintln(os.Stderr, string(data))
}

func decodeFramed(p packet.Pool, data []byte, limits bool) {
	buf := bytes.NewBuffer(data)
	var length uint32
	must(protocol.Varuint32(buf, &length))
	if uint64(length) != uint64(buf.Len()) {
		panic("invalid framed entry length")
	}
	decodeBuffer(p, buf, buf.Bytes(), limits)
}

func encodeEntry(out *bytes.Buffer, e entry) {
	must(e.header.Write(out))
	e.pk.Marshal(protocol.NewWriter(out, 0))
}

func codecCases(dir string) ([]codecCase, error) {
	manifest, err := os.ReadFile(filepath.Join(dir, "manifest.json"))
	if err != nil {
		return nil, err
	}
	var files []fixture
	if err := json.Unmarshal(manifest, &files); err != nil {
		return nil, err
	}
	var cases []codecCase
fixtures:
	for _, f := range files {
		batch, err := os.ReadFile(filepath.Join(dir, f.File))
		if err != nil {
			return nil, err
		}
		sum := sha256.Sum256(batch)
		if hex.EncodeToString(sum[:]) != f.SHA256 {
			return nil, fmt.Errorf("%s: SHA256 mismatch", f.File)
		}
		if len(batch) == 0 || batch[0] != 0xfe {
			return nil, fmt.Errorf("%s: invalid batch marker", f.File)
		}
		p, limits := packet.NewServerPool(), false
		if f.Sender == "client" {
			p, limits = packet.NewClientPool(), true
		} else if f.Sender != "server" {
			return nil, fmt.Errorf("invalid sender %q", f.Sender)
		}
		parts, err := packet.NewDecoder(&batchSource{batch}).Decode()
		if err != nil {
			return nil, fmt.Errorf("%s: %w", f.File, err)
		}
		if len(parts) != len(f.Packets) {
			return nil, fmt.Errorf("%s: packet count mismatch", f.File)
		}
		entries := make([]entry, len(parts))
		for i, part := range parts {
			var decodeErr error
			entries[i], decodeErr = prepareEntry(p, part, limits)
			if decodeErr != nil {
				unavailable(f.Name, decodeErr)
				continue fixtures
			}
			if entries[i].header.PacketID != f.Packets[i] {
				return nil, fmt.Errorf("%s: packet id mismatch", f.File)
			}
			var framed bytes.Buffer
			must(protocol.WriteVaruint32(&framed, uint32(len(part))))
			framed.Write(part)
			entries[i].framed = framed.Bytes()
		}
		if f.Set != "batch" {
			if len(entries) != 1 {
				return nil, fmt.Errorf("%s: expected one packet", f.File)
			}
			e := entries[0]
			var out, scratch bytes.Buffer
			encode := func() {
				scratch.Reset()
				encodeEntry(&scratch, e)
				out.Reset()
				must(protocol.WriteVaruint32(&out, uint32(scratch.Len())))
				out.Write(scratch.Bytes())
			}
			encode()
			if !bytes.Equal(out.Bytes(), e.framed) {
				unavailable(f.Name, fmt.Errorf("typed encoding differs from committed corpus"))
				continue fixtures
			}
			cases = append(cases, codecCase{f.Name, "decode", len(e.payload), func() { decodeFramed(p, e.framed, limits) }})
			cases = append(cases, codecCase{f.Name, "encode", len(e.payload), encode})
			continue
		}
		decoder := packet.NewDecoder(&batchSource{batch})
		decode := func(data []byte) error { decodeEntry(p, data, limits); return nil }
		var out bytes.Buffer
		encoder := packet.NewEncoder(&out)
		bufs := make([]bytes.Buffer, len(entries))
		payloads := make([][]byte, len(entries))
		encode := func() {
			out.Reset()
			for i, e := range entries {
				bufs[i].Reset()
				encodeEntry(&bufs[i], e)
				payloads[i] = bufs[i].Bytes()
			}
			must(encoder.Encode(payloads))
		}
		encode()
		if !bytes.Equal(out.Bytes(), batch) {
			unavailable(f.Name, fmt.Errorf("typed batch encoding differs from committed corpus"))
			continue fixtures
		}
		cases = append(cases, codecCase{f.Name, "decode_batch", len(batch), func() { must(decoder.DecodeFunc(decode)) }})
		cases = append(cases, codecCase{f.Name, "encode_batch", len(batch), encode})
		// Compression operates on the framed entries, excluding the 0xfe marker.
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
			return nil, fmt.Errorf("%s: compression round trip differs", f.File)
		}
		compressedFrame := append([]byte{0xfe, byte(packet.FlateCompression.EncodeCompression())}, compressed...)
		compressedDecoder := packet.NewDecoder(&batchSource{compressedFrame})
		compressedDecoder.EnableCompression(packet.FlateCompression, len(plain))
		decodeCompressed := func() { must(compressedDecoder.DecodeFunc(decode)) }
		decodeCompressed()
		cases = append(cases, codecCase{f.Name, "decode_compressed_batch", len(batch), decodeCompressed})
		cases = append(cases,
			codecCase{f.Name, "compress", len(plain), func() { data, err := packet.FlateCompression.Compress(plain); must(err); runtime.KeepAlive(data) }},
			codecCase{f.Name, "decompress", len(plain), func() {
				data, err := packet.FlateCompression.Decompress(compressed, len(plain))
				must(err)
				runtime.KeepAlive(data)
			}},
		)
	}
	return cases, nil
}
