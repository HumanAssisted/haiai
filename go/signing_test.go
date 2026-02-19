package haisdk

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/x509"
	"encoding/pem"
	"os"
	"path/filepath"
	"testing"
)

func TestGenerateKeyPair(t *testing.T) {
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	if len(pub) != ed25519.PublicKeySize {
		t.Errorf("expected public key size %d, got %d", ed25519.PublicKeySize, len(pub))
	}
	if len(priv) != ed25519.PrivateKeySize {
		t.Errorf("expected private key size %d, got %d", ed25519.PrivateKeySize, len(priv))
	}
}

func TestSignAndVerify(t *testing.T) {
	pub, priv, _ := GenerateKeyPair()

	message := []byte("test-agent:1700000000")
	sig := Sign(priv, message)

	if !Verify(pub, message, sig) {
		t.Error("signature should verify with correct key and message")
	}
}

func TestVerifyFailsWithWrongKey(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	otherPub, _, _ := GenerateKeyPair()

	message := []byte("test message")
	sig := Sign(priv, message)

	if Verify(otherPub, message, sig) {
		t.Error("signature should not verify with wrong key")
	}
}

func TestVerifyFailsWithWrongMessage(t *testing.T) {
	pub, priv, _ := GenerateKeyPair()

	sig := Sign(priv, []byte("original message"))

	if Verify(pub, []byte("different message"), sig) {
		t.Error("signature should not verify with wrong message")
	}
}

func TestVerifyFailsWithInvalidPublicKey(t *testing.T) {
	if Verify([]byte("short"), []byte("msg"), []byte("sig")) {
		t.Error("should fail with invalid public key")
	}
}

func TestPublicKeyFromPrivate(t *testing.T) {
	pub, priv, _ := GenerateKeyPair()

	derived := PublicKeyFromPrivate(priv)
	if !pub.Equal(derived) {
		t.Error("derived public key should match generated public key")
	}
}

func TestLoadPrivateKey(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	seed := priv.Seed()

	// Write PEM file with raw seed
	pemBlock := &pem.Block{
		Type:  "PRIVATE KEY",
		Bytes: seed,
	}
	pemData := pem.EncodeToMemory(pemBlock)

	tmpDir := t.TempDir()
	keyPath := filepath.Join(tmpDir, "test.private.pem")
	if err := os.WriteFile(keyPath, pemData, 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	loaded, err := LoadPrivateKey(keyPath, []byte("test-password"))
	if err != nil {
		t.Fatalf("LoadPrivateKey: %v", err)
	}

	// Verify the loaded key produces the same signatures
	msg := []byte("test message")
	sig1 := Sign(priv, msg)
	sig2 := Sign(loaded, msg)

	pub := PublicKeyFromPrivate(priv)
	if !Verify(pub, msg, sig1) || !Verify(pub, msg, sig2) {
		t.Error("both keys should produce valid signatures")
	}
}

func TestLoadPrivateKeyFullKey(t *testing.T) {
	_, priv, _ := GenerateKeyPair()

	// Write PEM file with full 64-byte key
	pemBlock := &pem.Block{
		Type:  "PRIVATE KEY",
		Bytes: priv,
	}
	pemData := pem.EncodeToMemory(pemBlock)

	tmpDir := t.TempDir()
	keyPath := filepath.Join(tmpDir, "test.private.pem")
	if err := os.WriteFile(keyPath, pemData, 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	loaded, err := LoadPrivateKey(keyPath, []byte("test-password"))
	if err != nil {
		t.Fatalf("LoadPrivateKey: %v", err)
	}

	if !priv.Equal(loaded) {
		t.Error("loaded key should equal original key")
	}
}

func TestLoadPrivateKeyPKCS8(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	seed := priv.Seed()

	// Construct a PKCS8-like DER structure for Ed25519:
	// 30 2e 02 01 00 30 05 06 03 2b 65 70 04 22 04 20 [32-byte seed]
	der := make([]byte, 0, 48)
	der = append(der, 0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20)
	der = append(der, seed...)

	pemBlock := &pem.Block{
		Type:  "PRIVATE KEY",
		Bytes: der,
	}
	pemData := pem.EncodeToMemory(pemBlock)

	tmpDir := t.TempDir()
	keyPath := filepath.Join(tmpDir, "test.private.pem")
	if err := os.WriteFile(keyPath, pemData, 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	loaded, err := LoadPrivateKey(keyPath, []byte("test-password"))
	if err != nil {
		t.Fatalf("LoadPrivateKey: %v", err)
	}

	// Verify loaded key works
	msg := []byte("pkcs8 test")
	sig := Sign(loaded, msg)
	pub := PublicKeyFromPrivate(priv)
	if !Verify(pub, msg, sig) {
		t.Error("PKCS8-loaded key should produce valid signatures")
	}
}

func TestLoadPrivateKeyEncryptedPEM(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	seed := priv.Seed()
	password := []byte("test-password")

	encBlock, err := x509.EncryptPEMBlock(
		rand.Reader,
		"PRIVATE KEY",
		seed,
		password,
		x509.PEMCipherAES256,
	)
	if err != nil {
		t.Fatalf("EncryptPEMBlock: %v", err)
	}

	tmpDir := t.TempDir()
	keyPath := filepath.Join(tmpDir, "encrypted.private.pem")
	if err := os.WriteFile(keyPath, pem.EncodeToMemory(encBlock), 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	loaded, err := LoadPrivateKey(keyPath, password)
	if err != nil {
		t.Fatalf("LoadPrivateKey: %v", err)
	}

	msg := []byte("encrypted key test")
	sig := Sign(loaded, msg)
	pub := PublicKeyFromPrivate(priv)
	if !Verify(pub, msg, sig) {
		t.Fatal("encrypted key should verify after load")
	}
}

func TestLoadPrivateKeyRequiresPassword(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	seed := priv.Seed()

	pemBlock := &pem.Block{Type: "PRIVATE KEY", Bytes: seed}
	tmpDir := t.TempDir()
	keyPath := filepath.Join(tmpDir, "test.private.pem")
	if err := os.WriteFile(keyPath, pem.EncodeToMemory(pemBlock), 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	_, err := LoadPrivateKey(keyPath, nil)
	if err == nil {
		t.Fatal("expected password-required error")
	}
}

func TestLoadPrivateKeyNotFound(t *testing.T) {
	_, err := LoadPrivateKey("/nonexistent/key.pem", []byte("test-password"))
	if err == nil {
		t.Fatal("expected error for missing key file")
	}

	sdkErr, ok := err.(*Error)
	if !ok {
		t.Fatalf("expected *Error, got %T", err)
	}
	if sdkErr.Kind != ErrSigningFailed {
		t.Errorf("expected ErrSigningFailed, got %v", sdkErr.Kind)
	}
}

func TestParsePrivateKeyInvalidPEM(t *testing.T) {
	_, err := ParsePrivateKey([]byte("not valid PEM data"))
	if err == nil {
		t.Fatal("expected error for invalid PEM")
	}
}
