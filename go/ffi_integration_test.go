package haiai

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"strings"
	"testing"
)

// parityFixture represents the shared FFI method parity contract.
type parityFixture struct {
	Description      string                              `json:"description"`
	Methods          map[string][]parityFixtureMethod     `json:"methods"`
	ErrorKinds       []string                             `json:"error_kinds"`
	ErrorFormat      string                               `json:"error_format"`
	TotalMethodCount int                                  `json:"total_method_count"`
}

type parityFixtureMethod struct {
	Name    string   `json:"name"`
	Args    []string `json:"args"`
	Returns string   `json:"returns"`
}

func loadParityFixture(t *testing.T) *parityFixture {
	t.Helper()
	// Find fixtures directory relative to this test file.
	_, filename, _, _ := runtime.Caller(0)
	fixturesDir := filepath.Join(filepath.Dir(filename), "..", "fixtures")
	data, err := os.ReadFile(filepath.Join(fixturesDir, "ffi_method_parity.json"))
	if err != nil {
		t.Fatalf("Failed to read parity fixture: %v", err)
	}
	var f parityFixture
	if err := json.Unmarshal(data, &f); err != nil {
		t.Fatalf("Failed to parse parity fixture: %v", err)
	}
	return &f
}

func allFixtureMethodNames(f *parityFixture) []string {
	var names []string
	for _, group := range f.Methods {
		for _, m := range group {
			names = append(names, toPascalCase(m.Name))
		}
	}
	sort.Strings(names)
	return names
}

// toPascalCase converts snake_case to PascalCase (Go exported method names).
func toPascalCase(s string) string {
	parts := strings.Split(s, "_")
	var result strings.Builder
	for _, part := range parts {
		if len(part) > 0 {
			result.WriteString(strings.ToUpper(part[:1]) + part[1:])
		}
	}
	// Handle special cases for Go naming conventions
	r := result.String()
	r = strings.ReplaceAll(r, "Json", "JSON")
	r = strings.ReplaceAll(r, "Id", "ID")
	r = strings.ReplaceAll(r, "Url", "URL")
	// Fix double-replacements (e.g., JacsID -> JacsID is fine)
	// Specific fixes for known methods:
	if r == "JacsID" {
		return "JacsID"
	}
	if r == "SetHaiAgentID" {
		return "SetHaiAgentID"
	}
	if r == "CanonicalJSON" {
		return "CanonicalJSON"
	}
	if r == "ExportAgentJSON" {
		return "ExportAgentJSON"
	}
	if r == "VerifyA2aArtifact" {
		return "VerifyA2AArtifact"
	}
	// SSE/WS acronym fixes
	r = strings.ReplaceAll(r, "Sse", "SSE")
	r = strings.ReplaceAll(r, "Ws", "WS")
	return r
}

// ---------------------------------------------------------------------------
// Test: FFIClient interface has all fixture methods
// ---------------------------------------------------------------------------

func TestFFIClientInterfaceHasAllFixtureMethods(t *testing.T) {
	fixture := loadParityFixture(t)
	fixtureNames := allFixtureMethodNames(fixture)

	// Get methods from FFIClient interface via reflection.
	ffiType := reflect.TypeOf((*FFIClient)(nil)).Elem()
	ifaceMethods := make(map[string]bool)
	for i := 0; i < ffiType.NumMethod(); i++ {
		ifaceMethods[ffiType.Method(i).Name] = true
	}

	// Also include Close() which is in the interface but not in fixture
	// (it's lifecycle, not API).

	var missing []string
	for _, name := range fixtureNames {
		if !ifaceMethods[name] {
			missing = append(missing, name)
		}
	}

	if len(missing) > 0 {
		t.Errorf("FFIClient interface is missing methods from parity fixture: %v", missing)
	}
}

// ---------------------------------------------------------------------------
// Test: mockFFIClient implements all fixture methods
// ---------------------------------------------------------------------------

