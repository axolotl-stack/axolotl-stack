package main

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"net"
	"sync"

	nethernet "github.com/df-mc/go-nethernet"
	"github.com/pion/webrtc/v4"
)

// The benchmark-local TCP transport carries one JSON string per line. The string
// is the reference Signal text, including literal SDP CRLF after JSON decoding.
// Network IDs are fixed out of band, never substituted into the signal payload.
type nethernetBridge struct {
	conn          net.Conn
	local, remote string
	ctx           context.Context
	cancel        context.CancelCauseFunc
	writeMu       sync.Mutex
	notifyMu      sync.Mutex
	notifiers     map[uint64]nethernet.Notifier
	next          uint64
}

func newNethernetBridge(conn net.Conn, local, remote string) *nethernetBridge {
	ctx, cancel := context.WithCancelCause(context.Background())
	return &nethernetBridge{conn: conn, local: local, remote: remote, ctx: ctx, cancel: cancel, notifiers: make(map[uint64]nethernet.Notifier)}
}
func (b *nethernetBridge) Close() error             { b.cancel(net.ErrClosed); return b.conn.Close() }
func (b *nethernetBridge) Context() context.Context { return b.ctx }
func (b *nethernetBridge) NetworkID() string        { return b.local }
func (b *nethernetBridge) PongData([]byte)          {}
func (b *nethernetBridge) Credentials(context.Context) (*nethernet.Credentials, error) {
	return nil, nil
}
func (b *nethernetBridge) Signal(ctx context.Context, signal *nethernet.Signal) error {
	b.writeMu.Lock()
	defer b.writeMu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if deadline, ok := ctx.Deadline(); ok {
		if err := b.conn.SetWriteDeadline(deadline); err != nil {
			return err
		}
	}
	return json.NewEncoder(b.conn).Encode(signal.String())
}
func (b *nethernetBridge) Notify(notifier nethernet.Notifier) func() {
	b.notifyMu.Lock()
	id := b.next
	b.next++
	b.notifiers[id] = notifier
	b.notifyMu.Unlock()
	return sync.OnceFunc(func() { b.notifyMu.Lock(); delete(b.notifiers, id); b.notifyMu.Unlock() })
}
func (b *nethernetBridge) readSignals() {
	scanner := bufio.NewScanner(b.conn)
	scanner.Buffer(make([]byte, 4096), 1024*1024)
	for scanner.Scan() {
		var text string
		if err := json.Unmarshal(scanner.Bytes(), &text); err != nil {
			b.cancel(err)
			return
		}
		signal := &nethernet.Signal{NetworkID: b.remote}
		if err := signal.UnmarshalText([]byte(text)); err != nil {
			b.cancel(err)
			return
		}
		b.notifyMu.Lock()
		targets := make([]nethernet.Notifier, 0, len(b.notifiers))
		for _, target := range b.notifiers {
			targets = append(targets, target)
		}
		b.notifyMu.Unlock()
		for _, target := range targets {
			target.NotifySignal(signal)
		}
	}
	err := scanner.Err()
	if err == nil {
		err = io.EOF
	}
	b.cancel(err)
}

func nethernetAPI() *webrtc.API {
	settings := webrtc.SettingEngine{}
	settings.SetIncludeLoopbackCandidate(true)
	settings.SetIPFilter(func(ip net.IP) bool { return ip.IsLoopback() })
	settings.SetNetworkTypes([]webrtc.NetworkType{webrtc.NetworkTypeUDP4})
	return webrtc.NewAPI(webrtc.WithSettingEngine(settings))
}
