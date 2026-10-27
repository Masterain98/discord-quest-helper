package auth

import (
	"fmt"

	"protonvpn-wg-confgen/internal/constants"
)

// Error codes from ProtonVPN API
// Official source: github.com/ProtonMail/protoncore_android/.../ResponseCodes.kt
// See API_REFERENCE.md for full documentation.
const (
	CodeSuccess              = constants.APICodeSuccess
	CodeWrongPassword        = 8002  // PASSWORD_WRONG: Incorrect password
	CodeWrongPasswordFormat  = 8004  // Password format is incorrect (observed)
	CodeCaptchaRequired      = 9001  // HUMAN_VERIFICATION_REQUIRED: CAPTCHA needed
	Code2FARequiredForVPN    = 9100  // VPN-specific: certificate endpoint requires 2FA session (not in official docs)
	CodeAccountDeleted       = 10002 // ACCOUNT_DELETED: Account has been deleted
	CodeAccountDisabled      = 10003 // ACCOUNT_DISABLED: Account has been disabled
	CodeMailboxPasswordError = 10013 // Legacy 2-password mode / invalid refresh token (context-dependent)
)

// Error represents an authentication error with ProtonVPN-specific error code
type Error struct {
	Code    int
	Message string
}

// Error implements the error interface
func (e Error) Error() string {
	return e.Message
}

// NewError creates a new authentication error from an API response code
func NewError(code int) error {
	message := getErrorMessage(code)
	return Error{
		Code:    code,
		Message: message,
	}
}

// getErrorMessage returns a human-readable error message for a given error code
func getErrorMessage(code int) string {
	switch code {
	case CodeWrongPassword:
		return "incorrect username or password"
	case CodeWrongPasswordFormat:
		return "password format is incorrect"
	case CodeCaptchaRequired:
		return "CAPTCHA verification required"
	case Code2FARequiredForVPN:
		return "2FA required for VPN operations - your session was authenticated without 2FA (device trust). Use --clear-session to force re-authentication with 2FA"
	case CodeAccountDeleted:
		return "account has been deleted"
	case CodeAccountDisabled:
		return "account has been disabled"
	case CodeMailboxPasswordError:
		return "account uses legacy 2-password mode - please switch to single-password mode at account.proton.me"
	default:
		return fmt.Sprintf("authentication failed with code: %d", code)
	}
}