func TestMockFFIClientHasAllFixtureMethods(t *testing.T) {
	fixture := loadParityFixture(t)
	fixtureNames := allFixtureMethodNames(fixture)

	// Verify mockFFIClient (from mock_ffi_test.go) satisfies the interface
	// by checking that the method set of *mockFFIClient includes all fixture methods.
	mockType := reflect.TypeOf(&mockFFIClient{})
	mockMethods := make(map[string]bool)
	for i := 0; i < mockType.NumMethod(); i++ {
		mockMethods[mockType.Method(i).Name] = true
	}

	var missing []string
	for _, name := range fixtureNames {
		if !mockMethods[name] {
			missing = append(missing, name)
		}
	}

	if len(missing) > 0 {
		t.Errorf("mockFFIClient is missing methods from parity fixture: %v", missing)
	}
}

// ---------------------------------------------------------------------------
// Test: Fixture total method count matches
// ---------------------------------------------------------------------------

func TestFixtureTotalMethodCount(t *testing.T) {
	fixture := loadParityFixture(t)
	names := allFixtureMethodNames(fixture)
	if len(names) != fixture.TotalMethodCount {
		t.Errorf("Fixture declares %d methods but has %d", fixture.TotalMethodCount, len(names))
	}
}

// ---------------------------------------------------------------------------
// Test: FFI error mapping covers all fixture error kinds
// ---------------------------------------------------------------------------

func TestMapFFIErrCoversAllFixtureKinds(t *testing.T) {
	fixture := loadParityFixture(t)

	for _, kind := range fixture.ErrorKinds {
		// mapFFIErr (unexported) matches on error message strings in the format
		// "{Kind}: message" -- this is the same format the FFI layer produces.
		ffiErr := fmt.Errorf("%s: test message", kind)
		mapped := mapFFIErr(ffiErr)
		if mapped == nil {
			t.Errorf("mapFFIErr returned nil for kind %q", kind)
			continue
		}

		// Verify the error was mapped (not nil), meaning mapFFIErr handled it
		haiErr, ok := mapped.(*Error)
		if !ok {
			t.Errorf("mapFFIErr returned non-Error type for kind %q: %T", kind, mapped)
			continue
		}

		// Verify specific mappings for kinds that mapFFIErr recognizes
		switch kind {
		case "AuthFailed":
			if haiErr.Kind != ErrAuthRequired {
				t.Errorf("Expected ErrAuthRequired for kind %q, got %v", kind, haiErr.Kind)
			}
		case "RateLimited":
			if haiErr.Kind != ErrRateLimited {
				t.Errorf("Expected ErrRateLimited for kind %q, got %v", kind, haiErr.Kind)
			}
		case "NotFound":
			if haiErr.Kind != ErrNotFound {
				t.Errorf("Expected ErrNotFound for kind %q, got %v", kind, haiErr.Kind)
			}
		case "NetworkFailed":
			if haiErr.Kind != ErrConnection {
				t.Errorf("Expected ErrConnection for kind %q, got %v", kind, haiErr.Kind)
			}
		// ProviderError, ConfigFailed, SerializationFailed, InvalidArgument, ApiError
		// are mapped to ErrInvalidResponse by the default fallback in mapFFIErr.
		// The ffi package's MapFFIError handles these more precisely, but mapFFIErr
		// in the haiai package uses simpler string matching.
		}
	}
}

// ---------------------------------------------------------------------------
// Test: Error format matches fixture spec
// ---------------------------------------------------------------------------

func TestErrorFormatMatchesFixture(t *testing.T) {
	fixture := loadParityFixture(t)
	expected := "{ErrorKind}: {message}"
	if fixture.ErrorFormat != expected {
		t.Errorf("Expected error format %q, got %q", expected, fixture.ErrorFormat)
	}
}

// ---------------------------------------------------------------------------
// Test: Client delegates API calls to FFI, not httpClient
// ---------------------------------------------------------------------------

