//go:build cgo

// Cross-language media verify-parity tests (Go side).
//
// Mirrors `rust/haiai/tests/cross_lang_contract.rs`
// (`cross_lang_signed_image_*` and `cross_lang_signed_text_md_*`),
// `python/tests/test_cross_lang_media.py`, and
// `node/tests/cross-lang-media.test.ts`. Loads the same pre-signed fixtures
// from `fixtures/media/signed.{png,jpg,webp,md}` (signed once by the Rust
// regenerator in `rust/haiai/tests/regen_media_fixtures.rs`, with its public
// key committed beside the media) and asserts that the Go FFI
// VerifyImage / VerifyText paths return the same Valid / HashMismatch
// verdicts.
//
// Any drift between languages here MUST be a parity bug, not a test-only
// quirk — that is the entire point of this suite (PRD §5.5, TASK_011).
//
// CGo build only: needs `libhaiigo.dylib` (or .so) on the linker path.

package haiai

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const verifierAgentPassword = "MediaFixtureVerifierPass!123"

func mediaRepoRoot(t *testing.T) string {
	t.Helper()
	wd, err := os.Getwd()
	if err != nil {
		t.Fatalf("Getwd: %v", err)
	}
	// go test runs in `go/`, repo root is one up.
	return filepath.Dir(wd)
}

func mediaDir(t *testing.T) string {
	return filepath.Join(mediaRepoRoot(t), "fixtures", "media")
}

// ---------------------------------------------------------------------------
// Signer fixture
// ---------------------------------------------------------------------------

type signerFixture struct {
	SignerID         string `json:"signer_id"`
	Algorithm        string `json:"algorithm"`
	PublicKeyFile    string `json:"public_key_file"`
	VerifierAgentDir string `json:"verifier_agent_dir"`
}

func loadSigner(t *testing.T) signerFixture {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(mediaDir(t), "SIGNER.json"))
	if err != nil {
		t.Fatalf("read SIGNER.json: %v", err)
	}
	var sf signerFixture
	if err := json.Unmarshal(raw, &sf); err != nil {
		t.Fatalf("decode SIGNER.json: %v", err)
	}
	return sf
}

// ---------------------------------------------------------------------------
// Checksum-verified fixture loader (mirrors Rust read_fixture_with_checksum)
// ---------------------------------------------------------------------------

func loadChecksum(t *testing.T, name string) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(mediaDir(t), "CHECKSUMS.txt"))
	if err != nil {
		t.Fatalf("read CHECKSUMS.txt: %v", err)
	}
	for _, line := range strings.Split(string(raw), "\n") {
		parts := strings.Fields(line)
		if len(parts) == 2 && parts[1] == name {
			return parts[0]
		}
	}
	t.Fatalf("no checksum for %s in CHECKSUMS.txt", name)
	return ""
}

func readSignedWithChecksum(t *testing.T, name string) []byte {
	t.Helper()
	bytes_, err := os.ReadFile(filepath.Join(mediaDir(t), name))
	if err != nil {
		t.Fatalf("read fixtures/media/%s: %v", name, err)
	}
	expected := loadChecksum(t, name)
	got := sha256.Sum256(bytes_)
	gotHex := hex.EncodeToString(got[:])
	if gotHex != expected {
		t.Fatalf("checksum drift on fixtures/media/%s: got %s, expected %s", name, gotHex, expected)
	}
	return bytes_
}

// ---------------------------------------------------------------------------
// Signed verifier staging — mirrors Rust `stage_media_verifier`.
// ---------------------------------------------------------------------------

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return err
	}
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()
	_, err = io.Copy(out, in)
	return err
}

// copyVerifierTree preserves directory names such as `public_keys`, while
// restoring the `:` in agent document `{id}:{version}.json` filenames that
// Git stores with `_` for Windows compatibility.
func copyVerifierTree(src, dst string, inAgentDir bool) error {
	if err := os.MkdirAll(dst, 0o755); err != nil {
		return err
	}
	entries, err := os.ReadDir(src)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		newName := entry.Name()
		if inAgentDir && !entry.IsDir() {
			newName = strings.ReplaceAll(newName, "_", ":")
		}
		srcPath := filepath.Join(src, entry.Name())
		dstPath := filepath.Join(dst, newName)
		if entry.IsDir() {
			if err := copyVerifierTree(srcPath, dstPath, inAgentDir || entry.Name() == "agent"); err != nil {
				return err
			}
		} else {
			if err := copyFile(srcPath, dstPath); err != nil {
				return err
			}
		}
	}
	return nil
}

