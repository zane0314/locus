package engines

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/netip"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"

	"github.com/metacubex/mihomo/component/dialer"
	"github.com/metacubex/mihomo/component/resolver"
	"github.com/metacubex/mihomo/config"
	mihomoConst "github.com/metacubex/mihomo/constant"
	"github.com/metacubex/mihomo/hub/executor"
	mihomoInbound "github.com/metacubex/mihomo/listener/inbound"
	"github.com/metacubex/mihomo/tunnel/statistic"
	miekgdns "github.com/miekg/dns"
	libxray "github.com/xtls/libxray/xray"
	xraycore "github.com/xtls/xray-core/core"
	_ "github.com/xtls/xray-core/main/distro/all"
	xrayinternet "github.com/xtls/xray-core/transport/internet"
)

type activeState struct {
	protect   func(int) bool
	dnsServer string
}

type envelope struct {
	DNSServer  string          `json:"dnsServer"`
	CoreConfig json.RawMessage `json:"coreConfig"`
}

var (
	initOnce sync.Once
	mu       sync.Mutex
	active   atomic.Pointer[activeState]
	dnsRes   *net.Resolver

	runningKind string
	used        bool
	socks       *socksAdapter
)

func initialize() {
	initOnce.Do(func() {
		r := &net.Resolver{
			PreferGo: true,
			Dial: func(ctx context.Context, network, _ string) (net.Conn, error) {
				st := active.Load()
				if st == nil || st.dnsServer == "" {
					return nil, errors.New("extra core stopped")
				}
				var d net.Dialer
				return d.DialContext(ctx, network, st.dnsServer)
			},
		}
		net.DefaultResolver = r
		dnsRes = r

		pr := protectedResolver{resolver: r}
		resolver.DefaultResolver = pr
		resolver.ProxyServerHostResolver = pr
		resolver.DirectHostResolver = pr
		resolver.DisableIPv6 = false

		dialer.DefaultSocketHook = control
		_ = xrayinternet.RegisterDialerController(control)
	})
}

func control(network, address string, c syscall.RawConn) error {
	st := active.Load()
	if st == nil {
		return errors.New("extra core stopped")
	}
	if address == st.dnsServer {
		return nil
	}
	var ok bool
	if err := c.Control(func(fd uintptr) {
		ok = st.protect(int(fd))
	}); err != nil {
		return err
	}
	if !ok {
		return errors.New("socket protection failed")
	}
	return nil
}

type protectedResolver struct {
	resolver *net.Resolver
}

func (p protectedResolver) lookup(ctx context.Context, network, host string) ([]netip.Addr, error) {
	if active.Load() == nil {
		return nil, errors.New("extra core stopped")
	}
	ips, err := p.resolver.LookupNetIP(ctx, network, host)
	if err != nil {
		return nil, err
	}
	out := make([]netip.Addr, 0, len(ips))
	for _, ip := range ips {
		out = append(out, ip.Unmap())
	}
	return out, nil
}

func (p protectedResolver) LookupIP(ctx context.Context, host string) ([]netip.Addr, error) {
	return p.lookup(ctx, "ip", host)
}

func (p protectedResolver) LookupIPv4(ctx context.Context, host string) ([]netip.Addr, error) {
	return p.lookup(ctx, "ip4", host)
}

func (p protectedResolver) LookupIPv6(ctx context.Context, host string) ([]netip.Addr, error) {
	return p.lookup(ctx, "ip6", host)
}

func (p protectedResolver) ResolveECH(ctx context.Context, host string) ([]byte, error) {
	return nil, errors.New("ECH lookup is not supported")
}

