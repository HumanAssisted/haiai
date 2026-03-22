package haiai

import "crypto/ed25519"

// Sign signs a message using the Ed25519 private key and returns the raw signature.
// Test-only helper retained for backward compatibility with unit tests.
func Sign(key ed25519.PrivateKey, message []byte) []byte {
	return ed25519.Sign(key, message)
}

// Verify checks an Ed25519 signature against a public key and message.
// Test-only helper retained for backward compatibility with unit tests.
func Verify(publicKey ed25519.PublicKey, message, signature []byte) bool {
	if len(publicKey) != ed25519.PublicKeySize {
		return false
	}
	return ed25519.Verify(publicKey, message, signature)
}

// PublicKeyFromPrivate extracts the public key from a private key.
// Test-only helper retained for backward compatibility with unit tests.
func PublicKeyFromPrivate(key ed25519.PrivateKey) ed25519.PublicKey {
	return key.Public().(ed25519.PublicKey)
}

// GenerateKeyPair generates a new Ed25519 key pair.
// Returns (publicKey, privateKey, error).
// Test-only helper retained for backward compatibility with unit tests.
func GenerateKeyPair() (ed25519.PublicKey, ed25519.PrivateKey, error) {
	pub, priv, err := ed25519.GenerateKey(nil)
	if err != nil {
		return nil, nil, wrapError(ErrSigningFailed, err, "failed to generate Ed25519 key pair")
	}
	return pub, priv, nil
}
