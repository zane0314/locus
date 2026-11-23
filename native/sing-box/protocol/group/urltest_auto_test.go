package group

import (
	"context"
	"net"
	"strings"
	"testing"
	"time"

	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing-box/adapter/outbound"
	"github.com/sagernet/sing-box/common/urltest"
	"github.com/sagernet/sing-box/log"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
	"github.com/sagernet/sing/service"
	"github.com/sagernet/sing/service/pause"
)

type stubOutbound struct {
	outbound.Adapter
}

func newStubOutbound(tag string) *stubOutbound {
	return &stubOutbound{
		Adapter: outbound.NewAdapter("stub", tag, []string{N.NetworkTCP, N.NetworkUDP}, nil),
	}
}

func (s *stubOutbound) DialContext(ctx context.Context, network string, destination M.Socksaddr) (net.Conn, error) {
	return nil, net.ErrClosed
}

func (s *stubOutbound) ListenPacket(ctx context.Context, destination M.Socksaddr) (net.PacketConn, error) {
	return nil, net.ErrClosed
}

type stubOutboundManager struct {
	adapter.OutboundManager
	items map[string]adapter.Outbound
}

func (m *stubOutboundManager) Outbound(tag string) (adapter.Outbound, bool) {
	out, ok := m.items[tag]
	return out, ok
}

func TestApplyAutoTestResultsRespectsTolerance(t *testing.T) {
	ctx := context.Background()
	ctx = service.ContextWithDefaultRegistry(ctx)
	ctx = pause.WithDefaultManager(ctx)
	history := urltest.NewHistoryStorage()
	service.MustRegisterPtr[urltest.HistoryStorage](ctx, history)

	a := newStubOutbound("node-1")
	b := newStubOutbound("node-2")
	mgr := &stubOutboundManager{items: map[string]adapter.Outbound{"node-1": a, "node-2": b}}

	grp, err := NewURLTestGroup(ctx, mgr, log.NewNOPFactory().Logger(), []adapter.Outbound{a, b}, "", time.Minute, 20, 10*time.Minute, false)
	if err != nil {
		t.Fatalf("NewURLTestGroup: %v", err)
	}
	grp.managedTest = true
	grp.PostStart()

	chosen := grp.ApplyAutoTestResults(map[string]uint16{"node-1": 100, "node-2": 120})
	if chosen != "node-1" {
		t.Fatalf("expected node-1, got %q", chosen)
	}

	// Within tolerance (100 <= 85 + 20): keep node-1
	chosen = grp.ApplyAutoTestResults(map[string]uint16{"node-1": 100, "node-2": 85})
	if chosen != "node-1" {
		t.Fatalf("expected tolerance to keep node-1, got %q", chosen)
	}

	// Exceeds tolerance (100 > 70 + 20): switch to node-2
	chosen = grp.ApplyAutoTestResults(map[string]uint16{"node-1": 100, "node-2": 70})
	if chosen != "node-2" {
		t.Fatalf("expected switch to node-2, got %q", chosen)
	}

	// Strict manual ApplyTestResults ignores tolerance
	chosen = grp.ApplyTestResults(map[string]uint16{"node-1": 65, "node-2": 70})
	if chosen != "node-1" {
		t.Fatalf("expected strict switch to node-1, got %q", chosen)
	}

	if grp.idleFor() < 0 {
		t.Fatalf("expected non-negative idleFor")
	}
	time.Sleep(5 * time.Millisecond)
	beforeIdle := grp.idleFor()
	grp.Touch()
	afterIdle := grp.idleFor()
	if afterIdle > beforeIdle {
		t.Fatalf("expected Touch to reset idleFor: before=%v after=%v", beforeIdle, afterIdle)
	}

	u := &URLTest{group: grp}
	if info := u.URLTestRoundInfo(); info != "" {
		t.Fatalf("expected empty roundInfo before native round, got %q", info)
	}
	grp.roundStartMs.Store(1000)
	grp.roundDurationMs.Store(45)
	grp.roundSuccess.Store(1)
	grp.roundTotal.Store(2)
	grp.roundSeq.Store(3)
	if info := u.URLTestRoundInfo(); !strings.HasPrefix(info, "3,1000,45,1,2") {
		t.Fatalf("unexpected roundInfo: %q", info)
	}
}