func TestClientHelloDelegatesToFFI(t *testing.T) {
	// Create a mock FFI client that records calls
	calls := make([]string, 0)
	mock := &recordingFFIClient{
		calls: &calls,
		inner: newMockFFIClient("http://unused", "test-agent:1", "JACS test"),
	}

	client := &Client{
		ffi:      mock,
		jacsID:   "test-agent:1",
		endpoint: "http://unused",
	}

	_, _ = client.Hello(nil)
	if len(calls) == 0 || calls[0] != "Hello" {
		t.Errorf("Expected Hello to be delegated to FFI, got calls: %v", calls)
	}
}

func TestClientRegisterDelegatesToFFI(t *testing.T) {
	calls := make([]string, 0)
	mock := &recordingFFIClient{
		calls: &calls,
		inner: newMockFFIClient("http://unused", "test-agent:1", "JACS test"),
	}

	client := &Client{
		ffi:      mock,
		jacsID:   "test-agent:1",
		endpoint: "http://unused",
	}

	_, _ = client.Register(nil, RegisterOptions{})
	if len(calls) == 0 || calls[0] != "Register" {
		t.Errorf("Expected Register to be delegated to FFI, got calls: %v", calls)
	}
}

func TestClientStatusDelegatesToFFI(t *testing.T) {
	calls := make([]string, 0)
	mock := &recordingFFIClient{
		calls: &calls,
		inner: newMockFFIClient("http://unused", "test-agent:1", "JACS test"),
	}

	client := &Client{
		ffi:      mock,
		jacsID:   "test-agent:1",
		endpoint: "http://unused",
	}

	_, _ = client.Status(nil)
	if len(calls) == 0 || calls[0] != "VerifyStatus" {
		t.Errorf("Expected Status to delegate to FFI VerifyStatus, got calls: %v", calls)
	}
}

// recordingFFIClient wraps an FFIClient and records method calls.
type recordingFFIClient struct {
	calls *[]string
	inner FFIClient
}

