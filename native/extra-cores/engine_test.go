package engines

import (
	"bytes"
	"encoding/json"
	"net"
	"strings"
	"testing"
)

func TestVersionStrings(t *testing.T) {
	if got := Version("xray"); got != "26.11.9" {
		t.Fatalf("unexpected xray version: %q", got)
	}
	if got := Version("mihomo"); got == "" {
		t.Fatal("expected non-empty mihomo version")
	}
	if got := Version("unknown"); got != "" {
		t.Fatalf("expected empty unknown version, got %q", got)
	}
}

func TestValidateXrayConfig(t *testing.T) {
	valid := `{"dnsServer":"127.0.0.1:1053","coreConfig":{"log":{"loglevel":"warning"},"inbounds":[{"tag":"socks-in","listen":"127.0.0.1","port":10808,"protocol":"socks","settings":{"udp":true,"ip":"127.0.0.1"}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}}`
	if err := Validate("xray", valid); err != nil {
		t.Fatalf("valid xray config rejected: %v", err)
	}

	nonLoopback := `{"dnsServer":"127.0.0.1:1053","coreConfig":{"inbounds":[{"tag":"socks-in","listen":"0.0.0.0","port":10808,"protocol":"socks","settings":{"udp":true}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}}`
	if err := Validate("xray", nonLoopback); err == nil {
		t.Fatal("non-loopback xray listener should fail validation")
	}

	noUDP := `{"dnsServer":"127.0.0.1:1053","coreConfig":{"inbounds":[{"tag":"socks-in","listen":"127.0.0.1","port":10808,"protocol":"socks","settings":{"udp":false}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}}`
	if err := Validate("xray", noUDP); err == nil {
		t.Fatal("xray listener without UDP should fail validation")
	}
}

func TestValidateMihomoConfig(t *testing.T) {
	valid := `{"dnsServer":"127.0.0.1:1053","coreConfig":{"mode":"global","log-level":"warning","listeners":[{"name":"socks-in","type":"socks","port":10809,"listen":"127.0.0.1","udp":true,"proxy":"DIRECT"}]}}`
	if err := Validate("mihomo", valid); err != nil {
		t.Fatalf("valid mihomo config rejected: %v", err)
	}

	withDNS := `{"dnsServer":"127.0.0.1:1053","coreConfig":{"mode":"global","dns":{"enable":true},"listeners":[{"name":"socks-in","type":"socks","port":10809,"listen":"127.0.0.1","udp":true,"proxy":"DIRECT"}]}}`
	if err := Validate("mihomo", withDNS); err == nil {
		t.Fatal("mihomo config with embedded DNS enabled should fail validation")
	}

	nonLoopback := `{"dnsServer":"127.0.0.1:1053","coreConfig":{"mode":"global","listeners":[{"name":"socks-in","type":"socks","port":10809,"listen":"0.0.0.0","udp":true,"proxy":"DIRECT"}]}}`
	if err := Validate("mihomo", nonLoopback); err == nil {
		t.Fatal("mihomo config with non-loopback listener should fail validation")
	}
}

func TestValidateEnvelopeRejectsBadInputs(t *testing.T) {
	if err := Validate("xray", `{"dnsServer":"8.8.8.8:53","coreConfig":{}}`); err == nil || !strings.Contains(err.Error(), "127.0.0.1") {
		t.Fatalf("expected loopback dns error, got %v", err)
	}
	if err := Validate("other", `{"dnsServer":"127.0.0.1:53","coreConfig":{}}`); err == nil || !strings.Contains(err.Error(), "unknown core") {
		t.Fatalf("expected unknown core error, got %v", err)
	}
}

func TestNormalizeAssociateRewritesUDPAssociate(t *testing.T) {
	c1, s1 := net.Pipe()
	c2, s2 := net.Pipe()
	defer c1.Close()
	defer s1.Close()
	defer c2.Close()
	defer s2.Close()

	// Client sends greeting, reads method reply, then sends UDP ASSOCIATE with non-zero IPv4 address.
	go func() {
		_, _ = c1.Write([]byte{0x05, 0x01, 0x00})
		reply := make([]byte, 2)
		_, _ = c1.Read(reply)
		_, _ = c1.Write([]byte{0x05, 0x03, 0x00, 0x01, 127, 0, 0, 1, 0xd4, 0x31})
		_ = c1.Close()
	}()

	done := make(chan error, 1)
	go func() {
		done <- normalizeAssociate(s1, c2)
	}()

	greeting := make([]byte, 3)
	if _, err := s2.Read(greeting); err != nil {
		t.Fatalf("read greeting failed: %v", err)
	}
	if !bytes.Equal(greeting, []byte{0x05, 0x01, 0x00}) {
		t.Fatalf("unexpected greeting: %x", greeting)
	}
	if _, err := s2.Write([]byte{0x05, 0x00}); err != nil {
		t.Fatalf("write method reply failed: %v", err)
	}

	req := make([]byte, 10)
	n := 0
	for n < len(req) {
		readN, err := s2.Read(req[n:])
		if err != nil {
			t.Fatalf("read rewritten request failed at %d: %v", n, err)
		}
		n += readN
	}
	if err := <-done; err != nil {
		t.Fatalf("normalizeAssociate failed: %v", err)
	}
	want := []byte{0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0}
	if !bytes.Equal(req, want) {
		t.Fatalf("unexpected rewritten bytes: got %x want %x", req, want)
	}
}

func TestPrepareSocksAdapterBindsAndRewritesPort(t *testing.T) {
	ln, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	pubPort := ln.Addr().(*net.TCPAddr).Port
	_ = ln.Close()

	rawCfg, err := json.Marshal(map[string]any{
		"inbounds": []map[string]any{
			{
				"tag":      "socks-in",
				"listen":   "127.0.0.1",
				"port":     pubPort,
				"protocol": "socks",
				"settings": map[string]any{"udp": true, "ip": "127.0.0.1"},
			},
		},
		"outbounds": []map[string]any{
			{"tag": "direct", "protocol": "freedom"},
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	rewritten, adapter, err := prepareSocksAdapter(rawCfg)
	if err != nil {
		t.Fatalf("prepareSocksAdapter failed: %v", err)
	}
	defer adapter.close()

	if len(adapter.bridges) != 1 {
		t.Fatalf("expected 1 bridge, got %d", len(adapter.bridges))
	}
	var parsed struct {
		Inbounds []struct {
			Port int `json:"port"`
		} `json:"inbounds"`
	}
	if err := json.Unmarshal(rewritten, &parsed); err != nil {
		t.Fatal(err)
	}
	if len(parsed.Inbounds) != 1 || parsed.Inbounds[0].Port == pubPort || parsed.Inbounds[0].Port <= 0 {
		t.Fatalf("expected rewritten internal port distinct from %d, got %+v", pubPort, parsed.Inbounds)
	}
}
