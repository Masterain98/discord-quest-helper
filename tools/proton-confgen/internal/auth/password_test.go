package auth

import (
	"errors"
	"strings"
	"testing"
)

func TestReadPasswordStdin(t *testing.T) {
	const testPassword = "password"
	for _, tt := range []struct {
		name, input, want string
		wantErr           bool
	}{
		{"no newline", testPassword, testPassword, false},
		{"LF", "password\n", testPassword, false},
		{"CRLF", "password\r\n", testPassword, false},
		{"preserve spaces", "  password  \n", "  password  ", false},
		{"empty", "", "", true},
		{"empty line", "\n", "", true},
		{"multiple lines", "password\nsecond\n", "", true},
		{"bare carriage return", "password\r", "", true},
	} {
		t.Run(tt.name, func(t *testing.T) {
			got, err := readPasswordStdin(strings.NewReader(tt.input))
			if (err != nil) != tt.wantErr || got != tt.want {
				t.Fatalf("password parsing mismatch, error: %v", err)
			}
		})
	}
}

type failingPasswordReader struct{}

func (failingPasswordReader) Read([]byte) (int, error) {
	return 0, errors.New("read failed")
}

func TestReadPasswordStdinReadError(t *testing.T) {
	if _, err := readPasswordStdin(failingPasswordReader{}); err == nil {
		t.Fatal("ignored password input failure")
	}
}
