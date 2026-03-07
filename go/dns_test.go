package haiai

import (
	"encoding/json"
	"strings"
	"testing"
)

// ===========================================================================
// GetDnsRecord tests
// ===========================================================================

func TestGetDnsRecordBasic(t *testing.T) {
	agentJSON := `{"jacsId":"agent-123","jacsSignature":{"publicKeyHash":"hash-abc"}}`
	record, err := GetDnsRecord(agentJSON, "example.com", 0)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(record, "_v1.agent.jacs.example.com.") {
		t.Errorf("expected owner prefix, got '%s'", record)
	}
	if !strings.Contains(record, "jacs_agent_id=agent-123") {
		t.Errorf("expected agent ID in record, got '%s'", record)
	}
	if !strings.Contains(record, "jac_public_key_hash=hash-abc") {
		t.Errorf("expected public key hash in record, got '%s'", record)
	}
	if !strings.Contains(record, "v=hai.ai") {
		t.Errorf("expected v=hai.ai in record, got '%s'", record)
	}
}

func TestGetDnsRecordDefaultTTL(t *testing.T) {
	agentJSON := `{"jacsId":"a","jacsSignature":{"publicKeyHash":"h"}}`
	record, err := GetDnsRecord(agentJSON, "example.com", 0)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(record, " 3600 IN TXT") {
		t.Errorf("expected default TTL 3600, got '%s'", record)
	}
}

func TestGetDnsRecordCustomTTL(t *testing.T) {
	agentJSON := `{"jacsId":"a","jacsSignature":{"publicKeyHash":"h"}}`
	record, err := GetDnsRecord(agentJSON, "example.com", 300)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(record, " 300 IN TXT") {
		t.Errorf("expected TTL 300, got '%s'", record)
	}
}

func TestGetDnsRecordStripsDot(t *testing.T) {
	agentJSON := `{"jacsId":"a","jacsSignature":{"publicKeyHash":"h"}}`
	record, err := GetDnsRecord(agentJSON, "example.com.", 0)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// Should not have double dots
	if strings.Contains(record, "example.com..") {
		t.Errorf("double dot in record: '%s'", record)
	}
	if !strings.HasPrefix(record, "_v1.agent.jacs.example.com.") {
		t.Errorf("expected properly formatted owner, got '%s'", record)
	}
}

func TestGetDnsRecordFallbackAgentId(t *testing.T) {
	agentJSON := `{"agentId":"fallback-id","jacsSignature":{"publicKeyHash":"h"}}`
	record, err := GetDnsRecord(agentJSON, "example.com", 0)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(record, "jacs_agent_id=fallback-id") {
		t.Errorf("expected fallback agentId, got '%s'", record)
	}
}

func TestGetDnsRecordInvalidJSON(t *testing.T) {
	_, err := GetDnsRecord("not-json", "example.com", 0)
	if err == nil {
		t.Fatal("expected error for invalid JSON")
	}
}

// ===========================================================================
// GetWellKnownJson tests
// ===========================================================================

func TestGetWellKnownJsonBasic(t *testing.T) {
	agentJSON := `{"jacsId":"agent-123","jacsPublicKey":"-----BEGIN PUBLIC KEY-----\ntest\n-----END PUBLIC KEY-----","jacsSignature":{"publicKeyHash":"hash-abc"}}`
	result, err := GetWellKnownJson(agentJSON)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result["agentId"] != "agent-123" {
		t.Errorf("expected agentId 'agent-123', got '%v'", result["agentId"])
	}
	if result["publicKeyHash"] != "hash-abc" {
		t.Errorf("expected publicKeyHash 'hash-abc', got '%v'", result["publicKeyHash"])
	}
	if result["algorithm"] != "SHA-256" {
		t.Errorf("expected algorithm 'SHA-256', got '%v'", result["algorithm"])
	}
	if result["publicKey"] == nil || result["publicKey"] == "" {
		t.Error("expected publicKey to be set")
	}
}

func TestGetWellKnownJsonFallbackAgentId(t *testing.T) {
	agentJSON := `{"agentId":"fallback-id","jacsSignature":{"publicKeyHash":"h"}}`
	result, err := GetWellKnownJson(agentJSON)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result["agentId"] != "fallback-id" {
		t.Errorf("expected fallback agentId, got '%v'", result["agentId"])
	}
}

func TestGetWellKnownJsonSerializesToJSON(t *testing.T) {
	agentJSON := `{"jacsId":"agent-1","jacsSignature":{"publicKeyHash":"h1"}}`
	result, err := GetWellKnownJson(agentJSON)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	data, err := json.Marshal(result)
	if err != nil {
		t.Fatalf("failed to marshal: %v", err)
	}
	if len(data) == 0 {
		t.Error("expected non-empty JSON")
	}
}

func TestGetWellKnownJsonInvalidJSON(t *testing.T) {
	_, err := GetWellKnownJson("not-json")
	if err == nil {
		t.Fatal("expected error for invalid JSON")
	}
}

func TestGetWellKnownJsonMissingSignature(t *testing.T) {
	agentJSON := `{"jacsId":"agent-1"}`
	result, err := GetWellKnownJson(agentJSON)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// Should still work, publicKeyHash will be empty
	if result["publicKeyHash"] != "" {
		t.Errorf("expected empty publicKeyHash, got '%v'", result["publicKeyHash"])
	}
}
