//go:build cgo

// Package ffi provides a Go wrapper around the haiigo Rust cdylib (libhaiigo).
//
// Every exported function loads the Rust shared library via CGo and calls the
// corresponding hai_* FFI function. Results are returned as JSON error envelopes:
//   - Success: {"ok": <result>}
//   - Error:   {"error": {"kind": "...", "message": "..."}}
//
// Memory management: every *C.char returned by hai_* functions MUST be freed
// with hai_free_string after use.
//
// ## Why CGo instead of purego
//
// The PRD (DRY_FFI.md Decision 5) required evaluating purego as an alternative
// to CGo. CGo was chosen for the following reasons:
//
// 1. **Memory management ergonomics.** Every FFI method returns a `char*` JSON
//    string that must be freed. CGo provides `C.GoString()` + `defer C.free()`
//    which is safe and idiomatic. purego requires manual `uintptr` return,
//    unsafe pointer cast to read the string, and explicit free — more error-prone.
//
// 2. **Stability.** purego remains beta with open issues (e.g., #399, #407 as
//    of March 2026). CGo is battle-tested and matches the existing JACS jacsgo
//    pattern.
//
// 3. **Build simplicity.** The haiigo Rust crate already builds as a cdylib for
//    CGo. purego would need the same cdylib but adds runtime dlopen complexity.
//
// purego may be reconsidered when it reaches v1.0 stable, particularly if
// `CGO_ENABLED=0` builds become a requirement.
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
extern char* hai_register(HaiClientHandle handle, const char* options_json);
extern char* hai_register_new_agent(HaiClientHandle handle, const char* options_json);
extern char* hai_rotate_keys(HaiClientHandle handle, const char* options_json);
extern char* hai_update_agent(HaiClientHandle handle, const char* agent_data);
extern char* hai_submit_response(HaiClientHandle handle, const char* params_json);
extern char* hai_verify_status(HaiClientHandle handle, const char* agent_id);

// Username
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

// Server Keys
extern char* hai_fetch_server_keys(HaiClientHandle handle);

// Raw Email Sign/Verify
extern char* hai_sign_email_raw(HaiClientHandle handle, const char* raw_email_b64);
extern char* hai_verify_email_raw(HaiClientHandle handle, const char* raw_email_b64);

// Attestations
extern char* hai_create_attestation(HaiClientHandle handle, const char* params_json);
extern char* hai_list_attestations(HaiClientHandle handle, const char* params_json);
extern char* hai_get_attestation(HaiClientHandle handle, const char* agent_id, const char* doc_id);
extern char* hai_verify_attestation(HaiClientHandle handle, const char* document);

// Email Templates
extern char* hai_create_email_template(HaiClientHandle handle, const char* options_json);
extern char* hai_list_email_templates(HaiClientHandle handle, const char* options_json);
extern char* hai_get_email_template(HaiClientHandle handle, const char* template_id);
extern char* hai_update_email_template(HaiClientHandle handle, const char* template_id, const char* options_json);
extern char* hai_delete_email_template(HaiClientHandle handle, const char* template_id);

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

// Client State (Read)
extern char* hai_jacs_id(HaiClientHandle handle);
extern char* hai_base_url(HaiClientHandle handle);
extern char* hai_hai_agent_id(HaiClientHandle handle);
extern char* hai_agent_email(HaiClientHandle handle);

// Client State (Mutating)
extern char* hai_set_hai_agent_id(HaiClientHandle handle, const char* id);
extern char* hai_set_agent_email(HaiClientHandle handle, const char* email);

// SSE Streaming
extern unsigned long long hai_connect_sse(HaiClientHandle handle);
extern char* hai_sse_next_event(unsigned long long handle_id);
extern void hai_sse_close(unsigned long long handle_id);

// WebSocket Streaming
extern unsigned long long hai_connect_ws(HaiClientHandle handle);
extern char* hai_ws_next_event(unsigned long long handle_id);
extern void hai_ws_close(unsigned long long handle_id);

