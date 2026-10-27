// Package api defines the data structures for ProtonVPN API responses.
package api

import (
	"bytes"
	"encoding/json"
)

// AuthInfoResponse represents the response from the auth info endpoint
type AuthInfoResponse struct {
	Code            int    `json:"Code"`
	Version         int    `json:"Version"`
	Modulus         string `json:"Modulus"`
	ServerEphemeral string `json:"ServerEphemeral"`
	Salt            string `json:"Salt"`
	SRPSession      string `json:"SRPSession"`
	TwoFA           struct {
		Enabled int `json:"Enabled"`
		TOTP    int `json:"TOTP"`
	} `json:"2FA"`
}

// Session represents a ProtonVPN session
type Session struct {
	Code         int      `json:"Code"`
	AccessToken  string   `json:"AccessToken"`
	RefreshToken string   `json:"RefreshToken"`
	TokenType    string   `json:"TokenType"`
	Scopes       []string `json:"Scopes"`
	UID          string   `json:"UID"`
	UserID       string   `json:"UserID"`
	EventID      string   `json:"EventID"`
	ServerProof  string   `json:"ServerProof"`
	PasswordMode int      `json:"PasswordMode"`
	ExpiresIn    int      `json:"ExpiresIn"` // Session expiration in seconds
	Error        string   `json:"Error,omitempty"`
	TwoFA        struct {
		Enabled int `json:"Enabled"`
		TOTP    int `json:"TOTP"`
	} `json:"2FA"`
	Details ErrorDetails `json:"Details"`
}

// ErrorDetails carries the extra payload the API attaches to some failures,
// most usefully the human verification challenge behind code 9001.
type ErrorDetails struct {
	HumanVerificationToken   string   `json:"HumanVerificationToken"`
	HumanVerificationMethods []string `json:"HumanVerificationMethods"`
}

// VPNInfo represents VPN certificate information
type VPNInfo struct {
	Code                 int          `json:"Code"`
	Error                string       `json:"Error,omitempty"`
	SerialNumber         string       `json:"SerialNumber"`
	ClientKeyFingerprint string       `json:"ClientKeyFingerprint"`
	ClientKey            string       `json:"ClientKey"`
	Certificate          string       `json:"Certificate"`
	ExpirationTime       int64        `json:"ExpirationTime"`
	RefreshTime          int64        `json:"RefreshTime"`
	Mode                 string       `json:"Mode"`
	DeviceName           string       `json:"DeviceName"`
	ServerPublicKeyMode  string       `json:"ServerPublicKeyMode"`
	ServerPublicKey      string       `json:"ServerPublicKey"`
	Features             CertFeatures `json:"Features"`
}

// CertFeatures is how the certificate *create* response reports features. The
// keys differ from the request side and moderate-nat is the inverse of
// RandomNAT. The list endpoint reports CertFeaturesSent instead.
type CertFeatures struct {
	Bouncing       bool `json:"bouncing"`
	ModerateNAT    bool `json:"moderate-nat"`
	NetshieldLevel int  `json:"netshield-level"`
	PortForwarding bool `json:"port-forwarding"`
	VPNAccelerator bool `json:"vpn-accelerator"`
}

// VPNCertificate represents a single certificate entry returned by /vpn/v1/certificate/all.
type VPNCertificate struct {
	SerialNumber         string           `json:"SerialNumber"`
	ClientKeyFingerprint string           `json:"ClientKeyFingerprint"`
	ClientKey            string           `json:"ClientKey"`
	DeviceName           string           `json:"DeviceName,omitempty"`
	Mode                 string           `json:"Mode"`
	ExpirationTime       int64            `json:"ExpirationTime"`
	RefreshTime          int64            `json:"RefreshTime"`
	Features             CertFeaturesSent `json:"Features,omitempty"` // nil when none were sent
}

// CertFeaturesSent is a certificate's features as the list endpoint reports
// them: an echo of whatever the creating client sent, in request-side keys.
// That makes it loose data. Clients differ in which keys they send and how:
// the same account can hold {"RandomNAT": false}, {"RandomNAT": 0} from another
// Proton app, and a full set of four. Unknown keys are kept so that a renewal
// can send everything back unchanged.
type CertFeaturesSent map[string]any

