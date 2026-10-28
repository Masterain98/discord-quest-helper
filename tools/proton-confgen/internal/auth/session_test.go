package auth

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"protonvpn-wg-confgen/internal/api"
)

func TestSessionSaveRepairsPermissions(t *testing.T) {
	path := filepath.Join(t.TempDir(), "session.json")
	if err := os.WriteFile(path, []byte("old"), 0o644); err != nil { //nolint:gosec // Regression fixture: replacement must repair an existing permissive file.
		t.Fatal(err)
	}
	store := &SessionStore{filePath: path}
	if err := store.Save(&api.Session{AccessToken: "test-token", ExpiresIn: 3600}, "alice", time.Hour); err != nil {
		t.Fatal(err)
	}
	session, _, err := store.Load("alice")
	if err != nil || session == nil || session.AccessToken != "test-token" {
		t.Fatalf("saved session did not round-trip: %v", err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("session permissions = %o, want 600", info.Mode().Perm())
	}
}
