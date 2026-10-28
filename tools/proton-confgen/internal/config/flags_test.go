package config

import (
	"flag"
	"io"
	"os"
	"testing"

	"protonvpn-wg-confgen/internal/constants"
)

func TestValidateFeatureFlags(t *testing.T) {
	tests := []struct {
		name    string
		cfg     Config
		wantErr bool
	}{
		{name: "NetShield malware", cfg: Config{Duration: constants.DefaultCertDuration, NetShield: 1}},
		{name: "NetShield ads", cfg: Config{Duration: constants.DefaultCertDuration, NetShield: 2}},
		{name: "NetShield negative", cfg: Config{Duration: constants.DefaultCertDuration, NetShield: -1}, wantErr: true},
		{name: "NetShield too high", cfg: Config{Duration: constants.DefaultCertDuration, NetShield: 3}, wantErr: true},
		{name: "stdin password", cfg: Config{Duration: constants.DefaultCertDuration, Username: "alice", PasswordStdin: true}},
		{name: "stdin missing username", cfg: Config{Duration: constants.DefaultCertDuration, PasswordStdin: true}, wantErr: true},
		{name: "stdin blank username", cfg: Config{Duration: constants.DefaultCertDuration, Username: "  ", PasswordStdin: true}, wantErr: true},
		{name: "stdin with password", cfg: Config{Duration: constants.DefaultCertDuration, Username: "alice", PasswordStdin: true, Password: "example"}, wantErr: true},
		{name: "defaults", cfg: Config{Duration: constants.DefaultCertDuration}},
		{name: "port forwarding", cfg: Config{Duration: constants.DefaultCertDuration, PortForwarding: true}},
		{name: "moderate NAT", cfg: Config{Duration: constants.DefaultCertDuration, ModerateNAT: true}},
		{
			name:    "mutually exclusive features",
			cfg:     Config{Duration: constants.DefaultCertDuration, PortForwarding: true, ModerateNAT: true},
			wantErr: true,
		},

		// Duration bounds, measured against the live API. See API_REFERENCE.md.
		{name: "minimum duration", cfg: Config{Duration: "10m"}},
		{name: "below minimum duration", cfg: Config{Duration: "9m"}, wantErr: true},
		{name: "above maximum duration", cfg: Config{Duration: "366d"}, wantErr: true},
		{name: "unparseable duration", cfg: Config{Duration: "soon"}, wantErr: true},

		// The API silently clamps session certificates to 7d, so reject longer
		// requests instead of handing back something shorter than asked for.
		{name: "session at cap", cfg: Config{Duration: "7d", NoSave: true}},
		{name: "session over cap", cfg: Config{Duration: "8d", NoSave: true}, wantErr: true},
		{name: "persistent over session cap", cfg: Config{Duration: constants.DefaultCertDuration}},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := validateFeatureFlags(&tt.cfg)
			if (err != nil) != tt.wantErr {
				t.Fatalf("validateFeatureFlags() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

// TestFlagGroupsCoverAllFlags guards --help: every registered flag must be
// listed in exactly one group, or it silently disappears from the output.
func TestFlagGroupsCoverAllFlags(t *testing.T) {
	// Registering flags is a side effect of Parse; do it on a scratch FlagSet
	// via the real registration path so this test tracks the actual list.
	old := flag.CommandLine
	flag.CommandLine = flag.NewFlagSet("test", flag.ContinueOnError)
	flag.CommandLine.SetOutput(io.Discard)
	defer func() { flag.CommandLine = old }()

	_, _ = Parse() // fails on missing --countries, but flags are registered by then

	registered := map[string]bool{}
	flag.VisitAll(func(f *flag.Flag) { registered[f.Name] = true })

	seen := map[string]int{}
	for _, g := range flagGroups {
		for _, name := range g.names {
			seen[name]++
			if !registered[name] {
				t.Errorf("group %q lists %q, which is not a registered flag", g.title, name)
			}
		}
	}
	for name := range registered {
		switch seen[name] {
		case 0:
			t.Errorf("flag %q is registered but missing from flagGroups, so it is absent from --help", name)
		case 1:
		default:
			t.Errorf("flag %q appears in %d groups", name, seen[name])
		}
	}
}

// TestHVTokenEnvFallback checks the env var fills --hv-token only when the
// flag is absent.
func TestHVTokenEnvFallback(t *testing.T) {
	const fromEnv = "from-env"
	for _, tt := range []struct{ name, flag, env, want string }{
		{"env only", "", fromEnv, fromEnv},
		{"flag wins over env", "from-flag", fromEnv, "from-flag"},
		{"neither", "", "", ""},
	} {
		t.Run(tt.name, func(t *testing.T) {
			old := flag.CommandLine
			flag.CommandLine = flag.NewFlagSet("test", flag.ContinueOnError)
			flag.CommandLine.SetOutput(io.Discard)
			defer func() { flag.CommandLine = old }()
			t.Setenv(constants.HVTokenEnv, tt.env)

			args := []string{"--list-configs"}
			if tt.flag != "" {
				args = append(args, "--hv-token", tt.flag)
			}
			os.Args = append([]string{"test"}, args...)

			cfg, err := Parse()
			if err != nil {
				t.Fatal(err)
			}
			if cfg.HVToken != tt.want {
				t.Errorf("HVToken = %q, want %q", cfg.HVToken, tt.want)
			}
		})
	}
}
