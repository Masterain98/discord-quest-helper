// Package main provides the command-line interface for generating ProtonVPN WireGuard configurations.
package main

import (
	"cmp"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"slices"
	"strings"
	"time"

	"protonvpn-wg-confgen/internal/api"
	"protonvpn-wg-confgen/internal/auth"
	"protonvpn-wg-confgen/internal/config"
	"protonvpn-wg-confgen/internal/constants"
	"protonvpn-wg-confgen/internal/vpn"
	"protonvpn-wg-confgen/internal/wireguard"

	"github.com/ProtonVPN/go-vpn-lib/ed25519"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}
}

func run() error {
	cfg, err := config.Parse()
	if err != nil {
		config.PrintUsage()
		return err
	}

	// With --json, stdout carries the JSON document and nothing else. Status
	// lines and prompts all print through os.Stdout, so point that at stderr
	// and keep the real one for the document.
	stdout := os.Stdout
	if cfg.JSON {
		os.Stdout = os.Stderr
	}

	authClient := auth.NewClient(cfg)
	session, err := authClient.Authenticate()
	if err != nil {
		return fmt.Errorf("authentication failed: %w", err)
	}
	fmt.Println("Authentication successful!")

	vpnClient := vpn.NewClient(cfg, session)
	if authClient.FreshLogin {
		// The official client loads its session data right after signing in.
		vpnClient.SyncSession()
	}

	switch {
	case cfg.ListConfigs:
		return listConfigs(cfg, vpnClient, stdout)
	case cfg.ListServers:
		return listServers(cfg, vpnClient, stdout)
	case cfg.RenewSerial != "":
		return renewSerial(cfg, vpnClient)
	case cfg.RevokeSerial != "":
		if err := vpnClient.RevokeCertificate(cfg.RevokeSerial); err != nil {
			return fmt.Errorf("failed to revoke certificate: %w", err)
		}
		fmt.Printf("Certificate revoked: %s\n", cfg.RevokeSerial)
		return nil
	default:
		return generateConfig(cfg, vpnClient)
	}
}

func generateConfig(cfg *config.Config, vpnClient *vpn.Client) error {
	keyPair, err := ed25519.NewKeyPair()
	if err != nil {
		return fmt.Errorf("failed to generate key pair: %w", err)
	}
	cfg.ClientPrivateKey = keyPair.ToX25519Base64()

	vpnInfo, err := vpnClient.GetCertificate(keyPair)
	if err != nil {
		return fmt.Errorf("failed to get VPN certificate: %w", err)
	}

	servers, err := vpnClient.GetServers()
	if err != nil {
		return fmt.Errorf("failed to get servers: %w", err)
	}

	selector := vpn.NewServerSelector(cfg)
	server, err := selector.SelectBest(servers)
	if err != nil {
		return err
	}

	features := api.GetFeatureNames(server.Features)
	featureStr := ""
	if len(features) > 0 {
		featureStr = fmt.Sprintf(", Features: %s", strings.Join(features, ", "))
	}

	countryStr := server.ExitCountry
	if server.HostCountry != "" && server.HostCountry != server.ExitCountry {
		countryStr = fmt.Sprintf("%s (host: %s)", server.ExitCountry, server.HostCountry)
	}

	fmt.Printf("Selected server: %s (Country: %s, City: %s, Tier: %s, Load: %d%%, Score: %.2f, Servers: %d%s)\n",
		server.Name, countryStr, server.City, api.GetTierName(server.Tier),
		server.Load, server.Score, len(server.Servers), featureStr)

	physicalServer := vpn.GetBestPhysicalServer(server)
	if physicalServer == nil {
		return fmt.Errorf("no physical servers available")
	}

	generator := wireguard.NewConfigGenerator(cfg)
	if err := generator.Generate(server, physicalServer, cfg.ClientPrivateKey, vpnInfo); err != nil {
		return fmt.Errorf("failed to generate WireGuard config: %w", err)
	}

	fmt.Printf("WireGuard configuration written to: %s\n", cfg.OutputFile)
	if vpnInfo.DeviceName != "" {
		fmt.Printf("Device name: %s (visible in ProtonVPN dashboard)\n", vpnInfo.DeviceName)
	}
	mode := vpnInfo.Mode
	if mode == "" {
		mode = "session"
	}
	fmt.Printf("Certificate: %s, serial %s, expires %s\n",
		mode, vpnInfo.SerialNumber, time.Unix(vpnInfo.ExpirationTime, 0).UTC().Format("2006-01-02 15:04 UTC"))
	fmt.Printf("\nSuccessfully generated config for %s\n", server.ExitCountry)
	return nil
}

