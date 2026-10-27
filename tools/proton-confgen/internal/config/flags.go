// Package config handles command-line argument parsing and configuration management.
package config

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"protonvpn-wg-confgen/internal/constants"
	"protonvpn-wg-confgen/internal/timeutil"
	"protonvpn-wg-confgen/internal/validation"
)

// Parse parses command-line flags and returns a Config
func Parse() (*Config, error) {
	cfg := &Config{}

	var countriesFlag string
	var dnsServersFlag string
	var allowedIPsFlag string

	// Set default DNS and allowed IPs based on IPv6 support
	defaultDNS := constants.DefaultDNSIPv4
	defaultAllowedIPs := constants.DefaultAllowedIPsIPv4

	// Authentication flags
	flag.StringVar(&cfg.Username, "username", "", "ProtonVPN username")
	flag.StringVar(&cfg.Password, "password", "", "ProtonVPN password (will prompt if not provided)")
	flag.BoolVar(&cfg.PasswordStdin, "password-stdin", false, "Read password from stdin through EOF (requires --username; incompatible with --password)")

	// Server selection flags
	flag.StringVar(&countriesFlag, "countries", "", "Comma-separated list of country codes (e.g., US,NL,CH)")
	flag.StringVar(&cfg.ServerName, "server", "", "Select a specific server by name (e.g., UA#122)")
	flag.BoolVar(&cfg.P2PServersOnly, "p2p-only", constants.DefaultP2POnly, "Use only P2P-enabled servers")
	flag.BoolVar(&cfg.SecureCoreOnly, "secure-core", false, "Use only Secure Core servers (multi-hop through privacy-friendly countries)")
	flag.BoolVar(&cfg.FreeOnly, "free-only", false, "Use only Free tier servers (tier 0)")

	// Output configuration
	flag.StringVar(&cfg.OutputFile, "output", "protonvpn.conf", "Output WireGuard configuration file")
	flag.StringVar(&cfg.DeviceName, "device-name", "", "Device name for WireGuard config (auto-generated if empty)")

	// Network configuration
	flag.BoolVar(&cfg.EnableIPv6, "ipv6", false, "Enable IPv6 support")
	flag.StringVar(&dnsServersFlag, "dns", "", "Comma-separated list of DNS servers (defaults based on IPv6 setting)")
	flag.StringVar(&allowedIPsFlag, "allowed-ips", "", "Comma-separated list of allowed IPs (defaults based on IPv6 setting)")
	flag.BoolVar(&cfg.EnableAccelerator, "accelerator", true, "Enable VPN accelerator")
	flag.BoolVar(&cfg.PortForwarding, "port-forwarding", false, "Enable NAT-PMP port forwarding (Plus tier, P2P servers only)")
	flag.BoolVar(&cfg.ModerateNAT, "moderate-nat", false, "Enable Moderate NAT (paid plans; incompatible with port forwarding)")
	flag.IntVar(&cfg.NetShield, "netshield", 0, "NetShield: 0 = off, 1 = block malware, 2 = block malware, ads and trackers")

	// Certificate configuration
	flag.StringVar(&cfg.Duration, "duration", constants.DefaultCertDuration, "Certificate duration (e.g., 30m, 24h, 7d, 1h30m). Min: 10m, max: 365d (7d with --no-save)")

	// Session management
	flag.BoolVar(&cfg.ClearSession, "clear-session", false, "Clear saved session and force re-authentication")
	flag.BoolVar(&cfg.NoSession, "no-session", false, "Don't save or use session persistence")
	flag.BoolVar(&cfg.ForceRefresh, "force-refresh", false, "Force session refresh even if not expired")
	flag.StringVar(&cfg.SessionDuration, "session-duration", "0", "Session cache duration (e.g., 12h, 24h, 7d). 0 = no expiration")

	// Human verification
	flag.StringVar(&cfg.HVToken, "hv-token", "", "Human verification token to replay after solving a CAPTCHA (see the code 9001 error); or set "+constants.HVTokenEnv)

	// Advanced configuration
	flag.StringVar(&cfg.APIURL, "api-url", constants.DefaultAPIURL, "ProtonVPN API URL")
	flag.BoolVar(&cfg.Debug, "debug", false, "Enable debug output")

	// Non-persistent mode
	flag.BoolVar(&cfg.NoSave, "no-save", false, "Generate config without registering on the account (session-only, max 7 days)")

	// List mode (enumerates persistent configurations registered on the account)
	flag.BoolVar(&cfg.ListConfigs, "list-configs", false, "List the persistent WireGuard configurations on the account and exit")
	flag.BoolVar(&cfg.WithSessions, "with-sessions", false, "With --list-configs: also list session certificates (those made with --no-save or by Proton apps)")
	flag.StringVar(&cfg.RevokeSerial, "revoke-serial", "", "Revoke a certificate by SerialNumber, persistent or session, and exit")

	// Server listing mode
	flag.BoolVar(&cfg.ListServers, "list-servers", false, "List available servers and exit (optionally filter by --countries)")
	flag.BoolVar(&cfg.JSON, "json", false, "Print --list-servers or --list-configs as JSON on stdout, for piping to jq")

	// Renew mode
	flag.StringVar(&cfg.RenewSerial, "renew-serial", "", "Renew a persistent configuration by SerialNumber (reuses existing key, no config file generated)")

	flag.Usage = PrintUsage
	flag.Parse()

	cfg.Explicit = map[string]bool{}
	flag.Visit(func(f *flag.Flag) { cfg.Explicit[f.Name] = true })

	// The token is long and pasted from a browser; the env var keeps it out of
	// shell history and ps. The flag wins when both are set.
	if cfg.HVToken == "" {
		cfg.HVToken = os.Getenv(constants.HVTokenEnv)
	}

	// Session certificates max out at 7 days, so fall back to that instead of
	// the 365d persistent default when --duration was not given explicitly.
	if cfg.NoSave && !cfg.Explicit["duration"] {
		cfg.Duration = constants.DefaultSessionCertDuration
	}

	if err := validateFeatureFlags(cfg); err != nil {
		return nil, err
	}

	countries, err := validatedCountries(countriesFlag)
	if err != nil {
		return nil, err
	}
	cfg.Countries = countries

	// The listing and renew modes take an optional country filter but never
	// require one, and need none of the network defaults below.
	if cfg.ListConfigs || cfg.ListServers || cfg.RenewSerial != "" || cfg.RevokeSerial != "" {
		cfg.Username = validation.CleanUsername(cfg.Username)
		return cfg, nil
	}

	// Validate required flags: countries are needed unless --server is specified
	if countriesFlag == "" && cfg.ServerName == "" {
		return nil, fmt.Errorf("countries flag is required (or use --server to select a specific server)")
	}

	// Set defaults based on IPv6 setting
	if cfg.EnableIPv6 {
		defaultDNS = fmt.Sprintf("%s,%s", constants.DefaultDNSIPv4, constants.DefaultDNSIPv6)
		defaultAllowedIPs = fmt.Sprintf("%s,%s", constants.DefaultAllowedIPsIPv4, constants.DefaultAllowedIPsIPv6)
	}

	// Use defaults if flags are empty
	if dnsServersFlag == "" {
		dnsServersFlag = defaultDNS
	}
	if allowedIPsFlag == "" {
		allowedIPsFlag = defaultAllowedIPs
	}

	// Parse lists (with space trimming)
	cfg.DNSServers = parseCommaSeparatedList(dnsServersFlag)
	cfg.AllowedIPs = parseCommaSeparatedList(allowedIPsFlag)

	// Clean up username
	cfg.Username = validation.CleanUsername(cfg.Username)

	return cfg, nil
}