func (p protectedResolver) ExchangeContext(ctx context.Context, m *miekgdns.Msg) (*miekgdns.Msg, error) {
	st := active.Load()
	if st == nil || st.dnsServer == "" {
		return nil, errors.New("extra core stopped")
	}
	udpClient := &miekgdns.Client{Net: "udp"}
	resp, _, err := udpClient.ExchangeContext(ctx, m, st.dnsServer)
	if err != nil || (resp != nil && resp.Truncated) {
		tcpClient := &miekgdns.Client{Net: "tcp"}
		tcpResp, _, tcpErr := tcpClient.ExchangeContext(ctx, m, st.dnsServer)
		if tcpErr == nil {
			return tcpResp, nil
		}
		if err != nil {
			return nil, err
		}
	}
	return resp, nil
}

// Invalid reports whether this resolver is active (mihomo's Resolver.Invalid returns true when usable).
func (p protectedResolver) Invalid() bool { return true }

func (p protectedResolver) ClearCache() {}

func (p protectedResolver) ResetConnection() {}

func parse(raw string) (*envelope, error) {
	dec := json.NewDecoder(strings.NewReader(raw))
	dec.DisallowUnknownFields()
	var env envelope
	if err := dec.Decode(&env); err != nil {
		return nil, err
	}
	var trailing any
	if err := dec.Decode(&trailing); err != io.EOF {
		return nil, errors.New("unexpected trailing JSON")
	}
	host, portStr, err := net.SplitHostPort(env.DNSServer)
	if err != nil || host != "127.0.0.1" {
		return nil, errors.New("dnsServer must be 127.0.0.1:port")
	}
	port, err := strconv.Atoi(portStr)
	if err != nil || port < 1 || port > 65535 {
		return nil, errors.New("invalid dnsServer port")
	}
	trimmed := bytes.TrimSpace(env.CoreConfig)
	if len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null")) {
		return nil, errors.New("missing coreConfig")
	}
	env.CoreConfig = trimmed
	return &env, nil
}

func Version(kind string) string {
	switch kind {
	case "xray":
		return xraycore.Version()
	case "mihomo":
		if mihomoConst.Version != "" && mihomoConst.Version != "unknown version" {
			return mihomoConst.Version
		}
		return "v1.19.34"
	default:
		return ""
	}
}

func Validate(kind, rawConfig string) error {
	mu.Lock()
	defer mu.Unlock()
	initialize()
	if runningKind != "" {
		return errors.New("core already running")
	}
	env, err := parse(rawConfig)
	if err != nil {
		return err
	}
	switch kind {
	case "xray":
		if err := checkXray(env.CoreConfig); err != nil {
			return err
		}
		return libxray.TestXray(string(env.CoreConfig))
	case "mihomo":
		_, err := prepareMihomo(env.CoreConfig)
		return err
	default:
		return errors.New("unknown core")
	}
}

func Start(kind, rawConfig string, protect func(int) bool) error {
	mu.Lock()
	defer mu.Unlock()
	initialize()
	if protect == nil {
		return errors.New("protector is required")
	}
	if runningKind != "" {
		return errors.New("core already running")
	}
	if used {
		return errors.New("a fresh process is required after start or startup failure")
	}
	if kind != "xray" && kind != "mihomo" {
		return errors.New("unknown core")
	}
	env, err := parse(rawConfig)
	if err != nil {
		return err
	}
	used = true
	active.Store(&activeState{
		protect:   protect,
		dnsServer: env.DNSServer,
	})

	switch kind {
	case "xray":
		if err := checkXray(env.CoreConfig); err != nil {
			active.Store(nil)
			return err
		}
		adapted, adapter, err := prepareSocksAdapter(env.CoreConfig)
		if err != nil {
			active.Store(nil)
			return err
		}
		if err := libxray.RunXray(string(adapted)); err != nil {
			adapter.close()
			active.Store(nil)
			return err
		}
		adapter.start()
		socks = adapter
		runningKind = "xray"
		return nil
	case "mihomo":
		if err := startMihomo(env.CoreConfig); err != nil {
			active.Store(nil)
			return err
		}
		runningKind = "mihomo"
		return nil
	default:
		active.Store(nil)
		return errors.New("unknown core")
	}
}

