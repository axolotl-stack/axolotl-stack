// gt-client captures gophertunnel's unbatched packet stream while replaying a shared scenario.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"os/signal"
	"reflect"
	"sync"
	"sync/atomic"
	"time"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type identity struct {
	DisplayName    string `json:"display_name"`
	UUID           string `json:"uuid"`
	DeviceID       string `json:"device_id"`
	ClientRandomID int64  `json:"client_random_id"`
	DeviceOS       int    `json:"device_os"`
}

type step struct {
	AfterMS int64  `json:"after_ms"`
	Kind    string `json:"kind"`
	Text    string `json:"text"`
}

type scenario struct {
	IdleMS int64  `json:"idle_ms"`
	Steps  []step `json:"steps"`
}

type scenariosFile struct {
	Identity  identity            `json:"identity"`
	Scenarios map[string]scenario `json:"scenarios"`
}

func validDelay(ms int64) bool {
	return ms >= 0 && ms <= int64((1<<63-1)/time.Millisecond)
}

func (s scenario) validate() error {
	if !validDelay(s.IdleMS) {
		return fmt.Errorf("idle_ms must be a non-negative duration in milliseconds")
	}
	for i, step := range s.Steps {
		if !validDelay(step.AfterMS) {
			return fmt.Errorf("step %d: invalid after_ms", i)
		}
		if step.Kind != "chat" && step.Kind != "command" {
			return fmt.Errorf("step %d: unsupported kind %q", i, step.Kind)
		}
		if step.Text == "" {
			return fmt.Errorf("step %d: text must not be empty", i)
		}
	}
	return nil
}

func loadScenario(path, name string) (identity, scenario, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return identity{}, scenario{}, err
	}
	var config scenariosFile
	if err := json.Unmarshal(data, &config); err != nil {
		return identity{}, scenario{}, err
	}
	s, ok := config.Scenarios[name]
	if !ok {
		return identity{}, scenario{}, fmt.Errorf("unknown scenario %q", name)
	}
	id := config.Identity
	if _, err := uuid.Parse(id.UUID); err != nil {
		return id, s, fmt.Errorf("identity UUID: %w", err)
	}
	if id.DisplayName == "" || id.DeviceID == "" || id.ClientRandomID == 0 || id.DeviceOS <= 0 || id.DeviceOS > 15 {
		return id, s, errors.New("scenario identity requires display_name, device_id, nonzero client_random_id and device_os in 1..15")
	}
	return id, s, s.validate()
}

type captureRecord struct {
	Seq    uint64 `json:"seq"`
	TimeMS int64  `json:"t_ms"`
	Dir    string `json:"dir"`
	ID     uint32 `json:"id"`
	Name   string `json:"name"`
	Hex    string `json:"hex"`
}

type capture struct {
	mu              sync.Mutex
	encoder         *json.Encoder
	start           time.Time
	remote          string
	seq             uint64
	names           map[uint32]string
	err             error
	cancel          context.CancelCauseFunc
	initialised     chan struct{}
	initialisedOnce sync.Once
	closed          bool
}

func newCapture(w io.Writer, remote string, cancel context.CancelCauseFunc) *capture {
	names := make(map[uint32]string)
	for _, pool := range []packet.Pool{packet.NewClientPool(), packet.NewServerPool()} {
		for id, construct := range pool {
			names[id] = reflect.TypeOf(construct()).Elem().Name()
		}
	}
	return &capture{encoder: json.NewEncoder(w), start: time.Now(), remote: remote, names: names, cancel: cancel, initialised: make(chan struct{})}
}

// PacketFunc supplies the decoded header separately from the decrypted, decompressed payload.
// Re-encoding that varuint header preserves packet ID and both sub-client IDs.
func (c *capture) packet(header packet.Header, payload []byte, src, _ net.Addr) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.err != nil || c.closed {
		return
	}
	direction := "c2s"
	if src.String() == c.remote {
		direction = "s2c"
	}
	var raw bytes.Buffer
	raw.Grow(5 + len(payload))
	if err := header.Write(&raw); err != nil {
		c.fail(err)
		return
	}
	_, _ = raw.Write(payload)
	name := c.names[header.PacketID]
	if name == "" {
		name = fmt.Sprintf("Unknown_%d", header.PacketID)
	}
	record := captureRecord{Seq: c.seq, TimeMS: time.Since(c.start).Milliseconds(), Dir: direction, ID: header.PacketID, Name: name, Hex: hex.EncodeToString(raw.Bytes())}
	if err := c.encoder.Encode(record); err != nil {
		c.fail(err)
		return
	}
	c.seq++
	if direction == "c2s" && header.PacketID == packet.IDSetLocalPlayerAsInitialised {
		c.initialisedOnce.Do(func() { close(c.initialised) })
	}
}

func (c *capture) fail(err error) {
	c.err = fmt.Errorf("capture packet: %w", err)
	c.cancel(c.err)
}

func (c *capture) finish() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.closed = true
	return c.err
}

