package main

import (
	"bytes"
	"fmt"
	"testing"

	"github.com/sandertv/go-raknet"
)

func TestCorpusOperations(t *testing.T) {
	cases, err := codecCases(defaultCorpus())
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range cases {
		t.Run(c.name+"/"+c.operation, func(t *testing.T) { c.run() })
	}
}

func TestPayload(t *testing.T) {
	for _, size := range sizes {
		p := payload(size)
		if len(p) != size || p[0] != 0xfe {
			t.Fatal("invalid payload")
		}
		for i := 1; i < size; i++ {
			if p[i] != byte(i%251) {
				t.Fatal("invalid pattern")
			}
		}
		if !bytes.Equal(p, payload(size)) {
			t.Fatal("non-deterministic payload")
		}
	}
}

func TestCloseAndDrainWaitsForConnectionLifecycle(t *testing.T) {
	listener, err := raknet.Listen("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	done := make(chan error, 1)
	go func() {
		done <- bounded(networkTimeout, func() error {
			conn, err := listener.Accept()
			if err != nil {
				return err
			}
			rc := conn.(*raknet.Conn)
			defer rc.Close()
			packet, err := rc.ReadPacket()
			if err != nil {
				return err
			}
			if _, err = rc.Write(packet); err != nil {
				return err
			}
			if err = closeAndDrain(rc); err != nil {
				return err
			}
			if rc.Context().Err() == nil {
				return fmt.Errorf("returned before connection drained")
			}
			return nil
		})
	}()
	conn, err := raknet.DialTimeout(listener.Addr().String(), networkTimeout)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	want := payload(65536)
	if _, err = conn.Write(want); err != nil {
		t.Fatal(err)
	}
	got, err := conn.ReadPacket()
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, want) {
		t.Fatal("final fragmented echo was truncated")
	}
	if err = conn.Close(); err != nil {
		t.Fatal(err)
	}
	if err = <-done; err != nil {
		t.Fatal(err)
	}
}

func BenchmarkCodec(b *testing.B) {
	cases, err := codecCases(defaultCorpus())
	if err != nil {
		b.Fatal(err)
	}
	for _, c := range cases {
		b.Run(c.name+"/"+c.operation, func(b *testing.B) {
			b.ReportAllocs()
			b.SetBytes(int64(c.size))
			for b.Loop() {
				c.run()
			}
		})
	}
}
