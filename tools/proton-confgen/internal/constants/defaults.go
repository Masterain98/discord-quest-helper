package constants

import "time"

// Certificate defaults
const (
	// MinCertDuration is the shortest duration the API accepts (code 2001 below this).
	MinCertDuration = 10 * time.Minute

	DefaultCertDuration = "365d"
	MaxCertDuration     = 365 // days
	CertMode            = "persistent"

	// HVMethodCaptcha is the human verification method -hv-token satisfies.
	// Proton's other methods (email, sms) hand back a code through a different
	// flow and are not replayable this way.
	HVMethodCaptcha = "captcha"
	// HVTokenEnv is the environment variable --hv-token falls back to.
	HVTokenEnv = "PROTONVPN_HV_TOKEN" //nolint:gosec // G101: a variable name, not a credential

	// Session (non-persistent) certificates are capped at 7 days by the API,
	// which silently clamps anything longer instead of returning an error.
	DefaultSessionCertDuration = "7d"
	MaxSessionCertDuration     = 7 // days
)

// Server selection defaults
const (
	DefaultP2POnly = true
)
