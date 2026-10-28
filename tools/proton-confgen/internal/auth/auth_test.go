package auth

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"protonvpn-wg-confgen/internal/api"
	"protonvpn-wg-confgen/internal/config"
	"protonvpn-wg-confgen/internal/constants"
)

const usernameField = "Username"

// TestSendAuthRequestHumanVerification checks that -hv-token reaches the /auth
// request as the headers Proton expects, and is absent when unset.
func TestSendAuthRequestHumanVerification(t *testing.T) {
	tests := []struct {
		name      string
		hvToken   string
		wantToken string
		wantType  string
	}{
		{name: "unset"},
		{name: "set", hvToken: "TOKEN-XYZ", wantToken: "TOKEN-XYZ", wantType: "captcha"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var gotToken, gotType string
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				gotToken = r.Header.Get("x-pm-human-verification-token")
				gotType = r.Header.Get("x-pm-human-verification-token-type")
				w.Header().Set("Content-Type", "application/json")
				_, _ = w.Write([]byte(`{"Code":1000}`))
			}))
			defer srv.Close()

			c := NewClient(&config.Config{APIURL: srv.URL, HVToken: tt.hvToken})
			if _, err := c.sendAuthRequest(api.Body{{Key: usernameField, Value: "u"}}); err != nil {
				t.Fatalf("sendAuthRequest: %v", err)
			}

			if gotToken != tt.wantToken {
				t.Errorf("token header = %q, want %q", gotToken, tt.wantToken)
			}
			if gotType != tt.wantType {
				t.Errorf("token type header = %q, want %q", gotType, tt.wantType)
			}
		})
	}
}

// TestCaptchaError checks that a 9001 response surfaces the token the user needs
// to solve and replay the challenge.
func TestCaptchaError(t *testing.T) {
	session := &api.Session{Code: 9001}
	session.Details.HumanVerificationMethods = []string{constants.HVMethodCaptcha}
	session.Details.HumanVerificationToken = "tok-123"

	msg := captchaError(session, "https://vpn-api.proton.me").Error()
	// The challenge token must appear in the widget URL, and the message must be
	// explicit that the replayed token is a different one.
	for _, want := range []string{
		"https://vpn-api.proton.me/core/v4/captcha?Token=tok-123",
		"--hv-token",
		"pm_captcha",
		"tok-123:<long-response>",
		"proton_captcha",
		constants.HVMethodCaptcha,
	} {
		if !strings.Contains(msg, want) {
			t.Errorf("captcha error missing %q:\n%s", want, msg)
		}
	}

	// With no token there is nothing to replay, so do not advertise the flag.
	bare := &api.Session{Code: 9001}
	if msg := captchaError(bare, "https://vpn-api.proton.me").Error(); strings.Contains(msg, "--hv-token") {
		t.Errorf("tokenless captcha error should not suggest --hv-token:\n%s", msg)
	}
}
