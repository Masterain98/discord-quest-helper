package api

import (
	"bytes"
	"net/http"
	"strings"
	"testing"

	"protonvpn-wg-confgen/internal/constants"
)

const usernameKey = "Username"

// The expected strings in this file are what aiohttp 3.9.1 on Ubuntu 24.04 put
// on the wire for the same calls, captured from a raw socket.

func TestBodyEncodesLikePython(t *testing.T) {
	tests := []struct {
		name string
		body Body
		want string
	}{
		{
			name: "auth",
			body: Body{{usernameKey, "t"}, {"ClientEphemeral", "AAA="}, {"ClientProof", "BBB/+="}, {"SRPSession", "abc"}},
			want: `{"Username": "t", "ClientEphemeral": "AAA=", "ClientProof": "BBB/+=", "SRPSession": "abc"}`,
		},
		{
			name: "certificate with nested object, newlines, bools and ints",
			body: Body{
				{"ClientPublicKey", "-----BEGIN PUBLIC KEY-----\nMCow\n-----END PUBLIC KEY-----\n"},
				{"Duration", "10080 min"},
				{"Features", Body{{"NetShieldLevel", 0}, {"RandomNAT", true}, {"PortForwarding", false}, {"SplitTCP", true}}},
			},
			want: `{"ClientPublicKey": "-----BEGIN PUBLIC KEY-----\nMCow\n-----END PUBLIC KEY-----\n", "Duration": "10080 min", "Features": {"NetShieldLevel": 0, "RandomNAT": true, "PortForwarding": false, "SplitTCP": true}}`,
		},
		{
			// json.dumps escapes everything outside printable ASCII, astral
			// characters as surrogate pairs, and leaves <, > and & alone.
			name: "non-ASCII and HTML characters",
			body: Body{{usernameKey, "\u00e9<&>\U0001F600\x7f"}},
			want: strings.ReplaceAll(`{"Username": "#u00e9<&>#ud83d#ude00#u007f"}`, "#", `\`),
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := string(tt.body.encode()); got != tt.want {
				t.Errorf("got  %s\nwant %s", got, tt.want)
			}
		})
	}
}

func TestWriteRequestMatchesAiohttp(t *testing.T) {
	tz := ""
	if name := localTimezone(); name != "" {
		tz = "x-pm-timezone: " + name + "\r\n"
	}
	common := "x-pm-appversion: " + constants.AppVersion() + "\r\n" +
		"User-Agent: " + constants.UserAgent() + "\r\n"

	tests := []struct {
		name    string
		method  string
		path    string
		body    Body
		session *Session
		want    string
	}{
		{
			name: "bodyless and unauthenticated", method: http.MethodGet, path: "/vpn/v2",
			want: "GET /vpn/v2 HTTP/1.1\r\nHost: 127.0.0.1:18101\r\n" + common + tz +
				"Accept: */*\r\nAccept-Encoding: gzip, deflate\r\n\r\n",
		},
		{
			name: "json body", method: http.MethodPost, path: "/auth/info", body: Body{{usernameKey, "t"}},
			want: "POST /auth/info HTTP/1.1\r\nHost: 127.0.0.1:18101\r\n" + common + tz +
				"Accept: */*\r\nAccept-Encoding: gzip, deflate\r\nContent-Length: 17\r\nContent-Type: application/json\r\n\r\n" +
				`{"Username": "t"}`,
		},
		{
			name: "authenticated", method: http.MethodGet, path: "/vpn/v1/logicals",
			session: &Session{UID: "UIDVALUE", AccessToken: "TOKENVALUE"},
			want: "GET /vpn/v1/logicals HTTP/1.1\r\nHost: 127.0.0.1:18101\r\n" + common +
				"x-pm-uid: UIDVALUE\r\nAuthorization: Bearer TOKENVALUE\r\n" + tz +
				"Accept: */*\r\nAccept-Encoding: gzip, deflate\r\n\r\n",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req, err := NewRequest(tt.method, "http://127.0.0.1:18101"+tt.path, tt.body, tt.session)
			if err != nil {
				t.Fatal(err)
			}
			var buf bytes.Buffer
			if err := writeRequest(&buf, req, req.URL.RequestURI(), ""); err != nil {
				t.Fatal(err)
			}
			if got := buf.String(); got != tt.want {
				t.Errorf("got:\n%q\nwant:\n%q", got, tt.want)
			}
		})
	}
}
