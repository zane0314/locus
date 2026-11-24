package group

import (
	"context"
	"fmt"
	"io"
	"maps"
	"net"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing-box/adapter/outbound"
	"github.com/sagernet/sing-box/common/interrupt"
	"github.com/sagernet/sing-box/common/urltest"
	C "github.com/sagernet/sing-box/constant"
	"github.com/sagernet/sing-box/log"
	"github.com/sagernet/sing-box/option"
	"github.com/sagernet/sing/common"
	"github.com/sagernet/sing/common/batch"
	E "github.com/sagernet/sing/common/exceptions"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
	"github.com/sagernet/sing/common/x/list"
	"github.com/sagernet/sing/service"
	"github.com/sagernet/sing/service/pause"
)

func RegisterURLTest(registry *outbound.Registry) {
	outbound.Register[option.URLTestOutboundOptions](registry, C.TypeURLTest, NewURLTest)
}

var (
	_ adapter.OutboundGroup           = (*URLTest)(nil)
	_ adapter.InterfaceUpdateListener = (*URLTest)(nil)
	_ adapter.Referrer                = (*URLTest)(nil)
)

type URLTest struct {
	outbound.Adapter
	ctx                          context.Context
	outbound                     adapter.OutboundManager
	logger                       log.ContextLogger
	tags                         []string
	link                         string
	interval                     time.Duration
	tolerance                    uint16
	idleTimeout                  time.Duration
	group                        *URLTestGroup
	checkAccess                  sync.Mutex
	interruptExternalConnections bool
	managedTest                  bool
}

func NewURLTest(ctx context.Context, router adapter.Router, logger log.ContextLogger, tag string, options option.URLTestOutboundOptions) (adapter.Outbound, error) {
	outbound := &URLTest{
		Adapter:                      outbound.NewAdapter(C.TypeURLTest, tag, []string{N.NetworkTCP, N.NetworkUDP}, options.Outbounds),
		ctx:                          ctx,
		outbound:                     service.FromContext[adapter.OutboundManager](ctx),
		logger:                       logger,
		tags:                         options.Outbounds,
		link:                         options.URL,
		interval:                     time.Duration(options.Interval),
		tolerance:                    options.Tolerance,
		idleTimeout:                  time.Duration(options.IdleTimeout),
		interruptExternalConnections: options.InterruptExistConnections,
		managedTest:                  options.ManagedTest,
	}
	if len(outbound.tags) == 0 {
		return nil, E.New("missing tags")
	}
	for i, tag := range outbound.tags {
		if slices.Contains(outbound.tags[:i], tag) {
			return nil, E.New("duplicate tag: ", tag)
		}
	}
	return outbound, nil
}

func (s *URLTest) Start(stage adapter.StartStage, scope *adapter.Scope) error {
	switch stage {
	case adapter.StartStateStart:
		outbounds := make([]adapter.Outbound, 0, len(s.tags))
		for i, tag := range s.tags {
			detour, loaded := s.outbound.Outbound(tag)
			if !loaded {
				return E.New("outbound ", i, " not found: ", tag)
			}
			outbounds = append(outbounds, detour)
		}
		group, err := NewURLTestGroup(s.ctx, s.outbound, s.logger, outbounds, s.link, s.interval, s.tolerance, s.idleTimeout, s.interruptExternalConnections)
		if err != nil {
			return err
		}
		group.managedTest = s.managedTest
		s.group = group
	case adapter.StartStateStarted:
		s.group.PostStart()
		scope.Add(s.group.Close)
	}
	return nil
}

func (s *URLTest) All() []string {
	return s.tags
}

func (s *URLTest) Selected(network string) adapter.Outbound {
	var outbound adapter.Outbound
	if network == N.NetworkUDP {
		outbound = s.group.selectedOutboundUDP.Load()
	} else {
		outbound = s.group.selectedOutboundTCP.Load()
	}
	if outbound == nil {
		outbound, _ = s.group.Select(network)
	}
	return outbound
}

func (s *URLTest) Now() string {
	if s.group.selectedOutboundTCP.Load() != nil {
		return s.group.selectedOutboundTCP.Load().Tag()
	} else if s.group.selectedOutboundUDP.Load() != nil {
		return s.group.selectedOutboundUDP.Load().Tag()
	}
	return ""
}

func (s *URLTest) AttachConnection(closer io.Closer) func() {
	s.group.Touch()
	return s.group.interruptGroup.Add(closer, true)
}

