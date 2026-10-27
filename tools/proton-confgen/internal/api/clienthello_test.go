package api

import (
	"encoding/binary"
	"fmt"
	"net"
	"net/http"
	"reflect"
	"testing"
	"time"
)

// helloShape is everything in a ClientHello that identifies the TLS stack. The
// random, session ID and key share contents are fresh on every handshake and
// are left out; their sizes are kept.
type helloShape struct {
	RecordLen     int
	Version       uint16
	SessionIDLen  int
	CipherSuites  []uint16
	Compression   []byte
	Extensions    []uint16
	Groups        []uint16
	PointFormats  []byte
	SigAlgs       []uint16
	ALPN          []byte
	Versions      []uint16
	KeyShares     []uint16
	KeyShareSizes []int
}

func u16s(b []byte) []uint16 {
	out := make([]uint16, 0, len(b)/2)
	for i := 0; i+1 < len(b); i += 2 {
		out = append(out, binary.BigEndian.Uint16(b[i:]))
	}
	return out
}

func parseHello(rec []byte) (shape helloShape, err error) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("malformed ClientHello: %v", r)
		}
	}()

	shape.RecordLen = len(rec)
	p := 5 + 4 // record header, handshake header
	shape.Version = binary.BigEndian.Uint16(rec[p:])
	p += 2 + 32 // version, random
	shape.SessionIDLen = int(rec[p])
	p += 1 + shape.SessionIDLen
	n := int(binary.BigEndian.Uint16(rec[p:]))
	shape.CipherSuites = u16s(rec[p+2 : p+2+n])
	p += 2 + n
	n = int(rec[p])
	shape.Compression = rec[p+1 : p+1+n]
	p += 1 + n
	end := p + 2 + int(binary.BigEndian.Uint16(rec[p:]))
	p += 2

	for p < end {
		typ := binary.BigEndian.Uint16(rec[p:])
		size := int(binary.BigEndian.Uint16(rec[p+2:]))
		data := rec[p+4 : p+4+size]
		p += 4 + size
		shape.Extensions = append(shape.Extensions, typ)

		switch typ {
		case 10:
			shape.Groups = u16s(data[2:])
		case 11:
			shape.PointFormats = data[1:]
		case 13:
			shape.SigAlgs = u16s(data[2:])
		case 16:
			shape.ALPN = data[2:]
		case 43:
			shape.Versions = u16s(data[1:])
		case 51:
			for q := 2; q < len(data); {
				keyLen := int(binary.BigEndian.Uint16(data[q+2:]))
				shape.KeyShares = append(shape.KeyShares, binary.BigEndian.Uint16(data[q:]))
				shape.KeyShareSizes = append(shape.KeyShareSizes, keyLen)
				q += 4 + keyLen
			}
		}
	}
	return shape, nil
}

// TestClientHelloMatchesOfficialClient sends a real handshake from the wire
// transport and compares it with the ClientHello recorded from the official
// client's production TLS path (test/parity/official.py).
func TestClientHelloMatchesOfficialClient(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = listener.Close() }()

	captured := make(chan []byte, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			return
		}
		defer func() { _ = conn.Close() }()
		buf := make([]byte, 65536)
		n, _ := conn.Read(buf)
		captured <- buf[:n]
	}()

	// "localhost" keeps the SNI the same length as in the recording, which is
	// what the padding extension, and so the record length, depends on.
	_, port, _ := net.SplitHostPort(listener.Addr().String())
	req, err := NewRequest(http.MethodGet, "https://localhost:"+port+"/tests/ping", nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	// The handshake cannot complete against a plain socket; only the hello matters.
	if resp, err := NewHTTPClient(2 * time.Second).Do(req); err == nil {
		_ = resp.Body.Close()
	}

	var raw []byte
	select {
	case raw = <-captured:
	case <-time.After(5 * time.Second):
		t.Fatal("no ClientHello received")
	}

	got, err := parseHello(raw)
	if err != nil {
		t.Fatal(err)
	}
	want, err := parseHello(clientHello)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("ClientHello differs from the official client\n got: %+v\nwant: %+v", got, want)
	}
}
