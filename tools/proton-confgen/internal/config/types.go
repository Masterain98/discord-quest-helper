package config

// Config holds all configuration options
type Config struct {
	// Authentication
	Username      string
	Password      string
	PasswordStdin bool

	// Server selection
	Countries      []string
	ServerName     string
	P2PServersOnly bool
	SecureCoreOnly bool
	FreeOnly       bool

	// Output configuration
	OutputFile       string
	ClientPrivateKey string
	DeviceName       string

	// Network configuration
	DNSServers        []string
	AllowedIPs        []string
	EnableAccelerator bool
	EnableIPv6        bool
	PortForwarding    bool
	ModerateNAT       bool
	NetShield         int

	// Certificate configuration
	Duration string

	// Session management
	ClearSession    bool
	NoSession       bool
	ForceRefresh    bool
	SessionDuration string

	// Advanced configuration
	APIURL string
	Debug  bool

	// Management mode
	ListConfigs bool

	// List servers mode
	ListServers bool

	// JSON switches the listing modes to machine-readable output
	JSON bool

	// Renew certificate by serial number
	RenewSerial string

	// Revoke certificate by serial number
	RevokeSerial string

	// WithSessions makes the configuration listing include session certificates
	WithSessions bool

	// Non-persistent mode (do not register on account)
	NoSave bool

	// Human verification token replayed after solving a CAPTCHA out of band
	HVToken string

	// Explicit records which flags were given on the command line, so callers
	// can tell "--netshield 0" apart from the default.
	Explicit map[string]bool
}