func listServers(cfg *config.Config, vpnClient *vpn.Client, stdout io.Writer) error {
	servers, err := vpnClient.GetServers()
	if err != nil {
		return fmt.Errorf("failed to get servers: %w", err)
	}

	filtered := vpn.EligibleServers(cfg, servers)

	if len(filtered) == 0 {
		if len(cfg.Countries) > 0 {
			return fmt.Errorf("no online servers found for countries: %v", cfg.Countries)
		}
		return fmt.Errorf("no online servers found")
	}

	slices.SortFunc(filtered, func(a, b api.LogicalServer) int {
		if c := cmp.Compare(a.ExitCountry, b.ExitCountry); c != 0 {
			return c
		}
		return cmp.Compare(a.Score, b.Score)
	})

	if cfg.JSON {
		return writeJSON(stdout, serversJSON(filtered))
	}

	fmt.Printf("%-7s  %-14s  %-18s  %5s  %6s  %-10s  %s\n",
		"Country", "Server", "City", "Load", "Score", "Tier", "Features")
	fmt.Println(strings.Repeat("-", 100))

	for i := range filtered {
		s := &filtered[i]
		features := api.GetFeatureNames(s.Features)
		featureStr := "-"
		if len(features) > 0 {
			featureStr = strings.Join(features, ", ")
		}

		serverName := s.Name
		if s.HostCountry != "" && s.HostCountry != s.ExitCountry {
			serverName = fmt.Sprintf("%s(%s)", s.Name, s.HostCountry)
		}
		fmt.Printf("%-7s  %-14s  %-18s  %3d%%  %6.2f  %-10s  %s\n",
			s.ExitCountry, serverName, s.City, s.Load, s.Score,
			api.GetTierName(s.Tier), featureStr)
	}

	// Count unique countries
	seen := map[string]struct{}{}
	for i := range filtered {
		seen[filtered[i].ExitCountry] = struct{}{}
	}
	fmt.Printf("\n%d servers found across %d countries.\n", len(filtered), len(seen))
	return nil
}

func renewSerial(cfg *config.Config, vpnClient *vpn.Client) error {
	certs, err := vpnClient.ListCertificates(false)
	if err != nil {
		return fmt.Errorf("failed to list certificates: %w", err)
	}

	var target *api.VPNCertificate
	for i := range certs {
		if certs[i].SerialNumber == cfg.RenewSerial {
			target = &certs[i]
			break
		}
	}

	if target == nil {
		return fmt.Errorf("certificate with SerialNumber %s not found (use --list-configs to see available certificates)", cfg.RenewSerial)
	}

	if target.ClientKey == "" {
		return fmt.Errorf("certificate %s has no public key data", cfg.RenewSerial)
	}

	deviceName := target.DeviceName
	if deviceName == "" {
		return fmt.Errorf("certificate %s has no device name", cfg.RenewSerial)
	}

	vpnInfo, err := vpnClient.RenewCertificate(target.ClientKey, deviceName, target.Features)
	if err != nil {
		return fmt.Errorf("failed to renew certificate: %w", err)
	}

	// Renewal issues a replacement certificate: the old serial disappears from
	// the account and any future renewal must use the new one.
	fmt.Printf("Certificate renewed: %s -> %s\n", cfg.RenewSerial, vpnInfo.SerialNumber)
	fmt.Printf("Device name: %s\n", deviceName)
	fmt.Printf("New expiry: %s\n", time.Unix(vpnInfo.ExpirationTime, 0).UTC().Format("2006-01-02 15:04 UTC"))
	return nil
}

func listConfigs(cfg *config.Config, vpnClient *vpn.Client, stdout io.Writer) error {
	certs, err := vpnClient.ListCertificates(cfg.WithSessions)
	if err != nil {
		return fmt.Errorf("failed to list configurations: %w", err)
	}
	if cfg.JSON {
		return writeJSON(stdout, configsJSON(certs))
	}
	if len(certs) == 0 {
		fmt.Println("No configurations found.")
		return nil
	}

	fmt.Printf("%-14s  %-10s  %-34s  %-20s  %s\n", "SerialNumber", "Mode", "DeviceName", "Expires", "Fingerprint")
	fmt.Println(strings.Repeat("-", 120))
	for _, c := range certs {
		exp := time.Unix(c.ExpirationTime, 0).UTC().Format("2006-01-02 15:04 UTC")
		name := c.DeviceName
		if name == "" {
			name = "-"
		}
		fmt.Printf("%-14s  %-10s  %-34s  %-20s  %s\n", c.SerialNumber, c.Mode, name, exp, c.ClientKeyFingerprint)
	}
	fmt.Printf("\nTotal: %d\n", len(certs))
	return nil
}

