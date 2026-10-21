package main

import (
	"encoding/binary"
	"testing"
)

func traceFragment(sequence byte, index uint32) []byte {
	data := []byte{0x84, sequence, 0, 0, 0x70, 0, 8, 0, 0, 0, 0, 0, 0, 0}
	split := make([]byte, 10)
	binary.BigEndian.PutUint32(split, 2)
	binary.BigEndian.PutUint16(split[4:], 7)
	binary.BigEndian.PutUint32(split[6:], index)
	return append(append(data, split...), byte(index))
}

func TestSplitTraceDistinguishesPostCompletionDuplicate(t *testing.T) {
	trace := &traceConn{groups: make(map[splitKey]*splitSeen)}
	trace.observe("peer", traceFragment(1, 0))
	if trace.completed != 0 {
		t.Fatal("first fragment completed a two-fragment packet")
	}
	trace.observe("peer", traceFragment(2, 1))
	if trace.completed != 1 {
		t.Fatal("complete split not recognized")
	}
	trace.observe("peer", traceFragment(3, 0))
	if trace.completed != 1 || trace.postComplete != 1 || trace.duplicatePostComplete != 1 {
		t.Fatalf("late duplicate misclassified: complete=%d post=%d duplicate=%d", trace.completed, trace.postComplete, trace.duplicatePostComplete)
	}
}
