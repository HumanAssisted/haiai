package haiai

import (
	"encoding/json"
	"fmt"
	"strings"
)

// getStringField extracts a string value from a map, returning "" if not found.
func getStringField(doc map[string]interface{}, key string) string {
	if v, ok := doc[key].(string); ok {
		return v
	}
	return ""
}

// GetDnsRecord returns the DNS TXT record line for an agent document.
// The agentJSON should be a serialized JACS agent document.
// Format: _v1.agent.jacs.{domain}. TTL IN TXT "v=hai.ai; jacs_agent_id=...; alg=SHA-256; enc=base64; jac_public_key_hash=..."
// If ttl is 0, defaults to 3600.
func GetDnsRecord(agentJSON string, domain string, ttl uint32) (string, error) {
	var doc map[string]interface{}
	if err := json.Unmarshal([]byte(agentJSON), &doc); err != nil {
		return "", fmt.Errorf("failed to parse agent JSON: %w", err)
	}

	jacsID := getStringField(doc, "jacsId")
	if jacsID == "" {
		jacsID = getStringField(doc, "agentId")
	}

	sig, _ := doc["jacsSignature"].(map[string]interface{})
	publicKeyHash := ""
	if sig != nil {
		publicKeyHash = getStringField(sig, "publicKeyHash")
	}

	d := strings.TrimSuffix(domain, ".")
	owner := "_v1.agent.jacs." + d + "."
	txt := "v=hai.ai; jacs_agent_id=" + jacsID + "; alg=SHA-256; enc=base64; jac_public_key_hash=" + publicKeyHash
	if ttl == 0 {
		ttl = 3600
	}
	return fmt.Sprintf("%s %d IN TXT \"%s\"", owner, ttl, txt), nil
}

// GetWellKnownJson returns the well-known JSON object for an agent document
// (e.g. for /.well-known/jacs-pubkey.json).
// The agentJSON should be a serialized JACS agent document.
// Keys: publicKey, publicKeyHash, algorithm, agentId.
func GetWellKnownJson(agentJSON string) (map[string]interface{}, error) {
	var doc map[string]interface{}
	if err := json.Unmarshal([]byte(agentJSON), &doc); err != nil {
		return nil, fmt.Errorf("failed to parse agent JSON: %w", err)
	}

	jacsID := getStringField(doc, "jacsId")
	if jacsID == "" {
		jacsID = getStringField(doc, "agentId")
	}

	sig, _ := doc["jacsSignature"].(map[string]interface{})
	publicKeyHash := ""
	if sig != nil {
		publicKeyHash = getStringField(sig, "publicKeyHash")
	}

	publicKey := getStringField(doc, "jacsPublicKey")

	return map[string]interface{}{
		"publicKey":     publicKey,
		"publicKeyHash": publicKeyHash,
		"algorithm":     "SHA-256",
		"agentId":       jacsID,
	}, nil
}
