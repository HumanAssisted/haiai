// Package ffi provides a Go wrapper around the haiigo Rust cdylib (libhaiigo).
//
// Every exported function loads the Rust shared library via CGo and calls the
// corresponding hai_* FFI function. Results are returned as JSON error envelopes:
//   - Success: {"ok": <result>}
//   - Error:   {"error": {"kind": "...", "message": "..."}}
//
// Memory management: every *C.char returned by hai_* functions MUST be freed
// with hai_free_string after use.
package ffi

/*
#cgo LDFLAGS: -lhaiigo
#include <stdlib.h>

// Handle management
typedef const void* HaiClientHandle;
extern HaiClientHandle hai_client_new(const char* config_json);
extern void hai_client_free(HaiClientHandle handle);
extern void hai_free_string(char* s);

// Registration & Identity
extern char* hai_hello(HaiClientHandle handle, _Bool include_test);
extern char* hai_check_username(HaiClientHandle handle, const char* username);
extern char* hai_register(HaiClientHandle handle, const char* options_json);
extern char* hai_rotate_keys(HaiClientHandle handle, const char* options_json);
extern char* hai_update_agent(HaiClientHandle handle, const char* agent_data);
extern char* hai_submit_response(HaiClientHandle handle, const char* params_json);
extern char* hai_verify_status(HaiClientHandle handle, const char* agent_id);

// Username
extern char* hai_claim_username(HaiClientHandle handle, const char* agent_id, const char* username);
extern char* hai_update_username(HaiClientHandle handle, const char* agent_id, const char* username);
extern char* hai_delete_username(HaiClientHandle handle, const char* agent_id);

// Email Core
extern char* hai_send_email(HaiClientHandle handle, const char* options_json);
extern char* hai_send_signed_email(HaiClientHandle handle, const char* options_json);
extern char* hai_list_messages(HaiClientHandle handle, const char* options_json);
extern char* hai_update_labels(HaiClientHandle handle, const char* params_json);
extern char* hai_get_email_status(HaiClientHandle handle);
extern char* hai_get_message(HaiClientHandle handle, const char* message_id);
extern char* hai_get_unread_count(HaiClientHandle handle);

// Email Actions
extern char* hai_mark_read(HaiClientHandle handle, const char* message_id);
extern char* hai_mark_unread(HaiClientHandle handle, const char* message_id);
extern char* hai_delete_message(HaiClientHandle handle, const char* message_id);
extern char* hai_archive(HaiClientHandle handle, const char* message_id);
extern char* hai_unarchive(HaiClientHandle handle, const char* message_id);
extern char* hai_reply_with_options(HaiClientHandle handle, const char* params_json);
extern char* hai_forward(HaiClientHandle handle, const char* params_json);

// Search & Contacts
extern char* hai_search_messages(HaiClientHandle handle, const char* options_json);
extern char* hai_contacts(HaiClientHandle handle);

// Key Operations
extern char* hai_fetch_remote_key(HaiClientHandle handle, const char* jacs_id, const char* version);
extern char* hai_fetch_key_by_hash(HaiClientHandle handle, const char* hash);
extern char* hai_fetch_key_by_email(HaiClientHandle handle, const char* email);
extern char* hai_fetch_key_by_domain(HaiClientHandle handle, const char* domain);
extern char* hai_fetch_all_keys(HaiClientHandle handle, const char* jacs_id);

// Verification
extern char* hai_verify_document(HaiClientHandle handle, const char* document);
extern char* hai_get_verification(HaiClientHandle handle, const char* agent_id);
extern char* hai_verify_agent_document(HaiClientHandle handle, const char* request_json);

// Benchmarks
extern char* hai_benchmark(HaiClientHandle handle, const char* name, const char* tier);
extern char* hai_free_run(HaiClientHandle handle, const char* transport);
extern char* hai_pro_run(HaiClientHandle handle, const char* options_json);
extern char* hai_enterprise_run(HaiClientHandle handle);

// JACS Delegation
extern char* hai_sign_message(HaiClientHandle handle, const char* message);
extern char* hai_canonical_json(HaiClientHandle handle, const char* value_json);
extern char* hai_verify_a2a_artifact(HaiClientHandle handle, const char* wrapped_json);
extern char* hai_build_auth_header(HaiClientHandle handle);
extern char* hai_export_agent_json(HaiClientHandle handle);

// Client State (Mutating)
extern char* hai_set_hai_agent_id(HaiClientHandle handle, const char* id);
extern char* hai_set_agent_email(HaiClientHandle handle, const char* email);
*/
import "C"
import (
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"unsafe"
)

