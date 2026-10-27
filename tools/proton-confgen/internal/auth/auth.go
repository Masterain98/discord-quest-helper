// Package auth handles ProtonVPN authentication using the SRP protocol.
package auth

import (
	"bufio"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"protonvpn-wg-confgen/internal/api"
	"protonvpn-wg-confgen/internal/config"
	"protonvpn-wg-confgen/internal/constants"
	"protonvpn-wg-confgen/internal/timeutil"

	"github.com/ProtonMail/go-srp"
	"golang.org/x/term"
)

// Client handles ProtonVPN authentication
type Client struct {
	config       *config.Config
	httpClient   *http.Client
	sessionStore *SessionStore

	// FreshLogin is set when Authenticate signed in with the password rather
	// than reusing a saved session.
	FreshLogin bool
}

// NewClient creates a new authentication client
func NewClient(cfg *config.Config) *Client {
	return &Client{
		config:       cfg,
		sessionStore: NewSessionStore(),
		httpClient:   api.NewHTTPClient(30 * time.Second),
	}
}

// handleSessionRefresh attempts to refresh a session and save it if successful
func (c *Client) handleSessionRefresh(savedSession *api.Session, reason string) (*api.Session, error) {
	fmt.Println(reason)
	refreshedSession, err := RefreshSession(c.httpClient, c.config.APIURL, savedSession)
	if err != nil {
		fmt.Printf("Token refresh failed: %v\n", err)
		fmt.Println("Re-authenticating with password...")
		fmt.Println("(Your trusted device status for MFA will be preserved)")
		_ = c.sessionStore.Delete()
		return nil, err
	}

	fmt.Println("Session refreshed successfully!")
	// Check if refresh token was rotated
	if savedSession.RefreshToken != refreshedSession.RefreshToken {
		fmt.Println("Refresh token was rotated")
	}

	// Save the refreshed session
	if !c.config.NoSession {
		sessionDuration, _ := timeutil.ParseSessionDuration(c.config.SessionDuration)
		if err := c.sessionStore.Save(refreshedSession, c.config.Username, sessionDuration); err != nil {
			fmt.Printf("Warning: Failed to save refreshed session: %v\n", err)
		}
	}

	return refreshedSession, nil
}

// tryExistingSession attempts to use an existing saved session
func (c *Client) tryExistingSession() (*api.Session, error) {
	savedSession, timeUntilExpiry, err := c.sessionStore.Load(c.config.Username)
	if err != nil {
		fmt.Printf("Warning: Failed to load saved session: %v\n", err)
		return nil, err
	}

	if savedSession == nil {
		return nil, nil
	}

	// Determine what to do with the saved session
	switch {
	case c.config.ForceRefresh:
		reason := fmt.Sprintf("Forcing session refresh (current session expires in %s)", timeutil.HumanizeDuration(timeUntilExpiry))
		return c.handleSessionRefresh(savedSession, reason)

	case timeUntilExpiry < time.Duration(constants.SessionRefreshDays)*24*time.Hour && timeUntilExpiry > 0:
		reason := fmt.Sprintf("Session expires soon (in %s), attempting refresh...", timeutil.HumanizeDuration(timeUntilExpiry))
		return c.handleSessionRefresh(savedSession, reason)

	case VerifySession(c.httpClient, c.config.APIURL, savedSession):
		fmt.Printf("Using saved session (expires in %s)\n", timeutil.HumanizeDuration(timeUntilExpiry))
		return savedSession, nil

	default:
		fmt.Println("Saved session invalid, re-authenticating...")
		_ = c.sessionStore.Delete()
		return nil, nil
	}
}

// Authenticate performs the full authentication flow
func (c *Client) Authenticate() (*api.Session, error) {
	if err := c.ensureUsername(); err != nil {
		return nil, err
	}

	c.ping()

	// Try existing session unless clearing or disabled
	if session := c.handleExistingSession(); session != nil {
		return session, nil
	}

	if err := c.ensurePassword(); err != nil {
		return nil, err
	}

	// Perform fresh authentication
	session, err := c.performFreshAuth()
	if err != nil {
		return nil, err
	}

	// Handle session scope upgrade if needed
	if err := c.upgradeSessionIfNeeded(session); err != nil {
		return nil, err
	}

	c.saveSessionIfEnabled(session)
	c.FreshLogin = true
	return session, nil
}

