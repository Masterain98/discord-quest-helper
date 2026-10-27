package main

import (
	"bytes"
	"encoding/json"
	"testing"

	"protonvpn-wg-confgen/internal/api"
	"protonvpn-wg-confgen/internal/constants"
)

// The JSON output is a contract for scripts: these pin the field names and the
// decoding that distinguishes it from the raw API response.
func TestServersJSON(t *testing.T) {
	servers := []api.LogicalServer{{
		Name: "IS-NL#1", Domain: "is-nl-01.protonvpn.net", ExitCountry: "NL", EntryCountry: "IS", HostCountry: "NL",
		City: "Amsterdam", Tier: api.TierPlus, Load: 27, Score: 1.5, Features: api.FeatureSecureCore | api.FeatureP2P,
		Servers: []api.PhysicalServer{{Domain: "node-nl-01.protonvpn.net", EntryIP: "192.0.2.1", ExitIP: "192.0.2.2", X25519PublicKey: "key=", Status: constants.StatusOnline}},
	}, {Name: "US#1", ExitCountry: "US"}}

	var buf bytes.Buffer
	if err := writeJSON(&buf, serversJSON(servers)); err != nil {
		t.Fatal(err)
	}
	var got []map[string]any
	if err := json.Unmarshal(buf.Bytes(), &got); err != nil {
		t.Fatal(err)
	}

	want := map[string]any{
		"name": "IS-NL#1", "hostname": "is-nl-01.protonvpn.net", "country": "NL", "entry_country": "IS",
		"city": "Amsterdam", "tier": "Plus", "load": float64(27), "score": 1.5,
	}
	for key, value := range want {
		if got[0][key] != value {
			t.Errorf("%s = %v, want %v", key, got[0][key], value)
		}
	}
	if _, present := got[0]["host_country"]; present {
		t.Error("host_country should be omitted when it equals the exit country")
	}
	if features := got[0]["features"].([]any); len(features) != 2 || features[0] != "SecureCore" || features[1] != "P2P" {
		t.Errorf("features = %v", features)
	}
	endpoint := got[0]["endpoints"].([]any)[0].(map[string]any)
	if endpoint["hostname"] != "node-nl-01.protonvpn.net" || endpoint["entry_ip"] != "192.0.2.1" || endpoint["online"] != true {
		t.Errorf("endpoint = %v", endpoint)
	}
	// Empty lists must be [], not null, or `jq '.features[]'` style filters break.
	if features, ok := got[1]["features"].([]any); !ok || len(features) != 0 {
		t.Errorf("empty features = %#v, want []", got[1]["features"])
	}
}

func TestConfigsJSON(t *testing.T) {
	certs := []api.VPNCertificate{{
		SerialNumber: "123", Mode: "persistent", DeviceName: "router", ExpirationTime: 1820845845, ClientKeyFingerprint: "fp",
		Features: api.CertFeaturesSent{"NetShieldLevel": float64(2), "RandomNAT": false, "PortForwarding": true, "SplitTCP": true},
	}, {
		// As another Proton app records it: a number for a flag, nothing else.
		SerialNumber: "456", Mode: "session", Features: api.CertFeaturesSent{"RandomNAT": float64(0)},
	}, {SerialNumber: "789", Mode: "session"}}
	var buf bytes.Buffer
	if err := writeJSON(&buf, configsJSON(certs)); err != nil {
		t.Fatal(err)
	}
	var got []map[string]any
	if err := json.Unmarshal(buf.Bytes(), &got); err != nil {
		t.Fatal(err)
	}
	if got[0]["serial"] != "123" || got[0]["mode"] != "persistent" || got[0]["device_name"] != "router" || got[0]["expires"] != "2027-09-13T14:30:45Z" {
		t.Errorf("config = %v", got[0])
	}
	features := got[0]["features"].(map[string]any)
	if features["moderate_nat"] != true || features["netshield"] != float64(2) || features["port_forwarding"] != true {
		t.Errorf("features = %v (RandomNAT false must read as moderate_nat true)", features)
	}

	sparse := got[1]["features"].(map[string]any)
	if sparse["moderate_nat"] != true {
		t.Errorf("RandomNAT 0 must read as moderate_nat true, got %v", sparse)
	}
	if _, present := sparse["port_forwarding"]; present || len(sparse) != 1 {
		t.Errorf("keys the certificate does not record must be omitted, got %v", sparse)
	}
	if _, present := got[2]["features"]; present {
		t.Errorf("a certificate without features must omit the field, got %v", got[2]["features"])
	}
}