// FFIError represents an error returned by the Rust FFI layer.
type FFIError struct {
	Kind    string `json:"kind"`
	Message string `json:"message"`
}

func (e *FFIError) Error() string {
	return fmt.Sprintf("%s: %s", e.Kind, e.Message)
}

// envelope is the JSON error envelope returned by all FFI functions.
type envelope struct {
	OK    json.RawMessage `json:"ok"`
	Error *FFIError       `json:"error"`
}

// parseEnvelope parses a JSON error envelope and returns the "ok" payload or an error.
func parseEnvelope(jsonStr string) (json.RawMessage, error) {
	var env envelope
	if err := json.Unmarshal([]byte(jsonStr), &env); err != nil {
		return nil, fmt.Errorf("failed to parse FFI response: %w", err)
	}
	if env.Error != nil {
		return nil, env.Error
	}
	return env.OK, nil
}

// goString converts a C string to a Go string and frees the C string.
func goString(cstr *C.char) string {
	s := C.GoString(cstr)
	C.hai_free_string(cstr)
	return s
}

// cString converts a Go string to a C string. Caller must free with C.free.
func cString(s string) *C.char {
	return C.CString(s)
}

// Client wraps a Rust HaiClientWrapper handle via CGo.
type Client struct {
	handle    C.HaiClientHandle
	closeOnce sync.Once
	closed    bool
}

// NewClient creates a new FFI client from a config JSON string.
func NewClient(configJSON string) (*Client, error) {
	cs := cString(configJSON)
	defer C.free(unsafe.Pointer(cs))

	handle := C.hai_client_new(cs)
	if handle == nil {
		return nil, fmt.Errorf("failed to create HAI client from config")
	}
	return &Client{handle: handle}, nil
}

// Close frees the underlying Rust client. Safe to call multiple times.
func (c *Client) Close() {
	c.closeOnce.Do(func() {
		if c.handle != nil {
			C.hai_client_free(c.handle)
			c.handle = nil
			c.closed = true
		}
	})
}

// checkClosed returns an error if the client has been closed.
func (c *Client) checkClosed() error {
	if c.closed || c.handle == nil {
		return fmt.Errorf("client is closed")
	}
	return nil
}

// callStr calls an FFI function that takes one string arg and returns JSON envelope.
func (c *Client) callStr(fn func(C.HaiClientHandle, *C.char) *C.char, arg string) (json.RawMessage, error) {
	cs := cString(arg)
	defer C.free(unsafe.Pointer(cs))
	result := goString(fn(c.handle, cs))
	return parseEnvelope(result)
}

// callNoArg calls an FFI function that takes no args and returns JSON envelope.
func (c *Client) callNoArg(fn func(C.HaiClientHandle) *C.char) (json.RawMessage, error) {
	result := goString(fn(c.handle))
	return parseEnvelope(result)
}

// callTwoStr calls an FFI function that takes two string args and returns JSON envelope.
func (c *Client) callTwoStr(fn func(C.HaiClientHandle, *C.char, *C.char) *C.char, arg1, arg2 string) (json.RawMessage, error) {
	cs1 := cString(arg1)
	defer C.free(unsafe.Pointer(cs1))
	cs2 := cString(arg2)
	defer C.free(unsafe.Pointer(cs2))
	result := goString(fn(c.handle, cs1, cs2))
	return parseEnvelope(result)
}

// --- Registration & Identity ---

func (c *Client) Hello(includeTest bool) (json.RawMessage, error) {
	result := goString(C.hai_hello(c.handle, C._Bool(includeTest)))
	return parseEnvelope(result)
}

func (c *Client) CheckUsername(username string) (json.RawMessage, error) {
	return c.callStr(C.hai_check_username, username)
}