type stagedAgent struct {
	configPath         string
	tmpDir             string
	verificationKeyDir string
}

func stageVerifierAgent(t *testing.T, signer signerFixture) stagedAgent {
	t.Helper()
	t.Setenv("JACS_PRIVATE_KEY_PASSWORD", verifierAgentPassword)

	tmpDir := t.TempDir()
	verifierDir := filepath.Join(mediaRepoRoot(t), signer.VerifierAgentDir)
	if err := copyVerifierTree(verifierDir, tmpDir, false); err != nil {
		t.Fatalf("stage signed verifier agent: %v", err)
	}
	configPath := filepath.Join(tmpDir, "jacs.config.json")
	verificationKeyDir := filepath.Join(tmpDir, "verification-keys")
	if err := os.MkdirAll(verificationKeyDir, 0o755); err != nil {
		t.Fatalf("create verification key directory: %v", err)
	}
	if err := copyFile(
		filepath.Join(mediaRepoRoot(t), signer.PublicKeyFile),
		filepath.Join(verificationKeyDir, signer.SignerID+".public.pem"),
	); err != nil {
		t.Fatalf("stage signer public key: %v", err)
	}

	return stagedAgent{
		configPath:         configPath,
		tmpDir:             tmpDir,
		verificationKeyDir: verificationKeyDir,
	}
}

// ---------------------------------------------------------------------------
// FFI client — wraps the staged fixture agent into a real `Client`.
// ---------------------------------------------------------------------------

func mediaParityVerifier(t *testing.T, signer signerFixture) (*Client, string) {
	t.Helper()

	staged := stageVerifierAgent(t, signer)
	cfgRaw, err := os.ReadFile(staged.configPath)
	if err != nil {
		t.Fatalf("re-read staged config: %v", err)
	}
	var cfg map[string]interface{}
	if err := json.Unmarshal(cfgRaw, &cfg); err != nil {
		t.Fatalf("decode staged config: %v", err)
	}
	idAndVer := cfg["jacs_agent_id_and_version"].(string)
	jacsID := strings.SplitN(idAndVer, ":", 2)[0]

	ffiConfig := map[string]interface{}{
		"jacs_id":          jacsID,
		"agent_name":       "MediaFixtureVerifier",
		"agent_version":    "1.0.0",
		"key_dir":          filepath.Join(staged.tmpDir, cfg["jacs_key_directory"].(string)),
		"jacs_config_path": staged.configPath,
		"base_url":         "http://localhost:1", // never used; verify is local-only
	}
	configJSON, err := json.Marshal(ffiConfig)
	if err != nil {
		t.Fatalf("marshal ffi config: %v", err)
	}
	ffi, err := newFFIClientFromConfig(string(configJSON))
	if err != nil {
		t.Skipf("Go FFI does not have the Layer-8 media methods loaded "+
			"(libhaiigo.dylib must be rebuilt against JACS 0.10.0+ source): %v", err)
	}
	cl, err := NewClient(WithFFIClient(ffi), WithJACSID(jacsID))
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	return cl, staged.verificationKeyDir
}

// mediaParityClient is shared by the Go sign-image/text tests. It uses the
// same current signed verifier fixture but does not need the external PQ key
// directory when signing and self-verifying newly created media.
func mediaParityClient(t *testing.T) *Client {
	t.Helper()
	client, _ := mediaParityVerifier(t, loadSigner(t))
	return client
}

// ---------------------------------------------------------------------------
// Tampering helpers — mirror Rust tamper_after / tamper_text_body.
// ---------------------------------------------------------------------------

