package auth

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"protonvpn-wg-confgen/internal/api"
	"protonvpn-wg-confgen/internal/constants"
	"protonvpn-wg-confgen/internal/securefile"
)

// SessionStore handles persistent session storage
type SessionStore struct {
	filePath string
}

// NewSessionStore creates a new session store
func NewSessionStore() *SessionStore {
	homeDir, err := os.UserHomeDir()
	if err != nil {
		// Fallback to current directory
		homeDir = "."
	}

	return &SessionStore{
		filePath: filepath.Join(homeDir, constants.SessionFileName),
	}
}

// SavedSession represents a session with metadata
type SavedSession struct {
	Session   *api.Session `json:"session"`
	Username  string       `json:"username"`
	SavedAt   time.Time    `json:"saved_at"`
	ExpiresAt time.Time    `json:"expires_at"`
}

// Save stores the session to disk
func (s *SessionStore) Save(session *api.Session, username string, duration time.Duration) error {
	savedSession := &SavedSession{
		Session:  session,
		Username: username,
		SavedAt:  time.Now(),
	}

	// Calculate expiration based on API response
	apiExpiration := time.Now().Add(time.Duration(session.ExpiresIn) * time.Second)

	if duration == 0 {
		// Use the API's expiration
		savedSession.ExpiresAt = apiExpiration
	} else {
		// Use the user-specified duration, but cap it at API expiration
		userExpiration := time.Now().Add(duration)
		if userExpiration.After(apiExpiration) {
			savedSession.ExpiresAt = apiExpiration
		} else {
			savedSession.ExpiresAt = userExpiration
		}
	}

	data, err := json.MarshalIndent(savedSession, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to marshal session: %w", err)
	}

	err = securefile.Write(s.filePath, data)
	if err != nil {
		return fmt.Errorf("failed to write session file: %w", err)
	}

	return nil
}

// Load retrieves a saved session from disk
func (s *SessionStore) Load(username string) (*api.Session, time.Duration, error) {
	data, err := os.ReadFile(s.filePath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, 0, nil // No saved session
		}
		return nil, 0, fmt.Errorf("failed to read session file: %w", err)
	}

	var savedSession SavedSession
	err = json.Unmarshal(data, &savedSession)
	if err != nil {
		return nil, 0, fmt.Errorf("failed to unmarshal session: %w", err)
	}

	// Check if session is for the same user
	if savedSession.Username != username {
		return nil, 0, nil
	}

	// Check if session has expired
	now := time.Now()
	if now.After(savedSession.ExpiresAt) {
		// Delete expired session
		_ = s.Delete()
		return nil, 0, nil
	}

	// Calculate time until expiration
	timeUntilExpiry := savedSession.ExpiresAt.Sub(now)

	return savedSession.Session, timeUntilExpiry, nil
}

// Delete removes the saved session
func (s *SessionStore) Delete() error {
	err := os.Remove(s.filePath)
	if err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("failed to delete session file: %w", err)
	}
	return nil
}

// RefreshSession attempts to refresh the session using the refresh token.
// It returns a new session with updated tokens if successful.
func RefreshSession(httpClient *http.Client, apiURL string, oldSession *api.Session) (*api.Session, error) {
	// Based on proton-python-client/proton/api.py refresh() method
	reqBody := api.Body{
		{Key: "ResponseType", Value: "token"},
		{Key: "GrantType", Value: "refresh_token"},
		{Key: "RefreshToken", Value: oldSession.RefreshToken},
		{Key: "RedirectURI", Value: "http://protonmail.ch"},
	}

	req, err := api.NewCoreRequest(http.MethodPost, apiURL+constants.RefreshPath, reqBody, oldSession)
	if err != nil {
		return nil, err
	}

	var session api.Session
	if err := api.Do(httpClient, req, &session); err != nil {
		return nil, err
	}

	// A non-success code means the refresh token is spent; re-authentication follows.
	if !constants.IsSuccessCode(session.Code) {
		return nil, fmt.Errorf("refresh failed (code %d)", session.Code)
	}

	return &session, nil
}

// VerifySession checks if a session is still valid by making a test API request.
func VerifySession(httpClient *http.Client, apiURL string, session *api.Session) bool {
	// Make a simple request to verify the session. This one inspects the status
	// code directly rather than decoding a body, so it does not use api.Do.
	req, err := api.NewRequest(http.MethodGet, apiURL+constants.LogicalsPath, nil, session)
	if err != nil {
		return false
	}

	resp, err := httpClient.Do(req)
	if err != nil {
		return false
	}
	defer func() { _ = resp.Body.Close() }()

	// If we get a 401, the session is invalid
	if resp.StatusCode == http.StatusUnauthorized {
		return false
	}

	// Any 2xx response means the session is valid
	return resp.StatusCode >= 200 && resp.StatusCode < 300
}
