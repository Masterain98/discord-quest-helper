// Package vpn manages VPN certificate generation and server interactions.
package vpn

import (
	"fmt"
	"maps"
	"net"
	"net/http"
	"slices"
	"sync"
	"time"

	"protonvpn-wg-confgen/internal/api"
	"protonvpn-wg-confgen/internal/config"
	"protonvpn-wg-confgen/internal/constants"
	"protonvpn-wg-confgen/internal/timeutil"

	"github.com/ProtonVPN/go-vpn-lib/ed25519"
)

// Client handles VPN operations
type Client struct {
	config     *config.Config
	session    *api.Session
	httpClient *http.Client

	// servers holds the list fetched by SyncSession, so that a login followed
	// by a listing does not request it twice, which the official client never does.
	servers []api.LogicalServer
}

// NewClient creates a new VPN client
func NewClient(cfg *config.Config, session *api.Session) *Client {
	return &Client{
		config:     cfg,
		session:    session,
		httpClient: api.NewHTTPClient(10 * time.Second),
	}
}

// doJSON performs an authenticated request and decodes the JSON response into out.
func (c *Client) doJSON(method, url string, body api.Body, out any) error {
	req, err := api.NewRequest(method, url, body, c.session)
	if err != nil {
		return err
	}
	return api.Do(c.httpClient, req, out)
}

// requestCertificate posts a certificate request and validates the response code.
func (c *Client) requestCertificate(certReq api.Body) (*api.VPNInfo, error) {
	var vpnInfo api.VPNInfo
	if err := c.doJSON(http.MethodPost, c.config.APIURL+constants.CertificatePath, certReq, &vpnInfo); err != nil {
		return nil, err
	}

	if !constants.IsSuccessCode(vpnInfo.Code) {
		if vpnInfo.Error != "" {
			return nil, fmt.Errorf("certificate error (code %d): %s", vpnInfo.Code, vpnInfo.Error)
		}
		return nil, fmt.Errorf("certificate request failed, code: %d", vpnInfo.Code)
	}

	return &vpnInfo, nil
}

// GetCertificate generates a VPN certificate
func (c *Client) GetCertificate(keyPair *ed25519.KeyPair) (*api.VPNInfo, error) {
	publicKeyPEM, err := keyPair.PublicKeyPKIXPem()
	if err != nil {
		return nil, fmt.Errorf("failed to get public key PEM: %w", err)
	}

	durationStr, err := timeutil.ParseToMinutes(c.config.Duration)
	if err != nil {
		return nil, fmt.Errorf("failed to parse duration: %w", err)
	}

	// The official client sends exactly ClientPublicKey, Duration and Features,
	// in that order (fetcher.py), which is all a session certificate needs.
	certReq := api.Body{
		{Key: keyClientPublicKey, Value: publicKeyPEM},
		{Key: keyDuration, Value: durationStr},
		{Key: "Features", Value: featuresBody(c.certificateFeatures())},
	}

	// Mode and DeviceName register the certificate on the account, which is the
	// dashboard's flow rather than the Linux client's. NoSave omits them.
	if !c.config.NoSave {
		certReq = append(certReq,
			api.Field{Key: "Mode", Value: constants.CertMode},
			api.Field{Key: "DeviceName", Value: c.deviceName()})
	}

	return c.requestCertificate(certReq)
}

// RenewCertificate renews an existing persistent certificate by reusing its public key.
// Unlike GetCertificate, this does not generate a new key pair and sends Renew: true.
// current is the certificate's features as reported by the API; they are kept
// unless the matching flag was given explicitly. A nil current falls back to
// the flags entirely.
func (c *Client) RenewCertificate(publicKeyPEM, deviceName string, current api.CertFeaturesSent) (*api.VPNInfo, error) {
	durationStr, err := timeutil.ParseToMinutes(c.config.Duration)
	if err != nil {
		return nil, fmt.Errorf("failed to parse duration: %w", err)
	}

	return c.requestCertificate(api.Body{
		{Key: keyClientPublicKey, Value: publicKeyPEM},
		{Key: keyDuration, Value: durationStr},
		{Key: "Features", Value: featuresBody(c.renewalFeatures(current))},
		{Key: "Mode", Value: constants.CertMode},
		{Key: "DeviceName", Value: deviceName},
		{Key: "Renew", Value: true},
	})
}

