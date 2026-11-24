package engines

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"strconv"
	"sync"
	"time"
)

type socksBridge struct {
	listener net.Listener
	upstream string
}

type socksAdapter struct {
	bridges []socksBridge
	mu      sync.Mutex
	closed  bool
	conns   map[net.Conn]struct{}
	wg      sync.WaitGroup
}

func prepareSocksAdapter(raw json.RawMessage) ([]byte, *socksAdapter, error) {
	var root map[string]json.RawMessage
	if err := json.Unmarshal(raw, &root); err != nil {
		return nil, nil, err
	}
	var inbounds []map[string]json.RawMessage
	if err := json.Unmarshal(root["inbounds"], &inbounds); err != nil {
		return nil, nil, err
	}

	adapter := &socksAdapter{
		conns: make(map[net.Conn]struct{}),
	}
	cleanup := func() {
		adapter.close()
	}

	reserved := make([]net.Listener, 0, len(inbounds))
	releaseReserved := func() {
		for _, ln := range reserved {
			_ = ln.Close()
		}
	}

	for i := range inbounds {
		var port int
		if err := json.Unmarshal(inbounds[i]["port"], &port); err != nil {
			releaseReserved()
			cleanup()
			return nil, nil, err
		}
		extAddr := net.JoinHostPort("127.0.0.1", strconv.Itoa(port))
		extLn, err := net.Listen("tcp4", extAddr)
		if err != nil {
			releaseReserved()
			cleanup()
			return nil, nil, fmt.Errorf("bind SOCKS adapter: %w", err)
		}

		resLn, err := net.Listen("tcp4", "127.0.0.1:0")
		if err != nil {
			_ = extLn.Close()
			releaseReserved()
			cleanup()
			return nil, nil, fmt.Errorf("reserve Xray listener: %w", err)
		}
		reserved = append(reserved, resLn)
		intPort := resLn.Addr().(*net.TCPAddr).Port
		portJSON, err := json.Marshal(intPort)
		if err != nil {
			_ = extLn.Close()
			releaseReserved()
			cleanup()
			return nil, nil, err
		}
		inbounds[i]["port"] = portJSON
		adapter.bridges = append(adapter.bridges, socksBridge{
			listener: extLn,
			upstream: net.JoinHostPort("127.0.0.1", strconv.Itoa(intPort)),
		})
	}

	// Release reserved internal ports immediately before Xray binds them.
	releaseReserved()

	inboundsJSON, err := json.Marshal(inbounds)
	if err != nil {
		cleanup()
		return nil, nil, err
	}
	root["inbounds"] = inboundsJSON
	adapted, err := json.Marshal(root)
	if err != nil {
		cleanup()
		return nil, nil, err
	}
	return adapted, adapter, nil
}

func (a *socksAdapter) start() {
	for _, b := range a.bridges {
		a.wg.Add(1)
		go a.accept(b)
	}
}

func (a *socksAdapter) accept(b socksBridge) {
	defer a.wg.Done()
	for {
		client, err := b.listener.Accept()
		if err != nil {
			return
		}
		a.mu.Lock()
		if a.closed {
			a.mu.Unlock()
			_ = client.Close()
			return
		}
		a.conns[client] = struct{}{}
		a.wg.Add(1)
		a.mu.Unlock()
		go a.handle(client, b.upstream)
	}
}

func (a *socksAdapter) forget(c net.Conn) {
	a.mu.Lock()
	delete(a.conns, c)
	a.mu.Unlock()
}

func (a *socksAdapter) close() {
	a.mu.Lock()
	if a.closed {
		a.mu.Unlock()
		return
	}
	a.closed = true
	for _, b := range a.bridges {
		_ = b.listener.Close()
	}
	for c := range a.conns {
		_ = c.Close()
	}
	a.mu.Unlock()
	a.wg.Wait()
}

