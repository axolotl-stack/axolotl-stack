package main

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"sync"
)

// Diagnostic-only wire observation through the public listener socket hook.
// Never enabled during a timed comparison; it does not implement reassembly.
type traceListener struct{}
type splitKey struct {
	peer string
	id   uint16
}
type splitSeen struct {
	count    uint32
	indexes  map[uint32]bool
	complete bool
}
type traceConn struct {
	net.PacketConn
	mu                                             sync.Mutex
	groups                                         map[splitKey]*splitSeen
	completed, postComplete, duplicatePostComplete int
}

func (traceListener) ListenPacket(network, address string) (net.PacketConn, error) {
	conn, err := net.ListenPacket(network, address)
	if err != nil {
		return nil, err
	}
	return &traceConn{PacketConn: conn, groups: make(map[splitKey]*splitSeen)}, nil
}

func traceEmit(value any) {
	data, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	fmt.Fprintln(os.Stderr, string(data))
}

func (c *traceConn) ReadFrom(p []byte) (int, net.Addr, error) {
	n, addr, err := c.PacketConn.ReadFrom(p)
	if err == nil {
		c.observe(addr.String(), p[:n])
	}
	return n, addr, err
}

func (c *traceConn) Close() error {
	err := c.PacketConn.Close()
	c.mu.Lock()
	defer c.mu.Unlock()
	traceEmit(map[string]any{"area": "split_trace_summary", "direction": os.Getenv("BENCH_TRACE_DIRECTION"), "completed_groups": c.completed, "post_completion_fragments": c.postComplete, "duplicate_indexes_after_completion": c.duplicatePostComplete})
	return err
}

func (c *traceConn) observe(peer string, data []byte) {
	// Data datagrams have a one-byte flag and little-endian 24-bit sequence.
	// ACK/NACK packets do not contain encapsulated frames.
	if len(data) < 4 || data[0]&0xe0 != 0x80 {
		return
	}
	sequence := uint32(data[1]) | uint32(data[2])<<8 | uint32(data[3])<<16
	c.mu.Lock()
	defer c.mu.Unlock()
	for offset := 4; offset < len(data); {
		if len(data)-offset < 3 {
			return
		}
		flags := data[offset]
		length := (int(binary.BigEndian.Uint16(data[offset+1:])) + 7) / 8
		offset += 3
		reliability := flags >> 5
		switch reliability {
		case 2, 3, 4, 6, 7:
			offset += 3
		}
		switch reliability {
		case 1, 4:
			offset += 3
		}
		switch reliability {
		case 1, 3, 4, 7:
			offset += 4
		}
		if flags&0x10 != 0 {
			if offset > len(data) || len(data)-offset < 10 {
				return
			}
			count := binary.BigEndian.Uint32(data[offset:])
			id := binary.BigEndian.Uint16(data[offset+4:])
			index := binary.BigEndian.Uint32(data[offset+6:])
			offset += 10
			if count == 0 || index >= count {
				return
			}
			key := splitKey{peer, id}
			group := c.groups[key]
			if group == nil {
				group = &splitSeen{count: count, indexes: make(map[uint32]bool)}
				c.groups[key] = group
			}
			if group.complete {
				c.postComplete++
				if group.indexes[index] {
					c.duplicatePostComplete++
				}
				traceEmit(map[string]any{"area": "split_trace", "direction": os.Getenv("BENCH_TRACE_DIRECTION"), "peer": peer, "datagram_sequence": sequence, "split_id": id, "fragment_index": index, "fragment_count": count, "post_completion": true, "duplicate_index": group.indexes[index]})
			}
			group.indexes[index] = true
			if !group.complete && len(group.indexes) == int(group.count) {
				group.complete = true
				c.completed++
			}
		}
		if length <= 0 || offset > len(data) || length > len(data)-offset {
			return
		}
		offset += length
	}
}
