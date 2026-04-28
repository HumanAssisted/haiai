package haiai

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

// Test parseRawEmailJSON unit behavior — the pure parser.

func TestParseRawEmailJSONAvailableTrue(t *testing.T) {
	raw := []byte("From: a\r\n\r\nbody with \x00 NUL and \xc3\xa9\r\n")
	b64 := base64.StdEncoding.EncodeToString(raw)
	wire, err := json.Marshal(map[string]interface{}{
		"message_id":      "m.1",
		"rfc_message_id":  "<a@b>",
		"available":       true,
		"raw_email_b64":   b64,
		"size_bytes":      len(raw),
		"omitted_reason":  nil,
	})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	result, err := parseRawEmailJSON(wire)
	if err != nil {
		t.Fatalf("parseRawEmailJSON: %v", err)
	}
	if !result.Available {
		t.Fatalf("expected available=true")
	}
	if !bytes.Equal(result.RawEmail, raw) {
		t.Fatalf("byte-identity broken: got %d bytes", len(result.RawEmail))
	}
	if result.SizeBytes != len(raw) {
		t.Fatalf("size_bytes: got %d want %d", result.SizeBytes, len(raw))
	}
	if result.MessageID != "m.1" {
		t.Fatalf("message_id: %q", result.MessageID)
	}
	if result.RfcMessageID != "<a@b>" {
		t.Fatalf("rfc_message_id: %q", result.RfcMessageID)
	}
	if result.OmittedReason != "" {
		t.Fatalf("omitted_reason: %q", result.OmittedReason)
	}
}

func TestParseRawEmailJSONNotStored(t *testing.T) {
	wire := json.RawMessage(`{"message_id":"m.2","available":false,"raw_email_b64":null,"size_bytes":null,"omitted_reason":"not_stored"}`)
	result, err := parseRawEmailJSON(wire)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if result.Available {
		t.Fatal("expected available=false")
	}
	if result.RawEmail != nil {
		t.Fatal("expected RawEmail=nil")
	}
	if result.OmittedReason != "not_stored" {
		t.Fatalf("omitted_reason: %q", result.OmittedReason)
	}
}

func TestParseRawEmailJSONOversize(t *testing.T) {
	wire := json.RawMessage(`{"message_id":"m.3","available":false,"raw_email_b64":null,"omitted_reason":"oversize"}`)
	result, err := parseRawEmailJSON(wire)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if result.OmittedReason != "oversize" {
		t.Fatalf("omitted_reason: %q", result.OmittedReason)
	}
	if result.RawEmail != nil {
		t.Fatal("expected RawEmail=nil")
	}
}

// Test GetRawEmail through the mock FFI (HTTP-backed mock).

