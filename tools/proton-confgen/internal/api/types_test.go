package api

import (
	"encoding/json"
	"testing"
)

// The list endpoint reports features as an object, except for certificates
// issued without any, where it sends an empty array. Both must decode.
func TestCertificateFeaturesShapes(t *testing.T) {
	raw := `{"Code":1000,"Certificates":[
		{"SerialNumber":"1","Mode":"persistent","DeviceName":"a","ExpirationTime":10,"Features":{"NetShieldLevel":2,"RandomNAT":false,"PortForwarding":true,"SplitTCP":true}},
		{"SerialNumber":"2","Mode":"session","DeviceName":null,"ExpirationTime":20,"Features":[]},
		{"SerialNumber":"3","Mode":"session","ExpirationTime":30}
	]}`
	var list CertListResponse
	if err := json.Unmarshal([]byte(raw), &list); err != nil {
		t.Fatal(err)
	}
	if len(list.Certificates) != 3 {
		t.Fatalf("decoded %d certificates, want 3", len(list.Certificates))
	}

	first := list.Certificates[0]
	if first.SerialNumber != "1" || first.Mode != "persistent" || first.ExpirationTime != 10 {
		t.Errorf("plain fields lost: %+v", first)
	}
	f := first.Features
	if level, ok := f.Int("NetShieldLevel"); !ok || level != 2 {
		t.Errorf("NetShieldLevel = %d, %v", level, ok)
	}
	if random, ok := f.Bool("RandomNAT"); !ok || random {
		t.Errorf("RandomNAT = %v, %v", random, ok)
	}
	for _, c := range list.Certificates[1:] {
		if c.Features != nil {
			t.Errorf("certificate %s: features = %+v, want nil", c.SerialNumber, c.Features)
		}
		if c.Mode != "session" {
			t.Errorf("certificate %s: mode = %q", c.SerialNumber, c.Mode)
		}
	}
}

// Other Proton clients send 0/1 for flags and keys this tool has never seen.
func TestCertificateFeaturesLooseValues(t *testing.T) {
	var cert VPNCertificate
	if err := json.Unmarshal([]byte(`{"SerialNumber":"4","Features":{"RandomNAT":0,"Future":"x"}}`), &cert); err != nil {
		t.Fatal(err)
	}
	odd := cert.Features
	if random, ok := odd.Bool("RandomNAT"); !ok || random {
		t.Errorf("numeric RandomNAT = %v, %v, want false, true", random, ok)
	}
	if _, ok := odd.Bool("PortForwarding"); ok {
		t.Error("an absent key must report ok=false")
	}
	if odd["Future"] != "x" {
		t.Errorf("unknown keys must be kept, got %v", odd)
	}
}