func (s *URLTest) References() []string {
	group := s.group
	if group == nil {
		return nil
	}
	var references []string
	tcp := group.selectedOutboundTCP.Load()
	udp := group.selectedOutboundUDP.Load()
	if tcp != nil {
		references = append(references, tcp.Tag())
	}
	if udp != nil && udp != tcp {
		references = append(references, udp.Tag())
	}
	return references
}

func (s *URLTest) URLTest(ctx context.Context) (map[string]uint16, error) {
	return s.group.URLTest(ctx)
}

func (s *URLTest) CheckOutbounds() {
	s.group.CheckOutbounds(s.ctx, true)
}

func (s *URLTest) PerformUpdateCheck() {
	s.group.performUpdateCheck()
}

func (s *URLTest) InterfaceUpdated(ctx context.Context) {
	group := s.group
	if group == nil || group.managedTest {
		return
	}
	if group.pause.IsDevicePaused() || group.pause.IsNetworkPaused() {
		return
	}
	go func() {
		s.checkAccess.Lock()
		defer s.checkAccess.Unlock()
		if ctx.Err() != nil {
			return
		}
		group.CheckOutbounds(ctx, true)
	}()
}

func (s *URLTest) DialContext(ctx context.Context, network string, destination M.Socksaddr) (net.Conn, error) {
	s.group.Touch()
	var outbound adapter.Outbound
	switch N.NetworkName(network) {
	case N.NetworkTCP:
		outbound = s.group.selectedOutboundTCP.Load()
	case N.NetworkUDP:
		outbound = s.group.selectedOutboundUDP.Load()
	default:
		return nil, E.Extend(N.ErrUnknownNetwork, network)
	}
	if outbound == nil {
		outbound, _ = s.group.Select(network)
	}
	if outbound == nil {
		return nil, E.New("missing supported outbound")
	}
	conn, err := outbound.DialContext(ctx, network, destination)
	if err == nil {
		return s.group.interruptGroup.NewConn(conn, interrupt.IsExternalConnectionFromContext(ctx)), nil
	}
	s.logger.ErrorContext(ctx, err)
	s.group.history.DeleteURLTestHistory(outbound.Tag())
	return nil, err
}

func (s *URLTest) ListenPacket(ctx context.Context, destination M.Socksaddr) (net.PacketConn, error) {
	s.group.Touch()
	outbound := s.group.selectedOutboundUDP.Load()
	if outbound == nil {
		outbound, _ = s.group.Select(N.NetworkUDP)
	}
	if outbound == nil {
		return nil, E.New("missing supported outbound")
	}
	conn, err := outbound.ListenPacket(ctx, destination)
	if err == nil {
		return s.group.interruptGroup.NewPacketConn(conn, interrupt.IsExternalConnectionFromContext(ctx)), nil
	}
	s.logger.ErrorContext(ctx, err)
	s.group.history.DeleteURLTestHistory(outbound.Tag())
	return nil, err
}

type URLTestGroup struct {
	ctx                          context.Context
	outbound                     adapter.OutboundManager
	pause                        pause.Manager
	pauseCallback                *list.Element[pause.Callback]
	logger                       log.Logger
	outbounds                    []adapter.Outbound
	link                         string
	interval                     time.Duration
	tolerance                    uint16
	idleTimeout                  time.Duration
	history                      *urltest.HistoryStorage
	checking                     atomic.Bool
	selectedOutboundTCP          common.TypedValue[adapter.Outbound]
	selectedOutboundUDP          common.TypedValue[adapter.Outbound]
	interruptGroup               *interrupt.Group
	interruptExternalConnections bool
	access                       sync.Mutex
	updateAccess                 sync.Mutex
	ticker                       *time.Ticker
	close                        chan struct{}
	started                      bool
	lastActive                   common.TypedValue[time.Time]
	lastActiveNs                 atomic.Int64
	lastRoundAt                  atomic.Int64
	roundSeq                     atomic.Uint64
	roundStartMs                 atomic.Int64
	roundDurationMs              atomic.Int64
	roundSuccess                 atomic.Int32
	roundTotal                   atomic.Int32
	managedTest                  bool
}

