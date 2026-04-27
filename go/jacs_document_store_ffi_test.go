package haiai

// Issue 025 — Go FFI tests for the 7 D5/D9 JACS Document Store methods.
//
// Exercises every D5 (SaveMemory / SaveSoul / GetMemory / GetSoul) and D9
// (StoreTextFile / StoreImageFile / GetRecordBytes) wrapper through the
// FFIClient interface using the mockFFIClient test infrastructure. The
// fixture file `fixtures/ffi_method_parity.json` declares these methods in
// the `jacs_document_store` group; this test file pins their wire shape
// (argument names, return types, error mapping) at the adapter boundary.
//
// Mock-only: these tests do NOT load libhaiigo. The full HTTP round-trip is
// exercised by `haisdk/rust/haiai/tests/jacs_remote_integration.rs`
// (`--ignored` against a live hosted stack).

import (
	"bytes"
	"encoding/json"
	"testing"
)

// =============================================================================
// D5 — MEMORY / SOUL wrappers
// =============================================================================

func TestSaveMemoryCapturesContent(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	var captured string
	mock.saveMemoryFn = func(content string) (string, error) {
		captured = content
		return "mem-id:v1", nil
	}
	key, err := mock.SaveMemory("# MEMORY.md\n\nproject: foo")
	if err != nil {
		t.Fatalf("SaveMemory returned error: %v", err)
	}
	if key != "mem-id:v1" {
		t.Errorf("expected key=mem-id:v1, got %q", key)
	}
	if captured != "# MEMORY.md\n\nproject: foo" {
		t.Errorf("captured content mismatch: got %q", captured)
	}
}

func TestSaveSoulCapturesContent(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	var captured string
	mock.saveSoulFn = func(content string) (string, error) {
		captured = content
		return "soul-id:v1", nil
	}
	key, err := mock.SaveSoul("# SOUL.md\n\nvoice: terse")
	if err != nil {
		t.Fatalf("SaveSoul returned error: %v", err)
	}
	if key != "soul-id:v1" {
		t.Errorf("expected key=soul-id:v1, got %q", key)
	}
	if captured != "# SOUL.md\n\nvoice: terse" {
		t.Errorf("captured content mismatch: got %q", captured)
	}
}

func TestGetMemoryReturnsEnvelopeJSON(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	envelope := `{"jacsId":"mem-1","jacsType":"memory","body":"x"}`
	mock.getMemoryFn = func() (string, error) {
		return envelope, nil
	}
	out, err := mock.GetMemory()
	if err != nil {
		t.Fatalf("GetMemory returned error: %v", err)
	}
	if out != envelope {
		t.Errorf("envelope mismatch: got %q, want %q", out, envelope)
	}
}

func TestGetSoulReturnsEnvelopeJSON(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	envelope := `{"jacsId":"soul-1","jacsType":"soul"}`
	mock.getSoulFn = func() (string, error) {
		return envelope, nil
	}
	out, err := mock.GetSoul()
	if err != nil {
		t.Fatalf("GetSoul returned error: %v", err)
	}
	if out != envelope {
		t.Errorf("envelope mismatch: got %q, want %q", out, envelope)
	}
}

// =============================================================================
// D9 — typed-content helpers
// =============================================================================

func TestStoreTextFileCapturesPath(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	var captured string
	mock.storeTextFileFn = func(path string) (string, error) {
		captured = path
		return "txt-id:v1", nil
	}
	key, err := mock.StoreTextFile("/tmp/signed.md")
	if err != nil {
		t.Fatalf("StoreTextFile returned error: %v", err)
	}
	if key != "txt-id:v1" {
		t.Errorf("expected key=txt-id:v1, got %q", key)
	}
	if captured != "/tmp/signed.md" {
		t.Errorf("captured path mismatch: got %q", captured)
	}
}

func TestStoreImageFileCapturesPath(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	var captured string
	mock.storeImageFileFn = func(path string) (string, error) {
		captured = path
		return "png-id:v1", nil
	}
	key, err := mock.StoreImageFile("/tmp/signed.png")
	if err != nil {
		t.Fatalf("StoreImageFile returned error: %v", err)
	}
	if key != "png-id:v1" {
		t.Errorf("expected key=png-id:v1, got %q", key)
	}
	if captured != "/tmp/signed.png" {
		t.Errorf("captured path mismatch: got %q", captured)
	}
}

func TestGetRecordBytesReturnsBytes(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	pngMagic := []byte{0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A}
	mock.getRecordBytesFn = func(key string) ([]byte, error) {
		if key != "png-id:v1" {
			t.Errorf("unexpected key %q", key)
		}
		return pngMagic, nil
	}
	out, err := mock.GetRecordBytes("png-id:v1")
	if err != nil {
		t.Fatalf("GetRecordBytes returned error: %v", err)
	}
	if !bytes.Equal(out, pngMagic) {
		t.Errorf("byte mismatch: got %v, want %v", out, pngMagic)
	}
}

