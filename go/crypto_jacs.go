//go:build cgo && jacs

package haiai

import (
	"crypto/ed25519"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"log"
	"os"
	"sync"

	jacs "github.com/HumanAssisted/jacsgo"
)

func init() {
	cryptoBackend = &jacsBackend{}
}

// jacsBackend implements CryptoBackend using the JACS Rust core via CGo.
// The package-level instance provides GenerateKeyPair and standalone verify.
type jacsBackend struct{}

func (b *jacsBackend) SignString(message string) (string, error) {
	return "", fmt.Errorf("jacs backend: SignString requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) SignBytes(data []byte) ([]byte, error) {
	return nil, fmt.Errorf("jacs backend: SignBytes requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) VerifyBytes(data, signature []byte, publicKeyPEM string) error {
	// Use the JACS standalone verify path
	sigB64 := base64.StdEncoding.EncodeToString(signature)
	return jacs.VerifyString(string(data), sigB64, []byte(publicKeyPEM), "pem")
}

func (b *jacsBackend) SignRequest(payloadJSON string) (string, error) {
	return jacs.SignRequest(json.RawMessage(payloadJSON))
}

func (b *jacsBackend) VerifyResponse(documentJSON string) (string, error) {
	result, err := jacs.VerifyResponse(documentJSON)
	if err != nil {
		return "", err
	}
	encoded, err := json.Marshal(result)
	if err != nil {
		return "", fmt.Errorf("jacs backend: failed to marshal verify result: %w", err)
	}
	return string(encoded), nil
}

// jacsKeygenWarningOnce guards the one-time deprecation notice for Ed25519 keygen
// in JACS builds.
var jacsKeygenWarningOnce sync.Once

func (b *jacsBackend) GenerateKeyPair() ([]byte, []byte, error) {
	// KNOWN LIMITATION: Generates Ed25519 keys locally because the JACS Go binding
	// (jacsgo) does not yet expose pq2025 key generation via FFI. The JACS core
	// supports pq2025 internally, but that functionality is not available from Go.
	// The actual signing with these keys still goes through the CryptoBackend
	// (JACS agent or Ed25519 fallback).
	//
	// TODO: Replace with jacs.GenerateKeyPair(algorithm) when jacsgo exposes pq2025 keygen FFI
	jacsKeygenWarningOnce.Do(func() {
		log.Println("WARNING: JACS backend GenerateKeyPair uses local Ed25519 — jacsgo does not yet expose pq2025 keygen FFI")
	})
	pub, priv, err := ed25519.GenerateKey(nil)
	if err != nil {
		return nil, nil, fmt.Errorf("jacs backend: key generation failed: %w", err)
	}

	pubDER, err := x509.MarshalPKIXPublicKey(pub)
	if err != nil {
		return nil, nil, fmt.Errorf("jacs backend: failed to marshal public key: %w", err)
	}
	pubPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pubDER})

	privDER, err := x509.MarshalPKCS8PrivateKey(priv)
	if err != nil {
		return nil, nil, fmt.Errorf("jacs backend: failed to marshal private key: %w", err)
	}
	privPEM := pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: privDER})

	return pubPEM, privPEM, nil
}

func (b *jacsBackend) Algorithm() string {
	return "JACS"
}