// featuresBody orders the features as fetcher.py's _convert_features does,
// followed by any keys this tool does not know about.
func featuresBody(features map[string]any) api.Body {
	known := []string{featRandomNAT, featSplitTCP, featPortForwarding, featNetShield}
	body := make(api.Body, 0, len(features))
	for _, key := range known {
		if value, ok := features[key]; ok {
			body = append(body, api.Field{Key: key, Value: value})
		}
	}
	for _, key := range slices.Sorted(maps.Keys(features)) {
		if !slices.Contains(known, key) {
			body = append(body, api.Field{Key: key, Value: features[key]})
		}
	}
	return body
}

// Certificate request keys shared by every kind of request.
const (
	keyClientPublicKey = "ClientPublicKey"
	keyDuration        = "Duration"
)

// Request-side feature keys, from python-proton-vpn-api-core's fetcher.py.
const (
	featNetShield      = "NetShieldLevel"
	featRandomNAT      = "RandomNAT"
	featPortForwarding = "PortForwarding"
	featSplitTCP       = "SplitTCP"
)

// renewalFeatures starts from the certificate's features exactly as the API
// reports them, unknown keys and odd value types included, and lets explicitly
// passed flags override individual values. A certificate with no recorded
// features gets the flags as a whole.
func (c *Client) renewalFeatures(current api.CertFeaturesSent) map[string]any {
	flags := c.certificateFeatures()
	if current == nil {
		return flags
	}
	features := maps.Clone(map[string]any(current))
	for flagName, key := range map[string]string{
		"netshield":       featNetShield,
		"moderate-nat":    featRandomNAT,
		"port-forwarding": featPortForwarding,
		"accelerator":     featSplitTCP,
	} {
		if c.config.Explicit[flagName] {
			features[key] = flags[key]
		}
	}
	return features
}

// GetServers fetches the list of VPN servers the way the official client does:
// it looks up the caller's location first, since the listing carries the
// truncated address as X-PM-netzone.
func (c *Client) GetServers() ([]api.LogicalServer, error) {
	if c.servers != nil {
		return c.servers, nil
	}

	netzone, err := c.netzone()
	if err != nil {
		return nil, err
	}
	return c.fetchServers(netzone)
}

func (c *Client) fetchServers(netzone string) ([]api.LogicalServer, error) {
	req, err := api.NewRequest(http.MethodGet, c.config.APIURL+constants.LogicalsPath+constants.LogicalsQuery, nil, c.session)
	if err != nil {
		return nil, err
	}
	api.SetServerListHeaders(req, netzone)

	var response api.LogicalsResponse
	if err := api.Do(c.httpClient, req, &response); err != nil {
		return nil, err
	}
	if !constants.IsSuccessCode(response.Code) {
		return nil, fmt.Errorf("API returned error code: %d", response.Code)
	}

	c.servers = response.LogicalServers
	return c.servers, nil
}

// netzone returns the caller's IPv4 address with the last octet zeroed, as
// truncate_ip_address does in the official client.
func (c *Client) netzone() (string, error) {
	var location struct {
		Code int    `json:"Code"`
		IP   string `json:"IP"`
	}
	if err := c.doJSON(http.MethodGet, c.config.APIURL+constants.LocationPath, nil, &location); err != nil {
		return "", err
	}
	ip := net.ParseIP(location.IP).To4()
	if ip == nil {
		return "", fmt.Errorf("location lookup returned no IPv4 address (code %d)", location.Code)
	}
	ip[3] = 0
	return ip.String(), nil
}

