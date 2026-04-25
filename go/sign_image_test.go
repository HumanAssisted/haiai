//go:build cgo

// Per-language signing-side tests for the Go SDK (Issue 002).
//
// Mirrors `python/tests/test_sign_image.py` and
// `node/tests/sign-image.test.ts`. Exercises the full Go SDK code path
// (Client.SignImage -> ffi -> haiigo cdylib -> binding-core -> JACS).
//
// CGo build only: needs `libhaiigo.dylib` (or .so) on the linker path. The
// test reuses `mediaParityClient` from `cross_lang_media_test.go` for the
// staged fixture-agent + FFI plumbing, so a `t.Skipf` from there
// propagates if the cdylib pre-dates TASK_009.

package haiai

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// signImageSourcePNG returns the bytes of the committed source PNG used as
// SignImage input across languages. The fixture is shaped for metadata-only
// signing; robust LSB embedding needs more pixel capacity (see TASK_011 +
// Issue 002 robust-test note in `python/tests/test_sign_image.py`).
func signImageSourcePNG(t *testing.T) []byte {
	t.Helper()
	root := mediaRepoRoot(t)
	path := filepath.Join(root, "fixtures", "media", "_source", "source.png")
	bytesIn, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read fixtures/media/_source/source.png: %v", err)
	}
	return bytesIn
}

func TestSignImagePNGRoundTrip(t *testing.T) {
	cl := mediaParityClient(t)
	ctx := context.Background()

	in := writeStaged(t, "in.png", signImageSourcePNG(t))
	out := filepath.Join(filepath.Dir(in), "out.png")

	signed, err := cl.SignImage(ctx, in, out, SignImageOptions{})
	if err != nil {
		t.Fatalf("SignImage: %v", err)
	}
	if signed.Format != "png" {
		t.Fatalf("expected format=png, got %q", signed.Format)
	}
	if signed.SignerID == "" {
		t.Fatalf("expected non-empty signer_id")
	}
	if signed.OutPath != out {
		t.Fatalf("expected out_path=%s, got %s", out, signed.OutPath)
	}

	verified, err := cl.VerifyImage(ctx, out, VerifyImageOptions{})
	if err != nil {
		t.Fatalf("VerifyImage: %v", err)
	}
	if verified.Status != "valid" {
		t.Fatalf("expected status=valid, got %s", verified.Status)
	}
	if verified.SignerID == nil || *verified.SignerID != signed.SignerID {
		t.Fatalf("verified signer_id mismatch: %v vs %s", verified.SignerID, signed.SignerID)
	}
}

func TestVerifyImageTamperedReturnsHashMismatch(t *testing.T) {
	cl := mediaParityClient(t)
	ctx := context.Background()

	in := writeStaged(t, "in_tamper.png", signImageSourcePNG(t))
	out := filepath.Join(filepath.Dir(in), "out_tamper.png")
	if _, err := cl.SignImage(ctx, in, out, SignImageOptions{}); err != nil {
		t.Fatalf("SignImage: %v", err)
	}

	// Flip a byte in the IDAT region (after the iTXt signature chunk).
	buf, err := os.ReadFile(out)
	if err != nil {
		t.Fatalf("read signed: %v", err)
	}
	idat := bytes.Index(buf, []byte("IDAT"))
	if idat == -1 {
		t.Fatalf("IDAT marker not found")
	}
	buf[idat+6] ^= 0x01
	if err := os.WriteFile(out, buf, 0o644); err != nil {
		t.Fatalf("write tampered: %v", err)
	}

	verified, err := cl.VerifyImage(ctx, out, VerifyImageOptions{})
	if err != nil {
		t.Fatalf("VerifyImage tampered: %v", err)
	}
	if verified.Status != "hash_mismatch" && verified.Status != "invalid_signature" {
		t.Fatalf("expected hash_mismatch or invalid_signature, got %s", verified.Status)
	}
}

func TestExtractMediaSignatureReturnsPayload(t *testing.T) {
	cl := mediaParityClient(t)
	ctx := context.Background()

	in := writeStaged(t, "in_extract.png", signImageSourcePNG(t))
	out := filepath.Join(filepath.Dir(in), "out_extract.png")
	if _, err := cl.SignImage(ctx, in, out, SignImageOptions{}); err != nil {
		t.Fatalf("SignImage: %v", err)
	}

	extracted, err := cl.ExtractMediaSignature(ctx, out, false)
	if err != nil {
		t.Fatalf("ExtractMediaSignature: %v", err)
	}
	if !extracted.Present {
		t.Fatalf("expected present=true")
	}
	if extracted.Payload == nil {
		t.Fatalf("expected non-nil payload")
	}
	var inner map[string]interface{}
	if err := json.Unmarshal([]byte(*extracted.Payload), &inner); err != nil {
		t.Fatalf("decoded payload should be JSON: %v", err)
	}
}

func TestExtractMediaSignatureRawReturnsBase64URL(t *testing.T) {
	cl := mediaParityClient(t)
	ctx := context.Background()

	in := writeStaged(t, "in_raw.png", signImageSourcePNG(t))
	out := filepath.Join(filepath.Dir(in), "out_raw.png")
	if _, err := cl.SignImage(ctx, in, out, SignImageOptions{}); err != nil {
		t.Fatalf("SignImage: %v", err)
	}

	raw, err := cl.ExtractMediaSignature(ctx, out, true)
	if err != nil {
		t.Fatalf("ExtractMediaSignature raw: %v", err)
	}
	if !raw.Present || raw.Payload == nil {
		t.Fatalf("expected present payload with raw_payload=true")
	}
	for _, ch := range *raw.Payload {
		ok := (ch >= 'A' && ch <= 'Z') || (ch >= 'a' && ch <= 'z') ||
			(ch >= '0' && ch <= '9') || ch == '-' || ch == '_'
		if !ok {
			t.Fatalf("payload contains non-base64url char %q in %q",
				ch, *raw.Payload)
		}
	}
}

func TestExtractMediaSignatureUnsignedReturnsPresentFalse(t *testing.T) {
	cl := mediaParityClient(t)
	ctx := context.Background()

	in := writeStaged(t, "unsigned.png", signImageSourcePNG(t))
	extracted, err := cl.ExtractMediaSignature(ctx, in, false)
	if err != nil {
		t.Fatalf("ExtractMediaSignature unsigned: %v", err)
	}
	if extracted.Present {
		t.Fatalf("expected present=false on unsigned PNG")
	}
}

func TestSignImageNoBackupSkipsBak(t *testing.T) {
	// Issue 003: NoBackup=true must propagate to JACS as backup=false.
	cl := mediaParityClient(t)
	ctx := context.Background()

	in := writeStaged(t, "in_nobak.png", signImageSourcePNG(t))
	out := filepath.Join(filepath.Dir(in), "out_nobak.png")
	if _, err := cl.SignImage(ctx, in, out, SignImageOptions{NoBackup: true}); err != nil {
		t.Fatalf("SignImage with NoBackup: %v", err)
	}
	if _, err := os.Stat(out + ".bak"); err == nil {
		t.Fatalf("expected no .bak file when NoBackup=true, but %s.bak exists", out)
	} else if !strings.Contains(err.Error(), "no such file") {
		t.Fatalf("unexpected stat error: %v", err)
	}
}
