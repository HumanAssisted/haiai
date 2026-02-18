package haisdk

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/pem"
	"errors"
	"os"
)

// LoadPrivateKey loads an Ed25519 private key from a PEM file.
//
// Supports two PEM formats:
//   - PKCS8 (48 bytes DER): standard "PRIVATE KEY" PEM with ASN.1 wrapper
//   - Raw seed (32 bytes): just the seed bytes in PEM
//   - Full key (64 bytes): seed + public key concatenated
func LoadPrivateKey(path string) (ed25519.PrivateKey, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, newError(ErrSigningFailed, "private key not found: %s", path)
		}
		return nil, wrapError(ErrSigningFailed, err, "failed to read private key: %s", path)
	}

	return ParsePrivateKey(data)
}

// ParsePrivateKey parses an Ed25519 private key from PEM-encoded bytes.
func ParsePrivateKey(pemData []byte) (ed25519.PrivateKey, error) {
	block, _ := pem.Decode(pemData)
	if block == nil {
		return nil, newError(ErrSigningFailed, "no PEM block found in private key data")
	}

	return parsePrivateKeyDER(block.Bytes)
}

// parsePrivateKeyDER extracts an Ed25519 private key from DER bytes.
func parsePrivateKeyDER(der []byte) (ed25519.PrivateKey, error) {
	switch len(der) {
	case ed25519.PrivateKeySize:
		// Full 64-byte key (seed + public)
		return ed25519.PrivateKey(der), nil

	case ed25519.SeedSize:
		// 32-byte seed only
		return ed25519.NewKeyFromSeed(der), nil

	default:
		// Try PKCS8-style: look for the 32-byte seed inside ASN.1 structure.
		// Ed25519 PKCS8 structure:
		//   30 2e 02 01 00 30 05 06 03 2b 65 70 04 22 04 20 [32 bytes seed]
		// The seed is the last 32 bytes if length is 48.
		if len(der) == 48 {
			seed := der[len(der)-ed25519.SeedSize:]
			return ed25519.NewKeyFromSeed(seed), nil
		}

		// Generic ASN.1: search for the OCTET STRING containing the seed.
		// Look for 04 20 (OCTET STRING, length 32) followed by 32 bytes.
		for i := 0; i+2+ed25519.SeedSize <= len(der); i++ {
			if der[i] == 0x04 && der[i+1] == 0x20 {
				seed := der[i+2 : i+2+ed25519.SeedSize]
				return ed25519.NewKeyFromSeed(seed), nil
			}
		}

		return nil, newError(ErrSigningFailed,
			"unsupported private key format (length %d bytes)", len(der))
	}
}

// Sign signs a message using the Ed25519 private key and returns the raw signature.
func Sign(key ed25519.PrivateKey, message []byte) []byte {
	return ed25519.Sign(key, message)
}

// Verify checks an Ed25519 signature against a public key and message.
func Verify(publicKey ed25519.PublicKey, message, signature []byte) bool {
	if len(publicKey) != ed25519.PublicKeySize {
		return false
	}
	return ed25519.Verify(publicKey, message, signature)
}

// PublicKeyFromPrivate extracts the public key from a private key.
func PublicKeyFromPrivate(key ed25519.PrivateKey) ed25519.PublicKey {
	return key.Public().(ed25519.PublicKey)
}

// GenerateKeyPair generates a new Ed25519 key pair.
// Returns (publicKey, privateKey, error).
func GenerateKeyPair() (ed25519.PublicKey, ed25519.PrivateKey, error) {
	pub, priv, err := ed25519.GenerateKey(nil)
	if err != nil {
		return nil, nil, wrapError(ErrSigningFailed, err, "failed to generate Ed25519 key pair")
	}
	return pub, priv, nil
}

// ErrInvalidKeyFormat is returned when a key cannot be parsed.
var ErrInvalidKeyFormat = errors.New("invalid key format")

// ParsePublicKey parses an Ed25519 public key from PEM-encoded bytes.
func ParsePublicKey(pemData []byte) (ed25519.PublicKey, error) {
	block, _ := pem.Decode(pemData)
	if block == nil {
		return nil, newError(ErrSigningFailed, "no PEM block found in public key data")
	}

	der := block.Bytes

	switch len(der) {
	case ed25519.PublicKeySize:
		// Raw 32-byte public key
		return ed25519.PublicKey(der), nil
	default:
		// SPKI format: the last 32 bytes contain the key.
		// Ed25519 SPKI structure:
		//   30 2a 30 05 06 03 2b 65 70 03 21 00 [32 bytes]
		if len(der) >= ed25519.PublicKeySize {
			pubBytes := der[len(der)-ed25519.PublicKeySize:]
			return ed25519.PublicKey(pubBytes), nil
		}
		return nil, newError(ErrSigningFailed,
			"unsupported public key format (length %d bytes)", len(der))
	}
}

// VerifyHaiMessage verifies a message signed by HAI or another agent.
// The signature is expected to be base64-encoded. The publicKeyPem
// should be a PEM-encoded Ed25519 public key.
func VerifyHaiMessage(message string, signatureB64 string, publicKeyPem string) (bool, error) {
	if message == "" || signatureB64 == "" || publicKeyPem == "" {
		return false, nil
	}

	sigBytes, err := base64.StdEncoding.DecodeString(signatureB64)
	if err != nil {
		// Try URL-safe base64
		sigBytes, err = base64.RawStdEncoding.DecodeString(signatureB64)
		if err != nil {
			return false, nil
		}
	}

	pubKey, err := ParsePublicKey([]byte(publicKeyPem))
	if err != nil {
		return false, err
	}

	return Verify(pubKey, []byte(message), sigBytes), nil
}