// ping mirrors the official client, whose AutoTransport probes /tests/ping to
// pick a transport before its first real request. The result is irrelevant
// here; only the request pattern matters.
func (c *Client) ping() {
	req, err := api.NewCoreRequest(http.MethodGet, c.config.APIURL+constants.PingPath, nil, nil)
	if err != nil {
		return
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return
	}
	_ = resp.Body.Close()
}

// handleExistingSession handles session clearing or reuse
func (c *Client) handleExistingSession() *api.Session {
	if c.config.ClearSession {
		fmt.Println("Clearing saved session...")
		_ = c.sessionStore.Delete()
		return nil
	}

	if c.config.NoSession {
		return nil
	}

	session, err := c.tryExistingSession()
	if err == nil && session != nil {
		return session
	}
	return nil
}

// performFreshAuth performs SRP authentication and returns a new session
func (c *Client) performFreshAuth() (*api.Session, error) {
	authInfo, err := c.getAuthInfo()
	if err != nil {
		return nil, fmt.Errorf("failed to get auth info: %w", err)
	}

	clientProofs, err := c.generateSRPProofs(authInfo)
	if err != nil {
		return nil, err
	}

	// The second factor is never sent here. Like the official client, 2FA is
	// submitted afterwards through /auth/2fa, see upgradeSessionIfNeeded.
	session, err := c.sendAuthRequest(c.buildAuthRequest(authInfo, clientProofs))
	if err != nil {
		return nil, err
	}

	// Verify server proof
	if session.ServerProof != base64.StdEncoding.EncodeToString(clientProofs.ExpectedServerProof) {
		return nil, fmt.Errorf("server proof verification failed")
	}

	return session, nil
}

// generateSRPProofs generates SRP client proofs for authentication
func (c *Client) generateSRPProofs(authInfo *api.AuthInfoResponse) (*srp.Proofs, error) {
	auth, err := srp.NewAuth(
		authInfo.Version,
		c.config.Username,
		[]byte(c.config.Password),
		authInfo.Salt,
		authInfo.Modulus,
		authInfo.ServerEphemeral,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to create SRP auth: %w", err)
	}

	proofs, err := auth.GenerateProofs(2048)
	if err != nil {
		return nil, fmt.Errorf("failed to generate SRP proofs: %w", err)
	}
	return proofs, nil
}

// buildAuthRequest builds the authentication request payload
func (c *Client) buildAuthRequest(authInfo *api.AuthInfoResponse, proofs *srp.Proofs) api.Body {
	return api.Body{
		{Key: "Username", Value: c.config.Username},
		{Key: "ClientEphemeral", Value: base64.StdEncoding.EncodeToString(proofs.ClientEphemeral)},
		{Key: "ClientProof", Value: base64.StdEncoding.EncodeToString(proofs.ClientProof)},
		{Key: "SRPSession", Value: authInfo.SRPSession},
	}
}

// upgradeSessionIfNeeded upgrades session with 2FA if VPN scope is missing
func (c *Client) upgradeSessionIfNeeded(session *api.Session) error {
	hasVPNScope, hasTwoFactorScope := c.checkSessionScopes(session)

	if hasVPNScope || !hasTwoFactorScope {
		return nil
	}

	fmt.Println("Session lacks VPN scope - 2FA verification required to upgrade session...")
	code, err := c.get2FACode()
	if err != nil {
		return fmt.Errorf("failed to get 2FA code: %w", err)
	}

	updatedScopes, err := c.submit2FA(session, code)
	if err != nil {
		return fmt.Errorf("2FA verification failed: %w", err)
	}
	session.Scopes = updatedScopes
	fmt.Println("2FA verified - session upgraded with VPN scope")
	return nil
}

// checkSessionScopes checks if session has VPN and twofactor scopes
func (c *Client) checkSessionScopes(session *api.Session) (hasVPN, hasTwoFactor bool) {
	for _, scope := range session.Scopes {
		switch scope {
		case "vpn":
			hasVPN = true
		case "twofactor":
			hasTwoFactor = true
		}
	}
	return
}

// saveSessionIfEnabled saves the session if persistence is enabled
func (c *Client) saveSessionIfEnabled(session *api.Session) {
	if c.config.NoSession {
		return
	}

	sessionDuration, err := timeutil.ParseSessionDuration(c.config.SessionDuration)
	if err != nil {
		fmt.Printf("Warning: Invalid session duration, using default: %v\n", err)
		sessionDuration = 0
	}

	if err := c.sessionStore.Save(session, c.config.Username, sessionDuration); err != nil {
		fmt.Printf("Warning: Failed to save session: %v\n", err)
	}
}

