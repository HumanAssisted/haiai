//go:build !cgo || !jacs

package haiai

import (
	"crypto/ed25519"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"os"
	"sort"
	"sync"
)

func init() {
	cryptoBackend = &ed25519Fallback{}
	printFallbackWarning()
}

var fallbackWarningOnce sync.Once

func printFallbackWarning() {
	fallbackWarningOnce.Do(func() {
		// Only print if not in test mode
		if os.Getenv("HAIAI_QUIET_FALLBACK") == "" {
			fmt.Fprintln(os.Stderr,
				"WARNING: haiai using local Ed25519 fallback crypto. "+
					"Build with '-tags jacs' and install JACS for full algorithm support (Ed25519, RSA-PSS, PQ2025).")
		}
	})
}

// ed25519Fallback implements CryptoBackend using Go's crypto/ed25519.
type ed25519Fallback struct{}

func (f *ed25519Fallback) SignString(message string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: SignString requires a loaded private key; use Client.crypto instead")
}

func (f *ed25519Fallback) SignBytes(data []byte) ([]byte, error) {
	return nil, fmt.Errorf("ed25519 fallback: SignBytes requires a loaded private key; use Client.crypto instead")
}

func (f *ed25519Fallback) VerifyBytes(data, signature []byte, publicKeyPEM string) error {
	pubKey, err := ParsePublicKey([]byte(publicKeyPEM))
	if err != nil {
		return fmt.Errorf("ed25519 fallback: %w", err)
	}
	if !ed25519.Verify(pubKey, data, signature) {
		return fmt.Errorf("ed25519 fallback: signature verification failed")
	}
	return nil
}

func (f *ed25519Fallback) SignRequest(payloadJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: SignRequest requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) VerifyResponse(documentJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: VerifyResponse requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) GenerateKeyPair() ([]byte, []byte, error) {
	pub, priv, err := ed25519.GenerateKey(nil)
	if err != nil {
		return nil, nil, fmt.Errorf("ed25519 fallback: key generation failed: %w", err)
	}

	pubDER, err := x509.MarshalPKIXPublicKey(pub)
	if err != nil {
		return nil, nil, fmt.Errorf("ed25519 fallback: failed to marshal public key: %w", err)
	}
	pubPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pubDER})

	privDER, err := x509.MarshalPKCS8PrivateKey(priv)
	if err != nil {
		return nil, nil, fmt.Errorf("ed25519 fallback: failed to marshal private key: %w", err)
	}
	privPEM := pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: privDER})

	return pubPEM, privPEM, nil
}

func (f *ed25519Fallback) Algorithm() string {
	return "Ed25519"
}

func (f *ed25519Fallback) CanonicalizeJSON(jsonStr string) (string, error) {
	return canonicalizeJSONLocal(jsonStr)
}

func (f *ed25519Fallback) SignResponse(payloadJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: SignResponse requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) EncodeVerifyPayload(document string) (string, error) {
	return base64.RawURLEncoding.EncodeToString([]byte(document)), nil
}

func (f *ed25519Fallback) UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: UnwrapSignedEvent requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) BuildAuthHeader() (string, error) {
	return "", fmt.Errorf("ed25519 fallback: BuildAuthHeader requires a loaded private key; use Client.buildAuthHeader instead")
}

func (f *ed25519Fallback) SignA2AArtifact(artifactJSON string, artifactType string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: SignA2AArtifact requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) VerifyA2AArtifact(wrappedJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: VerifyA2AArtifact requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: VerifyA2AArtifactWithPolicy requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) AssessA2AAgent(agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: AssessA2AAgent requires JACS backend; build with '-tags jacs'")
}

func (f *ed25519Fallback) ExportAgentCard(agentDataJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: ExportAgentCard requires JACS backend; build with '-tags jacs'")
}

// clientEd25519Backend implements CryptoBackend bound to a specific Client's
// private key. This is the per-client backend used in fallback mode.
type clientEd25519Backend struct {
	privateKey ed25519.PrivateKey
	jacsID     string
}

func (b *clientEd25519Backend) SignString(message string) (string, error) {
	if b.privateKey == nil {
		return "", fmt.Errorf("ed25519 fallback: private key not loaded")
	}
	sig := ed25519.Sign(b.privateKey, []byte(message))
	return base64.StdEncoding.EncodeToString(sig), nil
}

func (b *clientEd25519Backend) SignBytes(data []byte) ([]byte, error) {
	if b.privateKey == nil {
		return nil, fmt.Errorf("ed25519 fallback: private key not loaded")
	}
	return ed25519.Sign(b.privateKey, data), nil
}