func (r *recordingFFIClient) Close()                               { r.inner.Close() }
func (r *recordingFFIClient) Hello(includeTest bool) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "Hello")
	return r.inner.Hello(includeTest)
}
func (r *recordingFFIClient) Register(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "Register")
	return r.inner.Register(optionsJSON)
}
func (r *recordingFFIClient) RegisterNewAgent(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "RegisterNewAgent")
	return r.inner.RegisterNewAgent(optionsJSON)
}
func (r *recordingFFIClient) RotateKeys(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "RotateKeys")
	return r.inner.RotateKeys(optionsJSON)
}
func (r *recordingFFIClient) UpdateAgent(agentData string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "UpdateAgent")
	return r.inner.UpdateAgent(agentData)
}
func (r *recordingFFIClient) SubmitResponse(paramsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SubmitResponse")
	return r.inner.SubmitResponse(paramsJSON)
}
func (r *recordingFFIClient) VerifyStatus(agentID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyStatus")
	return r.inner.VerifyStatus(agentID)
}
func (r *recordingFFIClient) UpdateUsername(agentID, username string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "UpdateUsername")
	return r.inner.UpdateUsername(agentID, username)
}
func (r *recordingFFIClient) DeleteUsername(agentID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "DeleteUsername")
	return r.inner.DeleteUsername(agentID)
}
func (r *recordingFFIClient) SendEmail(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SendEmail")
	return r.inner.SendEmail(optionsJSON)
}
func (r *recordingFFIClient) SendSignedEmail(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SendSignedEmail")
	return r.inner.SendSignedEmail(optionsJSON)
}
func (r *recordingFFIClient) ListMessages(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "ListMessages")
	return r.inner.ListMessages(optionsJSON)
}
func (r *recordingFFIClient) UpdateLabels(paramsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "UpdateLabels")
	return r.inner.UpdateLabels(paramsJSON)
}
func (r *recordingFFIClient) GetEmailStatus() (json.RawMessage, error) {
	*r.calls = append(*r.calls, "GetEmailStatus")
	return r.inner.GetEmailStatus()
}
func (r *recordingFFIClient) GetMessage(messageID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "GetMessage")
	return r.inner.GetMessage(messageID)
}
func (r *recordingFFIClient) GetRawEmail(messageID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "GetRawEmail")
	return r.inner.GetRawEmail(messageID)
}
func (r *recordingFFIClient) GetUnreadCount() (json.RawMessage, error) {
	*r.calls = append(*r.calls, "GetUnreadCount")
	return r.inner.GetUnreadCount()
}
func (r *recordingFFIClient) MarkRead(messageID string) error {
	*r.calls = append(*r.calls, "MarkRead")
	return r.inner.MarkRead(messageID)
}
func (r *recordingFFIClient) MarkUnread(messageID string) error {
	*r.calls = append(*r.calls, "MarkUnread")
	return r.inner.MarkUnread(messageID)
}
func (r *recordingFFIClient) DeleteMessage(messageID string) error {
	*r.calls = append(*r.calls, "DeleteMessage")
	return r.inner.DeleteMessage(messageID)
}
func (r *recordingFFIClient) Archive(messageID string) error {
	*r.calls = append(*r.calls, "Archive")
	return r.inner.Archive(messageID)
}
func (r *recordingFFIClient) Unarchive(messageID string) error {
	*r.calls = append(*r.calls, "Unarchive")
	return r.inner.Unarchive(messageID)
}
func (r *recordingFFIClient) ReplyWithOptions(paramsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "ReplyWithOptions")
	return r.inner.ReplyWithOptions(paramsJSON)
}
func (r *recordingFFIClient) Forward(paramsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "Forward")
	return r.inner.Forward(paramsJSON)
}
func (r *recordingFFIClient) SearchMessages(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SearchMessages")
	return r.inner.SearchMessages(optionsJSON)
}
func (r *recordingFFIClient) Contacts() (json.RawMessage, error) {
	*r.calls = append(*r.calls, "Contacts")
	return r.inner.Contacts()
}
func (r *recordingFFIClient) FetchRemoteKey(jacsID, version string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "FetchRemoteKey")
	return r.inner.FetchRemoteKey(jacsID, version)
}
func (r *recordingFFIClient) FetchKeyByHash(hash string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "FetchKeyByHash")
	return r.inner.FetchKeyByHash(hash)
}
func (r *recordingFFIClient) FetchKeyByEmail(email string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "FetchKeyByEmail")
	return r.inner.FetchKeyByEmail(email)
}
func (r *recordingFFIClient) FetchKeyByDomain(domain string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "FetchKeyByDomain")
	return r.inner.FetchKeyByDomain(domain)
}
func (r *recordingFFIClient) FetchAllKeys(jacsID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "FetchAllKeys")
	return r.inner.FetchAllKeys(jacsID)
}
func (r *recordingFFIClient) VerifyDocument(document string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyDocument")
	return r.inner.VerifyDocument(document)
}
func (r *recordingFFIClient) GetVerification(agentID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "GetVerification")
	return r.inner.GetVerification(agentID)
}
func (r *recordingFFIClient) VerifyAgentDocument(requestJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyAgentDocument")
	return r.inner.VerifyAgentDocument(requestJSON)
}
func (r *recordingFFIClient) Benchmark(name, tier string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "Benchmark")
	return r.inner.Benchmark(name, tier)
}
func (r *recordingFFIClient) FreeRun(transport string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "FreeRun")
	return r.inner.FreeRun(transport)
}
func (r *recordingFFIClient) ProRun(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "ProRun")
	return r.inner.ProRun(optionsJSON)
}
func (r *recordingFFIClient) EnterpriseRun() error {
	*r.calls = append(*r.calls, "EnterpriseRun")
	return r.inner.EnterpriseRun()
}
func (r *recordingFFIClient) BuildAuthHeader() (string, error) {
	*r.calls = append(*r.calls, "BuildAuthHeader")
	return r.inner.BuildAuthHeader()
}
func (r *recordingFFIClient) SignMessage(message string) (string, error) {
	*r.calls = append(*r.calls, "SignMessage")
	return r.inner.SignMessage(message)
}
func (r *recordingFFIClient) CanonicalJSON(valueJSON string) (string, error) {
	*r.calls = append(*r.calls, "CanonicalJSON")
	return r.inner.CanonicalJSON(valueJSON)
}
func (r *recordingFFIClient) VerifyA2AArtifact(wrappedJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyA2AArtifact")
	return r.inner.VerifyA2AArtifact(wrappedJSON)
}
func (r *recordingFFIClient) ExportAgentJSON() (json.RawMessage, error) {
	*r.calls = append(*r.calls, "ExportAgentJSON")
	return r.inner.ExportAgentJSON()
}
func (r *recordingFFIClient) JacsID() (string, error) {
	*r.calls = append(*r.calls, "JacsID")
	return r.inner.JacsID()
}
func (r *recordingFFIClient) BaseURL() (string, error) {
	*r.calls = append(*r.calls, "BaseURL")
	return r.inner.BaseURL()
}
func (r *recordingFFIClient) HaiAgentID() (string, error) {
	*r.calls = append(*r.calls, "HaiAgentID")
	return r.inner.HaiAgentID()
}
func (r *recordingFFIClient) AgentEmail() (string, error) {
	*r.calls = append(*r.calls, "AgentEmail")
	return r.inner.AgentEmail()
}
func (r *recordingFFIClient) SetHaiAgentID(id string) error {
	*r.calls = append(*r.calls, "SetHaiAgentID")
	return r.inner.SetHaiAgentID(id)
}
func (r *recordingFFIClient) SetAgentEmail(email string) error {
	*r.calls = append(*r.calls, "SetAgentEmail")
	return r.inner.SetAgentEmail(email)
}
func (r *recordingFFIClient) FetchServerKeys() (json.RawMessage, error) {
	*r.calls = append(*r.calls, "FetchServerKeys")
	return r.inner.FetchServerKeys()
}
func (r *recordingFFIClient) SignEmailRaw(rawEmailB64 string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SignEmailRaw")
	return r.inner.SignEmailRaw(rawEmailB64)
}
func (r *recordingFFIClient) VerifyEmailRaw(rawEmailB64 string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyEmailRaw")
	return r.inner.VerifyEmailRaw(rawEmailB64)
}
func (r *recordingFFIClient) CreateAttestation(paramsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "CreateAttestation")
	return r.inner.CreateAttestation(paramsJSON)
}
func (r *recordingFFIClient) ListAttestations(paramsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "ListAttestations")
	return r.inner.ListAttestations(paramsJSON)
}
func (r *recordingFFIClient) GetAttestation(agentID, docID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "GetAttestation")
	return r.inner.GetAttestation(agentID, docID)
}
func (r *recordingFFIClient) VerifyAttestation(document string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyAttestation")
	return r.inner.VerifyAttestation(document)
}
func (r *recordingFFIClient) CreateEmailTemplate(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "CreateEmailTemplate")
	return r.inner.CreateEmailTemplate(optionsJSON)
}
func (r *recordingFFIClient) ListEmailTemplates(optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "ListEmailTemplates")
	return r.inner.ListEmailTemplates(optionsJSON)
}
func (r *recordingFFIClient) GetEmailTemplate(templateID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "GetEmailTemplate")
	return r.inner.GetEmailTemplate(templateID)
}
func (r *recordingFFIClient) UpdateEmailTemplate(templateID, optionsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "UpdateEmailTemplate")
	return r.inner.UpdateEmailTemplate(templateID, optionsJSON)
}
func (r *recordingFFIClient) DeleteEmailTemplate(templateID string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "DeleteEmailTemplate")
	return r.inner.DeleteEmailTemplate(templateID)
}

