package timeutil

import (
	"testing"
	"time"
)

func TestHumanizeDuration(t *testing.T) {
	const day = 24 * time.Hour

	tests := []struct {
		d    time.Duration
		want string
	}{
		{-time.Second, "expired"},
		{0, "less than a minute"},
		{59 * time.Second, "less than a minute"},

		// Singular and plural must agree with the count at every tier. The
		// composite tiers used to render "1 days 1 hours".
		{time.Minute, "1 minute"},
		{2 * time.Minute, "2 minutes"},
		{time.Hour, "1 hour"},
		{time.Hour + time.Minute, "1 hour 1 minute"},
		{day, "1 day"},
		{day + time.Hour + time.Minute, "1 day 1 hour 1 minute"},
		{7 * day, "1 week"},
		{8*day + time.Hour, "1 week 1 day 1 hour"},
		{365*day + 35*day, "1 year 1 month"},

		// Zero components drop out rather than rendering as "0 hours".
		{2*day + 30*time.Minute, "2 days 30 minutes"},
		{3 * time.Hour, "3 hours"},

		// Tier boundaries: the unit switches at 1 day, 7 days, 30 days, 365 days.
		{23*time.Hour + 59*time.Minute, "23 hours 59 minutes"},
		{6*day + 23*time.Hour, "6 days 23 hours"},
		{29 * day, "4 weeks 1 day"},
		{30 * day, "1 month"},
		{364 * day, "12 months 4 days"},
		{365 * day, "1 year"},
	}

	for _, tt := range tests {
		t.Run(tt.d.String(), func(t *testing.T) {
			if got := HumanizeDuration(tt.d); got != tt.want {
				t.Errorf("HumanizeDuration(%s) = %q, want %q", tt.d, got, tt.want)
			}
		})
	}
}

func TestParseDuration(t *testing.T) {
	tests := []struct {
		in      string
		want    time.Duration
		wantErr bool
	}{
		{in: "7d", want: 7 * 24 * time.Hour},
		{in: "365d", want: 365 * 24 * time.Hour},
		{in: "0d", want: 0},
		{in: "30m", want: 30 * time.Minute},
		{in: "1h30m", want: 90 * time.Minute},
		{in: "24h", want: 24 * time.Hour},
		{in: "", wantErr: true},
		{in: "soon", wantErr: true},
		{in: "d", wantErr: true},
		{in: "7days", wantErr: true},
		// "1.5d" is not a Go duration and Atoi rejects the fraction.
		{in: "1.5d", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.in, func(t *testing.T) {
			got, err := ParseDuration(tt.in)
			if (err != nil) != tt.wantErr {
				t.Fatalf("ParseDuration(%q) error = %v, wantErr %v", tt.in, err, tt.wantErr)
			}
			if err == nil && got != tt.want {
				t.Errorf("ParseDuration(%q) = %v, want %v", tt.in, got, tt.want)
			}
		})
	}
}

// TestParseToMinutes pins the wire format and the API's accepted range.
// The bounds were measured against the live API; see API_REFERENCE.md.
func TestParseToMinutes(t *testing.T) {
	tests := []struct {
		in      string
		want    string
		wantErr bool
	}{
		{in: "10m", want: "10 min"},
		{in: "1h30m", want: "90 min"},
		{in: "7d", want: "10080 min"},
		{in: "365d", want: "525600 min"},

		// Below the API's 10-minute floor (rejected with code 2001).
		{in: "9m", wantErr: true},
		{in: "1m", wantErr: true},
		{in: "30s", wantErr: true},

		// Above the 365-day ceiling.
		{in: "366d", wantErr: true},

		{in: "garbage", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.in, func(t *testing.T) {
			got, err := ParseToMinutes(tt.in)
			if (err != nil) != tt.wantErr {
				t.Fatalf("ParseToMinutes(%q) error = %v, wantErr %v", tt.in, err, tt.wantErr)
			}
			if err == nil && got != tt.want {
				t.Errorf("ParseToMinutes(%q) = %q, want %q", tt.in, got, tt.want)
			}
		})
	}
}

func TestParseSessionDuration(t *testing.T) {
	tests := []struct {
		in      string
		want    time.Duration
		wantErr bool
	}{
		{in: "0", want: 0}, // sentinel: use the API's own expiration
		{in: "12h", want: 12 * time.Hour},
		{in: "30d", want: 30 * 24 * time.Hour},
		{in: "31d", wantErr: true},
		{in: "nope", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.in, func(t *testing.T) {
			got, err := ParseSessionDuration(tt.in)
			if (err != nil) != tt.wantErr {
				t.Fatalf("ParseSessionDuration(%q) error = %v, wantErr %v", tt.in, err, tt.wantErr)
			}
			if err == nil && got != tt.want {
				t.Errorf("ParseSessionDuration(%q) = %v, want %v", tt.in, got, tt.want)
			}
		})
	}
}