func delay(ctx context.Context, ms int64) error {
	timer := time.NewTimer(time.Duration(ms) * time.Millisecond)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return context.Cause(ctx)
	case <-timer.C:
		return nil
	}
}

func run(ctx context.Context, address, scenarioName, scenariosPath, output string) (result error) {
	id, script, err := loadScenario(scenariosPath, scenarioName)
	if err != nil {
		return err
	}
	remote, err := net.ResolveUDPAddr("udp", address)
	if err != nil {
		return fmt.Errorf("resolve server: %w", err)
	}
	file, err := os.Create(output)
	if err != nil {
		return fmt.Errorf("create capture: %w", err)
	}
	ctx, cancel := context.WithCancelCause(ctx)
	defer cancel(nil)
	capture := newCapture(file, remote.String(), cancel)
	defer func() { result = errors.Join(result, capture.finish(), file.Close()) }()
	dialer := minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: id.DisplayName, Identity: id.UUID},
		ClientData: login.ClientData{
			DeviceID: login.DeviceID(id.DeviceID), ClientRandomID: id.ClientRandomID,
			DeviceOS: protocol.DeviceOS(id.DeviceOS), DeviceModel: "ClientDiff",
			GameVersion: "1.26.51", LanguageCode: "en_US",
			CurrentInputMode: 1, DefaultInputMode: 1,
			SelfSignedID: id.UUID, PlayFabID: "0000000000000000",
			SkinID:          id.UUID + ".Custom",
			SkinData:        base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{255, 255, 255, 255}, 64*64)),
			SkinImageHeight: 64, SkinImageWidth: 64,
			SkinResourcePatch: base64.StdEncoding.EncodeToString([]byte(`{"geometry":{"default":"geometry.humanoid.custom"}}`)),
		},
		PacketFunc:                 capture.packet,
		DisablePortFollowing:       true,
		DisconnectOnInvalidPackets: true,
	}
	joinCtx, joinCancel := context.WithTimeout(ctx, 60*time.Second)
	defer joinCancel()
	conn, err := dialer.DialContext(joinCtx, "raknet", remote.String())
	if err != nil {
		return fmt.Errorf("offline login: %w", err)
	}
	var stopping atomic.Bool
	var readerDone chan struct{}
	defer func() {
		stopping.Store(true)
		result = errors.Join(result, conn.Close())
		if readerDone != nil {
			<-readerDone
		}
	}()
	if err := conn.DoSpawnContext(joinCtx); err != nil {
		return fmt.Errorf("spawn: %w", err)
	}
	// The fork closes its spawn channel before writing the initialisation packet.
	select {
	case <-capture.initialised:
	case <-joinCtx.Done():
		return fmt.Errorf("wait for local player initialisation: %w", context.Cause(joinCtx))
	}
	if err := conn.Flush(); err != nil {
		return fmt.Errorf("flush initialisation: %w", err)
	}
	joinCancel()
	readerDone = make(chan struct{})
	go func() {
		defer close(readerDone)
		for {
			pk, err := conn.ReadPacket()
			if err != nil {
				if !stopping.Load() {
					cancel(fmt.Errorf("read server packet: %w", err))
				}
				return
			}
			if latency, ok := pk.(*packet.NetworkStackLatency); ok && latency.NeedsResponse {
				err := conn.WritePacket(&packet.NetworkStackLatency{Timestamp: latency.Timestamp})
				if err == nil {
					err = conn.Flush()
				}
				if err != nil {
					if !stopping.Load() {
						cancel(fmt.Errorf("respond to latency: %w", err))
					}
					return
				}
			}
		}
	}()
	for i, step := range script.Steps {
		if err := delay(ctx, step.AfterMS); err != nil {
			return err
		}
		var pk packet.Packet
		switch step.Kind {
		case "chat":
			pk = &packet.Text{TextType: packet.TextTypeChat, SourceName: id.DisplayName, Message: step.Text}
		case "command":
			pk = &packet.CommandRequest{CommandLine: step.Text, CommandOrigin: protocol.CommandOrigin{Origin: protocol.CommandOriginPlayer, UUID: uuid.MustParse(id.UUID)}, Version: "latest"}
		}
		if err := conn.WritePacket(pk); err != nil {
			return fmt.Errorf("step %d: %w", i, err)
		}
		if err := conn.Flush(); err != nil {
			return fmt.Errorf("flush step %d: %w", i, err)
		}
	}
	if err := delay(ctx, script.IdleMS); err != nil {
		return err
	}
	return context.Cause(ctx)
}

func main() {
	address := flag.String("address", "127.0.0.1:19132", "BDS RakNet endpoint (offline authentication required)")
	scenario := flag.String("scenario", "join-idle", "shared scenario name")
	scenarios := flag.String("scenarios", "../scenarios.json", "shared scenario JSON")
	output := flag.String("output", "", "output packet JSONL path (required)")
	flag.Parse()
	if *output == "" || flag.NArg() != 0 {
		flag.Usage()
		os.Exit(2)
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	if err := run(ctx, *address, *scenario, *scenarios, *output); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