func tamperAfter(t *testing.T, buf []byte, marker []byte, offset int) []byte {
	t.Helper()
	idx := bytes.Index(buf, marker)
	if idx == -1 {
		t.Fatalf("marker %x not found in fixture bytes", marker)
	}
	target := idx + len(marker) + offset
	out := append([]byte(nil), buf...)
	out[target] ^= 0x01
	return out
}

func tamperTextBody(t *testing.T, buf []byte) []byte {
	t.Helper()
	marker := []byte("-----BEGIN JACS SIGNATURE-----")
	bodyEnd := bytes.Index(buf, marker)
	if bodyEnd == -1 {
		t.Fatalf("BEGIN marker not present in signed.md")
	}
	out := append([]byte(nil), buf...)
	for i := bodyEnd - 1; i >= 0; i-- {
		c := out[i]
		if (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) {
			out[i] ^= 0x20
			return out
		}
	}
	t.Fatalf("no ASCII letter found before signature block")
	return nil
}

// ---------------------------------------------------------------------------
// Helpers — stage signed.* into a tempdir for verify
// ---------------------------------------------------------------------------

func writeStaged(t *testing.T, name string, data []byte) string {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("write staged %s: %v", name, err)
	}
	return path
}

// ---------------------------------------------------------------------------
// Tests — verify_image / verify_text parity
// ---------------------------------------------------------------------------

func TestCrossLang_SignedImagePNGVerifies(t *testing.T) {
	signer := loadSigner(t)
	if signer.Algorithm != "pq2025" {
		t.Fatalf("fixture algorithm = %q, want pq2025 (parity baseline)", signer.Algorithm)
	}
	cl, keyDir := mediaParityVerifier(t, signer)

	path := writeStaged(t, "signed.png", readSignedWithChecksum(t, "signed.png"))
	got, err := cl.VerifyImage(context.Background(), path, VerifyImageOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyImage: %v", err)
	}
	if got.Status != MediaVerifyStatusValid {
		t.Fatalf("status = %q, want %q", got.Status, MediaVerifyStatusValid)
	}
	if got.SignerID == nil || *got.SignerID != signer.SignerID {
		t.Fatalf("signer_id = %v, want %q", got.SignerID, signer.SignerID)
	}
}

func TestCrossLang_SignedImagePNGTamperedReturnsHashMismatch(t *testing.T) {
	signer := loadSigner(t)
	cl, keyDir := mediaParityVerifier(t, signer)

	tampered := tamperAfter(t, readSignedWithChecksum(t, "signed.png"), []byte("IDAT"), 6)
	path := writeStaged(t, "signed.png", tampered)
	got, err := cl.VerifyImage(context.Background(), path, VerifyImageOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyImage: %v", err)
	}
	if got.Status != MediaVerifyStatusHashMismatch {
		t.Fatalf("status = %q, want %q (after tampering)", got.Status, MediaVerifyStatusHashMismatch)
	}
}

func TestCrossLang_SignedImageJPEGVerifies(t *testing.T) {
	signer := loadSigner(t)
	cl, keyDir := mediaParityVerifier(t, signer)

	path := writeStaged(t, "signed.jpg", readSignedWithChecksum(t, "signed.jpg"))
	got, err := cl.VerifyImage(context.Background(), path, VerifyImageOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyImage: %v", err)
	}
	if got.Status != MediaVerifyStatusValid {
		t.Fatalf("status = %q, want %q", got.Status, MediaVerifyStatusValid)
	}
	if got.SignerID == nil || *got.SignerID != signer.SignerID {
		t.Fatalf("signer_id = %v, want %q", got.SignerID, signer.SignerID)
	}
}

func TestCrossLang_SignedImageJPEGTamperedReturnsHashMismatch(t *testing.T) {
	signer := loadSigner(t)
	cl, keyDir := mediaParityVerifier(t, signer)

	tampered := tamperAfter(t, readSignedWithChecksum(t, "signed.jpg"), []byte{0xFF, 0xDA}, 4)
	path := writeStaged(t, "signed.jpg", tampered)
	got, err := cl.VerifyImage(context.Background(), path, VerifyImageOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyImage: %v", err)
	}
	if got.Status != MediaVerifyStatusHashMismatch {
		t.Fatalf("status = %q, want %q (after tampering)", got.Status, MediaVerifyStatusHashMismatch)
	}
}