// SyncSession issues the requests the official client makes right after a
// login: account info, a session certificate, location and client config
// together, then feature flags, the server list and notifications. Only the
// location and the server list are used; the rest exists so that a login here
// is followed by the same traffic as a login there. Failures are ignored, as
// nothing depends on these calls succeeding.
func (c *Client) SyncSession() {
	var discard struct{}
	var netzone string
	var wg sync.WaitGroup

	get := func(path string) {
		defer wg.Done()
		_ = c.doJSON(http.MethodGet, c.config.APIURL+path, nil, &discard)
	}

	wg.Add(4)
	go get(constants.VPNInfoPath)
	go get(constants.ClientConfigPath)
	go func() {
		defer wg.Done()
		// The official client always fetches a 7-day session certificate for a
		// fresh key at login. It is never used here and expires on its own.
		keyPair, err := ed25519.NewKeyPair()
		if err != nil {
			return
		}
		pem, err := keyPair.PublicKeyPKIXPem()
		if err != nil {
			return
		}
		_ = c.doJSON(http.MethodPost, c.config.APIURL+constants.CertificatePath, api.Body{
			{Key: keyClientPublicKey, Value: pem},
			{Key: keyDuration, Value: constants.LoginCertDuration},
		}, &discard)
	}()
	go func() {
		defer wg.Done()
		netzone, _ = c.netzone()
	}()
	wg.Wait()

	_ = c.doJSON(http.MethodGet, c.config.APIURL+constants.FeatureFlagsPath, nil, &discard)
	if netzone != "" {
		_, _ = c.fetchServers(netzone)
	}
	_ = c.doJSON(http.MethodGet, c.config.APIURL+constants.NotificationsPath, nil, &discard)
}

// ListCertificates fetches the certificates on the account, paginating via
// BeginID: the persistent ones, or with withSessions every live certificate,
// session ones included.
func (c *Client) ListCertificates(withSessions bool) ([]api.VPNCertificate, error) {
	const pageSize = 50
	var all []api.VPNCertificate
	var beginID string

	for {
		filter := "Mode=" + constants.CertMode
		if withSessions {
			filter = "WithSessions=1"
		}
		u := fmt.Sprintf("%s%s/all?%s&Limit=%d", c.config.APIURL, constants.CertificatePath, filter, pageSize)
		if beginID != "" {
			u += "&BeginID=" + beginID
		}

		var page api.CertListResponse
		if err := c.doJSON(http.MethodGet, u, nil, &page); err != nil {
			return nil, err
		}
		if !constants.IsSuccessCode(page.Code) {
			if page.Error != "" {
				return nil, fmt.Errorf("list certificates error (code %d): %s", page.Code, page.Error)
			}
			return nil, fmt.Errorf("list certificates failed, code: %d", page.Code)
		}

		all = append(all, page.Certificates...)
		if len(page.Certificates) < pageSize {
			break
		}
		beginID = page.Certificates[len(page.Certificates)-1].SerialNumber
	}

	return all, nil
}

// RevokeCertificate revokes the certificate with the given serial number. It
// works for persistent and session certificates alike. The API answers success
// even when nothing matched, so a zero count is reported as an error.
func (c *Client) RevokeCertificate(serial string) error {
	var result struct {
		Code  int    `json:"Code"`
		Error string `json:"Error,omitempty"`
		Count int    `json:"Count"`
	}
	body := api.Body{{Key: "SerialNumber", Value: serial}}
	if err := c.doJSON(http.MethodDelete, c.config.APIURL+constants.CertificatePath, body, &result); err != nil {
		return err
	}
	if !constants.IsSuccessCode(result.Code) {
		if result.Error != "" {
			return fmt.Errorf("revoke error (code %d): %s", result.Code, result.Error)
		}
		return fmt.Errorf("revoke failed, code: %d", result.Code)
	}
	if result.Count == 0 {
		return fmt.Errorf("no certificate with SerialNumber %s (use --list-configs --with-sessions to see them)", serial)
	}
	return nil
}

// deviceName returns the configured device name, generating one if unset.
func (c *Client) deviceName() string {
	if c.config.DeviceName != "" {
		return c.config.DeviceName
	}
	return fmt.Sprintf("WireGuard-%s-%d", c.config.Username, time.Now().Unix())
}

func (c *Client) certificateFeatures() map[string]any {
	return map[string]any{
		featNetShield: c.config.NetShield,
		// Proton's API field is inverted: RandomNAT=false enables Moderate NAT.
		featRandomNAT:      !c.config.ModerateNAT,
		featPortForwarding: c.config.PortForwarding,
		featSplitTCP:       c.config.EnableAccelerator,
	}
}
