package api

import (
	"bufio"
	"bytes"
	_ "embed" // for the captured ClientHello
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode/utf16"

	utls "github.com/refraction-networking/utls"
)

// This file reproduces, byte for byte, what the official ProtonVPN Linux client
// puts on the wire. That client is python-proton-core on aiohttp and OpenSSL;
// the reference here is Ubuntu 24.04's stock packages (aiohttp 3.9.1, OpenSSL
// 3.0.13), matching the distribution named in the User-Agent. net/http cannot
// do this: it sorts headers, canonicalizes their names and has its own TLS
// handshake, so requests are written by hand over a uTLS connection.

// Field is one key of a JSON object body.
type Field struct {
	Key   string
	Value any
}

// Body is a JSON object whose keys keep their order. Python dicts are ordered
// and the official client's bodies go out in insertion order, which a Go map
// cannot express.
type Body []Field

// encode renders the body the way Python's json.dumps does by default: ", " and
// ": " separators and every non-ASCII character escaped.
func (b Body) encode() []byte {
	var buf bytes.Buffer
	writePyValue(&buf, b)
	return buf.Bytes()
}

func writePyValue(buf *bytes.Buffer, v any) {
	switch v := v.(type) {
	case Body:
		buf.WriteByte('{')
		for i, f := range v {
			if i > 0 {
				buf.WriteString(", ")
			}
			writePyString(buf, f.Key)
			buf.WriteString(": ")
			writePyValue(buf, f.Value)
		}
		buf.WriteByte('}')
	case string:
		writePyString(buf, v)
	case bool:
		buf.WriteString(strconv.FormatBool(v))
	case int:
		buf.WriteString(strconv.Itoa(v))
	case float64:
		// Numbers decoded from JSON; integral ones print as Python ints do.
		buf.WriteString(strconv.FormatFloat(v, 'f', -1, 64))
	default:
		// Not a type any request uses; fall back rather than drop the value.
		raw, _ := json.Marshal(v)
		buf.Write(raw)
	}
}

// writePyString escapes like json.dumps with ensure_ascii: only printable ASCII
// goes out literally, and astral characters become UTF-16 surrogate pairs.
func writePyString(buf *bytes.Buffer, s string) {
	buf.WriteByte('"')
	for _, r := range s {
		switch {
		case r == '"':
			buf.WriteString(`\"`)
		case r == '\\':
			buf.WriteString(`\\`)
		case r == '\n':
			buf.WriteString(`\n`)
		case r == '\r':
			buf.WriteString(`\r`)
		case r == '\t':
			buf.WriteString(`\t`)
		case r == '\b':
			buf.WriteString(`\b`)
		case r == '\f':
			buf.WriteString(`\f`)
		case r >= 0x20 && r <= 0x7e:
			buf.WriteRune(r)
		case r > 0xffff:
			hi, lo := utf16.EncodeRune(r)
			fmt.Fprintf(buf, `\u%04x\u%04x`, hi, lo)
		default:
			fmt.Fprintf(buf, `\u%04x`, r)
		}
	}
	buf.WriteByte('"')
}

// headerOrder is the order aiohttp emits headers in, after the Host line: the
// session's own headers, then per-request ones, then aiohttp's defaults, then
// the body headers. Names are sent exactly as spelled here.
var headerOrder = []string{
	"x-pm-appversion",
	"User-Agent",
	"x-pm-uid",
	"Authorization",
	timezoneHeader,
	netzoneHeader,
	modifiedSinceHeader,
	hvTokenHeader,
	hvTokenTypeHeader,
	"Accept",
	"Accept-Encoding",
	"Content-Length",
	"Content-Type",
}

// clientHello is a TLS ClientHello recorded from aiohttp 3.9.1 on OpenSSL
// 3.0.13 with ssl.create_default_context(), as python-proton-core configures it.
//
//go:embed clienthello_ubuntu2404.bin
var clientHello []byte

// wireTransport sends each request on its own connection, as the official
// client does: it opens a new aiohttp ClientSession per API call.
type wireTransport struct {
	// proxy picks the proxy for a request, nil for a direct connection.
	proxy func(*http.Request) (*url.URL, error)
}

// NewHTTPClient returns a client whose requests are indistinguishable on the
// wire from the official Linux client's. HTTP_PROXY and HTTPS_PROXY are
// honoured, which the official client does not do; someone who sets them needs
// them, and the API sees the same bytes either way.
func NewHTTPClient(timeout time.Duration) *http.Client {
	return &http.Client{Timeout: timeout, Transport: wireTransport{proxy: http.ProxyFromEnvironment}}
}

func (t wireTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	conn, requestURI, proxyAuth, err := t.open(req)
	if err != nil {
		return nil, err
	}

	if err := writeRequest(conn, req, requestURI, proxyAuth); err != nil {
		_ = conn.Close()
		return nil, err
	}

	resp, err := http.ReadResponse(bufio.NewReader(conn), req)
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	resp.Body = &connBody{ReadCloser: resp.Body, conn: conn}
	return resp, nil
}