func (c *Client) ensureUsername() error {
	if c.config.Username == "" {
		fmt.Print("Username (without @protonmail.com): ")
		reader := bufio.NewReader(os.Stdin)
		username, err := reader.ReadString('\n')
		if err != nil {
			return fmt.Errorf("error reading username: %w", err)
		}
		c.config.Username = strings.TrimSpace(username)
		if c.config.Username == "" {
			return fmt.Errorf("username cannot be empty")
		}
	}
	return nil
}

func (c *Client) ensurePassword() error {
	if c.config.PasswordStdin {
		password, err := readPasswordStdin(os.Stdin)
		if err != nil {
			return err
		}
		c.config.Password = password
		return nil
	}
	if c.config.Password == "" {
		const stdinFileDescriptor = 0
		fmt.Print("Password: ")
		passwordBytes, err := term.ReadPassword(stdinFileDescriptor)
		fmt.Println()
		if err != nil {
			return fmt.Errorf("error reading password: %w", err)
		}
		c.config.Password = string(passwordBytes)
	}
	return nil
}

// readPasswordStdin accepts one password, optionally followed by LF or CRLF.
// Preserve spaces: trimming them would silently change valid passwords.
func readPasswordStdin(input io.Reader) (string, error) {
	data, err := io.ReadAll(input)
	if err != nil {
		return "", fmt.Errorf("error reading password from stdin: %w", err)
	}
	password := string(data)
	if strings.HasSuffix(password, "\n") {
		password = strings.TrimSuffix(strings.TrimSuffix(password, "\n"), "\r")
	}
	if password == "" || strings.ContainsAny(password, "\r\n") {
		return "", fmt.Errorf("stdin must contain one non-empty password, optionally followed by a newline")
	}
	return password, nil
}

func (c *Client) get2FACode() (string, error) {
	input := os.Stdin
	if c.config.PasswordStdin {
		// Stdin belongs to the password producer. Ask for TOTP separately on
		// the controlling terminal so a password pipe still supports 2FA.
		tty, err := os.Open("/dev/tty")
		if err != nil {
			return "", fmt.Errorf("2FA requires a controlling terminal with --password-stdin; retry using interactive login: %w", err)
		}
		defer func() { _ = tty.Close() }()
		input = tty
	}
	fmt.Print("2FA Code: ")
	reader := bufio.NewReader(input)
	code, err := reader.ReadString('\n')
	if err != nil {
		return "", fmt.Errorf("error reading 2FA code: %w", err)
	}
	code = strings.TrimSpace(code)

	// Validate that code is numeric (TOTP codes are 6 digits)
	if code == "" {
		return "", fmt.Errorf("2FA code cannot be empty")
	}

	for _, c := range code {
		if c < '0' || c > '9' {
			return "", fmt.Errorf("2FA code must be numeric (TOTP only).\n" +
				"FIDO2/WebAuthn security keys are not supported.\n" +
				"Please ensure you have TOTP (authenticator app) configured as your 2FA method")
		}
	}

	return code, nil
}

func (c *Client) getAuthInfo() (*api.AuthInfoResponse, error) {
	req, err := api.NewRequest(http.MethodPost, c.config.APIURL+constants.AuthInfoPath,
		api.Body{{Key: "Username", Value: c.config.Username}}, nil)
	if err != nil {
		return nil, err
	}

	var authInfo api.AuthInfoResponse
	if err := api.Do(c.httpClient, req, &authInfo); err != nil {
		return nil, err
	}

	if authInfo.Code != CodeSuccess {
		return nil, fmt.Errorf("failed to get auth info, code: %d", authInfo.Code)
	}

	// Validate required fields
	if authInfo.Modulus == "" {
		return nil, fmt.Errorf("received empty modulus from auth info")
	}
	if authInfo.ServerEphemeral == "" {
		return nil, fmt.Errorf("received empty server ephemeral from auth info")
	}

	return &authInfo, nil
}