// The JSON shapes below are this tool's own, not the API's: field names are
// stable across Proton API changes, and bit masks and Unix times are decoded.

type serverJSON struct {
	Name         string         `json:"name"`
	Hostname     string         `json:"hostname"`
	Country      string         `json:"country"`
	EntryCountry string         `json:"entry_country"`
	HostCountry  string         `json:"host_country,omitempty"`
	City         string         `json:"city"`
	Tier         string         `json:"tier"`
	Load         int            `json:"load"`
	Score        float64        `json:"score"`
	Features     []string       `json:"features"`
	Endpoints    []endpointJSON `json:"endpoints"`
}

type endpointJSON struct {
	Hostname  string `json:"hostname"`
	EntryIP   string `json:"entry_ip"`
	ExitIP    string `json:"exit_ip"`
	PublicKey string `json:"public_key"`
	Online    bool   `json:"online"`
}

type configJSON struct {
	Serial      string        `json:"serial"`
	Mode        string        `json:"mode"`
	DeviceName  string        `json:"device_name"`
	Expires     time.Time     `json:"expires"`
	Fingerprint string        `json:"fingerprint"`
	Features    *featuresJSON `json:"features,omitempty"`
}

// featuresJSON omits what the certificate does not record: an absent key means
// the server default applies, which is not the same as false.
type featuresJSON struct {
	NetShield      *int  `json:"netshield,omitempty"`
	ModerateNAT    *bool `json:"moderate_nat,omitempty"`
	PortForwarding *bool `json:"port_forwarding,omitempty"`
	Accelerator    *bool `json:"accelerator,omitempty"`
}

func serversJSON(servers []api.LogicalServer) []serverJSON {
	out := make([]serverJSON, 0, len(servers))
	for i := range servers {
		s := &servers[i]
		features := api.GetFeatureNames(s.Features)
		if features == nil {
			features = []string{} // [] rather than null, so jq filters need no guard
		}
		endpoints := make([]endpointJSON, 0, len(s.Servers))
		for j := range s.Servers {
			p := &s.Servers[j]
			endpoints = append(endpoints, endpointJSON{
				Hostname:  p.Domain,
				EntryIP:   p.EntryIP,
				ExitIP:    p.ExitIP,
				PublicKey: p.X25519PublicKey,
				Online:    p.Status == constants.StatusOnline,
			})
		}
		host := s.HostCountry
		if host == s.ExitCountry {
			host = ""
		}
		out = append(out, serverJSON{
			Name: s.Name, Hostname: s.Domain, Country: s.ExitCountry, EntryCountry: s.EntryCountry,
			HostCountry: host, City: s.City, Tier: api.GetTierName(s.Tier), Load: s.Load, Score: s.Score,
			Features: features, Endpoints: endpoints,
		})
	}
	return out
}

func configsJSON(certs []api.VPNCertificate) []configJSON {
	out := make([]configJSON, 0, len(certs))
	for i := range certs {
		c := &certs[i]
		row := configJSON{
			Serial:      c.SerialNumber,
			Mode:        c.Mode,
			DeviceName:  c.DeviceName,
			Expires:     time.Unix(c.ExpirationTime, 0).UTC(),
			Fingerprint: c.ClientKeyFingerprint,
		}
		row.Features = certFeaturesJSON(c.Features)
		out = append(out, row)
	}
	return out
}

func certFeaturesJSON(f api.CertFeaturesSent) *featuresJSON {
	if f == nil {
		return nil
	}
	out := &featuresJSON{}
	if level, ok := f.Int("NetShieldLevel"); ok {
		out.NetShield = &level
	}
	if random, ok := f.Bool("RandomNAT"); ok {
		moderate := !random // RandomNAT is the inverse of Moderate NAT
		out.ModerateNAT = &moderate
	}
	if value, ok := f.Bool("PortForwarding"); ok {
		out.PortForwarding = &value
	}
	if value, ok := f.Bool("SplitTCP"); ok {
		out.Accelerator = &value
	}
	return out
}

func writeJSON(w io.Writer, v any) error {
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	return enc.Encode(v)
}