// Error retrieval for hai_client_new
extern char* hai_last_error();
*/
import "C"
import (
	"encoding/json"
	"fmt"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
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
//
// Thread safety: an atomic.Bool guards the closed state and a sync.RWMutex
// protects the handle pointer. Method calls acquire a read lock; Close()
// acquires a write lock. This prevents data races when Close() (or the GC
// finalizer) fires concurrently with an in-flight method call.
type Client struct {
	mu        sync.RWMutex
	handle    C.HaiClientHandle
	closeOnce sync.Once
	closed    atomic.Bool
}

// NewClient creates a new FFI client from a config JSON string.
//
// Uses runtime.LockOSThread to ensure hai_client_new and hai_last_error
// execute on the same OS thread, which is required because the Rust FFI
// layer stores constructor errors in thread-local storage.
func NewClient(configJSON string) (*Client, error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	cs := cString(configJSON)
	defer C.free(unsafe.Pointer(cs))

	handle := C.hai_client_new(cs)
	if handle == nil {
		// Retrieve detailed error from hai_last_error()
		// Safe because LockOSThread guarantees same OS thread as hai_client_new.
		errPtr := C.hai_last_error()
		if errPtr != nil {
			errJSON := goString(errPtr)
			_, parseErr := parseEnvelope(errJSON)
			if parseErr != nil {
				return nil, fmt.Errorf("failed to create HAI client: %w", parseErr)
			}
			return nil, fmt.Errorf("failed to create HAI client from config")
		}
		return nil, fmt.Errorf("failed to create HAI client from config")
	}
	c := &Client{handle: handle}
	// Safety net: free the Rust handle if the Go consumer forgets to call Close().
	runtime.SetFinalizer(c, (*Client).Close)
	return c, nil
}

// Close frees the underlying Rust client. Safe to call multiple times.
func (c *Client) Close() {
	c.closeOnce.Do(func() {
		c.closed.Store(true)
		c.mu.Lock()
		defer c.mu.Unlock()
		if c.handle != nil {
			C.hai_client_free(c.handle)
			c.handle = nil
		}
	})
}

// checkClosed returns an error if the client has been closed.
// Caller must hold c.mu.RLock() before calling.
func (c *Client) checkClosed() error {
	if c.closed.Load() || c.handle == nil {
		return fmt.Errorf("client is closed")
	}
	return nil
}

// callOneStr is a helper that calls a C function taking (handle, *C.char) -> *C.char.
// CGo does not allow passing C function pointers as Go function values, so each
// method inlines the C call directly. This helper exists only as documentation.

// --- Registration & Identity ---

func (c *Client) Hello(includeTest bool) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	result := goString(C.hai_hello(c.handle, C._Bool(includeTest)))
	return parseEnvelope(result)
}

func (c *Client) Register(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_register(c.handle, cs)))
}

func (c *Client) RegisterNewAgent(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_register_new_agent(c.handle, cs)))
}

func (c *Client) RotateKeys(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_rotate_keys(c.handle, cs)))
}

func (c *Client) UpdateAgent(agentData string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(agentData)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_update_agent(c.handle, cs)))
}

func (c *Client) SubmitResponse(paramsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(paramsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_submit_response(c.handle, cs)))
}

func (c *Client) VerifyStatus(agentID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(agentID)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_verify_status(c.handle, cs)))
}

// --- Username ---

func (c *Client) UpdateUsername(agentID, username string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs1 := cString(agentID)
	defer C.free(unsafe.Pointer(cs1))
	cs2 := cString(username)
	defer C.free(unsafe.Pointer(cs2))
	return parseEnvelope(goString(C.hai_update_username(c.handle, cs1, cs2)))
}

func (c *Client) DeleteUsername(agentID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(agentID)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_delete_username(c.handle, cs)))
}

// --- Email Core ---

func (c *Client) SendEmail(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_send_email(c.handle, cs)))
}

func (c *Client) SendSignedEmail(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_send_signed_email(c.handle, cs)))
}

func (c *Client) ListMessages(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_list_messages(c.handle, cs)))
}

func (c *Client) UpdateLabels(paramsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(paramsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_update_labels(c.handle, cs)))
}

func (c *Client) GetEmailStatus() (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	return parseEnvelope(goString(C.hai_get_email_status(c.handle)))
}

func (c *Client) GetMessage(messageID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(messageID)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_get_message(c.handle, cs)))
}

func (c *Client) GetUnreadCount() (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	return parseEnvelope(goString(C.hai_get_unread_count(c.handle)))
}

// --- Email Actions ---

func (c *Client) MarkRead(messageID string) error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	cs := cString(messageID)
	defer C.free(unsafe.Pointer(cs))
	_, err := parseEnvelope(goString(C.hai_mark_read(c.handle, cs)))
	return err
}

func (c *Client) MarkUnread(messageID string) error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	cs := cString(messageID)
	defer C.free(unsafe.Pointer(cs))
	_, err := parseEnvelope(goString(C.hai_mark_unread(c.handle, cs)))
	return err
}

func (c *Client) DeleteMessage(messageID string) error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	cs := cString(messageID)
	defer C.free(unsafe.Pointer(cs))
	_, err := parseEnvelope(goString(C.hai_delete_message(c.handle, cs)))
	return err
}

func (c *Client) Archive(messageID string) error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	cs := cString(messageID)
	defer C.free(unsafe.Pointer(cs))
	_, err := parseEnvelope(goString(C.hai_archive(c.handle, cs)))
	return err
}