func NewURLTestGroup(ctx context.Context, outboundManager adapter.OutboundManager, logger log.Logger, outbounds []adapter.Outbound, link string, interval time.Duration, tolerance uint16, idleTimeout time.Duration, interruptExternalConnections bool) (*URLTestGroup, error) {
	if interval == 0 {
		interval = C.DefaultURLTestInterval
	}
	if tolerance == 0 {
		tolerance = 50
	}
	if idleTimeout == 0 {
		idleTimeout = C.DefaultURLTestIdleTimeout
	}
	if interval > idleTimeout {
		return nil, E.New("interval must be less or equal than idle_timeout")
	}
	history := service.PtrFromContext[urltest.HistoryStorage](ctx)
	if history == nil {
		return nil, E.New("missing URL test history storage")
	}
	return &URLTestGroup{
		ctx:                          ctx,
		outbound:                     outboundManager,
		logger:                       logger,
		outbounds:                    outbounds,
		link:                         link,
		interval:                     interval,
		tolerance:                    tolerance,
		idleTimeout:                  idleTimeout,
		history:                      history,
		close:                        make(chan struct{}),
		pause:                        service.FromContext[pause.Manager](ctx),
		interruptGroup:               interrupt.NewGroup(),
		interruptExternalConnections: interruptExternalConnections,
	}, nil
}

func (g *URLTestGroup) PostStart() {
	g.lastActiveNs.Store(int64(elapsedBootTime()))
	if g.managedTest {
		return
	}
	g.access.Lock()
	defer g.access.Unlock()
	g.started = true
	g.lastActive.Store(time.Now())
	go g.CheckOutbounds(g.ctx, false)
}

func (g *URLTestGroup) Touch() {
	g.lastActiveNs.Store(int64(elapsedBootTime()))
	if g.managedTest {
		return
	}
	g.access.Lock()
	defer g.access.Unlock()
	if !g.started {
		return
	}
	if g.ticker != nil {
		g.lastActive.Store(time.Now())
		return
	}
	ticker := time.NewTicker(g.interval)
	g.ticker = ticker
	g.pauseCallback = pause.RegisterTicker(g.pause, ticker, g.interval, nil)
	go g.loopCheck(ticker, g.close)
}

func (g *URLTestGroup) Close() error {
	g.access.Lock()
	defer g.access.Unlock()
	if g.ticker == nil {
		return nil
	}
	g.ticker.Stop()
	g.ticker = nil
	g.pause.UnregisterCallback(g.pauseCallback)
	g.pauseCallback = nil
	close(g.close)
	return nil
}

func (g *URLTestGroup) Select(network string) (adapter.Outbound, bool) {
	if g.managedTest {
		var selected adapter.Outbound
		if network == N.NetworkTCP {
			selected = g.selectedOutboundTCP.Load()
		} else if network == N.NetworkUDP {
			selected = g.selectedOutboundUDP.Load()
		}
		return selected, selected != nil
	}
	var minDelay uint16
	var minOutbound adapter.Outbound
	switch network {
	case N.NetworkTCP:
		if selected := g.selectedOutboundTCP.Load(); selected != nil {
			if history := g.history.LoadURLTestHistory(RealTag(selected, N.NetworkTCP)); history != nil {
				minOutbound = selected
				minDelay = history.Delay
			}
		}
	case N.NetworkUDP:
		if selected := g.selectedOutboundUDP.Load(); selected != nil {
			if history := g.history.LoadURLTestHistory(RealTag(selected, N.NetworkUDP)); history != nil {
				minOutbound = selected
				minDelay = history.Delay
			}
		}
	}
	for _, detour := range g.outbounds {
		if !common.Contains(detour.Network(), network) {
			continue
		}
		history := g.history.LoadURLTestHistory(RealTag(detour, network))
		if history == nil {
			continue
		}
		if minDelay == 0 || minDelay > history.Delay+g.tolerance {
			minDelay = history.Delay
			minOutbound = detour
		}
	}
	if minOutbound == nil {
		for _, detour := range g.outbounds {
			if !common.Contains(detour.Network(), network) {
				continue
			}
			return detour, false
		}
		return nil, false
	}
	return minOutbound, true
}

func (g *URLTestGroup) loopCheck(ticker *time.Ticker, closeChan <-chan struct{}) {
	if time.Since(g.lastActive.Load()) > g.interval {
		g.lastActive.Store(time.Now())
		g.CheckOutbounds(g.ctx, false)
	}
	for {
		select {
		case <-closeChan:
			return
		case <-ticker.C:
		}
		if time.Since(g.lastActive.Load()) > g.idleTimeout {
			g.access.Lock()
			if g.ticker == ticker {
				g.ticker.Stop()
				g.ticker = nil
				g.pause.UnregisterCallback(g.pauseCallback)
				g.pauseCallback = nil
			}
			g.access.Unlock()
			return
		}
		g.CheckOutbounds(g.ctx, false)
	}
}