func (b *clientEd25519Backend) VerifyBytes(data, signature []byte, publicKeyPEM string) error {
	pubKey, err := ParsePublicKey([]byte(publicKeyPEM))
	if err != nil {
		return fmt.Errorf("ed25519 fallback: %w", err)
	}
	if !ed25519.Verify(pubKey, data, signature) {
		return fmt.Errorf("ed25519 fallback: signature verification failed")
	}
	return nil
}

func (b *clientEd25519Backend) SignRequest(payloadJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: SignRequest requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) VerifyResponse(documentJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: VerifyResponse requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) GenerateKeyPair() ([]byte, []byte, error) {
	return cryptoBackend.GenerateKeyPair()
}

func (b *clientEd25519Backend) Algorithm() string {
	return "Ed25519"
}

func (b *clientEd25519Backend) CanonicalizeJSON(jsonStr string) (string, error) {
	return canonicalizeJSONLocal(jsonStr)
}

func (b *clientEd25519Backend) SignResponse(payloadJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: SignResponse requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) EncodeVerifyPayload(document string) (string, error) {
	return base64.RawURLEncoding.EncodeToString([]byte(document)), nil
}

func (b *clientEd25519Backend) UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: UnwrapSignedEvent requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) BuildAuthHeader() (string, error) {
	if b.privateKey == nil {
		return "", fmt.Errorf("ed25519 fallback: private key not loaded")
	}
	return BuildAuthHeader(b.jacsID, b.privateKey), nil
}

func (b *clientEd25519Backend) SignA2AArtifact(artifactJSON string, artifactType string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: SignA2AArtifact requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) VerifyA2AArtifact(wrappedJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: VerifyA2AArtifact requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: VerifyA2AArtifactWithPolicy requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) AssessA2AAgent(agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: AssessA2AAgent requires JACS backend; build with '-tags jacs'")
}

func (b *clientEd25519Backend) ExportAgentCard(agentDataJSON string) (string, error) {
	return "", fmt.Errorf("ed25519 fallback: ExportAgentCard requires JACS backend; build with '-tags jacs'")
}

// newClientCryptoBackend creates a per-client CryptoBackend for fallback mode.
func newClientCryptoBackend(privateKey ed25519.PrivateKey, jacsID string) CryptoBackend {
	return &clientEd25519Backend{
		privateKey: privateKey,
		jacsID:     jacsID,
	}
}

// canonicalizeJSONLocal produces canonical JSON with sorted keys (local fallback).
// This is the Go equivalent of RFC 8785 for simple cases: parse, sort keys
// recursively, re-serialize with compact separators.
func canonicalizeJSONLocal(jsonStr string) (string, error) {
	var raw interface{}
	if err := json.Unmarshal([]byte(jsonStr), &raw); err != nil {
		return "", fmt.Errorf("canonicalize: invalid JSON: %w", err)
	}
	sorted := sortKeys(raw)
	result, err := json.Marshal(sorted)
	if err != nil {
		return "", fmt.Errorf("canonicalize: marshal failed: %w", err)
	}
	return string(result), nil
}

// sortKeys recursively sorts map keys for canonical JSON output.
func sortKeys(v interface{}) interface{} {
	switch val := v.(type) {
	case map[string]interface{}:
		keys := make([]string, 0, len(val))
		for k := range val {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		sorted := make(orderedMap, 0, len(val))
		for _, k := range keys {
			sorted = append(sorted, orderedEntry{k, sortKeys(val[k])})
		}
		return sorted
	case []interface{}:
		result := make([]interface{}, len(val))
		for i, item := range val {
			result[i] = sortKeys(item)
		}
		return result
	default:
		return v
	}
}

// orderedEntry holds a key-value pair for ordered JSON serialization.
type orderedEntry struct {
	Key   string
	Value interface{}
}

// orderedMap is a slice of entries that serializes with insertion order preserved.
type orderedMap []orderedEntry

func (om orderedMap) MarshalJSON() ([]byte, error) {
	buf := []byte{'{'}
	for i, entry := range om {
		if i > 0 {
			buf = append(buf, ',')
		}
		key, err := json.Marshal(entry.Key)
		if err != nil {
			return nil, err
		}
		val, err := json.Marshal(entry.Value)
		if err != nil {
			return nil, err
		}
		buf = append(buf, key...)
		buf = append(buf, ':')
		buf = append(buf, val...)
	}
	buf = append(buf, '}')
	return buf, nil
}
