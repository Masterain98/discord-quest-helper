package constants

import (
	"regexp"
	"testing"
)

// The official client sends linux-vpn-gui@<semver>+<arch> and names the distro
// as id/version. Proton keys client recognition on these, so pin the shape.
func TestClientHeaders(t *testing.T) {
	if got := AppVersion(); !regexp.MustCompile(`^linux-vpn-gui@\d+\.\d+\.\d+\+[a-zA-Z0-9-]+$`).MatchString(got) {
		t.Errorf("AppVersion() = %q", got)
	}
	if got, want := UserAgent(), "ProtonVPN/"+ClientVersion+" (Linux; ubuntu/24.04)"; got != want {
		t.Errorf("UserAgent() = %q, want %q", got, want)
	}
}
