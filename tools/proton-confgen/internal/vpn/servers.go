package vpn

import (
	"cmp"
	"errors"
	"fmt"
	"slices"
	"strings"

	"protonvpn-wg-confgen/internal/api"
	"protonvpn-wg-confgen/internal/config"
	"protonvpn-wg-confgen/internal/constants"
)

// ServerSelector handles server selection logic
type ServerSelector struct {
	config *config.Config
}

// NewServerSelector creates a new server selector
func NewServerSelector(cfg *config.Config) *ServerSelector {
	return &ServerSelector{config: cfg}
}

// EligibleServers returns the online servers matching the configured filters,
// preserving input order. An empty country list matches every country, which is
// what the listing mode uses; selection always has at least one country set.
func EligibleServers(cfg *config.Config, servers []api.LogicalServer) []api.LogicalServer {
	filtered := make([]api.LogicalServer, 0, len(servers))
	for i := range servers {
		if isEligible(cfg, &servers[i]) {
			filtered = append(filtered, servers[i])
		}
	}
	return filtered
}

func isEligible(cfg *config.Config, server *api.LogicalServer) bool {
	if server.Status != constants.StatusOnline || len(server.Servers) == 0 {
		return false
	}
	// Free tier is opt-in: -free-only selects it exclusively, otherwise it is excluded.
	if cfg.FreeOnly != (server.Tier == api.TierFree) {
		return false
	}
	if len(cfg.Countries) > 0 && !slices.Contains(cfg.Countries, server.ExitCountry) {
		return false
	}
	// The P2P filter does not apply to Secure Core or Free tier selections.
	if cfg.P2PServersOnly && !cfg.SecureCoreOnly && !cfg.FreeOnly && server.Features&api.FeatureP2P == 0 {
		return false
	}
	return !cfg.SecureCoreOnly || server.Features&api.FeatureSecureCore != 0
}

// SelectBest selects the best server based on configuration
func (s *ServerSelector) SelectBest(servers []api.LogicalServer) (*api.LogicalServer, error) {
	// If a specific server is requested, find it by exact name match
	if s.config.ServerName != "" {
		for i := range servers {
			if servers[i].Name == s.config.ServerName && servers[i].Status == constants.StatusOnline {
				return &servers[i], nil
			}
		}
		return nil, fmt.Errorf("server %q not found or offline", s.config.ServerName)
	}

	filtered := EligibleServers(s.config, servers)

	if s.config.Debug {
		s.printDebugServerList(filtered)
	}

	if len(filtered) == 0 {
		return nil, s.buildNoServersError()
	}

	// Sort servers: lowest score first (Proton API convention: lower = better for Quick Connect),
	// with lower load as tiebreaker.
	slices.SortFunc(filtered, func(a, b api.LogicalServer) int {
		if c := cmp.Compare(a.Score, b.Score); c != 0 {
			return c
		}
		return cmp.Compare(a.Load, b.Load)
	})

	return &filtered[0], nil
}

func (s *ServerSelector) buildNoServersError() error {
	errMsg := fmt.Sprintf("no suitable servers found for countries: %v", s.config.Countries)

	if s.config.SecureCoreOnly {
		errMsg += " with Secure Core"
	} else if s.config.P2PServersOnly {
		errMsg += " with P2P support"
	}

	return errors.New(errMsg)
}

// GetBestPhysicalServer returns the first online physical server, or nil if the
// logical server has none. Returning an offline server would produce a config
// pointing at a dead endpoint.
func GetBestPhysicalServer(server *api.LogicalServer) *api.PhysicalServer {
	for i := range server.Servers {
		if server.Servers[i].Status == constants.StatusOnline {
			return &server.Servers[i]
		}
	}
	return nil
}

// printDebugServerList prints a debug list of filtered servers
func (s *ServerSelector) printDebugServerList(servers []api.LogicalServer) {
	fmt.Printf("\nDEBUG: Found %d servers after filtering:\n", len(servers))
	fmt.Println("==================================================================================")
	fmt.Printf("%-15s | %-18s | %-12s | Load | Score | Features\n", "Server", "City", "Tier")
	fmt.Println("----------------------------------------------------------------------------------")

	for i := range servers {
		features := api.GetFeatureNames(servers[i].Features)
		featureStr := "-"
		if len(features) > 0 {
			featureStr = strings.Join(features, ", ")
		}

		fmt.Printf("%-15s | %-18s | %-12s | %3d%% | %.2f | %s\n",
			servers[i].Name,
			servers[i].City,
			api.GetTierName(servers[i].Tier),
			servers[i].Load,
			servers[i].Score,
			featureStr)
	}

	fmt.Println("==================================================================================")
}