func (c *Client) Register(optionsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_register, optionsJSON)
}

func (c *Client) RotateKeys(optionsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_rotate_keys, optionsJSON)
}

func (c *Client) UpdateAgent(agentData string) (json.RawMessage, error) {
	return c.callStr(C.hai_update_agent, agentData)
}

func (c *Client) SubmitResponse(paramsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_submit_response, paramsJSON)
}

func (c *Client) VerifyStatus(agentID string) (json.RawMessage, error) {
	return c.callStr(C.hai_verify_status, agentID)
}

// --- Username ---

func (c *Client) ClaimUsername(agentID, username string) (json.RawMessage, error) {
	return c.callTwoStr(C.hai_claim_username, agentID, username)
}

func (c *Client) UpdateUsername(agentID, username string) (json.RawMessage, error) {
	return c.callTwoStr(C.hai_update_username, agentID, username)
}

func (c *Client) DeleteUsername(agentID string) (json.RawMessage, error) {
	return c.callStr(C.hai_delete_username, agentID)
}

// --- Email Core ---

func (c *Client) SendEmail(optionsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_send_email, optionsJSON)
}

func (c *Client) SendSignedEmail(optionsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_send_signed_email, optionsJSON)
}

func (c *Client) ListMessages(optionsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_list_messages, optionsJSON)
}

func (c *Client) UpdateLabels(paramsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_update_labels, paramsJSON)
}

func (c *Client) GetEmailStatus() (json.RawMessage, error) {
	return c.callNoArg(C.hai_get_email_status)
}

func (c *Client) GetMessage(messageID string) (json.RawMessage, error) {
	return c.callStr(C.hai_get_message, messageID)
}

func (c *Client) GetUnreadCount() (json.RawMessage, error) {
	return c.callNoArg(C.hai_get_unread_count)
}

// --- Email Actions ---

func (c *Client) MarkRead(messageID string) error {
	_, err := c.callStr(C.hai_mark_read, messageID)
	return err
}

func (c *Client) MarkUnread(messageID string) error {
	_, err := c.callStr(C.hai_mark_unread, messageID)
	return err
}

func (c *Client) DeleteMessage(messageID string) error {
	_, err := c.callStr(C.hai_delete_message, messageID)
	return err
}

func (c *Client) Archive(messageID string) error {
	_, err := c.callStr(C.hai_archive, messageID)
	return err
}

func (c *Client) Unarchive(messageID string) error {
	_, err := c.callStr(C.hai_unarchive, messageID)
	return err
}

func (c *Client) ReplyWithOptions(paramsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_reply_with_options, paramsJSON)
}

func (c *Client) Forward(paramsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_forward, paramsJSON)
}

// --- Search & Contacts ---

func (c *Client) SearchMessages(optionsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_search_messages, optionsJSON)
}

func (c *Client) Contacts() (json.RawMessage, error) {
	return c.callNoArg(C.hai_contacts)
}

// --- Key Operations ---

func (c *Client) FetchRemoteKey(jacsID, version string) (json.RawMessage, error) {
	return c.callTwoStr(C.hai_fetch_remote_key, jacsID, version)
}

func (c *Client) FetchKeyByHash(hash string) (json.RawMessage, error) {
	return c.callStr(C.hai_fetch_key_by_hash, hash)
}

func (c *Client) FetchKeyByEmail(email string) (json.RawMessage, error) {
	return c.callStr(C.hai_fetch_key_by_email, email)
}

func (c *Client) FetchKeyByDomain(domain string) (json.RawMessage, error) {
	return c.callStr(C.hai_fetch_key_by_domain, domain)
}

func (c *Client) FetchAllKeys(jacsID string) (json.RawMessage, error) {
	return c.callStr(C.hai_fetch_all_keys, jacsID)
}

// --- Verification ---

func (c *Client) VerifyDocument(document string) (json.RawMessage, error) {
	return c.callStr(C.hai_verify_document, document)
}

func (c *Client) GetVerification(agentID string) (json.RawMessage, error) {
	return c.callStr(C.hai_get_verification, agentID)
}

func (c *Client) VerifyAgentDocument(requestJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_verify_agent_document, requestJSON)
}

// --- Benchmarks ---

