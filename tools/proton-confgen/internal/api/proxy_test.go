package api

import (
	"bufio"
	"net"
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"
)

// proxyRecorder accepts one connection, hands the first request head it reads
// to the test, answers it, and then reports the first byte that follows.
func proxyRecorder(t *testing.T, reply string) (addr string, head chan string, next chan byte) {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	head, next = make(chan string, 1), make(chan byte, 1)

	go func() {
		conn, err := listener.Accept()
		if err != nil {
			return
		}
		defer func() { _ = conn.Close() }()
		reader := bufio.NewReader(conn)
		var lines []string
		for {
			line, err := reader.ReadString('\n')
			if err != nil || line == "\r\n" {
				break
			}
			lines = append(lines, strings.TrimRight(line, "\r\n"))
		}
		head <- strings.Join(lines, "\n")
		_, _ = conn.Write([]byte(reply))
		if b, err := reader.ReadByte(); err == nil {
			next <- b
		}
	}()
	return listener.Addr().String(), head, next
}

func proxiedClient(proxyURL string) *http.Client {
	u, _ := url.Parse(proxyURL)
	return &http.Client{Timeout: 3 * time.Second, Transport: wireTransport{
		proxy: func(*http.Request) (*url.URL, error) { return u, nil },
	}}
}

// HTTPS goes through a CONNECT tunnel, so the API still sees the same TLS
// handshake and request bytes as on a direct connection.
func TestProxyTunnelsHTTPS(t *testing.T) {
	addr, head, next := proxyRecorder(t, "HTTP/1.1 200 Connection established\r\n\r\n")

	req, err := NewRequest(http.MethodGet, "https://vpn-api.example/tests/ping", nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	// The proxy never completes the TLS handshake, so the call itself fails.
	if resp, err := proxiedClient("http://user:secret@" + addr).Do(req); err == nil {
		_ = resp.Body.Close()
	}

	want := "CONNECT vpn-api.example:443 HTTP/1.1\nHost: vpn-api.example:443\nProxy-Authorization: Basic dXNlcjpzZWNyZXQ="
	if got := <-head; got != want {
		t.Errorf("CONNECT request\n got: %q\nwant: %q", got, want)
	}
	select {
	case b := <-next:
		if b != 0x16 {
			t.Errorf("first byte after CONNECT = %#x, want a TLS handshake record (0x16)", b)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("nothing sent through the tunnel")
	}
}

func TestProxyRefusal(t *testing.T) {
	addr, _, _ := proxyRecorder(t, "HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
	req, err := NewRequest(http.MethodGet, "https://vpn-api.example/tests/ping", nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	resp, err := proxiedClient("http://" + addr).Do(req)
	if err == nil {
		_ = resp.Body.Close()
		t.Fatal("a refused CONNECT must fail the request")
	}
	if !strings.Contains(err.Error(), "407") {
		t.Errorf("error does not name the proxy's status: %v", err)
	}
}

func TestProxyRejectsUnsupportedScheme(t *testing.T) {
	req, err := NewRequest(http.MethodGet, "https://vpn-api.example/tests/ping", nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	resp, err := proxiedClient("socks5://127.0.0.1:1080").Do(req)
	if err == nil {
		_ = resp.Body.Close()
		t.Fatal("socks proxies are not supported and must not be silently bypassed")
	}
}