func (a *socksAdapter) handle(client net.Conn, upstreamAddr string) {
	defer a.wg.Done()
	defer func() {
		a.forget(client)
		_ = client.Close()
	}()

	upstream, err := net.DialTimeout("tcp4", upstreamAddr, 5*time.Second)
	if err != nil {
		return
	}
	a.mu.Lock()
	if a.closed {
		a.mu.Unlock()
		_ = upstream.Close()
		return
	}
	a.conns[upstream] = struct{}{}
	a.mu.Unlock()
	defer func() {
		a.forget(upstream)
		_ = upstream.Close()
	}()

	deadline := time.Now().Add(10 * time.Second)
	_ = client.SetDeadline(deadline)
	_ = upstream.SetDeadline(deadline)
	if err := normalizeAssociate(client, upstream); err != nil {
		return
	}
	_ = client.SetDeadline(time.Time{})
	_ = upstream.SetDeadline(time.Time{})

	done := make(chan struct{})
	go func() {
		defer close(done)
		_, _ = io.Copy(client, upstream)
		if tc, ok := client.(*net.TCPConn); ok {
			_ = tc.CloseWrite()
		}
	}()
	_, _ = io.Copy(upstream, client)
	if tc, ok := upstream.(*net.TCPConn); ok {
		_ = tc.CloseWrite()
	}
	<-done
}

// normalizeAssociate proxies the SOCKS5 handshake to Xray and rewrites UDP ASSOCIATE
// requests (CMD=0x03) to ATYP=0x01 with 0.0.0.0:0 so Xray accepts sing-box UDP associations.
func normalizeAssociate(client, upstream net.Conn) error {
	header := make([]byte, 2)
	if _, err := io.ReadFull(client, header); err != nil {
		return err
	}
	if header[0] != 0x05 {
		return errors.New("SOCKS5 required")
	}
	methods := make([]byte, int(header[1]))
	if _, err := io.ReadFull(client, methods); err != nil {
		return err
	}
	greeting := append(header, methods...)
	if _, err := upstream.Write(greeting); err != nil {
		return err
	}

	reply := make([]byte, 2)
	if _, err := io.ReadFull(upstream, reply); err != nil {
		return err
	}
	if _, err := client.Write(reply); err != nil {
		return err
	}
	switch reply[1] {
	case 0x00:
		// No authentication
	case 0x02:
		// RFC 1929 username/password subnegotiation
		authHdr := make([]byte, 2)
		if _, err := io.ReadFull(client, authHdr); err != nil {
			return err
		}
		user := make([]byte, int(authHdr[1]))
		if _, err := io.ReadFull(client, user); err != nil {
			return err
		}
		plenBuf := make([]byte, 1)
		if _, err := io.ReadFull(client, plenBuf); err != nil {
			return err
		}
		pass := make([]byte, int(plenBuf[0]))
		if _, err := io.ReadFull(client, pass); err != nil {
			return err
		}
		authReq := make([]byte, 0, 3+len(user)+len(pass))
		authReq = append(authReq, authHdr...)
		authReq = append(authReq, user...)
		authReq = append(authReq, plenBuf[0])
		authReq = append(authReq, pass...)
		if _, err := upstream.Write(authReq); err != nil {
			return err
		}
		authResp := make([]byte, 2)
		if _, err := io.ReadFull(upstream, authResp); err != nil {
			return err
		}
		if _, err := client.Write(authResp); err != nil {
			return err
		}
		if authResp[1] != 0x00 {
			return errors.New("SOCKS authentication failed")
		}
	default:
		return errors.New("unsupported SOCKS authentication method")
	}

	reqHdr := make([]byte, 4)
	if _, err := io.ReadFull(client, reqHdr); err != nil {
		return err
	}
	var addrAndPort []byte
	switch reqHdr[3] {
	case 0x01:
		addrAndPort = make([]byte, 4+2)
		if _, err := io.ReadFull(client, addrAndPort); err != nil {
			return err
		}
	case 0x03:
		lenBuf := make([]byte, 1)
		if _, err := io.ReadFull(client, lenBuf); err != nil {
			return err
		}
		body := make([]byte, int(lenBuf[0])+2)
		if _, err := io.ReadFull(client, body); err != nil {
			return err
		}
		addrAndPort = append(lenBuf, body...)
	case 0x04:
		addrAndPort = make([]byte, 16+2)
		if _, err := io.ReadFull(client, addrAndPort); err != nil {
			return err
		}
	default:
		return errors.New("invalid SOCKS address type")
	}

	if reqHdr[1] == 0x03 {
		reqHdr[3] = 0x01
		addrAndPort = make([]byte, 6)
	}
	req := append(reqHdr, addrAndPort...)
	_, err := upstream.Write(req)
	return err
}