// =============================================================================
// Generic JACS Document Store CRUD — also part of the 20-method scope
// =============================================================================

func TestSignAndStorePassesDataJSON(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	var captured string
	mock.signAndStoreFn = func(dataJSON string) (json.RawMessage, error) {
		captured = dataJSON
		return json.RawMessage(`{"key":"id1:v1"}`), nil
	}
	out, err := mock.SignAndStore(`{"hello":"world"}`)
	if err != nil {
		t.Fatalf("SignAndStore returned error: %v", err)
	}
	if !bytes.Equal(out, []byte(`{"key":"id1:v1"}`)) {
		t.Errorf("output mismatch: got %s", string(out))
	}
	if captured != `{"hello":"world"}` {
		t.Errorf("captured data mismatch: got %q", captured)
	}
}

func TestSearchDocumentsForwardsArgs(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	var capturedQuery string
	var capturedLimit, capturedOffset int
	mock.searchDocumentsFn = func(query string, limit, offset int) (json.RawMessage, error) {
		capturedQuery = query
		capturedLimit = limit
		capturedOffset = offset
		return json.RawMessage(`{"results":[],"total_count":0}`), nil
	}
	_, err := mock.SearchDocuments("marker-xyz", 10, 0)
	if err != nil {
		t.Fatalf("SearchDocuments returned error: %v", err)
	}
	if capturedQuery != "marker-xyz" || capturedLimit != 10 || capturedOffset != 0 {
		t.Errorf("args mismatch: q=%q limit=%d offset=%d", capturedQuery, capturedLimit, capturedOffset)
	}
}

func TestQueryByTypeForwardsArgs(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	mock.queryByTypeFn = func(docType string, limit, offset int) ([]string, error) {
		if docType != "memory" || limit != 25 || offset != 0 {
			t.Errorf("unexpected args: %s %d %d", docType, limit, offset)
		}
		return []string{}, nil
	}
	_, err := mock.QueryByType("memory", 25, 0)
	if err != nil {
		t.Fatalf("QueryByType returned error: %v", err)
	}
}

// Issue: array-shaped return for the five trait methods. The trait returns
// `Vec<String>` so the Go interface MUST surface `[]string`. Pin this.
func TestListDocumentsReturnsStringSlice(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	mock.listDocumentsFn = func(jacsType string) ([]string, error) {
		return []string{"id1:v1", "id2:v1"}, nil
	}
	out, err := mock.ListDocuments("")
	if err != nil {
		t.Fatalf("ListDocuments returned error: %v", err)
	}
	if len(out) != 2 || out[0] != "id1:v1" {
		t.Errorf("expected [id1:v1, id2:v1], got %v", out)
	}
}

func TestRemoveDocumentReturnsNil(t *testing.T) {
	mock := newMockFFIClient("http://localhost:0", "agent-test", "")
	called := false
	mock.removeDocumentFn = func(key string) error {
		if key != "id1:v1" {
			t.Errorf("expected key=id1:v1, got %q", key)
		}
		called = true
		return nil
	}
	err := mock.RemoveDocument("id1:v1")
	if err != nil {
		t.Fatalf("RemoveDocument returned error: %v", err)
	}
	if !called {
		t.Error("RemoveDocument did not invoke stub")
	}
}

// =============================================================================
// FFI surface area — every D5/D9 method appears in the parity fixture.
// =============================================================================

func TestD5MethodsAreInParityFixture(t *testing.T) {
	fixture := loadParityFixture(t)
	expected := []string{"save_memory", "save_soul", "get_memory", "get_soul"}
	all := make(map[string]bool)
	for _, group := range fixture.Methods {
		for _, m := range group {
			all[m.Name] = true
		}
	}
	for _, name := range expected {
		if !all[name] {
			t.Errorf("D5 method %q missing from ffi_method_parity.json", name)
		}
	}
}

func TestD9MethodsAreInParityFixture(t *testing.T) {
	fixture := loadParityFixture(t)
	expected := []string{"store_text_file", "store_image_file", "get_record_bytes"}
	all := make(map[string]bool)
	for _, group := range fixture.Methods {
		for _, m := range group {
			all[m.Name] = true
		}
	}
	for _, name := range expected {
		if !all[name] {
			t.Errorf("D9 method %q missing from ffi_method_parity.json", name)
		}
	}
}

// =============================================================================
// Real `ffi.Client` doc-store wiring
//
// The 20 doc-store methods previously returned a "not yet wired through
// libhaiigo" stub error. Wiring is now complete (TASK_004 of the
// JACS_DOCUMENT_STORE_FFI_PRD). Real-binding tests live in
// `go/ffi_native_smoke_test.go` (`//go:build cgo_smoke`).
// =============================================================================
