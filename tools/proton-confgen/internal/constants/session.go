package constants

// Session defaults
const (
	SessionFileName    = ".protonvpn-session.json"
	SessionFileMode    = 0o600 // Read/write for owner only
	SessionRefreshDays = 7     // Refresh when less than 7 days remain
)
