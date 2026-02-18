package main

import (
	"path/filepath"
	"testing"
)

func TestDefaultSecureKeyDirUsesHome(t *testing.T) {
	t.Setenv("HOME", "/tmp/haisdk-home")

	got := defaultSecureKeyDir()
	want := filepath.Join("/tmp/haisdk-home", ".jacs", "keys")
	if got != want {
		t.Fatalf("defaultSecureKeyDir() = %q, want %q", got, want)
	}
}