// Bool reads a flag, accepting booleans and the 0/1 some clients send. ok is
// false when the certificate does not carry the key.
func (f CertFeaturesSent) Bool(key string) (value, ok bool) {
	switch v := f[key].(type) {
	case bool:
		return v, true
	case float64:
		return v != 0, true
	}
	return false, false
}

// Int reads a numeric feature such as the NetShield level.
func (f CertFeaturesSent) Int(key string) (value int, ok bool) {
	if v, isNumber := f[key].(float64); isNumber {
		return int(v), true
	}
	return 0, false
}

// UnmarshalJSON exists because a certificate issued without features reports
// them as an empty array, [], where every other one has an object.
func (c *VPNCertificate) UnmarshalJSON(data []byte) error {
	type plain VPNCertificate // drops this method, avoiding recursion
	aux := struct {
		*plain
		Features json.RawMessage `json:"Features"`
	}{plain: (*plain)(c)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}

	c.Features = nil
	if trimmed := bytes.TrimSpace(aux.Features); len(trimmed) > 0 && trimmed[0] == '{' {
		return json.Unmarshal(trimmed, &c.Features)
	}
	return nil
}

// CertListResponse is the response body for GET /vpn/v1/certificate/all.
type CertListResponse struct {
	Code         int              `json:"Code"`
	Error        string           `json:"Error,omitempty"`
	Certificates []VPNCertificate `json:"Certificates"`
	Total        int              `json:"Total"`
}

// LogicalServer represents a ProtonVPN logical server
type LogicalServer struct {
	ID           string           `json:"ID"`
	Name         string           `json:"Name"`
	EntryCountry string           `json:"EntryCountry"`
	ExitCountry  string           `json:"ExitCountry"`
	Domain       string           `json:"Domain"`
	Tier         int              `json:"Tier"`
	Features     int              `json:"Features"`
	Region       string           `json:"Region"`
	City         string           `json:"City"`
	Score        float64          `json:"Score"`
	Load         int              `json:"Load"`
	Status       int              `json:"Status"`
	Servers      []PhysicalServer `json:"Servers"`
	HostCountry  string           `json:"HostCountry"`
	Location     struct {
		Lat  float64 `json:"Lat"`
		Long float64 `json:"Long"`
	} `json:"Location"`
}

// PhysicalServer represents a physical VPN server
type PhysicalServer struct {
	ID                 string `json:"ID"`
	EntryIP            string `json:"EntryIP"`
	ExitIP             string `json:"ExitIP"`
	Domain             string `json:"Domain"`
	Status             int    `json:"Status"`
	Label              string `json:"Label"`
	X25519PublicKey    string `json:"X25519PublicKey"`
	Generation         int    `json:"Generation"`
	ServicesDownReason string `json:"ServicesDownReason"`
}

// LogicalsResponse represents the response from the logicals endpoint
type LogicalsResponse struct {
	Code           int             `json:"Code"`
	LogicalServers []LogicalServer `json:"LogicalServers"`
}

// Server feature constants
const (
	FeatureSecureCore = 1
	FeatureTor        = 2
	FeatureP2P        = 4
	FeatureStreaming  = 8
	FeatureIPv6       = 16
)

// Server tier constants
const (
	TierFree = 0
	TierPlus = 2
	TierPM   = 3
)

// GetTierName returns a human-readable name for the server tier
func GetTierName(tier int) string {
	switch tier {
	case TierFree:
		return "Free"
	case TierPlus:
		return "Plus"
	case TierPM:
		return "ProtonMail"
	default:
		return "Unknown"
	}
}

// GetFeatureNames returns a list of enabled features for a server
func GetFeatureNames(features int) []string {
	var result []string
	if features&FeatureSecureCore != 0 {
		result = append(result, "SecureCore")
	}
	if features&FeatureTor != 0 {
		result = append(result, "Tor")
	}
	if features&FeatureP2P != 0 {
		result = append(result, "P2P")
	}
	if features&FeatureStreaming != 0 {
		result = append(result, "Streaming")
	}
	if features&FeatureIPv6 != 0 {
		result = append(result, "IPv6")
	}
	return result
}