func validateFeatureFlags(cfg *Config) error {
	if cfg.JSON && !cfg.ListServers && !cfg.ListConfigs {
		return fmt.Errorf("json requires list-servers or list-configs")
	}
	if cfg.WithSessions && !cfg.ListConfigs {
		return fmt.Errorf("with-sessions requires list-configs")
	}
	if cfg.RevokeSerial != "" && cfg.RenewSerial != "" {
		return fmt.Errorf("revoke-serial and renew-serial cannot be used together")
	}
	if cfg.NetShield < 0 || cfg.NetShield > 2 {
		return fmt.Errorf("netshield must be 0, 1, or 2")
	}
	if cfg.PasswordStdin {
		if cfg.Password != "" {
			return fmt.Errorf("password-stdin and password cannot be used together")
		}
		if validation.CleanUsername(cfg.Username) == "" {
			return fmt.Errorf("password-stdin requires username")
		}
	}
	if cfg.PortForwarding && cfg.ModerateNAT {
		return fmt.Errorf("port-forwarding and moderate-nat cannot be enabled together")
	}
	return validateDuration(cfg)
}

// validateDuration enforces the API's certificate duration bounds up front, so
// out-of-range values fail before authenticating rather than after.
func validateDuration(cfg *Config) error {
	duration, err := timeutil.ParseDuration(cfg.Duration)
	if err != nil {
		return fmt.Errorf("invalid duration format: %s", cfg.Duration)
	}
	// The API rejects shorter durations with code 2001.
	if duration < constants.MinCertDuration {
		return fmt.Errorf("duration must be at least 10 minutes")
	}
	if duration > constants.MaxCertDuration*24*time.Hour {
		return fmt.Errorf("duration cannot exceed %dd", constants.MaxCertDuration)
	}
	if cfg.NoSave && duration > constants.MaxSessionCertDuration*24*time.Hour {
		return fmt.Errorf("duration cannot exceed %dd with --no-save (the API silently clamps session certificates to %dd)",
			constants.MaxSessionCertDuration, constants.MaxSessionCertDuration)
	}
	return nil
}