func (c *Client) sendAuthRequest(authReq api.Body) (*api.Session, error) {
	req, err := api.NewRequest(http.MethodPost, c.config.APIURL+constants.AuthPath, authReq, nil)
	if err != nil {
		return nil, err
	}
	api.SetHumanVerification(req, c.config.HVToken, constants.HVMethodCaptcha)

	var session api.Session
	if err := api.Do(c.httpClient, req, &session); err != nil {
		return nil, err
	}

	// Handle mailbox password request (2-password mode)
	// Code 10013 means the account uses legacy 2-password mode which requires a separate mailbox password
	// VPN doesn't need mailbox decryption, but the auth flow requires completing it
	if session.Code == CodeMailboxPasswordError {
		return nil, fmt.Errorf("your account uses legacy 2-password mode which is not supported.\n" +
			"Please switch to single-password mode:\n" +
			"  1. Go to account.proton.me\n" +
			"  2. Settings -> All settings -> Account and password -> Passwords\n" +
			"  3. Switch to 'One-password mode'\n" +
			"This is recommended by Proton for most users and is required for this tool")
	}

	if session.Code == CodeCaptchaRequired {
		return nil, captchaError(&session, c.config.APIURL)
	}

	if session.Code != CodeSuccess {
		if session.Error != "" {
			return nil, fmt.Errorf("%s (code %d)", session.Error, session.Code)
		}
		return nil, NewError(session.Code)
	}

	return &session, nil
}

// captchaError explains a 9001 response and points at the CAPTCHA widget for
// this API entry point.
//
// The widget emits "<challenge>:<solved-response>" (see sendToken in the page
// it serves), and that combined string is what the API accepts back. Replaying
// the bare challenge token just earns a fresh challenge.
func captchaError(session *api.Session, apiURL string) error {
	msg := "CAPTCHA verification required by Proton (code 9001)"
	if methods := session.Details.HumanVerificationMethods; len(methods) > 0 {
		msg += fmt.Sprintf("\nAccepted verification methods: %s", strings.Join(methods, ", "))
	}

	if token := session.Details.HumanVerificationToken; token != "" {
		return errors.New(msg + "\n\n" +
			"Solve the CAPTCHA in a browser, then replay the token it produces:\n\n" +
			"  1. Open " + apiURL + constants.CaptchaPath + "?Token=" + token + "\n" +
			"  2. Paste this in the browser console BEFORE solving, so the result is\n" +
			"     not lost among messages from browser extensions:\n" +
			"       window.addEventListener('message', e => {\n" +
			"         const t = e.data?.type\n" +
			"         if (t === 'pm_captcha' || t === 'proton_captcha')\n" +
			"           console.log('HV TOKEN:', e.data.token)\n" +
			"       })\n" +
			"  3. Solve the CAPTCHA. The logged token looks like\n" +
			"       " + token + ":<long-response>\n" +
			"     that is, the challenge above, a colon, then the solved response.\n" +
			"  4. Re-run with the whole string, quoted, as --hv-token, or export it as\n" +
			"     " + constants.HVTokenEnv + " to keep it out of shell history:\n" +
			"       --hv-token '" + token + ":<long-response>'\n\n" +
			"Challenge tokens expire, so if step 4 reports 9001 again, start over\n" +
			"from the fresh token in the new error.\n\n" +
			"Proton challenges logins that look automated, most often from datacenter\n" +
			"or VPS addresses. Signing in once at https://account.proton.me from the\n" +
			"same network, or retrying from a residential connection, may also clear it.")
	}

	return errors.New(msg + "\n" +
		"Proton returned no verification token, so the challenge cannot be replayed.\n" +
		"Signing in once at https://account.proton.me from the same network, or\n" +
		"retrying from a residential connection, may clear it.")
}

// submit2FA submits a 2FA code to upgrade the session with additional scopes (like VPN)
func (c *Client) submit2FA(session *api.Session, code string) ([]string, error) {
	req, err := api.NewRequest(http.MethodPost, c.config.APIURL+constants.TwoFAPath,
		api.Body{{Key: "TwoFactorCode", Value: code}}, session)
	if err != nil {
		return nil, err
	}

	var twoFAResp struct {
		Code   int      `json:"Code"`
		Scopes []string `json:"Scopes"`
		Error  string   `json:"Error,omitempty"`
	}
	if err := api.Do(c.httpClient, req, &twoFAResp); err != nil {
		return nil, err
	}

	if twoFAResp.Code != CodeSuccess {
		if twoFAResp.Error != "" {
			return nil, fmt.Errorf("2FA failed (code %d): %s", twoFAResp.Code, twoFAResp.Error)
		}
		return nil, NewError(twoFAResp.Code)
	}

	return twoFAResp.Scopes, nil
}