func (g *URLTestGroup) CheckOutbounds(ctx context.Context, force bool) {
	_, _ = g.urlTest(ctx, force)
}

func (g *URLTestGroup) URLTest(ctx context.Context) (map[string]uint16, error) {
	return g.urlTest(ctx, true)
}

func (g *URLTestGroup) urlTest(ctx context.Context, force bool) (map[string]uint16, error) {
	result, _, err := g.urlTestRound(ctx, force)
	return result, err
}

func countLeafOutbounds(outboundManager adapter.OutboundManager, outbounds []adapter.Outbound, seen map[string]bool) int {
	total := 0
	for _, detour := range outbounds {
		if detour == nil {
			continue
		}
		tag := detour.Tag()
		if seen[tag] {
			continue
		}
		seen[tag] = true
		switch nested := detour.(type) {
		case *URLTest:
			if nested.group != nil {
				total += countLeafOutbounds(outboundManager, nested.group.outbounds, seen)
			}
		case adapter.OutboundGroup:
			members := common.FilterNotNil(common.Map(nested.All(), func(it string) adapter.Outbound {
				member, _ := outboundManager.Outbound(it)
				return member
			}))
			total += countLeafOutbounds(outboundManager, members, seen)
		default:
			total++
		}
	}
	return total
}

func (g *URLTestGroup) urlTestRound(ctx context.Context, force bool) (map[string]uint16, bool, error) {
	if g.managedTest {
		return make(map[string]uint16), false, nil
	}
	if unified, configured := urltest.UnifiedDelayPolicy(g.ctx); configured {
		ctx = urltest.WithUnifiedDelay(ctx, unified)
	}
	if g.checking.Swap(true) {
		return make(map[string]uint16), false, nil
	}
	defer g.checking.Store(false)
	startBoot := elapsedBootTime()
	result := URLTestOutbounds(ctx, g.outbound, g.history, g.logger, g.outbounds, g.link, g.interval, force)
	g.performUpdateCheck()
	endBoot := elapsedBootTime()
	durationMs := (endBoot - startBoot).Milliseconds()
	if durationMs < 0 {
		durationMs = 0
	}
	total := countLeafOutbounds(g.outbound, g.outbounds, make(map[string]bool))
	success := 0
	for _, delay := range result {
		if delay > 0 {
			success++
		}
	}
	if success > total {
		total = success
	}
	g.access.Lock()
	g.lastRoundAt.Store(int64(endBoot))
	g.roundStartMs.Store(startBoot.Milliseconds())
	g.roundDurationMs.Store(durationMs)
	g.roundSuccess.Store(int32(success))
	g.roundTotal.Store(int32(total))
	g.roundSeq.Add(1)
	g.access.Unlock()
	return result, true, nil
}

func (g *URLTestGroup) idleFor() time.Duration {
	last := g.lastActiveNs.Load()
	if last <= 0 {
		return 0
	}
	idle := elapsedBootTime() - time.Duration(last)
	if idle < 0 {
		return 0
	}
	return idle
}

func (g *URLTestGroup) dueIn() time.Duration {
	if g.checking.Load() {
		return g.interval
	}
	last := g.lastRoundAt.Load()
	if last <= 0 {
		return 0
	}
	elapsed := elapsedBootTime() - time.Duration(last)
	if elapsed >= g.interval {
		return 0
	}
	return g.interval - elapsed
}

func (g *URLTestGroup) urlTestIfDue(ctx context.Context, idleLimit time.Duration) bool {
	if g.managedTest {
		return false
	}
	if idleLimit > 0 && g.idleFor() > idleLimit {
		return false
	}
	if g.dueIn() > 0 {
		return false
	}
	_, ran, err := g.urlTestRound(ctx, true)
	return ran && err == nil
}

type urlTestResult struct {
	delay uint16
	err   error
}

type urlTestBatch struct {
	ctx      context.Context
	outbound adapter.OutboundManager
	history  *urltest.HistoryStorage
	logger   log.Logger
	batch    *batch.Batch[any]
	checked  map[string]bool
	groups   []adapter.OutboundGroup
	access   sync.Mutex
	result   map[string]uint16
}