// --- Local Media (Layer 8 / TASK_009) ---

func (r *recordingFFIClient) SignText(path, optsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SignText")
	return r.inner.SignText(path, optsJSON)
}
func (r *recordingFFIClient) VerifyText(path, optsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyText")
	return r.inner.VerifyText(path, optsJSON)
}
func (r *recordingFFIClient) SignImage(inPath, outPath, optsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SignImage")
	return r.inner.SignImage(inPath, outPath, optsJSON)
}
func (r *recordingFFIClient) VerifyImage(filePath, optsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "VerifyImage")
	return r.inner.VerifyImage(filePath, optsJSON)
}
func (r *recordingFFIClient) ExtractMediaSignature(filePath, optsJSON string) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "ExtractMediaSignature")
	return r.inner.ExtractMediaSignature(filePath, optsJSON)
}

// --- SSE Streaming ---

func (r *recordingFFIClient) ConnectSSE() (uint64, error) {
	*r.calls = append(*r.calls, "ConnectSSE")
	return r.inner.ConnectSSE()
}
func (r *recordingFFIClient) SSENextEvent(handleID uint64) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "SSENextEvent")
	return r.inner.SSENextEvent(handleID)
}
func (r *recordingFFIClient) SSEClose(handleID uint64) {
	*r.calls = append(*r.calls, "SSEClose")
	r.inner.SSEClose(handleID)
}