// open returns a connection ready for the request to be written on, together
// with the request URI and proxy credentials to write it with.
func (t wireTransport) open(req *http.Request) (conn net.Conn, requestURI, proxyAuth string, err error) {
	secure := req.URL.Scheme == "https"

	var proxy *url.URL
	if t.proxy != nil {
		if proxy, err = t.proxy(req); err != nil {
			return nil, "", "", err
		}
	}
	if proxy != nil && proxy.Scheme != "http" {
		return nil, "", "", fmt.Errorf("unsupported proxy scheme %q, only http proxies are supported", proxy.Scheme)
	}

	target := hostPort(req.URL, secure)
	addr := target
	if proxy != nil {
		addr = hostPort(proxy, false)
	}

	var dialer net.Dialer
	if conn, err = dialer.DialContext(req.Context(), "tcp", addr); err != nil {
		return nil, "", "", err
	}
	if deadline, ok := req.Context().Deadline(); ok {
		_ = conn.SetDeadline(deadline)
	}

	// Through a proxy, HTTPS is tunnelled with CONNECT and looks the same to
	// the API. Plain HTTP is sent to the proxy with an absolute request URI.
	requestURI = req.URL.RequestURI()
	switch {
	case proxy != nil && secure:
		if err = connect(conn, target, basicAuth(proxy)); err != nil {
			_ = conn.Close()
			return nil, "", "", err
		}
	case proxy != nil:
		requestURI = req.URL.String()
		proxyAuth = basicAuth(proxy)
	}

	if secure {
		if conn, err = handshake(req, conn); err != nil {
			return nil, "", "", err
		}
	}
	return conn, requestURI, proxyAuth, nil
}

func hostPort(u *url.URL, secure bool) string {
	if u.Port() != "" {
		return u.Host
	}
	if secure {
		return net.JoinHostPort(u.Hostname(), "443")
	}
	return net.JoinHostPort(u.Hostname(), "80")
}

func basicAuth(proxy *url.URL) string {
	if proxy.User == nil {
		return ""
	}
	password, _ := proxy.User.Password()
	return "Basic " + base64.StdEncoding.EncodeToString([]byte(proxy.User.Username()+":"+password))
}

// connect opens a tunnel to target through an HTTP proxy.
func connect(conn net.Conn, target, proxyAuth string) error {
	request := "CONNECT " + target + " HTTP/1.1\r\nHost: " + target + "\r\n"
	if proxyAuth != "" {
		request += "Proxy-Authorization: " + proxyAuth + "\r\n"
	}
	if _, err := io.WriteString(conn, request+"\r\n"); err != nil {
		return err
	}
	// Read byte by byte up to the blank line: a buffered reader could swallow
	// the first bytes of the TLS handshake that follows.
	var head []byte
	one := make([]byte, 1)
	for !bytes.HasSuffix(head, []byte("\r\n\r\n")) {
		if _, err := conn.Read(one); err != nil {
			return fmt.Errorf("proxy closed the connection during CONNECT: %w", err)
		}
		if head = append(head, one[0]); len(head) > 8192 {
			return fmt.Errorf("proxy sent an oversized CONNECT response")
		}
	}
	status, _, _ := strings.Cut(string(head), "\r\n")
	if fields := strings.Fields(status); len(fields) < 2 || fields[1] != "200" {
		return fmt.Errorf("proxy refused CONNECT: %s", status)
	}
	return nil
}

// handshake performs TLS with the recorded OpenSSL ClientHello. Certificate
// verification is uTLS's normal one against the system roots.
func handshake(req *http.Request, conn net.Conn) (net.Conn, error) {
	// A spec is consumed by the connection it is applied to, so build one each time.
	spec, err := (&utls.Fingerprinter{AllowBluntMimicry: true}).FingerprintClientHello(clientHello)
	if err != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("failed to load TLS fingerprint: %w", err)
	}
	tlsConn := utls.UClient(conn, &utls.Config{ServerName: req.URL.Hostname()}, utls.HelloCustom)
	if err := tlsConn.ApplyPreset(spec); err != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("failed to apply TLS fingerprint: %w", err)
	}
	if err := tlsConn.HandshakeContext(req.Context()); err != nil {
		_ = conn.Close()
		return nil, err
	}
	return tlsConn, nil
}

func writeRequest(w io.Writer, req *http.Request, requestURI, proxyAuth string) error {
	var body []byte
	if req.Body != nil && req.Body != http.NoBody {
		var err error
		if body, err = io.ReadAll(req.Body); err != nil {
			return err
		}
	}

	var buf bytes.Buffer
	fmt.Fprintf(&buf, "%s %s HTTP/1.1\r\nHost: %s\r\n", req.Method, requestURI, req.URL.Host)
	if proxyAuth != "" {
		fmt.Fprintf(&buf, "Proxy-Authorization: %s\r\n", proxyAuth)
	}
	for _, name := range headerOrder {
		if name == "Content-Length" {
			if body != nil {
				fmt.Fprintf(&buf, "Content-Length: %d\r\n", len(body))
			}
			continue
		}
		if values := req.Header[name]; len(values) > 0 {
			fmt.Fprintf(&buf, "%s: %s\r\n", name, values[0])
		}
	}
	buf.WriteString("\r\n")
	buf.Write(body)

	_, err := w.Write(buf.Bytes())
	return err
}

// connBody closes the connection once the response body is done with.
type connBody struct {
	io.ReadCloser
	conn net.Conn
}

func (b *connBody) Close() error {
	err := b.ReadCloser.Close()
	if cerr := b.conn.Close(); err == nil {
		err = cerr
	}
	return err
}
