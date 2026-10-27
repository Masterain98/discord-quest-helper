package api

import (
	"bytes"
	"compress/gzip"
	"compress/zlib"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

// first reads a header by its exact, non-canonical name.
func first(req *http.Request, name string) string {
	if v := req.Header[name]; len(v) > 0 {
		return v[0]
	}
	return ""
}

func TestSetHumanVerification(t *testing.T) {
	tests := []struct {
		name      string
		token     string
		method    string
		wantToken string
		wantType  string
	}{
		{
			name: "empty token leaves the request untouched",
		},
		{
			name:      "token and method are replayed verbatim",
			token:     "abc123",
			method:    "captcha",
			wantToken: "abc123",
			wantType:  "captcha",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req, err := http.NewRequest(http.MethodPost, "https://example.invalid", http.NoBody)
			if err != nil {
				t.Fatal(err)
			}
			SetHumanVerification(req, tt.token, tt.method)

			if got := first(req, hvTokenHeader); got != tt.wantToken {
				t.Errorf("%s = %q, want %q", hvTokenHeader, got, tt.wantToken)
			}
			if got := first(req, hvTokenTypeHeader); got != tt.wantType {
				t.Errorf("%s = %q, want %q", hvTokenTypeHeader, got, tt.wantType)
			}
		})
	}
}

// TestNewRequestHeaders pins the header set to what the official client sends:
// aiohttp's defaults, with Content-Type only when there is a body.
func TestNewRequestHeaders(t *testing.T) {
	get, err := NewRequest(http.MethodGet, "https://example.invalid", nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	post, err := NewRequest(http.MethodPost, "https://example.invalid", Body{{Key: "a", Value: 1}}, &Session{UID: "uid", AccessToken: "tok"})
	if err != nil {
		t.Fatal(err)
	}

	for _, req := range []*http.Request{get, post} {
		if got := first(req, "Accept"); got != "*/*" {
			t.Errorf("Accept = %q", got)
		}
		if got := first(req, "Accept-Encoding"); got != "gzip, deflate" {
			t.Errorf("Accept-Encoding = %q", got)
		}
	}
	if got := first(get, "Content-Type"); got != "" {
		t.Errorf("bodyless request has Content-Type %q", got)
	}
	if got := first(get, "Authorization"); got != "" {
		t.Errorf("unauthenticated request has Authorization %q", got)
	}
	if got := first(post, "Content-Type"); got != "application/json" {
		t.Errorf("Content-Type = %q", got)
	}
	if first(post, "Authorization") != "Bearer tok" || first(post, "x-pm-uid") != "uid" {
		t.Errorf("credentials not set: %v", post.Header)
	}
}

// TestDoDecodesCompressedBodies covers the consequence of setting
// Accept-Encoding by hand: net/http no longer decompresses for us.
func TestDoDecodesCompressedBodies(t *testing.T) {
	for _, enc := range []string{encodingGzip, encodingDeflate, ""} {
		t.Run("encoding="+enc, func(t *testing.T) {
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				var buf bytes.Buffer
				var wc io.WriteCloser
				switch enc {
				case encodingGzip:
					wc = gzip.NewWriter(&buf)
				case encodingDeflate:
					wc = zlib.NewWriter(&buf)
				}
				if wc == nil {
					buf.WriteString(`{"Code":1000}`)
				} else {
					_, _ = wc.Write([]byte(`{"Code":1000}`))
					_ = wc.Close()
					w.Header().Set("Content-Encoding", enc)
				}
				_, _ = w.Write(buf.Bytes())
			}))
			defer srv.Close()

			req, err := NewRequest(http.MethodGet, srv.URL, nil, nil)
			if err != nil {
				t.Fatal(err)
			}
			var out struct{ Code int }
			if err := Do(NewHTTPClient(5*time.Second), req, &out); err != nil {
				t.Fatal(err)
			}
			if out.Code != 1000 {
				t.Errorf("Code = %d, want 1000", out.Code)
			}
		})
	}
}