func Stop() error {
	mu.Lock()
	defer mu.Unlock()

	var firstErr error
	if socks != nil {
		socks.close()
		socks = nil
	}
	kind := runningKind
	runningKind = ""

	switch kind {
	case "xray":
		if err := libxray.StopXray(); err != nil && firstErr == nil {
			firstErr = err
		}
	case "mihomo":
		executor.Shutdown()
		statistic.DefaultManager.Range(func(c statistic.Tracker) bool {
			_ = c.Close()
			return true
		})
	}
	active.Store(nil)
	return firstErr
}

type xrayInboundCheck struct {
	Listen   string `json:"listen"`
	Port     int    `json:"port"`
	Protocol string `json:"protocol"`
	Settings struct {
		UDP bool   `json:"udp"`
		IP  string `json:"ip"`
	} `json:"settings"`
}

type xrayConfigCheck struct {
	Inbounds []xrayInboundCheck `json:"inbounds"`
}

func isLoopbackHost(host string) bool {
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

func checkXray(raw json.RawMessage) error {
	var cfg xrayConfigCheck
	if err := json.Unmarshal(raw, &cfg); err != nil {
		return err
	}
	if len(cfg.Inbounds) == 0 {
		return errors.New("missing SOCKS listeners")
	}
	for _, in := range cfg.Inbounds {
		if in.Protocol != "socks" || !in.Settings.UDP || !isLoopbackHost(in.Listen) || in.Port < 1 || in.Port > 65535 {
			return errors.New("only loopback SOCKS listeners with UDP are supported")
		}
		if in.Settings.IP != "" && !isLoopbackHost(in.Settings.IP) {
			return errors.New("SOCKS UDP must bind loopback")
		}
	}
	return nil
}

func prepareMihomo(raw json.RawMessage) (*config.Config, error) {
	cfg, err := config.Parse(raw)
	if err != nil {
		return nil, err
	}
	if len(cfg.Listeners) == 0 {
		return nil, errors.New("missing SOCKS listeners")
	}
	if cfg.General != nil && (cfg.General.Port != 0 || cfg.General.SocksPort != 0 || cfg.General.RedirPort != 0 || cfg.General.TProxyPort != 0 || cfg.General.MixedPort != 0 || cfg.General.Tun.Enable) {
		return nil, errors.New("only explicit SOCKS listeners and parent DNS are supported")
	}
	if cfg.Controller != nil && (cfg.Controller.ExternalController != "" || cfg.Controller.ExternalControllerTLS != "" || cfg.Controller.ExternalControllerUnix != "") {
		return nil, errors.New("only explicit SOCKS listeners and parent DNS are supported")
	}
	if cfg.DNS != nil && cfg.DNS.Enable {
		return nil, errors.New("only explicit SOCKS listeners and parent DNS are supported")
	}
	for name, ln := range cfg.Listeners {
		socksLn, ok := ln.(*mihomoInbound.Socks)
		if !ok {
			return nil, errors.New("only SOCKS listeners with UDP are supported")
		}
		socksOpt, ok := socksLn.Config().(*mihomoInbound.SocksOption)
		if !ok || socksOpt == nil || !socksOpt.UDP {
			return nil, errors.New("only SOCKS listeners with UDP are supported")
		}
		rawAddr := ln.RawAddress()
		if rawAddr == "" {
			return nil, fmt.Errorf("listener %s must bind loopback", name)
		}
		for _, part := range strings.Split(rawAddr, ",") {
			host, portStr, err := net.SplitHostPort(strings.TrimSpace(part))
			if err != nil || !isLoopbackHost(host) {
				return nil, fmt.Errorf("listener %s must bind loopback", name)
			}
			port, err := strconv.Atoi(portStr)
			if err != nil || port < 1 || port > 65535 {
				return nil, fmt.Errorf("listener %s must bind loopback", name)
			}
		}
	}
	return cfg, nil
}

func startMihomo(raw json.RawMessage) error {
	cfg, err := prepareMihomo(raw)
	if err != nil {
		return err
	}
	executor.ApplyConfig(cfg, true)
	return nil
}