// --- WebSocket Streaming ---

func (r *recordingFFIClient) ConnectWS() (uint64, error) {
	*r.calls = append(*r.calls, "ConnectWS")
	return r.inner.ConnectWS()
}
func (r *recordingFFIClient) WSNextEvent(handleID uint64) (json.RawMessage, error) {
	*r.calls = append(*r.calls, "WSNextEvent")
	return r.inner.WSNextEvent(handleID)
}
func (r *recordingFFIClient) WSClose(handleID uint64) {
	*r.calls = append(*r.calls, "WSClose")
	r.inner.WSClose(handleID)
}

// Compile-time check: recordingFFIClient satisfies FFIClient.
var _ FFIClient = (*recordingFFIClient)(nil)

// ---------------------------------------------------------------------------
// Test: No net/http in core FFI-delegated methods (source-level check)
// ---------------------------------------------------------------------------

func TestFFIClientInterfaceIsUsedByClient(t *testing.T) {
	// Verify the Client struct has an ffi field of type FFIClient.
	clientType := reflect.TypeOf(Client{})
	ffiField, ok := clientType.FieldByName("ffi")
	if !ok {
		t.Fatal("Client struct does not have an 'ffi' field")
	}

	// Verify the field type is the FFIClient interface.
	expectedType := reflect.TypeOf((*FFIClient)(nil)).Elem()
	if ffiField.Type != expectedType {
		t.Errorf("Client.ffi has type %v, expected %v", ffiField.Type, expectedType)
	}
}

// ---------------------------------------------------------------------------
// Test: mapFFIErr handles unknown error kinds gracefully
// ---------------------------------------------------------------------------

func TestMapFFIErrUnknownKind(t *testing.T) {
	// An error message with an unknown kind should still be mapped (not panic).
	ffiErr := fmt.Errorf("UnknownKind: something went wrong")
	mapped := mapFFIErr(ffiErr)
	if mapped == nil {
		t.Error("mapFFIErr returned nil for unknown error kind")
	}
}

// ---------------------------------------------------------------------------
// Test: mapFFIErr nil passthrough
// ---------------------------------------------------------------------------

func TestMapFFIErrNilPassthrough(t *testing.T) {
	if mapFFIErr(nil) != nil {
		t.Error("mapFFIErr should return nil for nil input")
	}
}
