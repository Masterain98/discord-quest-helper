package securefile

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestWriteCreatesAndReplacesPrivateFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "private")
	for _, content := range []string{"first secret", "replacement"} {
		if err := Write(path, []byte(content)); err != nil {
			t.Fatal(err)
		}
		data, err := os.ReadFile(path) //nolint:gosec // Path is generated inside t.TempDir.
		if err != nil || string(data) != content {
			t.Fatalf("file content mismatch, read error: %v", err)
		}
		info, err := os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
		if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
			t.Fatalf("permissions = %o, want 600", info.Mode().Perm())
		}
		// The next replacement must repair overly broad permissions.
		if err := os.Chmod(path, 0o644); err != nil { //nolint:gosec // Deliberately permissive regression fixture.
			t.Fatal(err)
		}
	}
	entries, err := os.ReadDir(dir)
	if err != nil || len(entries) != 1 {
		t.Fatalf("temporary files left behind: %v, %v", entries, err)
	}
}

func TestWriteRejectsSymlinks(t *testing.T) {
	for _, existing := range []bool{false, true} {
		dir := t.TempDir()
		target := filepath.Join(dir, "target")
		link := filepath.Join(dir, "link")
		if existing {
			if err := os.WriteFile(target, []byte("original"), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		if err := os.Symlink(target, link); err != nil {
			if runtime.GOOS == "windows" {
				t.Skipf("symlinks unavailable: %v", err)
			}
			t.Fatal(err)
		}
		if err := Write(link, []byte("replacement")); err == nil {
			t.Fatal("accepted symlink")
		}
		data, err := os.ReadFile(target) //nolint:gosec // Target is generated inside t.TempDir.
		if existing && (err != nil || string(data) != "original") {
			t.Fatalf("symlink target changed: %v", err)
		}
		if !existing && !os.IsNotExist(err) {
			t.Fatalf("dangling symlink target created: %v", err)
		}
		if _, err := os.Readlink(link); err != nil {
			t.Fatalf("symlink replaced: %v", err)
		}
	}
}

func TestWriteRejectsDirectory(t *testing.T) {
	dir := t.TempDir()
	if err := Write(dir, []byte("secret")); err == nil {
		t.Fatal("accepted directory")
	}
	entries, err := os.ReadDir(dir)
	if err != nil || len(entries) != 0 {
		t.Fatalf("directory changed: %v, %v", entries, err)
	}
}
