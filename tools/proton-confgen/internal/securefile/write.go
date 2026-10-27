// Package securefile writes private keys and session tokens to private files.
package securefile

import (
	"fmt"
	"os"
	"path/filepath"
)

// Write replaces path with a complete, mode-0600 file, using a temporary file
// in the same directory. Replacement is atomic on Unix. The parent directory
// must be trusted; final-component symlinks and non-regular files are rejected.
func Write(path string, data []byte) error {
	info, err := os.Lstat(path)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	if err == nil && !info.Mode().IsRegular() {
		return fmt.Errorf("refusing to replace non-regular file: %s", path)
	}

	f, err := os.CreateTemp(filepath.Dir(path), ".protonvpn-private-*")
	if err != nil {
		return err
	}
	defer func() {
		_ = f.Close()
		_ = os.Remove(f.Name())
	}()
	// Chmod also makes the final permissions independent of the caller's umask.
	if err := f.Chmod(0o600); err != nil {
		return err
	}
	if _, err := f.Write(data); err != nil {
		return err
	}
	if err := f.Sync(); err != nil {
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	return os.Rename(f.Name(), path)
}
