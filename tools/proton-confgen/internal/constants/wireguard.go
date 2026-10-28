package constants

// WireGuard defaults
const (
	WireGuardPort = 51820

	// IPv4 configuration
	WireGuardIPv4         = "10.2.0.2/32"
	DefaultDNSIPv4        = "10.2.0.1"
	DefaultAllowedIPsIPv4 = "0.0.0.0/0"

	// IPv6 configuration
	WireGuardIPv6         = "2a07:b944::2:2/128"
	DefaultDNSIPv6        = "2a07:b944::2:1"
	DefaultAllowedIPsIPv6 = "::/0"
)