func (b *jacsBackend) CanonicalizeJSON(jsonStr string) (string, error) {
	return "", fmt.Errorf("jacs backend: CanonicalizeJSON requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) SignResponse(payloadJSON string) (string, error) {
	return "", fmt.Errorf("jacs backend: SignResponse requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) EncodeVerifyPayload(document string) (string, error) {
	return "", fmt.Errorf("jacs backend: EncodeVerifyPayload requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error) {
	return "", fmt.Errorf("jacs backend: UnwrapSignedEvent requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) BuildAuthHeader() (string, error) {
	return "", fmt.Errorf("jacs backend: BuildAuthHeader requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) SignA2AArtifact(artifactJSON string, artifactType string) (string, error) {
	return "", fmt.Errorf("jacs backend: SignA2AArtifact requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) VerifyA2AArtifact(wrappedJSON string) (string, error) {
	return "", fmt.Errorf("jacs backend: VerifyA2AArtifact requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("jacs backend: VerifyA2AArtifactWithPolicy requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) AssessA2AAgent(agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("jacs backend: AssessA2AAgent requires a loaded agent; use Client.crypto instead")
}

func (b *jacsBackend) ExportAgentCard(agentDataJSON string) (string, error) {
	return "", fmt.Errorf("jacs backend: ExportAgentCard requires a loaded agent; use Client.crypto instead")
}

// clientJacsBackend implements CryptoBackend bound to a loaded JACS agent for
// a specific Client.
type clientJacsBackend struct {
	agent  *jacs.JacsAgent
	jacsID string
}

func (b *clientJacsBackend) SignString(message string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.SignString(message)
}

func (b *clientJacsBackend) SignBytes(data []byte) ([]byte, error) {
	if b.agent == nil {
		return nil, fmt.Errorf("jacs backend: agent not loaded")
	}
	sigB64, err := b.agent.SignString(string(data))
	if err != nil {
		return nil, err
	}
	return base64.StdEncoding.DecodeString(sigB64)
}

func (b *clientJacsBackend) VerifyBytes(data, signature []byte, publicKeyPEM string) error {
	if b.agent == nil {
		return fmt.Errorf("jacs backend: agent not loaded")
	}
	sigB64 := base64.StdEncoding.EncodeToString(signature)
	return b.agent.VerifyString(string(data), sigB64, []byte(publicKeyPEM), "pem")
}

func (b *clientJacsBackend) SignRequest(payloadJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.SignRequest(json.RawMessage(payloadJSON))
}

func (b *clientJacsBackend) VerifyResponse(documentJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	result, err := b.agent.VerifyResponse(documentJSON)
	if err != nil {
		return "", err
	}
	encoded, err := json.Marshal(result)
	if err != nil {
		return "", fmt.Errorf("jacs backend: failed to marshal verify result: %w", err)
	}
	return string(encoded), nil
}

func (b *clientJacsBackend) GenerateKeyPair() ([]byte, []byte, error) {
	return cryptoBackend.GenerateKeyPair()
}

func (b *clientJacsBackend) Algorithm() string {
	return "JACS"
}

func (b *clientJacsBackend) CanonicalizeJSON(jsonStr string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.CanonicalizeJson(jsonStr)
}

func (b *clientJacsBackend) SignResponse(payloadJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.SignResponse(payloadJSON)
}

func (b *clientJacsBackend) EncodeVerifyPayload(document string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.EncodeVerifyPayload(document)
}

func (b *clientJacsBackend) UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.UnwrapSignedEvent(eventJSON, serverKeysJSON)
}

func (b *clientJacsBackend) BuildAuthHeader() (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.BuildAuthHeader()
}

func (b *clientJacsBackend) SignA2AArtifact(artifactJSON string, artifactType string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.SignA2AArtifact(artifactJSON, artifactType)
}

func (b *clientJacsBackend) VerifyA2AArtifact(wrappedJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.VerifyA2AArtifact(wrappedJSON)
}

func (b *clientJacsBackend) VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON)
}

func (b *clientJacsBackend) AssessA2AAgent(agentCardJSON, policyJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	return b.agent.AssessA2AAgent(agentCardJSON, policyJSON)
}

func (b *clientJacsBackend) ExportAgentCard(agentDataJSON string) (string, error) {
	if b.agent == nil {
		return "", fmt.Errorf("jacs backend: agent not loaded")
	}
	// The JACS core ExportAgentCard uses the loaded agent's own metadata.
	// The agentDataJSON parameter is used by the Go-side orchestration in a2a.go
	// to overlay additional fields; the JACS core ignores it.
	return b.agent.ExportAgentCard()
}

// newClientCryptoBackend creates a per-client JACS CryptoBackend.
// In JACS mode, it attempts to load a JACS agent from config.
// Falls back to wrapping the Ed25519 private key if agent loading fails.
func newClientCryptoBackend(privateKey ed25519.PrivateKey, jacsID string) CryptoBackend {
	// Try loading a JACS agent from config
	agent, err := jacs.NewJacsAgent()
	if err == nil {
		configPath := discoverConfigPath()
		if configPath != "" {
			if loadErr := agent.Load(configPath); loadErr == nil {
				return &clientJacsBackend{
					agent:  agent,
					jacsID: jacsID,
				}
			}
		}
		agent.Close()
	}

	// JACS agent could not be loaded -- return error-only backend.
	// In v0.2.0+, falling back to local Ed25519 is no longer allowed.
	log.Printf("WARNING: JACS agent could not be loaded (err=%v). "+
		"All crypto operations will fail. Run 'haiai init' or set JACS_CONFIG_PATH.", err)
	return &jacsNotLoadedBackend{
		loadErr: err,
	}
}

// jacsNotLoadedBackend is returned when the JACS agent cannot be loaded in a
// jacs-tagged build. Every operation returns a clear error directing the
// developer to load a JACS agent. This replaces the old
// clientEd25519FallbackInJacs that silently signed with local Ed25519.
type jacsNotLoadedBackend struct {
	loadErr error
}

func (b *jacsNotLoadedBackend) errMsg(op string) string {
	return fmt.Sprintf("%s requires a loaded JACS agent (load error: %v). Run 'haiai init' or set JACS_CONFIG_PATH", op, b.loadErr)
}

func (b *jacsNotLoadedBackend) SignString(message string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("SignString"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) SignBytes(data []byte) ([]byte, error) {
	return nil, &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("SignBytes"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) VerifyBytes(data, signature []byte, publicKeyPEM string) error {
	// Standalone verify can use the module-level JACS backend
	return cryptoBackend.VerifyBytes(data, signature, publicKeyPEM)
}

func (b *jacsNotLoadedBackend) SignRequest(payloadJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("SignRequest"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) VerifyResponse(documentJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("VerifyResponse"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) GenerateKeyPair() ([]byte, []byte, error) {
	return cryptoBackend.GenerateKeyPair()
}

func (b *jacsNotLoadedBackend) Algorithm() string {
	return "JACS (not loaded)"
}

func (b *jacsNotLoadedBackend) CanonicalizeJSON(jsonStr string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("CanonicalizeJSON"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) SignResponse(payloadJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("SignResponse"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) EncodeVerifyPayload(document string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("EncodeVerifyPayload"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("UnwrapSignedEvent"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) BuildAuthHeader() (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("BuildAuthHeader"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) SignA2AArtifact(artifactJSON string, artifactType string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("SignA2AArtifact"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) VerifyA2AArtifact(wrappedJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("VerifyA2AArtifact"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("VerifyA2AArtifactWithPolicy"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) AssessA2AAgent(agentCardJSON, policyJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("AssessA2AAgent"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

func (b *jacsNotLoadedBackend) ExportAgentCard(agentDataJSON string) (string, error) {
	return "", &Error{Kind: ErrJacsNotLoaded, Message: b.errMsg("ExportAgentCard"), Action: "Run 'haiai init' or set JACS_CONFIG_PATH"}
}

// discoverConfigPath returns the first existing jacs config path, or empty string.
func discoverConfigPath() string {
	candidates := []string{
		os.Getenv("JACS_CONFIG_PATH"),
		"./jacs.config.json",
	}
	home, err := os.UserHomeDir()
	if err == nil {
		candidates = append(candidates, home+"/.jacs/jacs.config.json")
	}
	for _, p := range candidates {
		if p == "" {
			continue
		}
		if _, err := os.Stat(p); err == nil {
			return p
		}
	}
	return ""
}