func TestCrossLang_SignedImageWebPVerifies(t *testing.T) {
	signer := loadSigner(t)
	cl, keyDir := mediaParityVerifier(t, signer)

	path := writeStaged(t, "signed.webp", readSignedWithChecksum(t, "signed.webp"))
	got, err := cl.VerifyImage(context.Background(), path, VerifyImageOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyImage: %v", err)
	}
	if got.Status != MediaVerifyStatusValid {
		t.Fatalf("status = %q, want %q", got.Status, MediaVerifyStatusValid)
	}
	if got.SignerID == nil || *got.SignerID != signer.SignerID {
		t.Fatalf("signer_id = %v, want %q", got.SignerID, signer.SignerID)
	}
}

func TestCrossLang_SignedImageWebPTamperedReturnsHashMismatch(t *testing.T) {
	signer := loadSigner(t)
	cl, keyDir := mediaParityVerifier(t, signer)

	tampered := tamperAfter(t, readSignedWithChecksum(t, "signed.webp"), []byte("VP8L"), 4)
	path := writeStaged(t, "signed.webp", tampered)
	got, err := cl.VerifyImage(context.Background(), path, VerifyImageOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyImage: %v", err)
	}
	if got.Status != MediaVerifyStatusHashMismatch {
		t.Fatalf("status = %q, want %q (after tampering)", got.Status, MediaVerifyStatusHashMismatch)
	}
}

func TestCrossLang_SignedTextMDVerifies(t *testing.T) {
	signer := loadSigner(t)
	cl, keyDir := mediaParityVerifier(t, signer)

	path := writeStaged(t, "signed.md", readSignedWithChecksum(t, "signed.md"))
	got, err := cl.VerifyText(context.Background(), path, VerifyTextOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyText: %v", err)
	}
	if got.Status != VerifyTextStatusSigned {
		t.Fatalf("status = %q, want %q", got.Status, VerifyTextStatusSigned)
	}
	if len(got.Signatures) != 1 {
		t.Fatalf("len(signatures) = %d, want 1", len(got.Signatures))
	}
	sig := got.Signatures[0]
	if sig.Status != MediaVerifyStatusValid {
		t.Fatalf("sig.Status = %q, want %q", sig.Status, MediaVerifyStatusValid)
	}
	if sig.SignerID != signer.SignerID {
		t.Fatalf("sig.SignerID = %q, want %q", sig.SignerID, signer.SignerID)
	}
}

func TestCrossLang_SignedTextMDTamperedReturnsHashMismatch(t *testing.T) {
	signer := loadSigner(t)
	cl, keyDir := mediaParityVerifier(t, signer)

	tampered := tamperTextBody(t, readSignedWithChecksum(t, "signed.md"))
	path := writeStaged(t, "signed.md", tampered)
	got, err := cl.VerifyText(context.Background(), path, VerifyTextOptions{KeyDir: keyDir})
	if err != nil {
		t.Fatalf("VerifyText: %v", err)
	}
	if got.Status != VerifyTextStatusSigned {
		t.Fatalf("status = %q, want %q", got.Status, VerifyTextStatusSigned)
	}
	if len(got.Signatures) != 1 {
		t.Fatalf("len(signatures) = %d, want 1", len(got.Signatures))
	}
	sig := got.Signatures[0]
	if sig.Status != MediaVerifyStatusHashMismatch {
		t.Fatalf("sig.Status = %q, want %q (after tampering body)", sig.Status, MediaVerifyStatusHashMismatch)
	}
}

// Compile-time assertion the exported symbols we depend on are present in
// this build. If any of these go missing, this file will fail to compile,
// which is the right failure mode for a parity contract.
var _ = func() {
	var _ = MediaVerifyStatusValid
	var _ = MediaVerifyStatusHashMismatch
	var _ = VerifyTextStatusSigned
	var _ = VerifyImageOptions{}
	var _ = VerifyTextOptions{}
}