// parseCommaSeparatedList parses a comma-separated string into a trimmed slice
func parseCommaSeparatedList(input string) []string {
	parts := strings.Split(input, ",")
	var result []string
	for _, part := range parts {
		part = strings.TrimSpace(part)
		if part != "" {
			result = append(result, part)
		}
	}
	return result
}

// validatedCountries parses a comma-separated country list and rejects
// anything that is not a two-letter code. Empty input yields nil.
func validatedCountries(raw string) ([]string, error) {
	if raw == "" {
		return nil, nil
	}
	countries := parseCountries(raw)
	for _, c := range countries {
		if !validation.IsValidCountryCode(c) {
			return nil, fmt.Errorf("invalid country code: %s", c)
		}
	}
	return countries, nil
}

// parseCountries parses and normalizes country codes (deduplicated)
func parseCountries(countriesFlag string) []string {
	raw := parseCommaSeparatedList(strings.ToUpper(countriesFlag))
	seen := make(map[string]struct{}, len(raw))
	result := make([]string, 0, len(raw))
	for _, c := range raw {
		if _, ok := seen[c]; !ok {
			seen[c] = struct{}{}
			result = append(result, c)
		}
	}
	return result
}

// flagGroups orders the help output. Every registered flag must appear in
// exactly one group; TestFlagGroupsCoverAllFlags enforces it so a new flag
// cannot silently vanish from --help.
var flagGroups = []struct {
	title string
	names []string
}{
	{"Modes", []string{"list-servers", "list-configs", "with-sessions", "json", "renew-serial", "revoke-serial"}},
	{"Authentication", []string{"username", "password", "password-stdin"}},
	{"Server selection", []string{"countries", "server", "p2p-only", "secure-core", "free-only", "debug"}},
	{"Output and network", []string{"output", "device-name", "ipv6", "dns", "allowed-ips", "accelerator", "port-forwarding", "moderate-nat", "netshield"}},
	{"Certificate and session", []string{"duration", "no-save", "session-duration", "clear-session", "no-session", "force-refresh", "hv-token", "api-url"}},
}

// PrintUsage prints the synopsis followed by every flag, grouped.
func PrintUsage() {
	bin := filepath.Base(os.Args[0])
	out := func(format string, a ...any) { _, _ = fmt.Fprintf(os.Stderr, format, a...) }

	out("Usage:\n")
	out("  %s --username <user> --countries <codes> [flags]\n", bin)
	out("  %s --username <user> --server <name> [flags]\n", bin)
	out("  %s --username <user> --list-servers [--countries <codes>]\n", bin)
	out("  %s --username <user> --list-configs\n", bin)
	out("  %s --username <user> --renew-serial <serial>\n", bin)
	out("  %s --username <user> --revoke-serial <serial>\n\n", bin)

	out("Flags take --name value or --name=value. Booleans are switches: --ipv6 turns\n")
	out("one on, --accelerator=false turns one off. A single dash (-name) also works.\n")

	// Width of the longest "--name type" column, for alignment across groups.
	width := 0
	flag.VisitAll(func(f *flag.Flag) {
		if n := len(flagLabel(f)); n > width {
			width = n
		}
	})

	for _, g := range flagGroups {
		out("\n%s:\n", g.title)
		for _, name := range g.names {
			f := flag.Lookup(name)
			if f == nil {
				continue
			}
			_, usage := flag.UnquoteUsage(f)
			out("  %-*s  %s%s\n", width, flagLabel(f), usage, defaultSuffix(f))
		}
	}
}

// flagLabel renders "--name" plus the value type for non-boolean flags.
func flagLabel(f *flag.Flag) string {
	typ, _ := flag.UnquoteUsage(f)
	if typ == "" {
		return "--" + f.Name
	}
	return "--" + f.Name + " " + typ
}

// defaultSuffix renders " (default X)" for flags whose zero value is not the
// default, mirroring flag.PrintDefaults.
func defaultSuffix(f *flag.Flag) string {
	if f.DefValue == "" || f.DefValue == "false" || f.DefValue == "0" {
		return ""
	}
	if _, isBool := f.Value.(interface{ IsBoolFlag() bool }); isBool {
		return fmt.Sprintf(" (default %s)", f.DefValue)
	}
	return fmt.Sprintf(" (default %q)", f.DefValue)
}
