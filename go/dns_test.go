package haiai

import (
	"encoding/json"
	"os"
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
	if !strings.Contains(record, "jacs_public_key_hash=hash-abc") {
		t.Errorf("expected public key hash in record, got '%s'", record)
	}
	if !strings.Contains(record, "v=hai.ai") {
		t.Errorf("expected v=hai.ai in record, got '%s'", record)
	}
}

func TestGetDnsRecordMatchesSharedFixture(t *testing.T) {
	data, err := os.ReadFile("../fixtures/dns_txt_record.json")
	if err != nil {
		t.Fatalf("read DNS fixture: %v", err)
	}
	var fixture struct {
		AgentJSON                map[string]interface{} `json:"agent_json"`
		Domain                   string                 `json:"domain"`
		TTL                      uint32                 `json:"ttl"`
		BindRecord               string                 `json:"bind_record"`
		PublicKeyHashField       string                 `json:"public_key_hash_field"`
		LegacyPublicKeyHashField string                 `json:"legacy_public_key_hash_field"`
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode DNS fixture: %v", err)
	}
	agentJSON, err := json.Marshal(fixture.AgentJSON)
	if err != nil {
		t.Fatalf("encode fixture agent JSON: %v", err)
	}

	record, err := GetDnsRecord(string(agentJSON), fixture.Domain, fixture.TTL)
	if err != nil {
		t.Fatalf("GetDnsRecord: %v", err)
	}
	if record != fixture.BindRecord {
		t.Fatalf("DNS record drift:\n got: %s\nwant: %s", record, fixture.BindRecord)
	}
	if !strings.Contains(record, fixture.PublicKeyHashField+"=") {
		t.Fatalf("record missing public key hash field %q: %s", fixture.PublicKeyHashField, record)
	}
	if strings.Contains(record, fixture.LegacyPublicKeyHashField+"=") {
		t.Fatalf("record contains legacy public key hash field %q: %s", fixture.LegacyPublicKeyHashField, record)
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