func (c *Client) Unarchive(messageID string) error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	cs := cString(messageID)
	defer C.free(unsafe.Pointer(cs))
	_, err := parseEnvelope(goString(C.hai_unarchive(c.handle, cs)))
	return err
}

func (c *Client) ReplyWithOptions(paramsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(paramsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_reply_with_options(c.handle, cs)))
}

func (c *Client) Forward(paramsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(paramsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_forward(c.handle, cs)))
}

// --- Search & Contacts ---

func (c *Client) SearchMessages(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_search_messages(c.handle, cs)))
}

func (c *Client) Contacts() (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	return parseEnvelope(goString(C.hai_contacts(c.handle)))
}

// --- Server Keys ---

func (c *Client) FetchServerKeys() (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	return parseEnvelope(goString(C.hai_fetch_server_keys(c.handle)))
}

// --- Raw Email Sign/Verify ---

func (c *Client) SignEmailRaw(rawEmailB64 string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(rawEmailB64)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_sign_email_raw(c.handle, cs)))
}

func (c *Client) VerifyEmailRaw(rawEmailB64 string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(rawEmailB64)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_verify_email_raw(c.handle, cs)))
}

// --- Attestations ---

func (c *Client) CreateAttestation(paramsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(paramsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_create_attestation(c.handle, cs)))
}

func (c *Client) ListAttestations(paramsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(paramsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_list_attestations(c.handle, cs)))
}

func (c *Client) GetAttestation(agentID, docID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs1 := cString(agentID)
	defer C.free(unsafe.Pointer(cs1))
	cs2 := cString(docID)
	defer C.free(unsafe.Pointer(cs2))
	return parseEnvelope(goString(C.hai_get_attestation(c.handle, cs1, cs2)))
}

func (c *Client) VerifyAttestation(document string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(document)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_verify_attestation(c.handle, cs)))
}

// --- Email Templates ---

func (c *Client) CreateEmailTemplate(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_create_email_template(c.handle, cs)))
}

func (c *Client) ListEmailTemplates(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_list_email_templates(c.handle, cs)))
}

func (c *Client) GetEmailTemplate(templateID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(templateID)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_get_email_template(c.handle, cs)))
}

func (c *Client) UpdateEmailTemplate(templateID, optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs1 := cString(templateID)
	defer C.free(unsafe.Pointer(cs1))
	cs2 := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs2))
	return parseEnvelope(goString(C.hai_update_email_template(c.handle, cs1, cs2)))
}

func (c *Client) DeleteEmailTemplate(templateID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(templateID)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_delete_email_template(c.handle, cs)))
}

// --- Key Operations ---

func (c *Client) FetchRemoteKey(jacsID, version string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs1 := cString(jacsID)
	defer C.free(unsafe.Pointer(cs1))
	cs2 := cString(version)
	defer C.free(unsafe.Pointer(cs2))
	return parseEnvelope(goString(C.hai_fetch_remote_key(c.handle, cs1, cs2)))
}

func (c *Client) FetchKeyByHash(hash string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(hash)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_fetch_key_by_hash(c.handle, cs)))
}

func (c *Client) FetchKeyByEmail(email string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(email)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_fetch_key_by_email(c.handle, cs)))
}

func (c *Client) FetchKeyByDomain(domain string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(domain)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_fetch_key_by_domain(c.handle, cs)))
}

func (c *Client) FetchAllKeys(jacsID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(jacsID)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_fetch_all_keys(c.handle, cs)))
}

// --- Verification ---

func (c *Client) VerifyDocument(document string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(document)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_verify_document(c.handle, cs)))
}

func (c *Client) GetVerification(agentID string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(agentID)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_get_verification(c.handle, cs)))
}

func (c *Client) VerifyAgentDocument(requestJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(requestJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_verify_agent_document(c.handle, cs)))
}

// --- Benchmarks ---

// Benchmark starts a benchmark run. Pass empty string "" for name or tier to omit.
// Whitespace-only strings are also treated as absent on the Rust side.
func (c *Client) Benchmark(name, tier string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs1 := cString(name)
	defer C.free(unsafe.Pointer(cs1))
	cs2 := cString(tier)
	defer C.free(unsafe.Pointer(cs2))
	return parseEnvelope(goString(C.hai_benchmark(c.handle, cs1, cs2)))
}

func (c *Client) FreeRun(transport string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(transport)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_free_run(c.handle, cs)))
}

func (c *Client) ProRun(optionsJSON string) (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(optionsJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_pro_run(c.handle, cs)))
}

func (c *Client) EnterpriseRun() error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	_, err := parseEnvelope(goString(C.hai_enterprise_run(c.handle)))
	return err
}

// --- JACS Delegation ---