func URLTestOutbounds(ctx context.Context, outboundManager adapter.OutboundManager, history *urltest.HistoryStorage, logger log.Logger, outbounds []adapter.Outbound, link string, interval time.Duration, force bool) map[string]uint16 {
	b, _ := batch.New(ctx, batch.WithConcurrencyNum[any](10))
	testBatch := &urlTestBatch{
		ctx:      ctx,
		outbound: outboundManager,
		history:  history,
		logger:   logger,
		batch:    b,
		checked:  make(map[string]bool),
		result:   make(map[string]uint16),
	}
	testBatch.test(outbounds, link, interval, force)
	b.Wait()
	for _, outboundGroup := range testBatch.groups {
		groupHistory := history.LoadURLTestHistory(RealTag(outboundGroup, N.NetworkTCP))
		if groupHistory != nil {
			testBatch.result[outboundGroup.Tag()] = groupHistory.Delay
		}
	}
	return testBatch.result
}

func (b *urlTestBatch) test(outbounds []adapter.Outbound, link string, interval time.Duration, force bool) {
	for _, detour := range outbounds {
		tag := detour.Tag()
		if b.checked[tag] {
			continue
		}
		switch nested := detour.(type) {
		case *URLTest:
			b.checked[tag] = true
			b.groups = append(b.groups, nested)
			b.batch.Go(tag, func() (any, error) {
				nestedResult, _ := nested.group.urlTest(b.ctx, force)
				b.access.Lock()
				maps.Copy(b.result, nestedResult)
				b.access.Unlock()
				return nil, nil
			})
		case adapter.OutboundGroup:
			b.checked[tag] = true
			b.groups = append(b.groups, nested)
			b.test(common.FilterNotNil(common.Map(nested.All(), func(it string) adapter.Outbound {
				member, _ := b.outbound.Outbound(it)
				return member
			})), link, interval, force)
		default:
			history := b.history.LoadURLTestHistory(tag)
			if !force && history != nil && time.Since(history.Time) < interval {
				continue
			}
			b.checked[tag] = true
			b.batch.Go(tag, func() (any, error) {
				testCtx, cancel := context.WithTimeout(b.ctx, C.TCPTimeout)
				defer cancel()
				testChan := make(chan urlTestResult, 1)
				go func() {
					delay, testErr := urltest.URLTest(testCtx, link, detour)
					testChan <- urlTestResult{delay, testErr}
				}()
				var testResult urlTestResult
				select {
				case testResult = <-testChan:
				case <-testCtx.Done():
					testResult.err = testCtx.Err()
				}
				if testResult.err != nil {
					if b.ctx.Err() != nil {
						return nil, nil
					}
					b.logger.Debug("outbound ", tag, " unavailable: ", testResult.err)
					b.history.DeleteURLTestHistory(tag)
				} else {
					b.logger.Debug("outbound ", tag, " available: ", testResult.delay, "ms")
					b.history.StoreURLTestHistory(tag, &adapter.URLTestHistory{
						Time:  time.Now(),
						Delay: testResult.delay,
					})
					b.access.Lock()
					b.result[tag] = testResult.delay
					b.access.Unlock()
				}
				return nil, nil
			})
		}
	}
}

func (g *URLTestGroup) performUpdateCheck() {
	if g.managedTest {
		return
	}
	g.updateAccess.Lock()
	defer g.updateAccess.Unlock()
	var (
		updated  bool
		selected bool
	)
	if outbound, exists := g.Select(N.NetworkTCP); outbound != nil && (g.selectedOutboundTCP.Load() == nil || (exists && outbound != g.selectedOutboundTCP.Load())) {
		if g.selectedOutboundTCP.Load() != nil {
			updated = true
		}
		g.selectedOutboundTCP.Store(outbound)
		selected = true
	}
	if outbound, exists := g.Select(N.NetworkUDP); outbound != nil && (g.selectedOutboundUDP.Load() == nil || (exists && outbound != g.selectedOutboundUDP.Load())) {
		if g.selectedOutboundUDP.Load() != nil {
			updated = true
		}
		g.selectedOutboundUDP.Store(outbound)
		selected = true
	}
	if updated {
		g.interruptGroup.Interrupt(g.interruptExternalConnections)
	}
	if selected {
		g.history.NotifyUpdated()
	}
}

