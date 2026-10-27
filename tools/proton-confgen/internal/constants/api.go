// Package constants defines constants used throughout the application.
package constants

import "runtime"

// API endpoints
// Paths match the ProtonVPN Linux reference client (python-proton-core/python-proton-vpn-api-core).
const (
	DefaultAPIURL   = "https://vpn-api.proton.me"
	PingPath        = "/tests/ping"
	AuthInfoPath    = "/auth/info"
	AuthPath        = "/auth"
	TwoFAPath       = "/auth/2fa"
	RefreshPath     = "/auth/refresh"
	CertificatePath = "/vpn/v1/certificate"
	LogicalsPath    = "/vpn/v1/logicals"
	// LogicalsQuery is the query the official client always lists servers with.
	LogicalsQuery     = "?SecureCoreFilter=all&WithState=true"
	LocationPath      = "/vpn/v1/location"
	VPNInfoPath       = "/vpn/v2"
	ClientConfigPath  = "/vpn/v2/clientconfig"
	FeatureFlagsPath  = "/feature/v2/frontend"
	NotificationsPath = "/core/v4/notifications"
	// LoginCertDuration is what the official client requests for the session
	// certificate it fetches at login: 7 days, its refresh interval.
	LoginCertDuration = "10080 min"
	// CaptchaPath serves the human verification widget for this API entry point.
	CaptchaPath = "/core/v4/captcha"
)

// ClientVersion is the version of python-proton-vpn-api-core being imitated.
// Override at build time:
// go build -ldflags "-X .../internal/constants.ClientVersion=X.Y.Z"
//
// The official Linux client stamps its headers with this library's version,
// not the GTK app's, see SessionHolder in proton/vpn/core/session_holder.py.
var ClientVersion = "5.8.7"

// AppVersion returns the x-pm-appversion value the official Linux client sends:
// linux-vpn-gui@<api-core version>+<cpu architecture>. The architecture rides
// in the semver build metadata, formatted as Python's platform.machine() with
// underscores turned into hyphens.
func AppVersion() string {
	arch := runtime.GOARCH
	switch arch {
	case "amd64":
		arch = "x86-64"
	case "arm64":
		arch = "aarch64"
	case "arm":
		arch = "armv7l"
	}
	return "linux-vpn-gui@" + ClientVersion + "+" + arch
}

// UserAgent returns the User-Agent the official Linux client sends, which
// names the distribution as "<id>/<version>".
func UserAgent() string {
	return "ProtonVPN/" + ClientVersion + " (Linux; ubuntu/24.04)"
}

// API response codes
// Reference: proton-python-client/proton/api.py checks for codes 1000 and 1001
const (
	APICodeSuccess     = 1000
	APICodeMultiStatus = 1001 // Also indicates success in some contexts
)

// IsSuccessCode checks if an API response code indicates success
func IsSuccessCode(code int) bool {
	return code == APICodeSuccess || code == APICodeMultiStatus
}

// Server/feature status values
const (
	StatusOnline = 1
	EnabledTrue  = 1
)