func TestGetRawEmailHappyPath(t *testing.T) {
	raw := []byte("CRLF\r\nembed\x00NUL\r\nnon-ascii:\xc3\xa9\r\n")
	b64 := base64.StdEncoding.EncodeToString(raw)

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Path must be the /raw variant — mock FFI builds the path
		// using the agent ID baked into the mock client.
		expectedSuffix := "/email/messages/m.123/raw"
		if len(r.URL.Path) < len(expectedSuffix) || r.URL.Path[len(r.URL.Path)-len(expectedSuffix):] != expectedSuffix {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != http.MethodGet {
			t.Errorf("unexpected method: %s", r.Method)
		}
		w.Header().Set("Content-Type", "application/json")
		body, _ := json.Marshal(map[string]interface{}{
			"message_id":     "m.123",
			"rfc_message_id": "<m.123@hai.ai>",
			"available":      true,
			"raw_email_b64":  b64,
			"size_bytes":     len(raw),
			"omitted_reason": nil,
		})
		_, _ = w.Write(body)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.GetRawEmail(context.Background(), "m.123")
	if err != nil {
		t.Fatalf("GetRawEmail: %v", err)
	}
	if !result.Available {
		t.Fatal("expected Available=true")
	}
	// R2: byte-identity
	if !bytes.Equal(result.RawEmail, raw) {
		t.Fatal("RawEmail bytes differ from input")
	}
	if result.SizeBytes != len(raw) {
		t.Fatalf("SizeBytes: got %d want %d", result.SizeBytes, len(raw))
	}
}

func TestGetRawEmailEmptyMessageIDFails(t *testing.T) {
	cl, _ := newTestClient(t, "http://example.invalid")
	if _, err := cl.GetRawEmail(context.Background(), ""); err == nil {
		t.Fatal("expected error for empty messageID")
	}
}

func TestGetRawEmailAvailableFalse(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"legacy","available":false,"raw_email_b64":null,"size_bytes":null,"omitted_reason":"not_stored"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.GetRawEmail(context.Background(), "legacy")
	if err != nil {
		t.Fatalf("unexpected err: %v", err)
	}
	if result.Available {
		t.Fatal("expected Available=false")
	}
	if result.RawEmail != nil {
		t.Fatal("expected RawEmail=nil")
	}
	if result.OmittedReason != "not_stored" {
		t.Fatalf("OmittedReason: %q", result.OmittedReason)
	}
}

// Regression for issue 007: preserve wire null vs explicit empty for
// omitted_reason, size_bytes, and rfc_message_id. Without the Has... accessors,
// a happy-path row with omitted_reason=null and a legacy row with
// omitted_reason="" would collapse into indistinguishable Go structs.
func TestParseRawEmailJSONPreservesNullVsDefault(t *testing.T) {
	// Happy path: all non-bytes fields carry null / distinct values.
	rawBytes := []byte("From: a\r\n\r\nbody")
	b64 := base64.StdEncoding.EncodeToString(rawBytes)
	happy, err := json.Marshal(map[string]interface{}{
		"message_id":     "m.1",
		"rfc_message_id": "<a@b>",
		"available":      true,
		"raw_email_b64":  b64,
		"size_bytes":     len(rawBytes),
		"omitted_reason": nil,
	})
	if err != nil {
		t.Fatalf("marshal happy: %v", err)
	}
	h, err := parseRawEmailJSON(happy)
	if err != nil {
		t.Fatalf("parse happy: %v", err)
	}
	if !h.HasSizeBytes() || !h.HasRfcMessageID() {
		t.Fatalf("expected happy path to have size_bytes + rfc_message_id present, got HasSizeBytes=%v HasRfcMessageID=%v", h.HasSizeBytes(), h.HasRfcMessageID())
	}
	if h.HasOmittedReason() {
		t.Fatalf("expected happy path to NOT have omitted_reason, got OmittedReason=%q", h.OmittedReason)
	}

	// Legacy path: all three nullable wire fields come through as null.
	legacy, err := json.Marshal(map[string]interface{}{
		"message_id":     "m.legacy",
		"rfc_message_id": nil,
		"available":      false,
		"raw_email_b64":  nil,
		"size_bytes":     nil,
		"omitted_reason": "not_stored",
	})
	if err != nil {
		t.Fatalf("marshal legacy: %v", err)
	}
	l, err := parseRawEmailJSON(legacy)
	if err != nil {
		t.Fatalf("parse legacy: %v", err)
	}
	if l.HasSizeBytes() || l.HasRfcMessageID() {
		t.Fatalf("expected legacy path to NOT have size_bytes or rfc_message_id, got HasSizeBytes=%v HasRfcMessageID=%v", l.HasSizeBytes(), l.HasRfcMessageID())
	}
	if !l.HasOmittedReason() || l.OmittedReason != "not_stored" {
		t.Fatalf("expected legacy path omitted_reason=not_stored present, got Has=%v value=%q", l.HasOmittedReason(), l.OmittedReason)
	}

	// "available=true but empty size" sanity: null for size_bytes on a happy
	// row is uncommon but legal wire state. Make sure we track it.
	oddball, err := json.Marshal(map[string]interface{}{
		"message_id":     "m.odd",
		"available":      true,
		"raw_email_b64":  b64,
		"size_bytes":     nil,
		"omitted_reason": nil,
	})
	if err != nil {
		t.Fatalf("marshal oddball: %v", err)
	}
	o, err := parseRawEmailJSON(oddball)
	if err != nil {
		t.Fatalf("parse oddball: %v", err)
	}
	if o.HasSizeBytes() {
		t.Fatalf("expected oddball HasSizeBytes=false, got SizeBytes=%d", o.SizeBytes)
	}
	if o.HasOmittedReason() {
		t.Fatalf("expected oddball HasOmittedReason=false")
	}
	if !bytes.Equal(o.RawEmail, rawBytes) {
		t.Fatalf("oddball RawEmail mismatch")
	}
}

// Fixture-driven conformance — PRD §5.4 raw_email_roundtrip scenario.
//
// Issue 017 note: this test asserts *byte-identity forwarding through the
// verify call chain*, not real post-quantum crypto verification. The
// fixture's `verify_implemented_by: "rust_only"` key declares that only
// Rust's conformance test runs `jacs::email::verify_email_document`
// against the signed bytes. Go (and Python/Node) stub the verify HTTP
// response; `captured` below is what guarantees the wrapper did not
// mutate the bytes between GetRawEmail and VerifyEmail.

func TestRawEmailConformanceRoundtrip(t *testing.T) {
	fixturePath := filepath.Join("..", "fixtures", "email_conformance.json")
	data, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Fatalf("read fixture: %v", err)
	}
	var fixture map[string]interface{}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("parse fixture: %v", err)
	}
	scenario, ok := fixture["raw_email_roundtrip"].(map[string]interface{})
	if !ok {
		t.Fatal("missing raw_email_roundtrip scenario")
	}
	if vb, _ := scenario["verify_implemented_by"].(string); vb != "rust_only" {
		t.Fatalf("expected verify_implemented_by=\"rust_only\" (Issue 017); got %q", vb)
	}
	inputB64 := scenario["input_raw_b64"].(string)
	expectedBytes, err := base64.StdEncoding.DecodeString(inputB64)
	if err != nil {
		t.Fatalf("base64 decode input: %v", err)
	}
	sha := sha256.Sum256(expectedBytes)
	if hex.EncodeToString(sha[:]) != scenario["input_sha256"].(string) {
		t.Fatal("input_sha256 mismatch — fixture corrupted")
	}
	registry, ok := scenario["verify_registry"].(map[string]interface{})
	if !ok {
		t.Fatal("missing verify_registry")
	}

	// Capture any bytes sent to /api/v1/email/verify so we can assert Go's
	// wrapper forwards the fetched bytes byte-identically through the verify
	// call chain (PRD §5.4 second assertion).
	var captured []byte
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/email/verify" {
			body, _ := io.ReadAll(r.Body)
			captured = body
			w.Header().Set("Content-Type", "application/json")
			resp, _ := json.Marshal(map[string]interface{}{
				"valid":             scenario["expected_verify_valid"],
				"jacs_id":           registry["jacs_id"],
				"algorithm":         registry["algorithm"],
				"reputation_tier":   registry["reputation_tier"],
				"dns_verified":      nil,
				"field_results":     []interface{}{},
				"chain":             []interface{}{},
				"error":             nil,
				"agent_status":      registry["agent_status"],
				"benchmarks_completed": []interface{}{},
			})
			_, _ = w.Write(resp)
			return
		}
		// Default: GET raw email response.
		w.Header().Set("Content-Type", "application/json")
		body, _ := json.Marshal(map[string]interface{}{
			"message_id":     "conf-001",
			"available":      scenario["expected_available"],
			"raw_email_b64":  scenario["expected_raw_b64"],
			"size_bytes":     scenario["expected_size_bytes"],
			"omitted_reason": scenario["expected_omitted_reason"],
		})
		_, _ = w.Write(body)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.GetRawEmail(context.Background(), "conf-001")
	if err != nil {
		t.Fatalf("GetRawEmail: %v", err)
	}
	// Assertion 1 (PRD §5.4): byte-identity.
	if !bytes.Equal(result.RawEmail, expectedBytes) {
		t.Fatalf("byte-identity broken: got %d bytes, want %d", len(result.RawEmail), len(expectedBytes))
	}
	if int64(result.SizeBytes) != int64(scenario["expected_size_bytes"].(float64)) {
		t.Fatalf("SizeBytes: got %d want %v", result.SizeBytes, scenario["expected_size_bytes"])
	}

	// Assertion 2 (PRD §5.4): verify_email(fetched_bytes).valid == true.
	verifyResult, err := cl.VerifyEmail(context.Background(), result.RawEmail)
	if err != nil {
		t.Fatalf("VerifyEmail: %v", err)
	}
	if !verifyResult.Valid {
		t.Fatalf("expected verify_email(bytes).valid == true, got valid=%v error=%v", verifyResult.Valid, verifyResult.Error)
	}
	if verifyResult.JacsID != registry["jacs_id"].(string) {
		t.Fatalf("verify JacsID mismatch: got %q want %q", verifyResult.JacsID, registry["jacs_id"])
	}
	// R2 enforcement: bytes Go passed to verify must be byte-identical to
	// bytes Go fetched.
	if !bytes.Equal(captured, result.RawEmail) {
		t.Fatalf("verify_email forwarded %d bytes but fetched bytes were %d — byte drift through verify call", len(captured), len(result.RawEmail))
	}
}
