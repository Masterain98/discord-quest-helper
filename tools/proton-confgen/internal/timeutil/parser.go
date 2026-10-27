package timeutil

import (
	"fmt"
	"strconv"
	"strings"
	"time"
)

// parseDaysDuration handles duration strings like "7d", "30d", "365d"
func parseDaysDuration(durationStr string) (time.Duration, error) {
	if !strings.HasSuffix(durationStr, "d") {
		return 0, fmt.Errorf("not a day duration format")
	}

	daysStr := strings.TrimSuffix(durationStr, "d")
	days, err := strconv.Atoi(daysStr)
	if err != nil {
		return 0, fmt.Errorf("invalid day value: %s", daysStr)
	}

	return time.Duration(days) * 24 * time.Hour, nil
}

// ParseDuration parses a duration string that can be either Go duration or "Xd" format
func ParseDuration(durationStr string) (time.Duration, error) {
	// Try days format first
	if duration, err := parseDaysDuration(durationStr); err == nil {
		return duration, nil
	}

	// Fall back to standard Go duration
	return time.ParseDuration(durationStr)
}

// ParseToMinutes parses a duration string and converts it to minutes for the ProtonVPN API.
// Accepts formats like "7d", "30d", "365d", "30m", "24h", "1h30m".
// Returns the duration in "XXX min" format as expected by the API.
func ParseToMinutes(durationStr string) (string, error) {
	duration, err := ParseDuration(durationStr)
	if err != nil {
		return "", fmt.Errorf("invalid duration format: %s", durationStr)
	}

	// Convert to minutes
	minutes := int(duration.Minutes())
	// The API rejects anything shorter with code 2001.
	if minutes < 10 {
		return "", fmt.Errorf("duration must be at least 10 minutes")
	}

	// Max 365 days = 525600 minutes
	maxMinutes := 365 * 24 * 60
	if minutes > maxMinutes {
		return "", fmt.Errorf("duration cannot exceed 365 days")
	}

	return fmt.Sprintf("%d min", minutes), nil
}

// ParseSessionDuration parses a session duration string for local caching.
// Accepts formats like "7d", "30d", or standard Go duration strings.
// Returns 0 to indicate "use API default".
func ParseSessionDuration(durationStr string) (time.Duration, error) {
	// Handle "0" as use API default
	if durationStr == "0" {
		return 0, nil
	}

	duration, err := ParseDuration(durationStr)
	if err != nil {
		return 0, fmt.Errorf("invalid session duration format: %s", durationStr)
	}

	// Validate max duration (30 days)
	maxDuration := 30 * 24 * time.Hour
	if duration > maxDuration {
		return 0, fmt.Errorf("session duration cannot exceed 30 days (got: %s)", duration)
	}

	return duration, nil
}