func (c *Client) BuildAuthHeader() (string, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return "", err
	}
	raw, err := parseEnvelope(goString(C.hai_build_auth_header(c.handle)))
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
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return "", err
	}
	cs := cString(message)
	defer C.free(unsafe.Pointer(cs))
	raw, err := parseEnvelope(goString(C.hai_sign_message(c.handle, cs)))
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
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return "", err
	}
	cs := cString(valueJSON)
	defer C.free(unsafe.Pointer(cs))
	raw, err := parseEnvelope(goString(C.hai_canonical_json(c.handle, cs)))
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
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	cs := cString(wrappedJSON)
	defer C.free(unsafe.Pointer(cs))
	return parseEnvelope(goString(C.hai_verify_a2a_artifact(c.handle, cs)))
}

func (c *Client) ExportAgentJSON() (json.RawMessage, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return nil, err
	}
	return parseEnvelope(goString(C.hai_export_agent_json(c.handle)))
}

// --- Client State (Read) ---

// JacsID returns the JACS identity ID of the client.
func (c *Client) JacsID() (string, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return "", err
	}
	raw, err := parseEnvelope(goString(C.hai_jacs_id(c.handle)))
	if err != nil {
		return "", err
	}
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", fmt.Errorf("failed to parse jacs id: %w", err)
	}
	return s, nil
}

// BaseURL returns the base URL of the client.
func (c *Client) BaseURL() (string, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return "", err
	}
	raw, err := parseEnvelope(goString(C.hai_base_url(c.handle)))
	if err != nil {
		return "", err
	}
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", fmt.Errorf("failed to parse base url: %w", err)
	}
	return s, nil
}

// HaiAgentID returns the HAI-assigned agent UUID.
func (c *Client) HaiAgentID() (string, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return "", err
	}
	raw, err := parseEnvelope(goString(C.hai_hai_agent_id(c.handle)))
	if err != nil {
		return "", err
	}
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", fmt.Errorf("failed to parse hai agent id: %w", err)
	}
	return s, nil
}

// AgentEmail returns the agent's @hai.ai email address, or empty string if not set.
func (c *Client) AgentEmail() (string, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return "", err
	}
	raw, err := parseEnvelope(goString(C.hai_agent_email(c.handle)))
	if err != nil {
		return "", err
	}
	// Result can be null (no email set) or a string
	var s *string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", fmt.Errorf("failed to parse agent email: %w", err)
	}
	if s == nil {
		return "", nil
	}
	return *s, nil
}

// --- Client State (Mutating) ---

func (c *Client) SetHaiAgentID(id string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	cs := cString(id)
	defer C.free(unsafe.Pointer(cs))
	_, err := parseEnvelope(goString(C.hai_set_hai_agent_id(c.handle, cs)))
	return err
}

func (c *Client) SetAgentEmail(email string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.checkClosed(); err != nil {
		return err
	}
	cs := cString(email)
	defer C.free(unsafe.Pointer(cs))
	_, err := parseEnvelope(goString(C.hai_set_agent_email(c.handle, cs)))
	return err
}

// --- SSE Streaming ---

func (c *Client) ConnectSSE() (uint64, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return 0, err
	}
	handle := uint64(C.hai_connect_sse(c.handle))
	if handle == 0 {
		return 0, fmt.Errorf("failed to connect SSE")
	}
	return handle, nil
}

func (c *Client) SSENextEvent(handleID uint64) (json.RawMessage, error) {
	result := goString(C.hai_sse_next_event(C.ulonglong(handleID)))
	raw, err := parseEnvelope(result)
	if err != nil {
		return nil, err
	}
	// null means connection closed
	if string(raw) == "null" {
		return nil, nil
	}
	return raw, nil
}

func (c *Client) SSEClose(handleID uint64) {
	C.hai_sse_close(C.ulonglong(handleID))
}

// --- WebSocket Streaming ---

func (c *Client) ConnectWS() (uint64, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if err := c.checkClosed(); err != nil {
		return 0, err
	}
	handle := uint64(C.hai_connect_ws(c.handle))
	if handle == 0 {
		return 0, fmt.Errorf("failed to connect WebSocket")
	}
	return handle, nil
}

func (c *Client) WSNextEvent(handleID uint64) (json.RawMessage, error) {
	result := goString(C.hai_ws_next_event(C.ulonglong(handleID)))
	raw, err := parseEnvelope(result)
	if err != nil {
		return nil, err
	}
	if string(raw) == "null" {
		return nil, nil
	}
	return raw, nil
}

func (c *Client) WSClose(handleID uint64) {
	C.hai_ws_close(C.ulonglong(handleID))
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
	case strings.EqualFold(kind, "ProviderError"):
		return &mappedError{kind: "auth", message: msg}
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
