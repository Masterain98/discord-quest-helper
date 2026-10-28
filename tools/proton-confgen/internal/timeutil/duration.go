// Package timeutil provides time-related utility functions.
package timeutil

import (
	"fmt"
	"strings"
	"time"
)

const (
	lessThanAMinute = "less than a minute"

	unitMinute = "minute"
	unitHour   = "hour"
	unitDay    = "day"
	unitWeek   = "week"
	unitMonth  = "month"
	unitYear   = "year"
)

// part is a single count/unit pair of a humanized duration, e.g. 2 "week".
type part struct {
	count int
	unit  string
}

// format renders the non-zero parts, e.g. "2 weeks 1 day".
func format(parts ...part) string {
	var out []string
	for _, p := range parts {
		if p.count == 0 {
			continue
		}
		if p.count == 1 {
			out = append(out, "1 "+p.unit)
			continue
		}
		out = append(out, fmt.Sprintf("%d %ss", p.count, p.unit))
	}
	return strings.Join(out, " ")
}

// HumanizeDuration converts a duration to a human-readable format, showing
// progressively less detail for longer durations:
//   - Less than a minute: "less than a minute"
//   - Less than a day: "3 hours 5 minutes"
//   - Less than a week: "2 days 3 hours 5 minutes"
//   - Less than a month: "2 weeks 1 day 3 hours 5 minutes"
//   - Less than a year: "3 months 2 days"
//   - Longer: "1 year 3 months"
func HumanizeDuration(d time.Duration) string {
	if d < 0 {
		return "expired"
	}
	if d < time.Minute {
		return lessThanAMinute
	}

	days := int(d.Hours()) / 24
	hours := int(d.Hours()) % 24
	minutes := int(d.Minutes()) % 60

	switch {
	case days == 0:
		return format(part{hours, unitHour}, part{minutes, unitMinute})
	case days < 7:
		return format(part{days, unitDay}, part{hours, unitHour}, part{minutes, unitMinute})
	case days < 30:
		return format(part{days / 7, unitWeek}, part{days % 7, unitDay}, part{hours, unitHour}, part{minutes, unitMinute})
	case days < 365:
		return format(part{days / 30, unitMonth}, part{days % 30, unitDay})
	default:
		years, remainingDays := days/365, days%365
		if remainingDays < 30 {
			return format(part{years, unitYear}, part{remainingDays, unitDay})
		}
		return format(part{years, unitYear}, part{remainingDays / 30, unitMonth})
	}
}