func (c *Client) Benchmark(name, tier string) (json.RawMessage, error) {
	return c.callTwoStr(C.hai_benchmark, name, tier)
}

func (c *Client) FreeRun(transport string) (json.RawMessage, error) {
	cs := cString(transport)
	defer C.free(unsafe.Pointer(cs))
	result := goString(C.hai_free_run(c.handle, cs))
	return parseEnvelope(result)
}

func (c *Client) ProRun(optionsJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_pro_run, optionsJSON)
}

func (c *Client) EnterpriseRun() error {
	_, err := c.callNoArg(C.hai_enterprise_run)
	return err
}

// --- JACS Delegation ---

func (c *Client) BuildAuthHeader() (string, error) {
	raw, err := c.callNoArg(C.hai_build_auth_header)
	if err != nil {
		return "", err
	}
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", fmt.Errorf("failed to parse auth header: %w", err)
	}
	return s, nil
}

func (c *Client) SignMessage(message string) (string, error) {
	raw, err := c.callStr(C.hai_sign_message, message)
	if err != nil {
		return "", err
	}
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", fmt.Errorf("failed to parse signature: %w", err)
	}
	return s, nil
}

func (c *Client) CanonicalJSON(valueJSON string) (string, error) {
	raw, err := c.callStr(C.hai_canonical_json, valueJSON)
	if err != nil {
		return "", err
	}
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", fmt.Errorf("failed to parse canonical JSON: %w", err)
	}
	return s, nil
}

func (c *Client) VerifyA2AArtifact(wrappedJSON string) (json.RawMessage, error) {
	return c.callStr(C.hai_verify_a2a_artifact, wrappedJSON)
}

func (c *Client) ExportAgentJSON() (json.RawMessage, error) {
	return c.callNoArg(C.hai_export_agent_json)
}

// --- Client State (Mutating) ---

func (c *Client) SetHaiAgentID(id string) error {
	_, err := c.callStr(C.hai_set_hai_agent_id, id)
	return err
}

func (c *Client) SetAgentEmail(email string) error {
	_, err := c.callStr(C.hai_set_agent_email, email)
	return err
}

// MapFFIError converts an FFI error to the appropriate haiai error type.
// This is used by the SDK client layer to map FFI errors to Go SDK errors.
func MapFFIError(err error) error {
	if err == nil {
		return nil
	}
	ffiErr, ok := err.(*FFIError)
	if !ok {
		return err
	}

	msg := ffiErr.Message
	kind := ffiErr.Kind

	switch {
	case strings.EqualFold(kind, "AuthFailed"):
		return &mappedError{kind: "auth", message: msg, statusCode: 401}
	case strings.EqualFold(kind, "RateLimited"):
		return &mappedError{kind: "rate_limited", message: msg, statusCode: 429}
	case strings.EqualFold(kind, "NotFound"):
		return &mappedError{kind: "not_found", message: msg, statusCode: 404}
	case strings.EqualFold(kind, "NetworkFailed"):
		return &mappedError{kind: "connection", message: msg}
	case strings.EqualFold(kind, "ApiError"):
		return &mappedError{kind: "api", message: msg}
	default:
		return &mappedError{kind: "generic", message: msg}
	}
}

type mappedError struct {
	kind       string
	message    string
	statusCode int
}

func (e *mappedError) Error() string {
	if e.statusCode > 0 {
		return fmt.Sprintf("%s (HTTP %d): %s", e.kind, e.statusCode, e.message)
	}
	return fmt.Sprintf("%s: %s", e.kind, e.message)
}

// IsAuthError returns true if the error is an authentication error.
func IsAuthError(err error) bool {
	if m, ok := err.(*mappedError); ok {
		return m.kind == "auth"
	}
	return false
}

// IsRateLimited returns true if the error is a rate limit error.
func IsRateLimited(err error) bool {
	if m, ok := err.(*mappedError); ok {
		return m.kind == "rate_limited"
	}
	return false
}

// IsNotFound returns true if the error is a not-found error.
func IsNotFound(err error) bool {
	if m, ok := err.(*mappedError); ok {
		return m.kind == "not_found"
	}
	return false
}
