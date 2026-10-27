package api

import (
	"bytes"
	"compress/gzip"
	"compress/zlib"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"path/filepath"
	"strings"

	"protonvpn-wg-confgen/internal/constants"
)

// localTimezone returns the IANA name of the system timezone, e.g.
// "Europe/Zurich", or "" when it cannot be resolved. It mirrors the official
// client's get_local_timezone: the name is read from where /etc/localtime
// points, and nothing is guessed, since a wrong zone is worse than none.
func localTimezone() string {
	path, err := filepath.EvalSymlinks("/etc/localtime")
	if err != nil {
		return ""
	}
	_, name, found := strings.Cut(filepath.ToSlash(path), "zoneinfo/")
	if !found {
		return ""
	}
	// tzdata ships copies of the database under posix/ and right/.
	return strings.TrimPrefix(strings.TrimPrefix(name, "posix/"), "right/")
}

const (
	timezoneHeader      = "x-pm-timezone"
	netzoneHeader       = "X-PM-netzone"
	modifiedSinceHeader = "If-Modified-Since"

	// Content encodings aiohttp advertises without optional codecs installed.
	encodingGzip    = "gzip"
	encodingDeflate = "deflate"
)

// setRaw sets a header under its exact name. Header.Set would canonicalize it,
// and the wire transport looks headers up by the spelling it sends.
func setRaw(req *http.Request, name, value string) {
	req.Header[name] = []string{value}
}

// NewCoreRequest builds a request the way python-proton-core issues its own
// internal calls, the transport probe and the token refresh: without the client
// description headers that the VPN session layer adds to everything else.
func NewCoreRequest(method, url string, body Body, session *Session) (*http.Request, error) {
	req, err := NewRequest(method, url, body, session)
	if err != nil {
		return nil, err
	}
	delete(req.Header, timezoneHeader)
	return req, nil
}

// NewRequest builds a Proton API request with the headers every endpoint expects.
// A nil body sends no payload. A nil session omits the credentials, which is
// what the pre-authentication endpoints need.
func NewRequest(method, url string, body Body, session *Session) (*http.Request, error) {
	var payload io.Reader = http.NoBody
	if body != nil {
		payload = bytes.NewReader(body.encode())
	}

	req, err := http.NewRequest(method, url, payload)
	if err != nil {
		return nil, err
	}

	// The set and spelling follow the official Linux client; wire.go fixes the
	// order. The client also sends x-pm-locale, but only with a non-English
	// catalog active.
	setRaw(req, "Accept", "*/*")
	setRaw(req, "Accept-Encoding", encodingGzip+", "+encodingDeflate)
	if body != nil {
		setRaw(req, "Content-Type", "application/json")
	}
	setRaw(req, "x-pm-appversion", constants.AppVersion())
	setRaw(req, "User-Agent", constants.UserAgent())
	if tz := localTimezone(); tz != "" {
		setRaw(req, timezoneHeader, tz)
	}

	if session != nil {
		setRaw(req, "Authorization", "Bearer "+session.AccessToken)
		setRaw(req, "x-pm-uid", session.UID)
	}

	return req, nil
}

// Human verification headers, replayed after a code 9001 challenge has been
// solved. Names and semantics follow Proton's own client, see addHVToRequest in
// github.com/ProtonMail/go-proton-api.
const (
	//nolint:gosec // G101: header names, not credentials
	hvTokenHeader = "x-pm-human-verification-token"
	//nolint:gosec // G101: header names, not credentials
	hvTokenTypeHeader = "x-pm-human-verification-token-type"
)

// SetServerListHeaders adds what the official client sends when listing
// servers: the caller's network with the last octet zeroed, and a cache
// validator, which is the epoch when nothing is cached.
func SetServerListHeaders(req *http.Request, netzone string) {
	setRaw(req, netzoneHeader, netzone)
	setRaw(req, modifiedSinceHeader, "Thu, 01 Jan 1970 00:00:00 GMT")
}

// SetHumanVerification attaches a solved human verification token to a request.
// The token is the HumanVerificationToken handed back in the 9001 response, and
// method is the verification method used to satisfy it. Does nothing when the
// token is empty.
func SetHumanVerification(req *http.Request, token, method string) {
	if token == "" {
		return
	}
	setRaw(req, hvTokenHeader, token)
	setRaw(req, hvTokenTypeHeader, method)
}

// Do executes req and decodes the JSON response into out.
//
// The response is decoded regardless of status code: the API reports its own
// failures in the body (a Code plus an Error message), and those are far more
// useful than the HTTP status. Callers are expected to check the decoded Code.
// The status is only surfaced when the body is not JSON at all.
func Do(client *http.Client, req *http.Request, out any) error {
	// The request URL is built from the operator's own -api-url flag, so it is
	// not attacker-controlled input.
	resp, err := client.Do(req) //nolint:gosec // G704: URL is operator-supplied, not remote input
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()

	// Accept-Encoding is set explicitly, which turns off net/http's transparent
	// decompression, so undo the encoding here.
	var reader io.Reader = resp.Body
	switch resp.Header.Get("Content-Encoding") {
	case encodingGzip:
		if reader, err = gzip.NewReader(resp.Body); err != nil {
			return err
		}
	case encodingDeflate:
		if reader, err = zlib.NewReader(resp.Body); err != nil {
			return err
		}
	}

	body, err := io.ReadAll(reader)
	if err != nil {
		return err
	}

	if err := json.Unmarshal(body, out); err != nil {
		// Not JSON - a gateway error page or similar. Echo a snippet, otherwise
		// this surfaces as an opaque "invalid character '<'".
		snippet := string(body)
		if len(snippet) > 200 {
			snippet = snippet[:200] + "..."
		}
		return fmt.Errorf("unexpected response (HTTP %d): %s", resp.StatusCode, snippet)
	}
	return nil
}