// ApplyTestResults stores this round's delays for the given member tags (0 means
// failed) and makes the strictly fastest successful member the active choice,
// with no tolerance. TCP and UDP are chosen separately among members that
// support that network, so an UDP-less member never becomes the UDP choice.
// Members not named in results are ignored, and a network without any
// successful result keeps its current choice. It returns the TCP choice, or
// the UDP choice when TCP is unchanged and has no success, or "" if nothing
// succeeded.
func (g *URLTestGroup) ApplyTestResults(results map[string]uint16) string {
	return g.applyTestResults(results, 0)
}

func (g *URLTestGroup) ApplyAutoTestResults(results map[string]uint16) string {
	return g.applyTestResults(results, g.tolerance)
}

func (g *URLTestGroup) applyTestResults(results map[string]uint16, tolerance uint16) string {
	g.updateAccess.Lock()
	defer g.updateAccess.Unlock()
	now := time.Now()
	for _, detour := range g.outbounds {
		tag := RealTag(detour, N.NetworkTCP)
		if tag == "" {
			tag = detour.Tag()
		}
		delay, tested := results[detour.Tag()]
		if !tested {
			continue
		}
		if delay == 0 {
			g.history.DeleteURLTestHistory(tag)
		} else {
			g.history.StoreURLTestHistory(tag, &adapter.URLTestHistory{Time: now, Delay: delay})
		}
	}
	g.lastRoundAt.Store(int64(elapsedBootTime()))
	var chosen string
	var updated bool
	pick := func(network string, selected *common.TypedValue[adapter.Outbound]) {
		var best adapter.Outbound
		var bestDelay uint16
		for _, detour := range g.outbounds {
			if !common.Contains(detour.Network(), network) {
				continue
			}
			delay := results[detour.Tag()]
			if delay == 0 || (bestDelay != 0 && delay >= bestDelay) {
				continue
			}
			best, bestDelay = detour, delay
		}
		if best == nil {
			return
		}
		previous := selected.Load()
		if tolerance > 0 && previous != nil && common.Contains(previous.Network(), network) {
			if prevDelay := results[previous.Tag()]; prevDelay > 0 && uint32(prevDelay) <= uint32(bestDelay)+uint32(tolerance) {
				if chosen == "" || network == N.NetworkTCP {
					chosen = previous.Tag()
				}
				return
			}
		}
		if previous != nil && previous != best {
			updated = true
		}
		selected.Store(best)
		if chosen == "" || network == N.NetworkTCP {
			chosen = best.Tag()
		}
	}
	pick(N.NetworkTCP, &g.selectedOutboundTCP)
	pick(N.NetworkUDP, &g.selectedOutboundUDP)
	if updated {
		g.interruptGroup.Interrupt(g.interruptExternalConnections)
	}
	return chosen
}

// ApplyTestResults is called by libcore for a live group; see URLTestGroup.ApplyTestResults.
func (s *URLTest) ApplyTestResults(results map[string]uint16) string {
	if s.group == nil {
		return ""
	}
	return s.group.ApplyTestResults(results)
}

func (s *URLTest) ApplyAutoTestResults(results map[string]uint16) string {
	if s.group == nil {
		return ""
	}
	return s.group.ApplyAutoTestResults(results)
}

func (s *URLTest) TouchForTraffic() {
	if s.group != nil {
		s.group.Touch()
	}
}

func (s *URLTest) URLTestDueIn() time.Duration {
	if s.group == nil {
		return -1
	}
	return s.group.dueIn()
}

func (s *URLTest) URLTestIdleFor() time.Duration {
	if s.group == nil {
		return -1
	}
	return s.group.idleFor()
}

func (s *URLTest) URLTestInFlight() bool {
	return s.group != nil && s.group.checking.Load()
}

func (s *URLTest) URLTestIfDue(ctx context.Context, idleLimit time.Duration) bool {
	if s.group == nil {
		return false
	}
	if ctx == nil {
		ctx = s.ctx
	}
	return s.group.urlTestIfDue(ctx, idleLimit)
}

func (s *URLTest) URLTestRoundInfo() string {
	if s.group == nil {
		return ""
	}
	s.group.access.Lock()
	defer s.group.access.Unlock()
	seq := s.group.roundSeq.Load()
	if seq == 0 {
		return ""
	}
	return fmt.Sprintf(
		"%d,%d,%d,%d,%d",
		seq,
		s.group.roundStartMs.Load(),
		s.group.roundDurationMs.Load(),
		s.group.roundSuccess.Load(),
		s.group.roundTotal.Load(),
	)
}
