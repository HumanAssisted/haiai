//go:build cgo

// Per-language sign_text / verify_text tests for the Go SDK (Issue 002).
//
// Mirrors `python/tests/test_sign_text.py` and
// `node/tests/sign-text.test.ts`. Exercises the full Go SDK code path.
//
// CGo build only.

package haiai

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestSignTextRoundTrip(t *testing.T) {
	cl := mediaParityClient(t)
	ctx := context.Background()

	dir := t.TempDir()
	path := filepath.Join(dir, "hello.md")
	if err := os.WriteFile(path, []byte("# Hello\n"), 0o644); err != nil {
		t.Fatalf("write md: %v", err)
	}

	outcome, err := cl.SignText(ctx, path, SignTextOptions{})
	if err != nil {
		t.Fatalf("SignText: %v", err)
	}
	if outcome.SignersAdded != 1 {
		t.Fatalf("expected signers_added=1, got %d", outcome.SignersAdded)
	}

	verified, err := cl.VerifyText(ctx, path, VerifyTextOptions{})
	if err != nil {
		t.Fatalf("VerifyText: %v", err)
	}
	if verified.Status != "signed" {
		t.Fatalf("expected status=signed, got %s", verified.Status)
	}
	if len(verified.Signatures) != 1 {
		t.Fatalf("expected 1 signature, got %d", len(verified.Signatures))
	}
	if verified.Signatures[0].Status != "valid" {
		t.Fatalf("expected signature status=valid, got %s", verified.Signatures[0].Status)
	}
}

func TestSignTextDefaultBackupCreatesBakFile(t *testing.T) {
	// Issue 003 regression: default Go SignTextOptions{} (zero value) must
	// produce backup=true at the wire layer. Previously, marshalling the
	// struct produced backup=false, silently disabling backup writes.
	cl := mediaParityClient(t)
	ctx := context.Background()

	dir := t.TempDir()
	path := filepath.Join(dir, "with-bak.md")
	if err := os.WriteFile(path, []byte("# original\n"), 0o644); err != nil {
		t.Fatalf("write md: %v", err)
	}

	if _, err := cl.SignText(ctx, path, SignTextOptions{}); err != nil {
		t.Fatalf("SignText: %v", err)
	}
	if _, err := os.Stat(path + ".bak"); err != nil {
		t.Fatalf("expected %s.bak after default SignText, got: %v", path, err)
	}
}

func TestSignTextNoBackupSkipsBak(t *testing.T) {
	// Issue 003: NoBackup=true must propagate to backup=false on the wire.
	cl := mediaParityClient(t)
	ctx := context.Background()

	dir := t.TempDir()
	path := filepath.Join(dir, "no-bak.md")
	if err := os.WriteFile(path, []byte("# no backup\n"), 0o644); err != nil {
		t.Fatalf("write md: %v", err)
	}

	if _, err := cl.SignText(ctx, path, SignTextOptions{NoBackup: true}); err != nil {
		t.Fatalf("SignText with NoBackup: %v", err)
	}
	if _, err := os.Stat(path + ".bak"); err == nil {
		t.Fatalf("expected no .bak file when NoBackup=true, but %s.bak exists", path)
	} else if !strings.Contains(err.Error(), "no such file") {
		t.Fatalf("unexpected stat error: %v", err)
	}
}

func TestVerifyTextStrictMissingSignatureReturnsError(t *testing.T) {
	cl := mediaParityClient(t)
	ctx := context.Background()

	dir := t.TempDir()
	path := filepath.Join(dir, "unsigned.md")
	if err := os.WriteFile(path, []byte("# untouched\n"), 0o644); err != nil {
		t.Fatalf("write md: %v", err)
	}

	// Strict mode raises through the FFI when no signature block is found.
	if _, err := cl.VerifyText(ctx, path, VerifyTextOptions{Strict: true}); err == nil {
		t.Fatalf("expected error from VerifyText(strict=true) on unsigned file")
	}
}
